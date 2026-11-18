//! Capa de red: comandos de la TUI hacia la radio, fragmentación de
//! paquetes grandes y reensamblado.
//!
//! Fase 1: el backend BLE (Linux/BlueZ) vive en [`bluez`]. En el resto de
//! plataformas queda un canal abierto sin radio hasta la Fase 2.

use crate::proto::packet::msg_type;
use crate::proto::{Packet, ProtoError};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};

#[cfg(target_os = "linux")]
mod bluez;

/// Lo que la interfaz le pide a la radio.
#[derive(Clone, Debug)]
pub enum NetCommand {
    /// Packet ya codificado y firmado: transmitirlo a todos los vecinos.
    SendPacket(Vec<u8>),
    /// Cambiar el apodo visible en la malla.
    SetNickname(String),
    /// Parar el backend.
    Shutdown,
}

/// Emisor de comandos de red (lo usa la TUI).
pub type NetTx = Sender<NetCommand>;

/// Tamaño máximo del payload de un paquete antes de fragmentar.
pub const FRAGMENT_THRESHOLD: usize = 512;
/// Tamaño de cada trozo de fragmento.
pub const FRAGMENT_CHUNK: usize = 469;
/// Cabecera de [`Fragment`]: id(8) + índice(2) + total(2) + tipo(1).
pub const FRAGMENT_HEADER: usize = 13;
/// Espera máxima por un juego de fragmentos.
pub const FRAGMENT_TIMEOUT_MS: u64 = 30_000;

/// Payload de un paquete FRAGMENT (id del juego, posición, total, tipo
/// original y los bytes del trozo).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fragment {
    pub id: [u8; 8],
    pub index: u16,
    pub total: u16,
    pub original_type: u8,
    pub data: Vec<u8>,
}

impl Fragment {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(FRAGMENT_HEADER + self.data.len());
        out.extend_from_slice(&self.id);
        out.extend_from_slice(&self.index.to_be_bytes());
        out.extend_from_slice(&self.total.to_be_bytes());
        out.push(self.original_type);
        out.extend_from_slice(&self.data);
        out
    }

    pub fn decode(data: &[u8]) -> Result<Fragment, ProtoError> {
        if data.len() < FRAGMENT_HEADER {
            return Err(ProtoError::Truncated);
        }
        let mut id = [0u8; 8];
        id.copy_from_slice(&data[..8]);
        let mut idx = [0u8; 2];
        idx.copy_from_slice(&data[8..10]);
        let mut tot = [0u8; 2];
        tot.copy_from_slice(&data[10..12]);
        Ok(Fragment {
            id,
            index: u16::from_be_bytes(idx),
            total: u16::from_be_bytes(tot),
            original_type: data[12],
            data: data[13..].to_vec(),
        })
    }
}

/// Divide un packet en trozos; devuelve `None` si no hace falta fragmentar.
pub fn fragment_packet(packet: &Packet) -> Result<Option<Vec<Packet>>, ProtoError> {
    // El tamaño que viaja por el aire (con padding) decide la fragmentación.
    let wire = packet.encode(true)?;
    if wire.len() <= FRAGMENT_THRESHOLD {
        return Ok(None);
    }
    let chunks: Vec<&[u8]> = wire.chunks(FRAGMENT_CHUNK).collect();
    if chunks.len() > u16::MAX as usize {
        return Err(ProtoError::PayloadTooLarge);
    }
    let total = chunks.len() as u16;
    // Id de juego: algo estable pero no repetible entre envíos.
    let mut id = [0u8; 8];
    let mut h = Sha256::new();
    h.update(&wire);
    h.update(crate::proto::packet::now_millis().to_be_bytes());
    id.copy_from_slice(&h.finalize()[..8]);

    let mut out = Vec::with_capacity(chunks.len());
    for (i, chunk) in chunks.into_iter().enumerate() {
        let frag = Fragment {
            id,
            index: i as u16,
            total,
            original_type: packet.msg_type,
            data: chunk.to_vec(),
        };
        let mut p = Packet::new(msg_type::FRAGMENT, packet.sender, frag.encode());
        p.timestamp = packet.timestamp;
        p.ttl = packet.ttl;
        p.recipient = packet.recipient;
        out.push(p);
    }
    Ok(Some(out))
}

/// Ensambla juegos de fragmentos entrantes.
#[derive(Default)]
pub struct Reassembler {
    pending: HashMap<[u8; 8], Pending>,
}

struct Pending {
    original_type: u8,
    parts: HashMap<u16, Vec<u8>>,
    total: u16,
    received_at: u64,
}

impl Reassembler {
    /// Añade un fragmento. Devuelve el paquete original cuando está completo.
    pub fn push(&mut self, frag: Fragment) -> Option<(u8, Vec<u8>)> {
        self.cleanup();
        let now = crate::proto::packet::now_millis();
        let entry = self.pending.entry(frag.id).or_insert_with(|| Pending {
            original_type: frag.original_type,
            parts: HashMap::new(),
            total: frag.total,
            received_at: now,
        });
        entry.parts.insert(frag.index, frag.data);
        if entry.parts.len() as u16 == entry.total && entry.total > 0 {
            let pending = self.pending.remove(&frag.id)?;
            let mut data = Vec::new();
            for i in 0..pending.total {
                let part = pending.parts.get(&i)?;
                data.extend_from_slice(part);
            }
            Some((pending.original_type, data))
        } else {
            None
        }
    }

    /// Descarta juegos incompletos con más de [`FRAGMENT_TIMEOUT_MS`].
    fn cleanup(&mut self) {
        let now = crate::proto::packet::now_millis();
        self.pending
            .retain(|_, p| now.saturating_sub(p.received_at) < FRAGMENT_TIMEOUT_MS);
    }
}

/// Hash de dedup: cubre emisor + timestamp + payload, **sin** el TTL, para
/// que un eco retransmitido (con TTL ya decrementado) se descarte igual.
pub fn dedup_key(packet: &Packet) -> [u8; 8] {
    let mut h = Sha256::new();
    h.update(packet.sender);
    h.update(packet.timestamp.to_be_bytes());
    h.update(&packet.payload);
    let mut key = [0u8; 8];
    key.copy_from_slice(&h.finalize()[..8]);
    key
}

/// Arranca el backend de radio para esta plataforma. Devuelve el emisor de
/// comandos (la TUI lo usa para transmitir) y el receptor de eventos ya
/// entregado junto con su emisor.
pub fn spawn(
    identity: crate::identity::Identity,
    nickname: String,
    events: Sender<crate::mesh::MeshEvent>,
) -> NetTx {
    #[cfg(target_os = "linux")]
    {
        bluez::spawn(identity, nickname, events)
    }
    #[cfg(not(target_os = "linux"))]
    {
        // Fase 2: aquí entrará el backend de cada plataforma.
        let (tx, _rx) = channel::<NetCommand>();
        let _ = (identity, nickname, events);
        tx
    }
}

/// Canal de eventos para pruebas sin radio.
pub fn offline_events() -> (
    Sender<crate::mesh::MeshEvent>,
    Receiver<crate::mesh::MeshEvent>,
) {
    channel()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::Packet;

    #[test]
    fn fragment_y_reensambla() {
        // Texto largo de alta entropía: no comprime por debajo del umbral.
        let big: Vec<u8> = (0..2000u32)
            .map(|i| i.wrapping_mul(2_654_435_761) >> 24)
            .map(|v| v as u8)
            .collect();
        let p = Packet::new(msg_type::MESSAGE, [1; 8], big.clone());
        let frags = fragment_packet(&p).unwrap().expect("debe fragmentar");
        assert!(frags.len() >= 4);

        let mut re = Reassembler::default();
        let mut done = None;
        for f in &frags {
            let frag = Fragment::decode(&f.payload).unwrap();
            if let Some(result) = re.push(frag) {
                done = Some(result);
            }
        }
        let (original_type, data) = done.expect("debe completarse");
        assert_eq!(original_type, msg_type::MESSAGE);
        // Los bytes reensamblados son el frame original (con padding).
        let decoded = Packet::decode(&data).expect("frame válido");
        assert_eq!(decoded.payload, big);
    }

    #[test]
    fn paquetes_pequeños_no_se_fragmentan() {
        let p = Packet::new(msg_type::MESSAGE, [1; 8], b"hola".to_vec());
        assert!(fragment_packet(&p).unwrap().is_none());
    }

    #[test]
    fn fragmento_truncado_rechazado() {
        assert!(Fragment::decode(&[0u8; 5]).is_err());
    }

    #[test]
    fn dedup_ignora_ecos_con_ttl_distinto() {
        let mut p = Packet::new(msg_type::MESSAGE, [2; 8], b"eco".to_vec());
        let k1 = dedup_key(&p);
        p.ttl -= 1;
        let k2 = dedup_key(&p);
        assert_eq!(k1, k2, "el TTL no debe cambiar la clave");

        p.payload.push(b'!');
        let k3 = dedup_key(&p);
        assert_ne!(k1, k3, "contenido distinto = clave distinta");
    }

    #[test]
    fn reassembler_descarta_incompletos() {
        let mut re = Reassembler::default();
        let frag = Fragment {
            id: [9; 8],
            index: 0,
            total: 3,
            original_type: msg_type::MESSAGE,
            data: vec![1, 2, 3],
        };
        assert!(re.push(frag.clone()).is_none());
        // Un juego distinto (otro id) no completa el primero.
        let mut other = frag.clone();
        other.id = [8; 8];
        assert!(re.push(other).is_none());
    }
}
