// This PC's identity for phones: a self-signed TLS certificate made once and kept in
// %APPDATA%\com.minnion.deck\identity\. Phones pin its SHA-256 fingerprint from the
// pairing QR code, so no one else on the network can pose as this PC.
//
// The private key is a plain file in the app's data folder, like the rest of its data. If it
// is lost the PC makes a new one, and phones must be paired again.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

use crate::storage::{sha256_hex, write_atomic};

pub struct Identity {
    pub cert_der: Vec<u8>,
    key_der: Vec<u8>,
    /// SHA-256 of the certificate, lowercase hex: what goes in the QR code
    pub fingerprint: String,
}

impl Identity {
    pub fn load_or_create(dir: &Path) -> Result<Self, String> {
        fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
        let (cert_path, key_path) = (dir.join("cert.der"), dir.join("key.der"));
        if let (Ok(cert_der), Ok(key_der)) = (fs::read(&cert_path), fs::read(&key_path)) {
            let id = Identity { fingerprint: sha256_hex(&cert_der), cert_der, key_der };
            if id.server_config().is_ok() {
                return Ok(id);
            }
        }
        let made = rcgen::generate_simple_self_signed(vec!["minniondeck".to_string()]).map_err(|e| format!("Could not make this PC's certificate: {e}"))?;
        let (cert_der, key_der) = (made.cert.der().to_vec(), made.key_pair.serialize_der());
        write_atomic(&key_path, &key_der)?;
        write_atomic(&cert_path, &cert_der)?;
        Ok(Identity { fingerprint: sha256_hex(&cert_der), cert_der, key_der })
    }

    pub fn server_config(&self) -> Result<Arc<ServerConfig>, String> {
        let config = ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(self.cert_der.clone())],
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key_der.clone())),
            )
            .map_err(|e| format!("This PC's certificate could not be used: {e}"))?;
        Ok(Arc::new(config))
    }
}

#[cfg(test)]
mod tests {
    use super::Identity;

    #[test]
    fn the_identity_is_made_once_and_kept() {
        let dir = std::env::temp_dir().join(format!("minniondeck-test-id-{}", crate::storage::now_ms()));
        let first = Identity::load_or_create(&dir).unwrap();
        let again = Identity::load_or_create(&dir).unwrap();
        assert_eq!(first.fingerprint, again.fingerprint);
        assert_eq!(first.fingerprint.len(), 64);
        let _ = std::fs::remove_dir_all(dir);
    }
}
