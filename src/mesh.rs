//! Backend de malla: transporta paquetes entre la TUI y la radio.
//!
//! Fase 0: sin radio — el canal existe para que la interfaz funcione
//! completa; la Fase 1 conecta aquí el backend BLE (BlueZ).

use std::sync::mpsc::{channel, Receiver, Sender};

/// Lo que la malla le cuenta a la interfaz.
#[derive(Clone, Debug)]
pub enum MeshEvent {
    PeerJoined {
        id: String,
        nickname: String,
    },
    PeerLeft {
        id: String,
    },
    Message {
        from: String,
        channel: Option<String>,
        text: String,
    },
    Status(String),
}

/// Emisor que usa el backend para hablar con la TUI.
pub type MeshTx = Sender<MeshEvent>;

/// Crea el canal de malla. Sin radio todavía: el canal queda abierto y
/// quieto hasta que entre el backend BLE.
pub fn offline() -> (MeshTx, Receiver<MeshEvent>) {
    channel()
}
