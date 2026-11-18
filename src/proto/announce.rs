//! Anuncio de identidad en TLV: apodo, clave pública Noise (X25519),
//! clave pública de firma (Ed25519) y capacidades.

use crate::proto::{ProtoError, ProtoResult};

pub mod tlv_type {
    pub const NICKNAME: u8 = 0x01;
    pub const NOISE_PUBLIC_KEY: u8 = 0x02;
    pub const SIGNING_PUBLIC_KEY: u8 = 0x03;
    pub const CAPABILITIES: u8 = 0x05;
}

/// Extensiones que este build no entiende (p. ej. gossip 0x04): se conservan
/// para re-codificar sin borrar lo que añadan clientes más nuevos.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UnknownTlv {
    pub tlv_type: u8,
    pub value: Vec<u8>,
}

/// TLV de capacidades (bitfield little-endian, siempre ≥1 byte).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Capabilities(pub u64);

impl Capabilities {
    pub fn contains(&self, cap: u64) -> bool {
        self.0 & cap == cap
    }

    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut remaining = self.0;
        loop {
            out.push(remaining as u8);
            remaining >>= 8;
            if remaining == 0 {
                break;
            }
        }
        out
    }

    fn decode(bytes: &[u8]) -> Self {
        let mut raw: u64 = 0;
        for (i, &b) in bytes.iter().take(8).enumerate() {
            raw |= (b as u64) << (8 * i);
        }
        Capabilities(raw)
    }
}

/// Anuncio de identidad.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Announcement {
    pub nickname: String,
    pub noise_public: [u8; 32],
    pub signing_public: [u8; 32],
    pub capabilities: Option<Capabilities>,
    pub unknown: Vec<UnknownTlv>,
}

impl Announcement {
    pub fn new(
        nickname: impl Into<String>,
        noise_public: [u8; 32],
        signing_public: [u8; 32],
    ) -> Self {
        Self {
            nickname: nickname.into(),
            noise_public,
            signing_public,
            capabilities: None,
            unknown: Vec::new(),
        }
    }

    pub fn encode(&self) -> ProtoResult<Vec<u8>> {
        let nick = self.nickname.as_bytes();
        if nick.len() > 255 || self.unknown.iter().any(|t| t.value.len() > 255) {
            return Err(ProtoError::PayloadTooLarge);
        }
        let mut out = Vec::new();
        let put = |out: &mut Vec<u8>, t: u8, v: &[u8]| {
            out.push(t);
            out.push(v.len() as u8);
            out.extend_from_slice(v);
        };
        put(&mut out, tlv_type::NICKNAME, nick);
        put(&mut out, tlv_type::NOISE_PUBLIC_KEY, &self.noise_public);
        put(&mut out, tlv_type::SIGNING_PUBLIC_KEY, &self.signing_public);
        if let Some(caps) = &self.capabilities {
            put(&mut out, tlv_type::CAPABILITIES, &caps.encode());
        }
        for t in &self.unknown {
            put(&mut out, t.tlv_type, &t.value);
        }
        Ok(out)
    }

    pub fn decode(data: &[u8]) -> ProtoResult<Self> {
        let mut pos = 0usize;
        let mut nickname: Option<String> = None;
        let mut noise_public: Option<[u8; 32]> = None;
        let mut signing_public: Option<[u8; 32]> = None;
        let mut capabilities: Option<Capabilities> = None;
        let mut unknown = Vec::new();

        while pos + 2 <= data.len() {
            let t = data[pos];
            let len = data[pos + 1] as usize;
            pos += 2;
            if pos + len > data.len() {
                return Err(ProtoError::Truncated);
            }
            let value = &data[pos..pos + len];
            pos += len;
            match t {
                tlv_type::NICKNAME => {
                    nickname = Some(
                        std::str::from_utf8(value)
                            .map_err(|_| ProtoError::Malformed)?
                            .to_string(),
                    );
                }
                tlv_type::NOISE_PUBLIC_KEY => {
                    if value.len() != 32 {
                        return Err(ProtoError::Malformed);
                    }
                    let mut k = [0u8; 32];
                    k.copy_from_slice(value);
                    noise_public = Some(k);
                }
                tlv_type::SIGNING_PUBLIC_KEY => {
                    if value.len() != 32 {
                        return Err(ProtoError::Malformed);
                    }
                    let mut k = [0u8; 32];
                    k.copy_from_slice(value);
                    signing_public = Some(k);
                }
                tlv_type::CAPABILITIES => {
                    capabilities = Some(Capabilities::decode(value));
                }
                other => unknown.push(UnknownTlv {
                    tlv_type: other,
                    value: value.to_vec(),
                }),
            }
        }

        Ok(Self {
            nickname: nickname.ok_or(ProtoError::Malformed)?,
            noise_public: noise_public.ok_or(ProtoError::Malformed)?,
            signing_public: signing_public.ok_or(ProtoError::Malformed)?,
            capabilities,
            unknown,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let a = Announcement::new("akaomi", [1u8; 32], [2u8; 32]);
        let bytes = a.encode().unwrap();
        let q = Announcement::decode(&bytes).unwrap();
        assert_eq!(q, a);
    }

    #[test]
    fn conserva_tlvs_desconocidos() {
        let mut a = Announcement::new("luna", [3u8; 32], [4u8; 32]);
        a.capabilities = Some(Capabilities(0b101));
        a.unknown.push(UnknownTlv {
            tlv_type: 0x04, // gossip
            value: vec![9, 9, 9],
        });
        let q = Announcement::decode(&a.encode().unwrap()).unwrap();
        assert_eq!(q, a);
        assert_eq!(q.capabilities.unwrap().0, 0b101);
    }

    #[test]
    fn capacidades_bitfield() {
        let caps = Capabilities(0x0102);
        let enc = caps.encode();
        assert_eq!(enc, vec![0x02, 0x01]);
        assert_eq!(Capabilities::decode(&enc), caps);
        assert!(caps.contains(0x02));
        assert!(!caps.contains(0x04));
    }

    #[test]
    fn apodo_obligatorio() {
        // Sin TLV de apodo no es un anuncio válido.
        let mut bytes = Announcement::new("x", [0; 32], [0; 32]).encode().unwrap();
        bytes[1] = 0; // longitud del apodo a 0... pero sigue presente
                      // Mejor: recortar el TLV de apodo completo (2 bytes + 1 de valor).
        let full = Announcement::new("x", [5u8; 32], [6u8; 32])
            .encode()
            .unwrap();
        let sin_nick = &full[3..];
        assert!(Announcement::decode(sin_nick).is_err());
        let _ = bytes;
    }
}
