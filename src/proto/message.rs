//! Payload binario de mensaje de chat (campos opcionales por flags).

use crate::proto::{ProtoError, ProtoResult};

/// Flags de campos opcionales.
pub mod flags {
    pub const RELAY: u8 = 0x01;
    pub const PRIVATE: u8 = 0x02;
    pub const ORIGINAL_SENDER: u8 = 0x04;
    pub const RECIPIENT_NICKNAME: u8 = 0x08;
    pub const SENDER_PEER_ID: u8 = 0x10;
    pub const MENTIONS: u8 = 0x20;
    pub const CHANNEL: u8 = 0x40;
    pub const ENCRYPTED: u8 = 0x80;
}

/// Mensaje con sus metadatos, en formato binario.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ChatMessage {
    pub id: String,
    pub sender: String,
    pub content: String,
    pub timestamp_ms: u64,
    pub is_relay: bool,
    pub is_private: bool,
    pub is_encrypted: bool,
    pub original_sender: Option<String>,
    pub recipient_nickname: Option<String>,
    pub sender_peer_id: Option<String>,
    pub mentions: Vec<String>,
    pub channel: Option<String>,
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(255);
    out.push(len as u8);
    out.extend_from_slice(&bytes[..len]);
}

fn take_str(data: &[u8], pos: &mut usize) -> ProtoResult<String> {
    if *pos >= data.len() {
        return Err(ProtoError::Truncated);
    }
    let len = data[*pos] as usize;
    *pos += 1;
    if *pos + len > data.len() {
        return Err(ProtoError::Truncated);
    }
    let s = std::str::from_utf8(&data[*pos..*pos + len])
        .map_err(|_| ProtoError::Malformed)?
        .to_string();
    *pos += len;
    Ok(s)
}

impl ChatMessage {
    pub fn new(sender: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: new_message_id(),
            sender: sender.into(),
            content: content.into(),
            timestamp_ms: crate::proto::packet::now_millis(),
            ..Default::default()
        }
    }

    /// Codifica al formato binario del wire.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut f = 0u8;
        if self.is_relay {
            f |= flags::RELAY;
        }
        if self.is_private {
            f |= flags::PRIVATE;
        }
        if self.original_sender.is_some() {
            f |= flags::ORIGINAL_SENDER;
        }
        if self.recipient_nickname.is_some() {
            f |= flags::RECIPIENT_NICKNAME;
        }
        if self.sender_peer_id.is_some() {
            f |= flags::SENDER_PEER_ID;
        }
        if !self.mentions.is_empty() {
            f |= flags::MENTIONS;
        }
        if self.channel.is_some() {
            f |= flags::CHANNEL;
        }
        if self.is_encrypted {
            f |= flags::ENCRYPTED;
        }
        out.push(f);
        out.extend_from_slice(&self.timestamp_ms.to_be_bytes());
        put_str(&mut out, &self.id);
        put_str(&mut out, &self.sender);

        let content = self.content.as_bytes();
        let len = content.len().min(0xFFFF);
        out.extend_from_slice(&(len as u16).to_be_bytes());
        out.extend_from_slice(&content[..len]);

        if let Some(o) = &self.original_sender {
            put_str(&mut out, o);
        }
        if let Some(r) = &self.recipient_nickname {
            put_str(&mut out, r);
        }
        if let Some(p) = &self.sender_peer_id {
            put_str(&mut out, p);
        }
        if !self.mentions.is_empty() {
            let count = self.mentions.len().min(255);
            out.push(count as u8);
            for m in self.mentions.iter().take(count) {
                put_str(&mut out, m);
            }
        }
        if let Some(c) = &self.channel {
            put_str(&mut out, c);
        }
        out
    }

    /// Decodifica desde el formato binario del wire.
    pub fn decode(data: &[u8]) -> ProtoResult<Self> {
        if data.len() < 13 {
            return Err(ProtoError::Truncated);
        }
        let mut pos = 0usize;
        let f = data[pos];
        pos += 1;
        let mut ts = [0u8; 8];
        ts.copy_from_slice(&data[pos..pos + 8]);
        pos += 8;
        let timestamp_ms = u64::from_be_bytes(ts);

        let id = take_str(data, &mut pos)?;
        let sender = take_str(data, &mut pos)?;
        if pos + 2 > data.len() {
            return Err(ProtoError::Truncated);
        }
        let mut cl = [0u8; 2];
        cl.copy_from_slice(&data[pos..pos + 2]);
        pos += 2;
        let content_len = u16::from_be_bytes(cl) as usize;
        if pos + content_len > data.len() {
            return Err(ProtoError::Truncated);
        }
        let content = std::str::from_utf8(&data[pos..pos + content_len])
            .map_err(|_| ProtoError::Malformed)?
            .to_string();
        pos += content_len;

        let original_sender = if f & flags::ORIGINAL_SENDER != 0 {
            Some(take_str(data, &mut pos)?)
        } else {
            None
        };
        let recipient_nickname = if f & flags::RECIPIENT_NICKNAME != 0 {
            Some(take_str(data, &mut pos)?)
        } else {
            None
        };
        let sender_peer_id = if f & flags::SENDER_PEER_ID != 0 {
            Some(take_str(data, &mut pos)?)
        } else {
            None
        };
        let mentions = if f & flags::MENTIONS != 0 {
            if pos >= data.len() {
                return Err(ProtoError::Truncated);
            }
            let count = data[pos] as usize;
            pos += 1;
            let mut list = Vec::with_capacity(count);
            for _ in 0..count {
                list.push(take_str(data, &mut pos)?);
            }
            list
        } else {
            Vec::new()
        };
        let channel = if f & flags::CHANNEL != 0 {
            Some(take_str(data, &mut pos)?)
        } else {
            None
        };

        Ok(Self {
            id,
            sender,
            content,
            timestamp_ms,
            is_relay: f & flags::RELAY != 0,
            is_private: f & flags::PRIVATE != 0,
            is_encrypted: f & flags::ENCRYPTED != 0,
            original_sender,
            recipient_nickname,
            sender_peer_id,
            mentions,
            channel,
        })
    }
}

/// Identificador de mensaje: 8 hex aleatorios, con forma legible.
fn new_message_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut id = format!("{:08x}", nanos as u32);
    id.push_str(&format!("{:08x}", (nanos >> 32) as u32));
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_basico() {
        let m = ChatMessage::new("akaomi", "hola desde la terminal");
        let bytes = m.encode();
        let q = ChatMessage::decode(&bytes).unwrap();
        assert_eq!(q.sender, "akaomi");
        assert_eq!(q.content, "hola desde la terminal");
        assert_eq!(q.id, m.id);
        assert_eq!(q.timestamp_ms, m.timestamp_ms);
        assert!(q.mentions.is_empty());
        assert!(!q.is_private);
    }

    #[test]
    fn round_trip_con_todos_losopcionales() {
        let m = ChatMessage {
            id: "ab12cd34".into(),
            sender: "neon".into(),
            content: "hola @luna".into(),
            timestamp_ms: 1_700_000_000_123,
            is_relay: true,
            is_private: true,
            is_encrypted: false,
            original_sender: Some("origen".into()),
            recipient_nickname: Some("luna".into()),
            sender_peer_id: Some("aabbccdd00112233".into()),
            mentions: vec!["luna".into(), "kai".into()],
            channel: Some("general".into()),
        };
        let q = ChatMessage::decode(&m.encode()).unwrap();
        assert_eq!(q, m);
    }

    #[test]
    fn contenido_utf8_multibyte() {
        let m = ChatMessage::new("luna", "ñandú ✦ lluvia");
        let q = ChatMessage::decode(&m.encode()).unwrap();
        assert_eq!(q.content, "ñandú ✦ lluvia");
    }

    #[test]
    fn truncado_rechazado() {
        let m = ChatMessage::new("a", "b");
        let bytes = m.encode();
        assert!(ChatMessage::decode(&bytes[..10]).is_err());
    }
}
