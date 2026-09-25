//! Credential references (`env:NAME`, `keyring:NAME`, `none`) and their resolution.
//!
//! Secrets only ever live inside [`Secret`], whose `Debug`/`Display` are redacted,
//! so an accidental `{:?}` in a log line cannot leak them.

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use keyring::credential::CredentialPersistence;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Keyring service name under which OwO AI Gateway stores every secret.
pub const KEYRING_SERVICE: &str = "owo";
/// How long a secret read from the OS keyring is reused before it is read again, so a
/// running gateway neither queries the keyring on every request nor misses a rotated key
/// for long.
const KEYRING_CACHE_TTL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CredentialRef {
    Env(String),
    Keyring(String),
    /// A key written directly into config.toml. Displays and serializes as `inline`.
    Inline(Secret),
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CredentialError {
    #[error("invalid key reference `{0}`: expected `env:NAME`, `keyring:NAME`, `none`, or the key itself")]
    InvalidRef(String),
    #[error("environment variable `{0}` is not set")]
    EnvMissing(String),
    #[error("no keyring entry `{0}` (store it with `owo key set {0}`)")]
    KeyringMissing(String),
    #[error("keyring backend error for `{name}`: {message}")]
    Keyring { name: String, message: String },
    #[error("keyring access is disabled (`credentials.backend = \"env\"`), cannot read `{0}`")]
    KeyringDisabled(String),
    #[error("this platform has no persistent OS keyring, so `keyring:{0}` cannot be used; use an `env:` reference instead")]
    KeyringUnsupported(String),
}

impl FromStr for CredentialRef {
    type Err = CredentialError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s == "none" {
            return Ok(CredentialRef::None);
        }
        let reference = s.split_once(':').filter(|(scheme, _)| matches!(*scheme, "env" | "keyring"));
        let Some((scheme, name)) = reference else {
            if s.is_empty() {
                return Err(CredentialError::InvalidRef(String::new()));
            }
            return Ok(CredentialRef::Inline(Secret(s.to_string())));
        };
        let valid_name = !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
        if !valid_name {
            return Err(CredentialError::InvalidRef(redact_ref(s)));
        }
        Ok(if scheme == "env" { CredentialRef::Env(name.to_string()) } else { CredentialRef::Keyring(name.to_string()) })
    }
}

/// A malformed reference may be a pasted raw key; never echo more than a prefix of it.
fn redact_ref(s: &str) -> String {
    if s.len() <= 12 { s.to_string() } else { format!("{}…", &s[..s.char_indices().nth(8).map(|(i, _)| i).unwrap_or(s.len())]) }
}

impl fmt::Display for CredentialRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CredentialRef::Env(n) => write!(f, "env:{n}"),
            CredentialRef::Keyring(n) => write!(f, "keyring:{n}"),
            CredentialRef::Inline(_) => f.write_str("inline"),
            CredentialRef::None => f.write_str("none"),
        }
    }
}

impl Serialize for CredentialRef {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for CredentialRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

/// A secret value with redacted formatting.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Exposes the raw value. Call sites should be limited to building upstream requests.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialBackend {
    /// Environment variables plus the OS keyring (Windows Credential Manager,
    /// macOS Keychain, Linux Secret Service).
    #[default]
    System,
    /// Environment variables only; `keyring:` references fail.
    Env,
}

/// Resolves credential references to secrets. Clones share one keyring cache.
#[derive(Debug, Clone, Default)]
pub struct CredentialStore {
    backend: CredentialBackend,
    cache: Arc<Mutex<HashMap<String, (Instant, Secret)>>>,
}

impl CredentialStore {
    pub fn new(backend: CredentialBackend) -> Self {
        Self { backend, cache: Arc::default() }
    }

    fn cached(&self, name: &str) -> Option<Secret> {
        let cache = self.cache.lock().ok()?;
        cache.get(name).filter(|(at, _)| at.elapsed() < KEYRING_CACHE_TTL).map(|(_, s)| s.clone())
    }

    fn remember(&self, name: &str, secret: Option<&Secret>) {
        if let Ok(mut cache) = self.cache.lock() {
            match secret {
                Some(s) => cache.insert(name.to_string(), (Instant::now(), s.clone())),
                None => cache.remove(name),
            };
        }
    }

    /// Returns `Ok(None)` for [`CredentialRef::None`].
    pub fn resolve(&self, reference: &CredentialRef) -> Result<Option<Secret>, CredentialError> {
        match reference {
            CredentialRef::None => Ok(None),
            CredentialRef::Inline(secret) => Ok(Some(secret.clone())),
            CredentialRef::Env(name) => match std::env::var(name) {
                Ok(v) if !v.is_empty() => Ok(Some(Secret(v))),
                _ => Err(CredentialError::EnvMissing(name.clone())),
            },
            CredentialRef::Keyring(name) => {
                self.require_keyring(name)?;
                if let Some(secret) = self.cached(name) {
                    return Ok(Some(secret));
                }
                let entry = keyring_entry(name)?;
                match entry.get_password() {
                    Ok(v) => {
                        let secret = Secret(v);
                        self.remember(name, Some(&secret));
                        Ok(Some(secret))
                    }
                    Err(keyring::Error::NoEntry) => Err(CredentialError::KeyringMissing(name.clone())),
                    Err(e) => Err(keyring_err(name, e)),
                }
            }
        }
    }

    pub fn set_keyring(&self, name: &str, secret: &Secret) -> Result<(), CredentialError> {
        self.require_keyring(name)?;
        validate_name(name)?;
        keyring_entry(name)?.set_password(secret.expose()).map_err(|e| keyring_err(name, e))?;
        self.remember(name, Some(secret));
        Ok(())
    }

    pub fn delete_keyring(&self, name: &str) -> Result<(), CredentialError> {
        self.require_keyring(name)?;
        self.remember(name, None);
        match keyring_entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keyring_err(name, e)),
        }
    }

    fn require_keyring(&self, name: &str) -> Result<(), CredentialError> {
        match self.backend {
            CredentialBackend::System if keyring_is_persistent() => Ok(()),
            CredentialBackend::System => Err(CredentialError::KeyringUnsupported(name.to_string())),
            CredentialBackend::Env => Err(CredentialError::KeyringDisabled(name.to_string())),
        }
    }
}

/// `keyring` substitutes an in-memory store on platforms it has no OS keyring for; secrets
/// "saved" there would be gone when the process exits. (OwO AI Gateway never replaces keyring's
/// default store, so the platform store is the one entries use.)
pub fn keyring_is_persistent() -> bool {
    persists(keyring::default::default_credential_builder().as_ref())
}

fn persists(builder: &keyring::credential::CredentialBuilder) -> bool {
    matches!(builder.persistence(), CredentialPersistence::UntilDelete)
}

fn validate_name(name: &str) -> Result<(), CredentialError> {
    format!("keyring:{name}").parse::<CredentialRef>().map(|_| ())
}

fn keyring_entry(name: &str) -> Result<keyring::Entry, CredentialError> {
    keyring::Entry::new(KEYRING_SERVICE, name).map_err(|e| keyring_err(name, e))
}

fn keyring_err(name: &str, e: keyring::Error) -> CredentialError {
    let unreachable = matches!(e, keyring::Error::PlatformFailure(_) | keyring::Error::NoStorageAccess(_));
    let mut message = e.to_string();
    if unreachable && cfg!(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")) {
        // Servers, SSH sessions, WSL and containers usually run no Secret Service daemon.
        message.push_str(
            " (no Secret Service is reachable: start/unlock gnome-keyring or KWallet in this session, \
             or use an `env:` reference / `credentials.backend = \"env\"` on headless machines)",
        );
    } else if unreachable {
        message.push_str(" (the OS keyring is locked or unavailable in this session; an `env:` reference works everywhere)");
    }
    CredentialError::Keyring { name: name.to_string(), message }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_references() {
        assert_eq!("env:OPENAI_API_KEY".parse::<CredentialRef>().unwrap(), CredentialRef::Env("OPENAI_API_KEY".into()));
        assert_eq!("keyring:deepseek".parse::<CredentialRef>().unwrap(), CredentialRef::Keyring("deepseek".into()));
        assert_eq!("none".parse::<CredentialRef>().unwrap(), CredentialRef::None);
        assert!("keyring:bad name".parse::<CredentialRef>().is_err());
        assert!("".parse::<CredentialRef>().is_err());
    }

    #[test]
    fn inline_keys_resolve_but_never_print() {
        let r: CredentialRef = "sk-live-0123456789abcdef".parse().unwrap();
        assert_eq!(r.to_string(), "inline");
        assert!(!format!("{r:?}").contains("sk-live"));
        let store = CredentialStore::new(CredentialBackend::Env);
        assert_eq!(store.resolve(&r).unwrap().unwrap().expose(), "sk-live-0123456789abcdef");
    }

    #[test]
    fn malformed_references_are_not_echoed() {
        let err = "keyring:sk-live 0123456789abcdef".parse::<CredentialRef>().unwrap_err();
        let msg = err.to_string();
        assert!(!msg.contains("0123456789abcdef"), "{msg}");
        assert!("env:".parse::<CredentialRef>().is_err());
    }

    #[test]
    fn secret_is_redacted() {
        let s = Secret::new("sk-secret");
        assert_eq!(format!("{s:?} {s}"), "Secret(***) ***");
    }

    #[test]
    fn only_disk_backed_keyrings_count() {
        // The in-memory store `keyring` falls back to on platforms without an OS keyring.
        assert!(!persists(keyring::mock::default_credential_builder().as_ref()));
        // Windows Credential Manager, macOS Keychain and Secret Service persist.
        if cfg!(any(windows, target_os = "macos", target_os = "linux", target_os = "freebsd", target_os = "openbsd")) {
            assert!(keyring_is_persistent());
        }
    }

    #[test]
    fn env_resolution() {
        let store = CredentialStore::new(CredentialBackend::Env);
        assert!(matches!(
            store.resolve(&CredentialRef::Env("OWO_TEST_DEFINITELY_UNSET_VAR".into())),
            Err(CredentialError::EnvMissing(_))
        ));
        assert!(matches!(
            store.resolve(&CredentialRef::Keyring("x".into())),
            Err(CredentialError::KeyringDisabled(_))
        ));
        assert_eq!(store.resolve(&CredentialRef::None).unwrap(), None);
    }
}
