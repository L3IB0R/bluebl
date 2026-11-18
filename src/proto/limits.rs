//! Techos de payload por tipo de mensaje, en el lado receptor.
//!
//! Un frame comprimido declara su tamaño original: sin techo por tipo, 1 KB
//! en el aire podrían valer 1 MB en memoria tras descomprimirse.

use crate::proto::packet::msg_type;

const FRAGMENT_BYTES: usize = 1_280;
const CONTROL_BYTES: usize = 4 * 1024;
const REQUEST_SYNC_BYTES: usize = 8 * 1024;
const ENVELOPE_BYTES: usize = 64 * 1024;
const TWICE_V1_FRAME_BYTES: usize = 128 * 1024;

/// Límite del payload decodificado para un paquete de `msg_type`.
pub fn max_payload_bytes(msg_type: u8) -> usize {
    match msg_type {
        msg_type::FILE_TRANSFER | msg_type::NOISE_ENCRYPTED => MAX_PAYLOAD_LENGTH,
        msg_type::MESSAGE => TWICE_V1_FRAME_BYTES,
        msg_type::REQUEST_SYNC => REQUEST_SYNC_BYTES,
        msg_type::FRAGMENT => FRAGMENT_BYTES,
        msg_type::ANNOUNCE
        | msg_type::LEAVE
        | msg_type::NOISE_HANDSHAKE
        | msg_type::VOICE_FRAME => CONTROL_BYTES,
        // Tipos de iOS que aún no decodificamos: se respetan sus techos
        // para que sigan siendo retransmisibles sin ser amplificadores.
        0x04 | 0x28 => ENVELOPE_BYTES, // courier / transporte nostr
        0x23 | 0x24 | 0x26 | 0x27 => CONTROL_BYTES, // tablón, prekeys, ping/pong
        0x25 => TWICE_V1_FRAME_BYTES,  // mensaje de grupo
        // Desconocidos: un frame v1 completo sigue siendo retransmisible.
        _ => TWICE_V1_FRAME_BYTES,
    }
}

/// Límite global de payload que un emisor puede codificar (10 MiB).
pub const MAX_PAYLOAD_LENGTH: usize = 10_485_760;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn techos_por_tipo() {
        assert_eq!(max_payload_bytes(msg_type::MESSAGE), 128 * 1024);
        assert_eq!(max_payload_bytes(msg_type::FRAGMENT), 1_280);
        assert_eq!(max_payload_bytes(msg_type::ANNOUNCE), 4 * 1024);
        assert_eq!(max_payload_bytes(msg_type::REQUEST_SYNC), 8 * 1024);
        assert_eq!(max_payload_bytes(0x99), 128 * 1024);
    }
}
