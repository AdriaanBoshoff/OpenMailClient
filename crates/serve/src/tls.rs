//! The server's own certificate (M7.1): made on first run, kept beside the settings where only
//! this user can read it, and named by its SHA-256 fingerprint, which `openmailclient-cli web
//! status` prints so the owner can check, once, that the browser is talking to their own machine
//! and not to someone in between. A home has no domain and no public certificate authority will
//! sign for an IP address, so the browser warns the first time; the fingerprint is what makes
//! that warning answerable.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The file the certificate and its key are kept in.
pub fn certificate_file(dir: &Path) -> PathBuf {
    dir.join("web-certificate.json")
}

/// The most names a certificate is made for.
pub const MAX_NAMES: usize = 32;

/// The certificate kept: the names it was made for, its DER and its key's PKCS #8 DER.
pub struct Kept {
    pub names: Vec<String>,
    pub made_ms: u64,
    pub certificate: Vec<u8>,
    key: Vec<u8>,
}

impl Kept {
    /// The SHA-256 of the certificate, as browsers show it: pairs of hex digits between colons.
    pub fn fingerprint(&self) -> String {
        let digest = ring::digest::digest(&ring::digest::SHA256, &self.certificate);
        digest.as_ref().iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":")
    }

    /// The certificate as PEM, to be put in a browser's or a phone's trusted list by hand.
    pub fn pem(&self) -> String {
        let text = STANDARD.encode(&self.certificate);
        let mut out = String::from("-----BEGIN CERTIFICATE-----\n");
        for chunk in text.as_bytes().chunks(64) {
            out.push_str(&String::from_utf8_lossy(chunk));
            out.push('\n');
        }
        out.push_str("-----END CERTIFICATE-----\n");
        out
    }

    /// Whether it was made for every one of `names`.
    pub fn covers(&self, names: &[String]) -> bool {
        names.iter().all(|n| self.names.iter().any(|m| m.eq_ignore_ascii_case(n)))
    }

    /// What the TLS server is set up from: TLS 1.2 and 1.3 only, with rustls's safe suites.
    pub fn server_config(&self) -> Result<Arc<rustls::ServerConfig>, String> {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
            .map_err(|e| format!("TLS: {e}"))?
            .with_no_client_auth()
            .with_single_cert(vec![CertificateDer::from(self.certificate.clone())], PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key.clone())))
            .map_err(|e| format!("the web certificate cannot be used: {e}"))?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Arc::new(config))
    }
}

/// Make a certificate for `names` (host names and IP addresses).
pub fn make(names: &[String], now_ms: u64) -> Result<Kept, String> {
    let mut names: Vec<String> = names.iter().map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect();
    names.dedup();
    if names.is_empty() || names.len() > MAX_NAMES {
        return Err(format!("a certificate is made for 1 to {MAX_NAMES} names"));
    }
    let mut params = rcgen::CertificateParams::new(names.clone()).map_err(|e| format!("a certificate for those names: {e}"))?;
    let mut dn = rcgen::DistinguishedName::new();
    dn.push(rcgen::DnType::CommonName, "OpenMailClient web access");
    params.distinguished_name = dn;
    // Phones refuse a server certificate that lasts longer than 825 days even when trusted by
    // hand, and want it to say it is for a server; it is renewed before then.
    let today = now_ms / DAY_MS;
    let date = |(y, m, d): (i32, u8, u8)| rcgen::date_time_ymd(y, m, d);
    params.not_before = date(crate::civil(today.saturating_sub(1)));
    params.not_after = date(crate::civil(today.saturating_add(LASTS_DAYS)));
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    let key = rcgen::KeyPair::generate().map_err(|e| format!("a key for the certificate: {e}"))?;
    let cert = params.self_signed(&key).map_err(|e| format!("the certificate: {e}"))?;
    Ok(Kept { names, made_ms: now_ms, certificate: cert.der().to_vec(), key: key.serialize_der() })
}

const DAY_MS: u64 = 86_400_000;
/// How long a certificate lasts.
pub const LASTS_DAYS: u64 = 800;
/// How long before it ends a new one is made.
const RENEW_BEFORE_DAYS: u64 = 30;

/// The certificate kept beside the settings, or `None` when none has been made.
pub fn load(dir: &Path) -> Result<Option<Kept>, String> {
    let Some(text) = crate::read_small(&certificate_file(dir))? else { return Ok(None) };
    let bad = || format!("{}: not a certificate of ours; delete it and one is made again", certificate_file(dir).display());
    let value: Value = serde_json::from_str(&text).map_err(|_| bad())?;
    let bytes = |key: &str| value.get(key).and_then(Value::as_str).and_then(|t| STANDARD.decode(t).ok()).filter(|b| !b.is_empty());
    let (Some(certificate), Some(key)) = (bytes("certificate"), bytes("key")) else { return Err(bad()) };
    let names = value
        .get("names")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).take(MAX_NAMES).map(str::to_string).collect())
        .unwrap_or_default();
    Ok(Some(Kept { names, made_ms: value.get("made").and_then(Value::as_u64).unwrap_or(0), certificate, key }))
}

/// Keep `kept` beside the settings, readable by this user only.
pub fn save(dir: &Path, kept: &Kept) -> Result<(), String> {
    let value = json!({
        "about": "OpenMailClient web access: the server's own certificate and its private key. Keep this file private; delete it to have a new one made.",
        "names": kept.names,
        "made": kept.made_ms,
        "certificate": STANDARD.encode(&kept.certificate),
        "key": STANDARD.encode(&kept.key),
    });
    crate::write_private(&certificate_file(dir), &value.to_string())
}

/// The certificate kept, or a new one when there is none or it was not made for every name in
/// `names` (`renew` makes one anyway); the second is whether it is new.
pub fn ensure(dir: &Path, names: &[String], renew: bool, now_ms: u64) -> Result<(Kept, bool), String> {
    let fresh = |k: &Kept| now_ms / DAY_MS < (k.made_ms / DAY_MS).saturating_add(LASTS_DAYS - RENEW_BEFORE_DAYS);
    if !renew
        && let Some(kept) = load(dir)?
        && kept.covers(names)
        && fresh(&kept)
    {
        return Ok((kept, false));
    }
    let kept = make(names, now_ms)?;
    save(dir, &kept)?;
    Ok((kept, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_certificate_is_made_kept_and_used() {
        let dir = std::env::temp_dir().join(format!("omc-tls-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let names = vec!["localhost".to_string(), "127.0.0.1".to_string(), "192.168.1.20".to_string()];
        let (kept, new) = ensure(&dir, &names, false, 1_800_000_000_000).unwrap();
        assert!(new);
        assert_eq!(kept.fingerprint().len(), 32 * 3 - 1);
        assert!(kept.pem().starts_with("-----BEGIN CERTIFICATE-----\n"));
        kept.server_config().unwrap();
        let (again, new) = ensure(&dir, &names[..2], false, 1_800_000_000_000 + 86_400_000).unwrap();
        assert!(!new);
        assert_eq!(again.fingerprint(), kept.fingerprint());
        let (other, new) = ensure(&dir, &["mail.home.arpa".to_string()], false, 1_800_000_000_000).unwrap();
        assert!(new);
        assert_ne!(other.fingerprint(), kept.fingerprint());
        let (_, new) = ensure(&dir, &["mail.home.arpa".to_string()], false, 1_800_000_000_000 + 780 * 86_400_000).unwrap();
        assert!(new, "renewed before it ends");
        assert_eq!(crate::civil(0), (1970, 1, 1));
        assert_eq!(crate::civil(20_454), (2026, 1, 1));
        assert_eq!(crate::civil(11_016), (2000, 2, 29));
        std::fs::write(certificate_file(&dir), "{\"certificate\":\"!!\"}").unwrap();
        assert!(load(&dir).is_err());
        assert!(make(&[], 0).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
