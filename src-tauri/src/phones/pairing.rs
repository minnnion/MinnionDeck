// Pairing codes and the QR code that carries them.
//
// The QR holds a link the phone app opens:
//   minniondeck://pair?v=1&h=192.168.1.20,10.0.0.5&p=47800&c=123456&f=<fingerprint>&n=<PC name>
// h: this PC's addresses on the local network, p: port, c: the one-time code,
// f: SHA-256 of this PC's certificate (pinned by the phone), n: the PC's name.

use std::net::{IpAddr, Ipv4Addr};

use aes_gcm::aead::OsRng;
use aes_gcm::aead::rand_core::RngCore;
use serde::Serialize;

/// A code lasts this long; the Phones page says "expires in 5 minutes"
pub const CODE_LIFETIME_MS: u64 = 5 * 60 * 1000;
/// Wrong codes allowed before the code stops working
pub const MAX_TRIES: u8 = 5;

/// Six random digits. Rejection sampling keeps every code equally likely.
pub fn new_code() -> String {
    loop {
        let n = OsRng.next_u32();
        if n < u32::MAX - (u32::MAX % 1_000_000) {
            return format!("{:06}", n % 1_000_000);
        }
    }
}

/// The digits of what the phone sent, so "123 456" and "123456" both work
pub fn digits(code: &str) -> String {
    code.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// A secret for a newly paired phone, and what the PC keeps of it (only its hash)
pub fn new_token() -> String {
    use base64::Engine;
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub fn new_id() -> String {
    let mut bytes = [0u8; 8];
    OsRng.fill_bytes(&mut bytes);
    format!("ph-{}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

fn is_lan(ip: &Ipv4Addr) -> bool {
    ip.is_private() && !ip.is_loopback() && !ip.is_link_local()
}

/// This PC's IPv4 addresses on local networks, home Wi-Fi style (192.168.x.x) first
pub fn lan_addresses() -> Vec<String> {
    let mut found: Vec<Ipv4Addr> = local_ip_address::list_afinet_netifas()
        .map(|list| list.into_iter().filter_map(|(_, ip)| match ip { IpAddr::V4(v4) if is_lan(&v4) => Some(v4), _ => None }).collect())
        .unwrap_or_default();
    found.sort_by_key(|ip| (ip.octets()[0] != 192, *ip));
    found.dedup();
    found.into_iter().map(|ip| ip.to_string()).collect()
}

pub fn pc_name() -> String {
    std::env::var("COMPUTERNAME").ok().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| "This PC".into())
}

fn encode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

pub fn link(addresses: &[String], port: u16, code: &str, fingerprint: &str, pc_name: &str) -> String {
    format!("minniondeck://pair?v=1&h={}&p={port}&c={code}&f={fingerprint}&n={}", addresses.join(","), encode(pc_name))
}

/// The QR code as rows of "1" (dark) and "0" (light), for the UI to draw in its own style
#[derive(Serialize, Clone)]
pub struct Qr {
    pub size: usize,
    pub modules: String,
}

pub fn qr(text: &str) -> Result<Qr, String> {
    let code = qrcode::QrCode::with_error_correction_level(text.as_bytes(), qrcode::EcLevel::M).map_err(|e| format!("Could not make the QR code: {e}"))?;
    let modules = code.to_colors().iter().map(|c| if *c == qrcode::Color::Dark { '1' } else { '0' }).collect();
    Ok(Qr { size: code.width(), modules })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_six_digits_and_typing_spaces_is_fine() {
        for _ in 0..200 {
            let c = new_code();
            assert_eq!(c.len(), 6);
            assert!(c.chars().all(|ch| ch.is_ascii_digit()));
        }
        assert_eq!(digits("123 456"), "123456");
    }

    #[test]
    fn the_pairing_link_fits_in_a_qr_code() {
        let link = link(&["192.168.1.20".into()], 47800, "123456", &"a".repeat(64), "DESK TOP");
        assert!(link.starts_with("minniondeck://pair?v=1&h=192.168.1.20&p=47800&c=123456&f="));
        assert!(link.ends_with("&n=DESK%20TOP"));
        let q = qr(&link).unwrap();
        assert_eq!(q.modules.len(), q.size * q.size);
    }

    #[test]
    fn only_local_network_addresses_are_offered() {
        assert!(is_lan(&"192.168.1.5".parse().unwrap()));
        assert!(is_lan(&"10.0.0.2".parse().unwrap()));
        assert!(!is_lan(&"127.0.0.1".parse().unwrap()));
        assert!(!is_lan(&"169.254.1.1".parse().unwrap()));
        assert!(!is_lan(&"8.8.8.8".parse().unwrap()));
    }
}
