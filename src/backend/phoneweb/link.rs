//! The encrypted link on the plain http page. A browser only offers its own encryption (crypto.subtle)
//! on https pages, and an https page at a home address always warns, so the phone page on plain http
//! encrypts its WebSocket itself, with the same few lines on both ends (the page's copy is in
//! assets/phone/index.html):
//!
//! - **Secrets that never cross the Wi-Fi.** Pairing from the QR uses a random link key carried after
//!   the `#` of the QR's address (a browser never sends that part). A paired phone uses
//!   SHA-256(its token), which the PC keeps as the phone's token hash; the token itself is never
//!   sent again. Pairing by typing the code uses the code (weaker: see `code_secret`).
//! - **Fresh keys each connection.** The PC says hello with 16 random bytes, the phone answers with
//!   16 of its own and which secret it uses; both make the connection key
//!   HMAC-SHA256(secret, "minniondeck-link-v1" | PC's | phone's), and from it an encryption key and
//!   an authentication key.
//! - **Every message:** "e" + base64url(direction | nonce (12) | ChaCha20(message) | tag (16)), the
//!   tag the first 16 bytes of HMAC-SHA256 over direction, nonce and the encrypted message. Each
//!   message carries a number ("q") that must go up, so a recorded press can't be sent again.
//!
//! What it stops: anyone on the Wi-Fi reading the pairing code, a phone's key, your deck or what you
//! press, or replaying a press. What it can't stop: someone who changes the page itself on its way
//! to the phone (only https or an app can). Button pictures still travel as plain images.
use base64::Engine;
use ring::hmac;
use serde_json::Value;

pub const VERSION: &str = "minniondeck-link-v1";
/// Message from the phone to the PC, and from the PC to the phone
const TO_PC: u8 = 1;
const TO_PHONE: u8 = 2;
const NONCE: usize = 12;
const TAG: usize = 16;

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
}

pub fn encode(bytes: &[u8]) -> String {
    b64().encode(bytes)
}

pub fn decode(text: &str) -> Option<Vec<u8>> {
    b64().decode(text.trim_end_matches('=')).ok()
}

pub fn sha256(bytes: &[u8]) -> Vec<u8> {
    ring::digest::digest(&ring::digest::SHA256, bytes).as_ref().to_vec()
}

fn hmac(key: &[u8], parts: &[&[u8]]) -> Vec<u8> {
    let k = hmac::Key::new(hmac::HMAC_SHA256, key);
    let mut c = hmac::Context::with_key(&k);
    for p in parts {
        c.update(p);
    }
    c.sign().as_ref().to_vec()
}

/// The ChaCha20 stream (RFC 8439) over `data`, from block `counter`.
pub fn chacha20(key: &[u8; 32], nonce: &[u8; 12], counter: u32, data: &mut [u8]) {
    fn qr(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
        s[a] = s[a].wrapping_add(s[b]);
        s[d] = (s[d] ^ s[a]).rotate_left(16);
        s[c] = s[c].wrapping_add(s[d]);
        s[b] = (s[b] ^ s[c]).rotate_left(12);
        s[a] = s[a].wrapping_add(s[b]);
        s[d] = (s[d] ^ s[a]).rotate_left(8);
        s[c] = s[c].wrapping_add(s[d]);
        s[b] = (s[b] ^ s[c]).rotate_left(7);
    }
    let word = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let mut state = [0u32; 16];
    state[..4].copy_from_slice(&[0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574]);
    for i in 0..8 {
        state[4 + i] = word(key, i * 4);
    }
    for i in 0..3 {
        state[13 + i] = word(nonce, i * 4);
    }
    for (n, chunk) in data.chunks_mut(64).enumerate() {
        state[12] = counter.wrapping_add(n as u32);
        let mut x = state;
        for _ in 0..10 {
            qr(&mut x, 0, 4, 8, 12);
            qr(&mut x, 1, 5, 9, 13);
            qr(&mut x, 2, 6, 10, 14);
            qr(&mut x, 3, 7, 11, 15);
            qr(&mut x, 0, 5, 10, 15);
            qr(&mut x, 1, 6, 11, 12);
            qr(&mut x, 2, 7, 8, 13);
            qr(&mut x, 3, 4, 9, 14);
        }
        for (i, byte) in chunk.iter_mut().enumerate() {
            let w = x[i / 4].wrapping_add(state[i / 4]).to_le_bytes();
            *byte ^= w[i % 4];
        }
    }
}

/// The secret for pairing by a typed code (no link key: the address was typed, not scanned). A code
/// has only a million values, so someone who recorded that pairing could find it; the QR's link
/// key is what makes pairing private.
pub fn code_secret(code: &str) -> Vec<u8> {
    sha256(format!("minniondeck-code:{code}").as_bytes())
}

/// A paired phone's name for its secret, sent in the open so the PC knows which one it uses: the
/// start of SHA-256(secret), in hex.
pub fn kid(secret: &[u8]) -> String {
    sha256(secret)[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// One connection's keys and message numbers.
pub struct Seal {
    enc: [u8; 32],
    mac: Vec<u8>,
    sent: u64,
    got: u64,
}

impl Seal {
    /// `ours` and `theirs` are the PC's and the phone's 16 random bytes.
    pub fn new(secret: &[u8], pc: &[u8], phone: &[u8]) -> Seal {
        let conn = hmac(secret, &[VERSION.as_bytes(), pc, phone]);
        let mut enc = [0u8; 32];
        enc.copy_from_slice(&hmac(&conn, &[b"enc"]));
        Seal { enc, mac: hmac(&conn, &[b"mac"]), sent: 0, got: 0 }
    }

    fn tag(&self, head: &[u8], body: &[u8]) -> Vec<u8> {
        hmac(&self.mac, &[head, body])[..TAG].to_vec()
    }

    fn seal_dir(&mut self, mut v: Value, dir: u8) -> String {
        self.sent += 1;
        v["q"] = Value::from(self.sent);
        let mut body = v.to_string().into_bytes();
        let mut nonce = [0u8; NONCE];
        let _ = ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut nonce);
        chacha20(&self.enc, &nonce, 1, &mut body);
        let mut head = vec![dir];
        head.extend_from_slice(&nonce);
        let tag = self.tag(&head, &body);
        let mut out = head;
        out.extend_from_slice(&body);
        out.extend_from_slice(&tag);
        format!("e{}", encode(&out))
    }

    /// A message for the phone.
    pub fn seal(&mut self, v: Value) -> String {
        self.seal_dir(v, TO_PHONE)
    }

    /// A message from the phone: None when it wasn't sealed with this connection's keys, was meant
    /// the other way, or was sent before (its number didn't go up).
    pub fn open(&mut self, frame: &str) -> Option<Value> {
        self.open_dir(frame, TO_PC)
    }

    fn open_dir(&mut self, frame: &str, dir: u8) -> Option<Value> {
        let raw = decode(frame.strip_prefix('e')?)?;
        if raw.len() < 1 + NONCE + TAG || raw[0] != dir {
            return None;
        }
        let (head, rest) = raw.split_at(1 + NONCE);
        let (body, tag) = rest.split_at(rest.len() - TAG);
        // compared in full whatever the first wrong byte is
        let want = self.tag(head, body);
        if want.iter().zip(tag).fold(0u8, |a, (x, y)| a | (x ^ y)) != 0 {
            return None;
        }
        let mut nonce = [0u8; NONCE];
        nonce.copy_from_slice(&head[1..]);
        let mut plain = body.to_vec();
        chacha20(&self.enc, &nonce, 1, &mut plain);
        let v: Value = serde_json::from_slice(&plain).ok()?;
        let q = v.get("q").and_then(|q| q.as_u64())?;
        if q <= self.got {
            return None;
        }
        self.got = q;
        Some(v)
    }

    /// The phone's side, for tests (the page's copy is JavaScript).
    #[cfg(test)]
    pub fn phone_seal(&mut self, v: Value) -> String {
        self.seal_dir(v, TO_PC)
    }

    #[cfg(test)]
    pub fn phone_open(&mut self, frame: &str) -> Option<Value> {
        self.open_dir(frame, TO_PHONE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    // RFC 8439, 2.4.2: the page's JavaScript is checked against the same numbers
    #[test]
    fn chacha20_matches_rfc_8439() {
        let key: [u8; 32] = hex("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f").try_into().unwrap();
        let nonce: [u8; 12] = hex("000000000000004a00000000").try_into().unwrap();
        let mut text = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.".to_vec();
        chacha20(&key, &nonce, 1, &mut text);
        assert_eq!(
            text,
            hex("6e2e359a2568f98041ba0728dd0d6981e97e7aec1d4360c20a27afccfd9fae0bf91b65c5524733ab8f593dabcd62b3571639d624e65152ab8f530c359f0861d807ca0dbf500d6a6156a38e088a22b65e52bc514d16ccf806818ce91ab77937365af90bbf74a35be6b40b8eedf2785e42874d")
        );
    }

    #[test]
    fn a_sealed_message_opens_once_only_with_the_same_keys() {
        let (pc, phone_bytes) = ([1u8; 16], [2u8; 16]);
        let mut pc_side = Seal::new(b"secret", &pc, &phone_bytes);
        let mut phone = Seal::new(b"secret", &pc, &phone_bytes);
        // the phone presses; the PC reads it
        let f = phone.phone_seal(json!({ "t": "press", "button": "t-1" }));
        assert!(!f.contains("press") && !f.contains("t-1"), "nothing readable: {f}");
        assert_eq!(pc_side.open(&f).unwrap()["button"], "t-1");
        // the same frame again (a replay) is refused, and so is one with a changed byte
        assert!(pc_side.open(&f).is_none());
        let f2 = phone.phone_seal(json!({ "t": "press", "button": "t-2" }));
        let mut bad = decode(&f2[1..]).unwrap();
        bad[20] ^= 1;
        assert!(pc_side.open(&format!("e{}", encode(&bad))).is_none());
        assert_eq!(pc_side.open(&f2).unwrap()["button"], "t-2");
        // a message the PC sent can't be sent back to it as the phone's
        let to_phone = pc_side.seal(json!({ "t": "result" }));
        assert!(pc_side.open(&to_phone).is_none());
        assert_eq!(phone.phone_open(&to_phone).unwrap()["t"], "result");
        // another secret, or another connection's bytes: nothing opens
        let mut other = Seal::new(b"other", &pc, &phone_bytes);
        assert!(other.open(&phone.phone_seal(json!({ "t": "x" }))).is_none());
        let mut later = Seal::new(b"secret", &[9u8; 16], &phone_bytes);
        assert!(later.open(&phone.phone_seal(json!({ "t": "x" }))).is_none());
    }

    // fixed values the page's JavaScript must reproduce (checked by running the page's code in node)
    #[test]
    fn known_values_for_the_pages_copy() {
        let s = Seal::new(b"secret", &[1u8; 16], &[2u8; 16]);
        let h = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        println!("KNOWN enc={} mac={} kid={} code={}", h(&s.enc), h(&s.mac), kid(&sha256(b"token")), h(&code_secret("123456")));
        assert_eq!(h(&s.enc), "d8ea524fb2c4ebf5045534b0f6393f0f402b28f5adc71cb0946b89199a414a48");
        assert_eq!(kid(&sha256(b"token")), "66ae6d7bdd68a023");
        // a frame for the page's code to open, and (when MD_PHONEFRAME is set) one the page sealed
        let mut pc = Seal::new(b"secret", &[1u8; 16], &[2u8; 16]);
        println!("PCFRAME {}", pc.seal(json!({ "t": "result", "message": "héllo ✓" })));
        if let Ok(f) = std::env::var("MD_PHONEFRAME") {
            assert_eq!(pc.open(&f).expect("the page's frame opens")["button"], "t-1");
        }
    }
}
