//! Núcleo de protocolo portable: formato de paquete, padding, compresión,
//! límites por tipo, payload de mensaje y anuncio de identidad.
//!
//! Todo aquí es independiente de plataforma: se ejecuta igual en Linux,
//! Termux y Windows. Los backends de radio sólo transportan bytes.

pub mod announce;
pub mod compress;
pub mod limits;
pub mod message;
pub mod packet;
pub mod padding;

pub use announce::Announcement;
pub use message::ChatMessage;
pub use packet::{Packet, PeerId, BROADCAST};

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProtoError {
    #[error("paquete demasiado corto")]
    Truncated,
    #[error("versión de paquete no soportada: {0}")]
    BadVersion(u8),
    #[error("payload excede el límite para el tipo")]
    PayloadTooLarge,
    #[error("payload ilegible (firma o longitud inválidas)")]
    Malformed,
    #[error("compresión inválida")]
    BadCompression,
    #[error("ratio de compresión imposible (bomba deflate)")]
    CompressionBomb,
    #[error("padding PKCS#7 inválido")]
    BadPadding,
}

pub type ProtoResult<T> = Result<T, ProtoError>;
