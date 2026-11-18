//! Identidad del nodo: par de claves Noise (X25519) para el handshake y par
//! Ed25519 para firmar paquetes.
//!
//! El peer ID de la malla son los **primeros 8 bytes de SHA-256 de la clave
//! pública Noise**, en hex minúsculas de 16 caracteres. La derivación es la
//! identidad canónica: ni un anuncio autenticado ni un handshake completado
//! bastan sin que el ID reclamado coincida con esta función.

use crate::proto::PeerId;
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use rand::RngCore;
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

/// Identidad local (secreta) del nodo.
#[derive(Clone)]
pub struct Identity {
    noise_secret: StaticSecret,
    signing_secret: SigningKey,
}

impl Identity {
    /// Genera una identidad nueva con el CSPRNG del sistema.
    pub fn generate() -> Self {
        let mut rng = rand::rngs::OsRng;
        let mut noise = [0u8; 32];
        rng.fill_bytes(&mut noise);
        Self {
            noise_secret: StaticSecret::from(noise),
            signing_secret: SigningKey::generate(&mut rng),
        }
    }

    /// Restaura desde las claves en bruto (32 + 32 bytes).
    pub fn from_bytes(noise_secret: [u8; 32], signing_secret: [u8; 32]) -> Self {
        Self {
            noise_secret: StaticSecret::from(noise_secret),
            signing_secret: SigningKey::from_bytes(&signing_secret),
        }
    }

    /// Serializa las claves secretas para persistirlas.
    pub fn to_bytes(&self) -> ([u8; 32], [u8; 32]) {
        (self.noise_secret.to_bytes(), self.signing_secret.to_bytes())
    }

    pub fn noise_public(&self) -> [u8; 32] {
        PublicKey::from(&self.noise_secret).to_bytes()
    }

    pub fn signing_public(&self) -> [u8; 32] {
        self.signing_secret.verifying_key().to_bytes()
    }

    /// Peer ID canónico: `hex(sha256(clave pública Noise)[..8])`.
    pub fn peer_id(&self) -> PeerId {
        derive_peer_id(&self.noise_public())
    }

    /// Firma un mensaje con Ed25519.
    pub fn sign(&self, msg: &[u8]) -> [u8; 64] {
        self.signing_secret.sign(msg).to_bytes()
    }

    /// Clave de firma para firmar paquetes completos.
    pub fn signing_key(&self) -> &SigningKey {
        &self.signing_secret
    }

    /// Clave pública de firma como `VerifyingKey`, para verificar paquetes.
    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing_secret.verifying_key()
    }

    /// Clave pública Noise para el handshake X25519.
    pub fn noise_static_secret(&self) -> &StaticSecret {
        &self.noise_secret
    }
}

/// Deriva el peer ID de una clave pública Noise de 32 bytes.
pub fn derive_peer_id(noise_public: &[u8; 32]) -> PeerId {
    let hash = Sha256::digest(noise_public);
    let mut id = [0u8; 8];
    id.copy_from_slice(&hash[..8]);
    id
}

/// Verifica que el ID reclamado por un par coincida con su clave pública.
pub fn matches_claimed_peer_id(claimed: &str, noise_public: &[u8; 32]) -> bool {
    if claimed.len() != 16
        || !claimed
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return false;
    }
    hex::encode(derive_peer_id(noise_public)) == claimed
}

/// Verifica una firma Ed25519 con la clave pública dada.
pub fn verify_signature(signature: &[u8; 64], msg: &[u8], public: &[u8; 32]) -> bool {
    let Ok(key) = VerifyingKey::from_bytes(public) else {
        return false;
    };
    let sig = ed25519_dalek::Signature::from_bytes(signature);
    key.verify(msg, &sig).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_id_es_hex_de_16() {
        let id = Identity::generate();
        let hex_id = hex::encode(id.peer_id());
        assert_eq!(hex_id.len(), 16);
        assert!(hex_id
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn derivacion_estable_y_determinista() {
        let id = Identity::generate();
        let pubk = id.noise_public();
        assert_eq!(id.peer_id(), derive_peer_id(&pubk));
        assert_eq!(derive_peer_id(&pubk), derive_peer_id(&pubk));
        assert!(matches_claimed_peer_id(&hex::encode(id.peer_id()), &pubk));
    }

    #[test]
    fn identidad_distinta_da_peer_id_distinto() {
        let a = Identity::generate();
        let b = Identity::generate();
        assert_ne!(a.peer_id(), b.peer_id());
    }

    #[test]
    fn firma_y_verificacion() {
        let id = Identity::generate();
        let sig = id.sign(b"payload");
        assert!(verify_signature(&sig, b"payload", &id.signing_public()));
        assert!(!verify_signature(
            &sig,
            b"otro payload",
            &id.signing_public()
        ));
    }

    #[test]
    fn peer_id_coincide_con_sha256_de_la_clave() {
        // Vector autocontenido: el peer ID debe ser el sha256 de la clave.
        let id = Identity::from_bytes([1u8; 32], [2u8; 32]);
        let expected = {
            let mut h = Sha256::new();
            h.update(id.noise_public());
            let d = h.finalize();
            d[..8].to_vec()
        };
        assert_eq!(id.peer_id().to_vec(), expected);
        assert_eq!(hex::encode(id.peer_id()), hex::encode(expected));
    }

    #[test]
    fn claimed_id_rechaza_mayusculas() {
        let id = Identity::generate();
        let good = hex::encode(id.peer_id());
        let upper = good.to_uppercase();
        assert!(matches_claimed_peer_id(&good, &id.noise_public()));
        assert!(!matches_claimed_peer_id(&upper, &id.noise_public()));
        assert!(!matches_claimed_peer_id("corto", &id.noise_public()));
    }

    #[test]
    fn serializacion_de_claves() {
        let id = Identity::generate();
        let (ns, ss) = id.to_bytes();
        let restored = Identity::from_bytes(ns, ss);
        assert_eq!(id.peer_id(), restored.peer_id());
        assert_eq!(id.signing_public(), restored.signing_public());
    }
}
