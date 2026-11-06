//! The certificate the phone page is served with: made by this PC, kept in <data>\phones\, and made
//! again when this PC's address changes. A browser can't check a certificate nobody signed, so the
//! phone shows "not private" the first time: choose Advanced, then Continue. It only needs doing
//! once per phone and address.
use std::fs;
use std::path::Path;
use std::sync::Arc;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::ServerConfig;

use crate::backend::package::Kind;
use crate::backend::util::write_atomic;
use crate::backend::vault;

/// The names and addresses the certificate is for.
fn names(addrs: &[String]) -> Vec<String> {
    let mut n: Vec<String> = vec!["localhost".into()];
    n.extend(addrs.iter().cloned());
    n
}

/// Loads the certificate kept for these addresses, or makes a new one.
pub fn load_or_make(dir: &Path, addrs: &[String]) -> Result<Arc<ServerConfig>, String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    // the certificate is public; its private key is sealed
    let (cert_file, key_file, names_file) = (dir.join("cert.der"), dir.join("key.mdsave"), dir.join("cert-names.txt"));
    let _ = vault::migrate(&dir.join("key.der"), &key_file, Kind::Save);
    let want = names(addrs).join(",");
    let have = fs::read_to_string(&names_file).unwrap_or_default();
    // a certificate made for other addresses is made again (the phone asks once more)
    let (cert, key) = match (have == want, fs::read(&cert_file), vault::read(&key_file, Kind::Save)) {
        (true, Ok(c), Ok(k)) => (c, k),
        _ => {
            let (c, k) = make(addrs)?;
            write_atomic(&cert_file, &c)?;
            vault::write(&key_file, Kind::Save, &k)?;
            write_atomic(&names_file, want.as_bytes())?;
            (c, k)
        }
    };
    config(cert, key)
}

fn make(addrs: &[String]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let mut params = rcgen::CertificateParams::new(names(addrs)).map_err(|e| format!("Could not make the phone certificate: {e}"))?;
    let mut dn = rcgen::DistinguishedName::new();
    dn.push(rcgen::DnType::CommonName, "Minniondeck");
    params.distinguished_name = dn;
    // valid for ten years: a phone that accepted it once keeps accepting it
    params.not_before = rcgen::date_time_ymd(2024, 1, 1);
    params.not_after = rcgen::date_time_ymd(2044, 1, 1);
    // iPhones want a TLS server certificate to say it is for servers
    params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
    let key = rcgen::KeyPair::generate().map_err(|e| format!("Could not make the phone certificate: {e}"))?;
    let cert = params.self_signed(&key).map_err(|e| format!("Could not make the phone certificate: {e}"))?;
    Ok((cert.der().to_vec(), key.serialize_der()))
}

fn config(cert: Vec<u8>, key: Vec<u8>) -> Result<Arc<ServerConfig>, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cfg = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_no_client_auth()
        .with_single_cert(vec![CertificateDer::from(cert)], PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)))
        .map_err(|e| format!("The phone certificate can't be used: {e}"))?;
    Ok(Arc::new(cfg))
}
