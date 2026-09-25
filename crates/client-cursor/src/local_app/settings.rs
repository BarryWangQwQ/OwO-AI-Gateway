//! Integrates local application settings.
//!
//! Only the keys listed in `KEYS` are written; every other setting keeps its value and
//! position. Before each write the previous file is copied to the Cursor backups directory,
//! and the user's own values of `KEYS` are saved once and put back when the takeover ends.
use std::{fs, path::PathBuf};
#[cfg(test)]
use std::path::Path;

use serde_json::{Map, Value};

use crate::{Error, Result};

const NO_PROXY_KEY: &str = "http.noProxy";
const KEYS: [&str; 5] = [
    "http.proxy",
    "http.proxyKerberosServicePrincipal",
    "http.proxySupport",
    "cursor.general.disableHttp2",
    "http.experimental.systemCertificatesV2",
];

fn path() -> Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| Error::Config("cannot resolve user home directory".into()))?;
    match std::env::consts::OS {
        "macos" => Ok(home.join("Library/Application Support/Cursor/User/settings.json")),
        "windows" => Ok(std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Roaming"))
            .join("Cursor/User/settings.json")),
        "linux" => Ok(std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("Cursor/User/settings.json")),
        platform => Err(Error::Config(format!(
            "Cursor settings are unsupported on {platform}"
        ))),
    }
}

/// The settings file OwO AI Gateway edits (for status output).
pub fn settings_path() -> Result<PathBuf> {
    path()
}

struct Files {
    settings: PathBuf,
    data: PathBuf,
}

impl Files {
    fn managed() -> Result<Self> {
        Ok(Self {
            settings: path()?,
            data: crate::config::managed_data_dir()?,
        })
    }

    fn originals(&self) -> PathBuf {
        self.data.join("settings-originals.json")
    }

    fn read(&self) -> Result<Map<String, Value>> {
        let data = match fs::read_to_string(&self.settings) {
            Ok(data) => data,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
            Err(error) => return Err(error.into()),
        };
        if data.trim().is_empty() {
            return Ok(Map::new());
        }
        match json5::from_str::<Value>(&data)
            .map_err(|error| Error::Config(format!("parse Cursor settings JSONC: {error}")))?
        {
            Value::Object(map) => Ok(map),
            _ => Err(Error::Config("Cursor settings.json is not a JSON object".into())),
        }
    }

    fn write(&self, settings: &Map<String, Value>) -> Result<()> {
        if let Some(parent) = self.settings.parent() {
            fs::create_dir_all(parent)?;
        }
        self.backup()?;
        let data = serde_json::to_vec_pretty(settings)?;
        let temp = self.settings.with_extension("json.owo.tmp");
        fs::write(&temp, [data.as_slice(), b"\n"].concat())?;
        fs::rename(temp, &self.settings)?;
        Ok(())
    }

    fn backup(&self) -> Result<()> {
        if !self.settings.exists() {
            return Ok(());
        }
        let dir = self.data.join("backups");
        fs::create_dir_all(&dir)?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        fs::copy(&self.settings, dir.join(format!("{stamp}-settings.json")))?;
        Ok(())
    }

    /// Saves the user's values of the keys OwO AI Gateway overwrites (null when absent), unless a
    /// takeover is already recorded.
    fn remember_originals(&self, settings: &Map<String, Value>) -> Result<()> {
        let path = self.originals();
        if path.exists() || is_managed(settings) {
            return Ok(());
        }
        let originals: Map<String, Value> = KEYS
            .iter()
            .chain([&NO_PROXY_KEY])
            .map(|key| ((*key).to_owned(), settings.get(*key).cloned().unwrap_or(Value::Null)))
            .collect();
        if originals.get(KEYS[0]).is_some_and(|value| !value.is_null()) {
            tracing::warn!("Cursor's own http.proxy is bypassed while OwO AI Gateway handles Cursor; it is restored afterwards");
        }
        fs::create_dir_all(&self.data)?;
        fs::write(path, serde_json::to_vec_pretty(&originals)?)?;
        Ok(())
    }

    fn take_originals(&self) -> Result<Option<Map<String, Value>>> {
        match fs::read(self.originals()) {
            Ok(data) => Ok(serde_json::from_slice::<Map<String, Value>>(&data).ok()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn apply(&self, proxy_url: &str) -> Result<()> {
        let mut settings = self.read()?;
        self.remember_originals(&settings)?;
        settings.remove(NO_PROXY_KEY);
        settings.insert(KEYS[0].into(), Value::String(proxy_url.into()));
        settings.insert(KEYS[1].into(), Value::String(proxy_url.into()));
        settings.insert(KEYS[2].into(), Value::String("on".into()));
        settings.insert(KEYS[3].into(), Value::Bool(true));
        settings.insert(KEYS[4].into(), Value::Bool(true));
        self.write(&settings)
    }

    fn clear(&self) -> Result<()> {
        let mut settings = self.read()?;
        let before = settings.clone();
        let originals = self.take_originals()?;
        if originals.is_some() || is_managed(&settings) {
            for key in KEYS {
                settings.remove(key);
            }
        }
        for (key, value) in originals.iter().flatten() {
            if value.is_null() {
                settings.remove(key);
            } else {
                settings.insert(key.clone(), value.clone());
            }
        }
        if settings != before {
            self.write(&settings)?;
        }
        if originals.is_some() {
            fs::remove_file(self.originals())?;
        }
        Ok(())
    }
}

fn is_managed(settings: &Map<String, Value>) -> bool {
    let signature = settings.get(KEYS[2]) == Some(&Value::String("on".into()))
        && settings.get(KEYS[3]) == Some(&Value::Bool(true))
        && settings.get(KEYS[4]) == Some(&Value::Bool(true));
    let loopback = settings
        .get(KEYS[0])
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<reqwest::Url>().ok())
        .and_then(|url| url.host_str().map(str::to_owned))
        .is_some_and(|host| matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1" | "[::1]"));
    signature && loopback
}

fn matches(settings: &Map<String, Value>, proxy_url: &str) -> bool {
    settings.get(KEYS[0]) == Some(&Value::String(proxy_url.into()))
        && settings.get(KEYS[1]) == Some(&Value::String(proxy_url.into()))
        && settings.get(KEYS[2]) == Some(&Value::String("on".into()))
        && settings.get(KEYS[3]) == Some(&Value::Bool(true))
        && settings.get(KEYS[4]) == Some(&Value::Bool(true))
}

pub fn write_proxy_settings(proxy_url: &str) -> Result<()> {
    Files::managed()?.apply(proxy_url)
}

pub fn clear_proxy_settings() -> Result<()> {
    Files::managed()?.clear()
}

pub fn settings_match(proxy_url: &str) -> Result<bool> {
    Ok(matches(&Files::managed()?.read()?, proxy_url))
}

/// Removes a loopback proxy left behind by a previous run that did not shut down cleanly.
pub fn clear_stale_managed_settings() -> Result<()> {
    let files = Files::managed()?;
    if is_managed(&files.read()?) {
        files.clear()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files_at(settings: &Path, data: &Path) -> Files {
        Files {
            settings: settings.to_path_buf(),
            data: data.to_path_buf(),
        }
    }

    const PROXY: &str = "http://127.0.0.1:43210";

    fn setup(initial: &str) -> (tempfile::TempDir, Files) {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("User/settings.json");
        fs::create_dir_all(settings.parent().unwrap()).unwrap();
        fs::write(&settings, initial).unwrap();
        let files = files_at(&settings, &dir.path().join("data"));
        (dir, files)
    }

    #[test]
    fn clearing_restores_the_users_own_proxy_settings() {
        let (_dir, files) = setup(
            r#"{
  // user comment
  "editor.fontSize": 14,
  "http.proxy": "http://corp-proxy:8080",
  "http.noProxy": ["intranet"],
  "window.zoomLevel": 1
}"#,
        );
        files.apply(PROXY).unwrap();
        let applied = files.read().unwrap();
        assert!(matches(&applied, PROXY));
        assert!(!applied.contains_key(NO_PROXY_KEY));

        // A second start while the takeover is active must not overwrite the originals.
        files.apply("http://127.0.0.1:50000").unwrap();

        files.clear().unwrap();
        let restored = files.read().unwrap();
        assert_eq!(restored["http.proxy"], "http://corp-proxy:8080");
        assert_eq!(restored["http.noProxy"], serde_json::json!(["intranet"]));
        assert_eq!(restored["editor.fontSize"], 14);
        assert_eq!(restored["window.zoomLevel"], 1);
        for key in &KEYS[1..] {
            assert!(!restored.contains_key(*key), "{key} should be removed");
        }
        assert!(!files.originals().exists());
        assert!(files.data.join("backups").read_dir().unwrap().count() >= 1);
    }

    #[test]
    fn clearing_without_a_takeover_leaves_user_settings_alone() {
        let original = r#"{"http.proxy": "http://127.0.0.1:7890", "http.proxySupport": "on"}"#;
        let (_dir, files) = setup(original);
        files.clear().unwrap();
        assert_eq!(fs::read_to_string(&files.settings).unwrap(), original);
    }

    #[test]
    fn stale_takeover_without_originals_is_removed() {
        let (_dir, files) = setup(r#"{"editor.fontSize": 14}"#);
        files.apply(PROXY).unwrap();
        fs::remove_file(files.originals()).unwrap();
        assert!(is_managed(&files.read().unwrap()));
        files.clear().unwrap();
        let cleared = files.read().unwrap();
        assert_eq!(cleared.len(), 1);
        assert_eq!(cleared["editor.fontSize"], 14);
    }
}
