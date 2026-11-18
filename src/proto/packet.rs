//! Paquete binario del wire format (v1 y v2).
//!
//! Cabecera (14 B en v1, 16 B en v2):
//! `version(1) type(1) ttl(1) timestamp(8 BE) flags(1) payloadLen(2|4 BE)`
//! y después, en orden: `senderID(8)`, `recipientID(8)?`, ruta v2
//! (`count(1)+N*8)?`, payload (con el tamaño original por delante si va
//! comprimido), firma `Ed25519(64)?`.
//!
//! La firma se calcula sobre el paquete re-codificado con `ttl = 0` y sin
//! firma: el TTL cambia en cada salto y no debe invalidar la verificación.

use crate::proto::{compress, limits, padding, ProtoError, ProtoResult};
use ed25519_dalek::{Signer, Verifier};

pub const HEADER_SIZE_V1: usize = 14;
pub const HEADER_SIZE_V2: usize = 16;
pub const SENDER_ID_SIZE: usize = 8;
pub const RECIPIENT_ID_SIZE: usize = 8;
pub const SIGNATURE_SIZE: usize = 64;

/// TTL por defecto de los mensajes de usuario (saltos).
pub const MESSAGE_TTL_HOPS: u8 = 7;
/// TTL de los paquetes de sincronización entre vecinos (y de la firma).
pub const SYNC_TTL_HOPS: u8 = 0;

/// Identificador de 8 bytes en el wire (hex de 16 caracteres).
pub type PeerId = [u8; 8];

/// Destino de difusión: los 8 bytes a `0xFF`.
pub const BROADCAST: PeerId = [0xFF; 8];

pub mod msg_type {
    pub const ANNOUNCE: u8 = 0x01;
    pub const MESSAGE: u8 = 0x02;
    pub const LEAVE: u8 = 0x03;
    pub const NOISE_HANDSHAKE: u8 = 0x10;
    pub const NOISE_ENCRYPTED: u8 = 0x11;
    pub const FRAGMENT: u8 = 0x20;
    pub const REQUEST_SYNC: u8 = 0x21;
    pub const FILE_TRANSFER: u8 = 0x22;
    pub const VOICE_FRAME: u8 = 0x29;
}

pub mod flags {
    pub const HAS_RECIPIENT: u8 = 0x01;
    pub const HAS_SIGNATURE: u8 = 0x02;
    pub const IS_COMPRESSED: u8 = 0x04;
    pub const HAS_ROUTE: u8 = 0x08;
}

/// Bytes del payload tal como llegaron en el wire.
///
/// Las firmas cubren una re-codificación del paquete; DEFLATE no es canónico
/// (cada cliente comprime distinto), así que reutilizar los bytes recibidos
/// garantiza que un relay re-codifique exactamente lo mismo que firmó el
/// origen. `for_payload` los ata al payload al que decodifican: si el payload
/// cambia, el codificador vuelve a comprimir.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WirePayload {
    pub bytes: Vec<u8>,
    pub compressed: bool,
    pub for_payload: Vec<u8>,
}

/// Paquete de red completo.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Packet {
    pub version: u8,
    pub msg_type: u8,
    pub ttl: u8,
    pub timestamp: u64,
    pub sender: PeerId,
    pub recipient: Option<PeerId>,
    /// Ruta de origen (v2+): pares intermedios, sin emisor ni destino.
    pub route: Option<Vec<PeerId>>,
    pub payload: Vec<u8>,
    pub signature: Option<[u8; SIGNATURE_SIZE]>,
    /// Bytes originales del wire (ver [`WirePayload`]).
    pub wire: Option<WirePayload>,
}

impl Packet {
    pub fn new(msg_type: u8, sender: PeerId, payload: Vec<u8>) -> Self {
        Self {
            version: 1,
            msg_type,
            ttl: MESSAGE_TTL_HOPS,
            timestamp: now_millis(),
            sender,
            recipient: None,
            route: None,
            payload,
            signature: None,
            wire: None,
        }
    }

    /// ¿Es difusión (sin destinatario concreto)?
    pub fn is_broadcast(&self) -> bool {
        match self.recipient {
            None => true,
            Some(r) => r == BROADCAST,
        }
    }

    /// Codifica al wire. Con `padding = true` aplica el relleno normalizador
    /// (es también el preimage que se firma).
    pub fn encode(&self, pad: bool) -> ProtoResult<Vec<u8>> {
        if self.payload.len() > limits::MAX_PAYLOAD_LENGTH {
            return Err(ProtoError::PayloadTooLarge);
        }

        // Re-codificación de un paquete decodificado: reutiliza los bytes
        // originales del wire para que la firma siga siendo verificable.
        let mut payload = self.payload.clone();
        let mut original_size: Option<usize> = None;
        let mut compressed = false;

        let wire = self.wire.as_ref();
        if let Some(w) = wire {
            if w.for_payload == self.payload {
                if w.compressed {
                    payload = w.bytes.clone();
                    original_size = Some(self.payload.len());
                    compressed = true;
                }
            } else if compress::should_compress(&self.payload) {
                if let Some(c) = compress::compress(&self.payload) {
                    original_size = Some(self.payload.len());
                    payload = c;
                    compressed = true;
                }
            }
        } else if compress::should_compress(&self.payload) {
            if let Some(c) = compress::compress(&self.payload) {
                original_size = Some(self.payload.len());
                payload = c;
                compressed = true;
            }
        }

        let header_size = if self.version >= 2 {
            HEADER_SIZE_V2
        } else {
            HEADER_SIZE_V1
        };
        let mut flags: u8 = 0;
        if self.recipient.is_some() {
            flags |= flags::HAS_RECIPIENT;
        }
        if self.signature.is_some() {
            flags |= flags::HAS_SIGNATURE;
        }
        if compressed {
            flags |= flags::IS_COMPRESSED;
        }
        let route = if self.version >= 2 {
            self.route.as_ref().filter(|r| !r.is_empty())
        } else {
            None
        };
        if route.is_some() {
            flags |= flags::HAS_ROUTE;
        }

        let size_field = if compressed {
            if self.version >= 2 {
                4
            } else {
                2
            }
        } else {
            0
        };
        let payload_len = payload.len() + size_field;

        let mut out = Vec::with_capacity(header_size + 8 + 8 + payload_len + SIGNATURE_SIZE + 16);

        // Cabecera
        out.push(self.version);
        out.push(self.msg_type);
        out.push(self.ttl);
        out.extend_from_slice(&self.timestamp.to_be_bytes());
        out.push(flags);
        if self.version >= 2 {
            let len = u32::try_from(payload_len).map_err(|_| ProtoError::PayloadTooLarge)?;
            out.extend_from_slice(&len.to_be_bytes());
        } else {
            if payload_len > 0xFFFF || original_size.unwrap_or(0) > 0xFFFF {
                return Err(ProtoError::PayloadTooLarge);
            }
            out.extend_from_slice(&(payload_len as u16).to_be_bytes());
        }

        // Emisor y destinatario
        out.extend_from_slice(&self.sender);
        if let Some(r) = &self.recipient {
            out.extend_from_slice(r);
        }

        // Ruta de origen (v2+)
        if let Some(r) = route {
            let count = r.len().min(255);
            out.push(count as u8);
            for hop in r.iter().take(count) {
                out.extend_from_slice(hop);
            }
        }

        // Payload (con el tamaño original por delante si va comprimido)
        if let Some(orig) = original_size {
            if self.version >= 2 {
                let o = u32::try_from(orig).map_err(|_| ProtoError::PayloadTooLarge)?;
                out.extend_from_slice(&o.to_be_bytes());
            } else {
                if orig > 0xFFFF {
                    return Err(ProtoError::PayloadTooLarge);
                }
                out.extend_from_slice(&(orig as u16).to_be_bytes());
            }
        }
        out.extend_from_slice(&payload);

        // Firma
        if let Some(sig) = &self.signature {
            out.extend_from_slice(sig);
        }

        if pad {
            let target = padding::optimal_block_size(out.len());
            Ok(padding::pad(&out, target))
        } else {
            Ok(out)
        }
    }

    /// Preimage de la firma: el paquete sin firma y con TTL fijo a 0, para
    /// que un relay pueda decrementar el TTL sin romper la verificación.
    pub fn for_signing(&self) -> ProtoResult<Vec<u8>> {
        let mut clone = self.clone();
        clone.signature = None;
        clone.ttl = SYNC_TTL_HOPS;
        clone.encode(true)
    }

    /// Firma el paquete con Ed25519 (el preimage ya incluye el padding).
    pub fn sign(&mut self, key: &ed25519_dalek::SigningKey) -> ProtoResult<()> {
        let preimage = self.for_signing()?;
        let sig = key.sign(&preimage);
        self.signature = Some(sig.to_bytes());
        Ok(())
    }

    /// Verifica la firma con la clave pública Ed25519 del emisor.
    pub fn verify(&self, key: &ed25519_dalek::VerifyingKey) -> ProtoResult<bool> {
        let sig = self.signature.ok_or(ProtoError::Malformed)?;
        let preimage = self.for_signing()?;
        let sig = ed25519_dalek::Signature::from_bytes(&sig);
        Ok(key.verify(&preimage, &sig).is_ok())
    }

    /// Decodifica un frame del wire: primero tal cual (por si no llevaba
    /// padding) y si falla, elimina el padding y reintenta.
    pub fn decode(data: &[u8]) -> ProtoResult<Self> {
        if let Ok(p) = Self::decode_core(data) {
            return Ok(p);
        }
        let unpadded = padding::unpad(data);
        if unpadded == data {
            return Err(ProtoError::Truncated);
        }
        Self::decode_core(&unpadded)
    }

    fn decode_core(raw: &[u8]) -> ProtoResult<Self> {
        if raw.len() < HEADER_SIZE_V1 + SENDER_ID_SIZE {
            return Err(ProtoError::Truncated);
        }
        let version = raw[0];
        if version != 1 && version != 2 {
            return Err(ProtoError::BadVersion(version));
        }
        let header_size = if version >= 2 {
            HEADER_SIZE_V2
        } else {
            HEADER_SIZE_V1
        };
        let msg_type = raw[1];
        let ttl = raw[2];
        let mut ts = [0u8; 8];
        ts.copy_from_slice(&raw[3..11]);
        let timestamp = u64::from_be_bytes(ts);
        let flag = raw[11];
        let has_recipient = flag & flags::HAS_RECIPIENT != 0;
        let has_signature = flag & flags::HAS_SIGNATURE != 0;
        let is_compressed = flag & flags::IS_COMPRESSED != 0;
        // HAS_ROUTE sólo es válido en v2+; en v1 se ignora.
        let has_route = version >= 2 && flag & flags::HAS_ROUTE != 0;

        let payload_length = if version >= 2 {
            let mut l = [0u8; 4];
            l.copy_from_slice(&raw[12..16]);
            u32::from_be_bytes(l) as usize
        } else {
            let mut l = [0u8; 2];
            l.copy_from_slice(&raw[12..14]);
            u16::from_be_bytes(l) as usize
        };
        if payload_length > limits::MAX_PAYLOAD_LENGTH {
            return Err(ProtoError::PayloadTooLarge);
        }
        let max_payload = limits::max_payload_bytes(msg_type);

        let mut expected = header_size + SENDER_ID_SIZE + payload_length;
        if has_recipient {
            expected += RECIPIENT_ID_SIZE;
        }
        // La ruta se lee después del emisor/destinatario: hay que asomarse.
        let mut route_count = 0usize;
        if has_route {
            let mut offset = header_size + SENDER_ID_SIZE;
            if has_recipient {
                offset += RECIPIENT_ID_SIZE;
            }
            if raw.len() > offset {
                route_count = raw[offset] as usize;
            }
            expected += 1 + route_count * SENDER_ID_SIZE;
        }
        if has_signature {
            expected += SIGNATURE_SIZE;
        }
        if raw.len() < expected {
            return Err(ProtoError::Truncated);
        }

        let mut pos = header_size;
        let mut sender = [0u8; SENDER_ID_SIZE];
        sender.copy_from_slice(&raw[pos..pos + SENDER_ID_SIZE]);
        pos += SENDER_ID_SIZE;

        let recipient = if has_recipient {
            let mut r = [0u8; RECIPIENT_ID_SIZE];
            r.copy_from_slice(&raw[pos..pos + RECIPIENT_ID_SIZE]);
            pos += RECIPIENT_ID_SIZE;
            Some(r)
        } else {
            None
        };

        let route = if has_route {
            let count = raw[pos] as usize;
            pos += 1;
            if count == 0 {
                // Representación canónica: ruta vacía = sin ruta.
                None
            } else {
                let mut hops = Vec::with_capacity(count);
                for _ in 0..count {
                    let mut hop = [0u8; SENDER_ID_SIZE];
                    hop.copy_from_slice(&raw[pos..pos + SENDER_ID_SIZE]);
                    pos += SENDER_ID_SIZE;
                    hops.push(hop);
                }
                Some(hops)
            }
        } else {
            None
        };

        // Payload
        let (payload, wire_bytes, wire_compressed) = if is_compressed {
            let field = if version >= 2 { 4 } else { 2 };
            if payload_length < field {
                return Err(ProtoError::Truncated);
            }
            let original_size = if version >= 2 {
                let mut s = [0u8; 4];
                s.copy_from_slice(&raw[pos..pos + 4]);
                pos += 4;
                u32::from_be_bytes(s) as usize
            } else {
                let mut s = [0u8; 2];
                s.copy_from_slice(&raw[pos..pos + 2]);
                pos += 2;
                u16::from_be_bytes(s) as usize
            };
            if original_size == 0 || original_size > max_payload {
                return Err(ProtoError::PayloadTooLarge);
            }
            let compressed_size = payload_length - field;
            if compressed_size == 0 {
                return Err(ProtoError::BadCompression);
            }
            let comp = &raw[pos..pos + compressed_size];
            pos += compressed_size;
            let out = compress::decompress(comp, original_size)?;
            (out, comp.to_vec(), true)
        } else {
            if payload_length > max_payload {
                return Err(ProtoError::PayloadTooLarge);
            }
            let p = raw[pos..pos + payload_length].to_vec();
            pos += payload_length;
            let w = p.clone();
            (p, w, false)
        };

        let signature = if has_signature {
            let mut sig = [0u8; SIGNATURE_SIZE];
            sig.copy_from_slice(&raw[pos..pos + SIGNATURE_SIZE]);
            Some(sig)
        } else {
            None
        };

        Ok(Self {
            version,
            msg_type,
            ttl,
            timestamp,
            sender,
            recipient,
            route,
            signature,
            wire: Some(WirePayload {
                bytes: wire_bytes,
                compressed: wire_compressed,
                for_payload: payload.clone(),
            }),
            payload,
        })
    }
}

/// Marca de tiempo en milisegundos (suficiente; el wire usa u64 BE).
pub fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn sample_packet() -> Packet {
        let mut p = Packet::new(msg_type::MESSAGE, [0x11; 8], b"hola malla".to_vec());
        p.timestamp = 1_700_000_000_000;
        p
    }

    #[test]
    fn round_trip_v1_broadcast() {
        let p = sample_packet();
        let wire = p.encode(true).unwrap();
        let q = Packet::decode(&wire).unwrap();
        assert_eq!(q.version, 1);
        assert_eq!(q.msg_type, msg_type::MESSAGE);
        assert_eq!(q.ttl, MESSAGE_TTL_HOPS);
        assert_eq!(q.timestamp, 1_700_000_000_000);
        assert_eq!(q.sender, [0x11; 8]);
        assert!(q.is_broadcast());
        assert_eq!(q.payload, b"hola malla");
        assert_eq!(q.signature, None);
    }

    #[test]
    fn round_trip_v1_con_destinatario() {
        let mut p = sample_packet();
        p.recipient = Some([0x22; 8]);
        let q = Packet::decode(&p.encode(true).unwrap()).unwrap();
        assert_eq!(q.recipient, Some([0x22; 8]));
        assert!(!q.is_broadcast());
    }

    #[test]
    fn round_trip_v2_con_ruta() {
        let mut p = sample_packet();
        p.version = 2;
        p.route = Some(vec![[0x33; 8], [0x44; 8]]);
        p.recipient = Some([0x55; 8]);
        let q = Packet::decode(&p.encode(true).unwrap()).unwrap();
        assert_eq!(q.version, 2);
        assert_eq!(q.route, Some(vec![[0x33; 8], [0x44; 8]]));
        assert_eq!(q.recipient, Some([0x55; 8]));
    }

    #[test]
    fn payload_comprimido_round_trip() {
        let mut p = sample_packet();
        p.payload = vec![b'a'; 4096];
        let wire = p.encode(true).unwrap();
        // Debería ir comprimido: el frame es mucho menor que el original.
        assert!(wire.len() < 2048);
        let q = Packet::decode(&wire).unwrap();
        assert_eq!(q.payload, p.payload);
        assert!(q.wire.unwrap().compressed);
    }

    #[test]
    fn firma_round_trip_y_tamper() {
        let key = SigningKey::from_bytes(&[7u8; 32]);
        let mut p = sample_packet();
        p.sign(&key).unwrap();
        let wire = p.encode(true).unwrap();
        let q = Packet::decode(&wire).unwrap();
        assert!(q.signature.is_some());
        assert!(q.verify(&key.verifying_key()).unwrap());

        // Alterar el payload invalida la firma. El frame lleva padding hasta
        // 256 B: el header ocupa 14 y el emisor 8, así que el payload empieza
        // en 22 — tocar ahí cambia el preimage (tocar el padding, no).
        let mut tampered = wire.clone();
        tampered[24] ^= 0xFF;
        let r = Packet::decode(&tampered).expect("sigue siendo un frame válido");
        assert!(!r.verify(&key.verifying_key()).unwrap());
    }

    #[test]
    fn firma_sobrevive_cambio_de_ttl() {
        // Un relay decrementa el TTL: la firma debe seguir siendo válida.
        let key = SigningKey::from_bytes(&[9u8; 32]);
        let mut p = sample_packet();
        p.sign(&key).unwrap();
        let mut relayed = Packet::decode(&p.encode(true).unwrap()).unwrap();
        relayed.ttl -= 1;
        assert!(relayed.verify(&key.verifying_key()).unwrap());
    }

    #[test]
    fn reencode_byte_a_byte_estable() {
        // Codificar → decodificar → codificar produce los mismos bytes:
        // la reutilización del wire original lo garantiza.
        let key = SigningKey::from_bytes(&[3u8; 32]);
        let mut p = sample_packet();
        p.payload = vec![b'x'; 3000];
        p.sign(&key).unwrap();
        let w1 = p.encode(true).unwrap();
        let q = Packet::decode(&w1).unwrap();
        let w2 = q.encode(true).unwrap();
        assert_eq!(w1, w2);
    }

    #[test]
    fn version_desconocida_rechazada() {
        let mut wire = sample_packet().encode(true).unwrap();
        wire[0] = 9;
        assert!(matches!(
            Packet::decode(&wire),
            Err(ProtoError::BadVersion(9))
        ));
    }

    #[test]
    fn frame_corto_rechazado() {
        assert!(Packet::decode(&[1, 2, 3]).is_err());
    }
}
