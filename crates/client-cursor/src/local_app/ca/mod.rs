//! Installs and manages the local certificate authority.
use std::{fs, path::PathBuf};

#[cfg(target_os = "macos")]
use std::process::Command;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, GeneralSubtree, IsCa, Issuer,
    KeyPair, KeyUsagePurpose, NameConstraints, RsaKeySize, PKCS_RSA_SHA256,
};

/// Subject of the local CA, as shown in the OS certificate manager.
pub const CA_COMMON_NAME: &str = "OwO AI Gateway Cursor Local CA";
/// The only DNS subtree the CA may issue for (`*.cursor.sh`).
const PERMITTED_DOMAIN: &str = "cursor.sh";
use sha1::{Digest, Sha1};
use time::{Duration, OffsetDateTime};
use x509_parser::prelude::FromDer;

use crate::{config::managed_data_dir, Error, Result};

use super::CaState;

#[derive(Clone)]
pub struct CaManager {
    dir: PathBuf,
}

/// A command that changes the operating system's trust store; it needs administrator
/// rights (the caller elevates it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl TrustCommand {
    fn new(program: &str, args: &[&str]) -> Self {
        Self { program: program.into(), args: args.iter().map(|a| a.to_string()).collect() }
    }
}

#[cfg(target_os = "macos")]
const MACOS_SYSTEM_KEYCHAIN: &str = "/Library/Keychains/System.keychain";

pub struct LoadedCa {
    pub issuer: Issuer<'static, KeyPair>,
}

impl CaManager {
    pub fn managed() -> Result<Self> {
        Ok(Self {
            dir: managed_data_dir()?.join("ca"),
        })
    }

    fn cert_path(&self) -> PathBuf {
        self.dir.join("ca.crt")
    }
    fn key_path(&self) -> PathBuf {
        self.dir.join("ca.key")
    }

    pub fn state(&self) -> Result<CaState> {
        let cert = fs::read_to_string(self.cert_path());
        let key = fs::read_to_string(self.key_path());
        match (cert, key) {
            (Err(cert_error), Err(key_error))
                if cert_error.kind() == std::io::ErrorKind::NotFound
                    && key_error.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok(CaState::Missing)
            }
            (Ok(cert), Ok(key)) => {
                if parse_issuer(&cert, &key).is_err() {
                    return Ok(CaState::Invalid);
                }
                Ok(if is_installed(&cert)? {
                    CaState::Ready
                } else {
                    CaState::Untrusted
                })
            }
            _ => Ok(CaState::Invalid),
        }
    }

    pub fn load(&self) -> Result<LoadedCa> {
        let cert = fs::read_to_string(self.cert_path())?;
        let key = fs::read_to_string(self.key_path())?;
        Ok(LoadedCa {
            issuer: parse_issuer(&cert, &key)?,
        })
    }

    pub fn cert_file(&self) -> PathBuf {
        self.cert_path()
    }

    /// The trust commands as one line to run by hand in an administrator shell.
    pub fn install_command(&self) -> Option<String> {
        let quote = |a: &str| {
            if !a.is_empty() && !a.contains([' ', '\'', '"', '\\', '$', '&']) {
                a.to_string()
            } else if cfg!(windows) {
                format!("\"{a}\"")
            } else {
                format!("'{}'", a.replace('\'', "'\\''"))
            }
        };
        let sudo = if cfg!(windows) { "" } else { "sudo " };
        let lines: Vec<String> = self
            .trust_commands()
            .iter()
            .map(|c| format!("{sudo}{} {}", c.program, c.args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ")))
            .collect();
        (!lines.is_empty()).then(|| lines.join(" && "))
    }

    /// Commands that add the CA certificate to the OS trust store.
    pub fn trust_commands(&self) -> Vec<TrustCommand> {
        platform_trust(&self.cert_path().to_string_lossy())
    }

    /// Commands that remove the CA certificate from the OS trust store: matched by
    /// fingerprint while the certificate file exists, else by common name.
    pub fn untrust_commands(&self) -> Vec<TrustCommand> {
        let fingerprint = fs::read_to_string(self.cert_path()).ok().and_then(|pem| sha1_fingerprint(&pem).ok());
        platform_untrust(fingerprint.as_deref())
    }

    pub fn initialize_local(&self) -> Result<()> {
        match self.state()? {
            CaState::Invalid => {
                return Err(Error::Config("CA files are incomplete or invalid".into()))
            }
            CaState::Ready => return Ok(()),
            CaState::Missing => self.generate()?,
            CaState::Untrusted => {}
        }
        Ok(())
    }

    fn generate(&self) -> Result<()> {
        fs::create_dir_all(&self.dir)?;
        #[cfg(unix)]
        fs::set_permissions(&self.dir, fs::Permissions::from_mode(0o700))?;

        let key = KeyPair::generate_rsa_for(&PKCS_RSA_SHA256, RsaKeySize::_3072)
            .map_err(|error| Error::Config(format!("generate CA key: {error}")))?;
        let mut params = CertificateParams::new(Vec::<String>::new())
            .map_err(|error| Error::Config(format!("create CA parameters: {error}")))?;
        let mut name = DistinguishedName::new();
        name.push(DnType::CommonName, CA_COMMON_NAME);
        name.push(DnType::OrganizationName, "OwO AI Gateway");
        params.distinguished_name = name;
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        // Even if this key leaks, it can only vouch for Cursor's own hosts.
        params.name_constraints = Some(NameConstraints {
            permitted_subtrees: vec![GeneralSubtree::DnsName(PERMITTED_DOMAIN.into())],
            excluded_subtrees: Vec::new(),
        });
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
        ];
        params.not_before = OffsetDateTime::now_utc() - Duration::minutes(5);
        params.not_after = OffsetDateTime::now_utc() + Duration::days(3652);
        let cert = params
            .self_signed(&key)
            .map_err(|error| Error::Config(format!("generate CA certificate: {error}")))?;
        write_atomic(&self.key_path(), key.serialize_pem().as_bytes(), 0o600)?;
        write_atomic(&self.cert_path(), cert.pem().as_bytes(), 0o644)?;
        Ok(())
    }
}

fn parse_issuer(cert: &str, key: &str) -> Result<Issuer<'static, KeyPair>> {
    let key =
        KeyPair::from_pem(key).map_err(|error| Error::Config(format!("parse CA key: {error}")))?;
    let pem = pem::parse(cert).map_err(|error| Error::Config(format!("parse CA PEM: {error}")))?;
    let (_, parsed) = x509_parser::certificate::X509Certificate::from_der(pem.contents())
        .map_err(|error| Error::Config(format!("parse CA X.509 certificate: {error}")))?;
    if parsed.public_key().subject_public_key.data.as_ref() != key.public_key_raw() {
        return Err(Error::Config(
            "CA certificate and private key do not match".into(),
        ));
    }
    if !parsed.validity().is_valid() {
        return Err(Error::Config(
            "CA certificate is outside its validity period".into(),
        ));
    }
    if !parsed
        .basic_constraints()
        .map_err(|error| Error::Config(format!("read CA constraints: {error}")))?
        .is_some_and(|constraints| constraints.value.ca)
    {
        return Err(Error::Config("certificate is not a CA".into()));
    }
    Issuer::from_ca_cert_pem(cert, key)
        .map_err(|error| Error::Config(format!("parse CA certificate: {error}")))
}

fn write_atomic(path: &std::path::Path, data: &[u8], _mode: u32) -> Result<()> {
    let temp = path.with_extension("tmp");
    fs::write(&temp, data)?;
    #[cfg(unix)]
    fs::set_permissions(&temp, fs::Permissions::from_mode(_mode))?;
    fs::rename(&temp, path)?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(_mode))?;
    Ok(())
}

/// SHA-1 of the DER certificate, upper-case hex (the "thumbprint" trust stores show).
fn sha1_fingerprint(cert: &str) -> Result<String> {
    let pem = pem::parse(cert).map_err(|error| Error::Config(format!("parse CA PEM: {error}")))?;
    Ok(hex::encode_upper(Sha1::digest(pem.contents())))
}

#[cfg(target_os = "macos")]
fn is_installed(cert: &str) -> Result<bool> {
    let fingerprint = sha1_fingerprint(cert)?;
    for keychain in ["login.keychain-db", "/Library/Keychains/System.keychain"] {
        let output = Command::new("security")
            .args(["find-certificate", "-a", "-Z", keychain])
            .output()?;
        if output.status.success() && String::from_utf8_lossy(&output.stdout).contains(&fingerprint)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(target_os = "windows")]
fn is_installed(cert: &str) -> Result<bool> {
    windows::is_installed(cert)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn is_installed(cert: &str) -> Result<bool> {
    match fs::read_to_string(linux_anchor_file()) {
        Ok(installed) => Ok(installed.trim() == cert.trim()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const LINUX_ANCHOR_NAME: &str = "owo-cursor-local-ca.crt";

#[cfg(target_os = "windows")]
fn platform_trust(cert: &str) -> Vec<TrustCommand> {
    vec![TrustCommand::new("certutil", &["-addstore", "-f", "Root", cert])]
}

#[cfg(target_os = "windows")]
fn platform_untrust(fingerprint: Option<&str>) -> Vec<TrustCommand> {
    vec![TrustCommand::new("certutil", &["-delstore", "Root", fingerprint.unwrap_or(CA_COMMON_NAME)])]
}

#[cfg(target_os = "macos")]
fn platform_trust(cert: &str) -> Vec<TrustCommand> {
    vec![TrustCommand::new("security", &["add-trusted-cert", "-d", "-r", "trustRoot", "-p", "ssl", "-k", MACOS_SYSTEM_KEYCHAIN, cert])]
}

#[cfg(target_os = "macos")]
fn platform_untrust(fingerprint: Option<&str>) -> Vec<TrustCommand> {
    vec![match fingerprint {
        Some(f) => TrustCommand::new("security", &["delete-certificate", "-Z", f, MACOS_SYSTEM_KEYCHAIN]),
        None => TrustCommand::new("security", &["delete-certificate", "-c", CA_COMMON_NAME, MACOS_SYSTEM_KEYCHAIN]),
    }]
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_trust(cert: &str) -> Vec<TrustCommand> {
    let anchor = linux_anchor_file().to_string_lossy().into_owned();
    vec![TrustCommand::new("cp", &[cert, anchor.as_str()]), linux_refresh()]
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_untrust(_fingerprint: Option<&str>) -> Vec<TrustCommand> {
    let anchor = linux_anchor_file().to_string_lossy().into_owned();
    vec![TrustCommand::new("rm", &["-f", anchor.as_str()]), linux_refresh()]
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn linux_refresh() -> TrustCommand {
    match linux_refresh_command().split_once(' ') {
        Some((program, arg)) => TrustCommand::new(program, &[arg]),
        None => TrustCommand::new(linux_refresh_command(), &[]),
    }
}

/// The distribution's trust-anchor directory: Fedora/RHEL, Arch, else Debian/Ubuntu.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn linux_anchor_file() -> PathBuf {
    if PathBuf::from("/etc/pki/ca-trust/source/anchors").is_dir() {
        PathBuf::from("/etc/pki/ca-trust/source/anchors").join(LINUX_ANCHOR_NAME)
    } else if PathBuf::from("/etc/ca-certificates/trust-source/anchors").is_dir() {
        PathBuf::from("/etc/ca-certificates/trust-source/anchors").join(LINUX_ANCHOR_NAME)
    } else {
        PathBuf::from("/usr/local/share/ca-certificates").join(LINUX_ANCHOR_NAME)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn linux_refresh_command() -> &'static str {
    match linux_anchor_file().parent().and_then(|dir| dir.to_str()) {
        Some("/usr/local/share/ca-certificates") => "update-ca-certificates",
        _ => "update-ca-trust extract",
    }
}
