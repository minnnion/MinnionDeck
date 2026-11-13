//! The certificates of the phone page, made by this PC and kept in <data>\phones\.
//!
//! No public authority can vouch for a page at a home address like 192.168.1.20, so this PC is its
//! own small authority:
//!
//! - **This PC's certificate** (`ca.der`, its key sealed in `ca-key.mdsave`): made once, and the one
//!   to install on a phone (the phone page offers it as /minniondeck.cer, with the steps). It is
//!   limited to home-network addresses (10.x, 172.16–31.x, 192.168.x, 127.x and localhost), so a
//!   phone that trusts it trusts nothing else with it: not a bank, not any website.
//! - **The page's certificate** (`cert.der`, key in `key.mdsave`): signed by it, for this PC's
//!   addresses, valid 397 days (what phones accept), made again when the addresses change or it
//!   nears its end. A phone that installed this PC's certificate never warns, also after that.
//!
//! A phone that didn't install it warns about the page ("not private") and can go on anyway.
use std::fs;
use std::path::Path;
use std::sync::Arc;

use rcgen::{BasicConstraints, CertificateParams, CidrSubnet, DistinguishedName, DnType, ExtendedKeyUsagePurpose, GeneralSubtree, IsCa, KeyPair, KeyUsagePurpose, NameConstraints};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

use crate::backend::package::Kind;
use crate::backend::util::{now_ms, sha256_hex, write_atomic};
use crate::backend::vault;

/// How long the page's certificate is good for: phones refuse longer ones from an installed authority
const LEAF_DAYS: u64 = 397;
/// It is made again this long before it runs out
const RENEW_DAYS: u64 = 30;
const DAY_MS: u64 = 86_400_000;

/// The names and addresses the page's certificate is for.
fn names(addrs: &[String]) -> Vec<String> {
    let mut n: Vec<String> = vec!["localhost".into()];
    n.extend(addrs.iter().cloned());
    n
}

fn fail(e: impl std::fmt::Display) -> String {
    format!("Could not make the phone certificate: {e}")
}

/// The calendar day of unix milliseconds (UTC): (year, month, day)
fn ymd(ms: u64) -> (i32, u8, u8) {
    let s = crate::backend::backups::stamp(ms);
    let n = |a: usize, b: usize| s[a..b].parse::<i32>().unwrap_or(1);
    (n(0, 4), n(5, 7) as u8, n(8, 10) as u8)
}

/// This PC's authority, as made: what it may vouch for, its name, its use.
fn ca_params() -> CertificateParams {
    let mut p = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "MinnionDeck on this PC");
    dn.push(DnType::OrganizationName, "MinnionDeck");
    p.distinguished_name = dn;
    p.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    p.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign, KeyUsagePurpose::DigitalSignature];
    // home-network addresses only: trusting it can never make another website look genuine
    p.name_constraints = Some(NameConstraints {
        permitted_subtrees: vec![
            GeneralSubtree::IpAddress(CidrSubnet::from_v4_prefix([10, 0, 0, 0], 8)),
            GeneralSubtree::IpAddress(CidrSubnet::from_v4_prefix([172, 16, 0, 0], 12)),
            GeneralSubtree::IpAddress(CidrSubnet::from_v4_prefix([192, 168, 0, 0], 16)),
            GeneralSubtree::IpAddress(CidrSubnet::from_v4_prefix([127, 0, 0, 0], 8)),
            GeneralSubtree::DnsName("localhost".into()),
        ],
        excluded_subtrees: vec![],
    });
    p.not_before = rcgen::date_time_ymd(2024, 1, 1);
    p.not_after = rcgen::date_time_ymd(2044, 1, 1);
    p
}

/// This PC's authority: its certificate (DER) and key, made the first time.
fn authority(dir: &Path) -> Result<(Vec<u8>, KeyPair), String> {
    let (cert_file, key_file) = (dir.join("ca.der"), dir.join("ca-key.mdsave"));
    let kept = fs::read(&cert_file).ok().zip(vault::read(&key_file, Kind::Save).ok().and_then(|k| KeyPair::try_from(k.as_slice()).ok()));
    if let Some((c, key)) = kept {
        return Ok((c, key));
    }
    let key = KeyPair::generate().map_err(fail)?;
    let cert = ca_params().self_signed(&key).map_err(fail)?;
    vault::write(&key_file, Kind::Save, &key.serialize_der())?;
    write_atomic(&cert_file, cert.der())?;
    Ok((cert.der().to_vec(), key))
}

/// The page's certificate for these names, signed by the authority: (certificate, its key), both DER.
fn make_leaf(names: Vec<String>, ca_key: &KeyPair, now: u64) -> Result<(Vec<u8>, Vec<u8>), String> {
    // the authority is made again from its fixed name and key: that is all signing needs
    let ca = ca_params().self_signed(ca_key).map_err(fail)?;
    let mut params = CertificateParams::new(names).map_err(fail)?;
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "MinnionDeck phone page");
    params.distinguished_name = dn;
    let ((y0, m0, d0), (y1, m1, d1)) = (ymd(now.saturating_sub(DAY_MS)), ymd(now + LEAF_DAYS * DAY_MS));
    params.not_before = rcgen::date_time_ymd(y0, m0, d0);
    params.not_after = rcgen::date_time_ymd(y1, m1, d1);
    // iPhones want a TLS server certificate to say it is for servers
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.use_authority_key_identifier_extension = true;
    let key = KeyPair::generate().map_err(fail)?;
    let cert = params.signed_by(&key, &ca, ca_key).map_err(fail)?;
    Ok((cert.der().to_vec(), key.serialize_der()))
}

/// Loads the page's certificate kept for these addresses, or makes a new one (also when it nears
/// its end, or was made before this PC had its own authority).
pub fn load_or_make(dir: &Path, addrs: &[String]) -> Result<Arc<ServerConfig>, String> {
    load_or_make_at(dir, addrs, now_ms())
}

pub(crate) fn load_or_make_at(dir: &Path, addrs: &[String], now: u64) -> Result<Arc<ServerConfig>, String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let (ca_der, ca_key) = authority(dir)?;
    // the certificate is public; its private key is sealed
    let (cert_file, key_file, about_file) = (dir.join("cert.der"), dir.join("key.mdsave"), dir.join("cert-names.txt"));
    let _ = vault::migrate(&dir.join("key.der"), &key_file, Kind::Save);
    let want = format!("{}|ca:{}", names(addrs).join(","), &sha256_hex(&ca_der)[..16]);
    // what the kept one was made for: "<names>|ca:<authority>" and when it runs out
    let about = fs::read_to_string(&about_file).unwrap_or_default();
    let mut lines = about.lines();
    let (made_for, until) = (lines.next().unwrap_or(""), lines.next().and_then(|u| u.parse::<u64>().ok()).unwrap_or(0));
    let fresh = made_for == want && until > now + RENEW_DAYS * DAY_MS;
    let (cert, key) = match (fresh, fs::read(&cert_file), vault::read(&key_file, Kind::Save)) {
        (true, Ok(c), Ok(k)) => (c, k),
        _ => {
            let (c, k) = make_leaf(names(addrs), &ca_key, now)?;
            write_atomic(&cert_file, &c)?;
            vault::write(&key_file, Kind::Save, &k)?;
            write_atomic(&about_file, format!("{want}\n{}", now + LEAF_DAYS * DAY_MS).as_bytes())?;
            (c, k)
        }
    };
    config(cert, ca_der, key)
}

/// This PC's certificate, to install on a phone (DER), and its fingerprint as phones show it.
pub fn authority_der(dir: &Path) -> Option<Vec<u8>> {
    fs::read(dir.join("ca.der")).ok()
}

/// "AB12 CD34 …": the start of the certificate's SHA-256, to compare with what the phone shows.
pub fn fingerprint(der: &[u8]) -> String {
    let hex = sha256_hex(der).to_ascii_uppercase();
    hex.as_bytes()[..24].chunks(4).map(|c| String::from_utf8_lossy(c).into_owned()).collect::<Vec<_>>().join(" ")
}

fn config(cert: Vec<u8>, ca: Vec<u8>, key: Vec<u8>) -> Result<Arc<ServerConfig>, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let cfg = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_no_client_auth()
        // the page's certificate, and the authority's, so a phone sees the whole chain
        .with_single_cert(vec![CertificateDer::from(cert), CertificateDer::from(ca)], PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)))
        .map_err(|e| format!("The phone certificate can't be used: {e}"))?;
    Ok(Arc::new(cfg))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::client::WebPkiServerVerifier;
    use rustls::client::danger::ServerCertVerifier;
    use rustls::pki_types::{ServerName, UnixTime};

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("md-cert-{tag}-{}", now_ms()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// Whether a phone that installed `ca` accepts `leaf` for `name`.
    fn phone_accepts(ca: &[u8], leaf: &[u8], name: &str) -> bool {
        let mut roots = rustls::RootCertStore::empty();
        roots.add(CertificateDer::from(ca.to_vec())).unwrap();
        let v = WebPkiServerVerifier::builder_with_provider(Arc::new(roots), Arc::new(rustls::crypto::ring::default_provider())).build().unwrap();
        v.verify_server_cert(&CertificateDer::from(leaf.to_vec()), &[], &ServerName::try_from(name.to_string()).unwrap(), &[], UnixTime::now()).is_ok()
    }

    // installed once on a phone, this PC's certificate makes the page trusted, at any of its addresses
    #[test]
    fn a_phone_that_installed_this_pcs_certificate_trusts_the_page() {
        let d = dir("trust");
        load_or_make(&d, &["192.168.1.20".into()]).unwrap();
        let (ca, leaf) = (fs::read(d.join("ca.der")).unwrap(), fs::read(d.join("cert.der")).unwrap());
        assert!(phone_accepts(&ca, &leaf, "192.168.1.20"));
        assert!(phone_accepts(&ca, &leaf, "localhost"));
        assert!(!phone_accepts(&ca, &leaf, "192.168.1.21"), "only the PC's own address");
        // a new address: a new page certificate from the same authority, still trusted
        load_or_make(&d, &["10.0.0.7".into()]).unwrap();
        assert_eq!(fs::read(d.join("ca.der")).unwrap(), ca, "the authority stays the same");
        let leaf2 = fs::read(d.join("cert.der")).unwrap();
        assert_ne!(leaf2, leaf);
        assert!(phone_accepts(&ca, &leaf2, "10.0.0.7"));
        // the same addresses again: the kept one is used
        load_or_make(&d, &["10.0.0.7".into()]).unwrap();
        assert_eq!(fs::read(d.join("cert.der")).unwrap(), leaf2);
        let _ = fs::remove_dir_all(&d);
    }

    // trusting it can never make a real website look genuine: the authority is limited to home addresses
    #[test]
    fn this_pcs_certificate_vouches_for_nothing_outside_the_home_network() {
        let d = dir("limits");
        let (ca, key) = authority(&d).unwrap();
        for public in ["8.8.8.8", "bank.example.com"] {
            let (leaf, _) = make_leaf(vec![public.into()], &key, now_ms()).unwrap();
            assert!(!phone_accepts(&ca, &leaf, public), "{public} was accepted");
        }
        let (leaf, _) = make_leaf(vec!["172.20.1.5".into()], &key, now_ms()).unwrap();
        assert!(phone_accepts(&ca, &leaf, "172.20.1.5"));
        assert_eq!(fingerprint(&ca).len(), 29);
        let _ = fs::remove_dir_all(&d);
    }

    // the page's certificate is renewed before it runs out, and a certificate from before this
    // version (no authority) is replaced
    #[test]
    fn the_pages_certificate_is_renewed_in_time() {
        let d = dir("renew");
        let addrs = vec!["192.168.0.9".to_string()];
        load_or_make_at(&d, &addrs, now_ms()).unwrap();
        let first = fs::read(d.join("cert.der")).unwrap();
        load_or_make_at(&d, &addrs, now_ms() + 100 * DAY_MS).unwrap();
        assert_eq!(fs::read(d.join("cert.der")).unwrap(), first, "still good");
        load_or_make_at(&d, &addrs, now_ms() + (LEAF_DAYS - 10) * DAY_MS).unwrap();
        assert_ne!(fs::read(d.join("cert.der")).unwrap(), first, "near its end: made again");
        // as an earlier version left it: names only
        fs::write(d.join("cert-names.txt"), "localhost,192.168.0.9").unwrap();
        let before = fs::read(d.join("cert.der")).unwrap();
        load_or_make(&d, &addrs).unwrap();
        assert_ne!(fs::read(d.join("cert.der")).unwrap(), before);
        let _ = fs::remove_dir_all(&d);
    }
}
