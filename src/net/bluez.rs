//! Backend BLE en Linux: BlueZ vía D-Bus (zbus) para el rol de peripheral
//! —anuncio LE y servidor GATT— y btleplug para el rol central —escanear,
//! conectar y escribir a los vecinos—.
//!
//! Cada nodo anuncia su peer ID en el service data del paquete de
//! publicación y publica la characteristic del wire format. Los paquetes
//! que entran por el servidor o por notificaciones se decodifican, se
//! retransmiten (flood con TTL y dedup) y se suben a la interfaz.

use crate::identity::Identity;
use crate::mesh::MeshEvent;
use crate::net::{dedup_key, Fragment, NetCommand, Reassembler};
use crate::proto::packet::msg_type;
use crate::proto::{Announcement, ChatMessage, Packet, BROADCAST};
use btleplug::api::{
    Central, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Peripheral, PeripheralId};
use futures::StreamExt;
use std::collections::{HashMap, HashSet, VecDeque};
use std::str::FromStr;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc as tokio_mpsc;
use uuid::Uuid;
use zbus::interface;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

/// UUID del servicio GATT del wire format.
const SERVICE_UUID: &str = "f47b5e2d-4a9e-4c5a-9b3f-8e1d2c3a4b5c";
/// UUID de la characteristic del wire format.
const CHAR_UUID: &str = "a1b2c3d4-e5f6-4a5b-8c9d-0e1f2a3b4c5d";
/// Path de la aplicación GATT en el bus del sistema.
const GATT_ROOT: &str = "/bluebl";
const SERVICE_PATH: &str = "/bluebl/service0";
const CHAR_PATH: &str = "/bluebl/service0/char0";
const ADV_PATH: &str = "/bluebl/adv";
/// Cadencia del anuncio de identidad.
const ANNOUNCE_INTERVAL: Duration = Duration::from_secs(30);
/// Cuántos paquetes recuerda el dedup antes de olvidar.
const DEDUP_CAP: usize = 4096;

// ── Objetos D-Bus del rol peripheral ───────────────────────────────────

/// Estado compartido de la characteristic (registro + acceso del backend).
#[derive(Clone)]
struct CharHandle(Arc<CharShared>);

struct CharShared {
    incoming: tokio_mpsc::Sender<Vec<u8>>,
    value: Mutex<Vec<u8>>,
    notifying: Mutex<bool>,
}

/// `org.bluez.GattCharacteristic1` — por aquí entran los paquetes que
/// otros nodos nos escriben.
#[interface(name = "org.bluez.GattCharacteristic1")]
impl CharHandle {
    #[zbus(property)]
    // BlueZ busca "UUID" (siglas), no "Uuid" (PascalCase normal).
    #[zbus(name = "UUID")]
    fn uuid(&self) -> String {
        CHAR_UUID.to_string()
    }

    #[zbus(property)]
    fn service(&self) -> OwnedObjectPath {
        // BlueZ espera aquí un object path (`o`), no una cadena.
        OwnedObjectPath::try_from(SERVICE_PATH).expect("path constante")
    }

    #[zbus(property)]
    fn flags(&self) -> Vec<String> {
        vec![
            "read".into(),
            "write".into(),
            "write-without-response".into(),
            "notify".into(),
        ]
    }

    #[zbus(property)]
    fn value(&self) -> Vec<u8> {
        self.0.value.lock().unwrap().clone()
    }

    #[zbus(property)]
    fn notifying(&self) -> bool {
        *self.0.notifying.lock().unwrap()
    }

    /// Paquete entrante desde un vecino conectado.
    async fn write_value(
        &self,
        value: Vec<u8>,
        _options: HashMap<String, OwnedValue>,
    ) -> zbus::fdo::Result<()> {
        let _ = self.0.incoming.send(value).await;
        Ok(())
    }

    async fn read_value(
        &self,
        _options: HashMap<String, OwnedValue>,
    ) -> zbus::fdo::Result<Vec<u8>> {
        Ok(self.0.value.lock().unwrap().clone())
    }

    async fn start_notify(&self) -> zbus::fdo::Result<()> {
        *self.0.notifying.lock().unwrap() = true;
        Ok(())
    }

    async fn stop_notify(&self) -> zbus::fdo::Result<()> {
        *self.0.notifying.lock().unwrap() = false;
        Ok(())
    }
}

/// `org.bluez.GattService1` — el servicio que publicamos.
#[interface(name = "org.bluez.GattService1")]
impl ServiceIface {
    #[zbus(property)]
    // BlueZ busca "UUID" (siglas), no "Uuid" (PascalCase normal).
    #[zbus(name = "UUID")]
    fn uuid(&self) -> String {
        SERVICE_UUID.to_string()
    }

    #[zbus(property)]
    fn primary(&self) -> bool {
        true
    }
}

/// Servicio registrado en el bus.
struct ServiceIface;

/// `org.bluez.LEAdvertisement1` — el anuncio que nos hace visibles con
/// nuestro peer ID en el service data.
struct AdvIface {
    nickname: String,
    peer_id: Vec<u8>,
}

#[interface(name = "org.bluez.LEAdvertisement1")]
impl AdvIface {
    #[zbus(property)]
    fn r#type(&self) -> String {
        "peripheral".into()
    }

    #[zbus(property)]
    #[zbus(name = "ServiceUUIDs")]
    fn service_uuids(&self) -> Vec<String> {
        vec![SERVICE_UUID.to_string()]
    }

    #[zbus(property)]
    fn service_data(&self) -> HashMap<String, OwnedValue> {
        let mut map = HashMap::new();
        // Service data del anuncio: el peer ID como bytes crudos.
        if let Ok(value) = OwnedValue::try_from(Value::from(self.peer_id.as_slice())) {
            map.insert(SERVICE_UUID.to_string(), value);
        }
        map
    }

    #[zbus(property)]
    fn local_name(&self) -> String {
        self.nickname.clone()
    }

    #[zbus(property)]
    fn includes(&self) -> Vec<String> {
        vec!["tx-power".into()]
    }

    /// BlueZ avisa al desregistrar el anuncio.
    async fn release(&self) {}
}

// ── Arranque ───────────────────────────────────────────────────────────

/// Lanza el backend en su propio hilo con runtime tokio.
pub fn spawn(
    identity: Identity,
    nickname: String,
    events: Sender<MeshEvent>,
) -> Sender<NetCommand> {
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<NetCommand>();
    std::thread::Builder::new()
        .name("bluebl-ble".into())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("runtime tokio");
            rt.block_on(async_main(identity, nickname, events, cmd_rx));
        })
        .expect("hilo BLE");
    cmd_tx
}

async fn async_main(
    identity: Identity,
    nickname: String,
    events: Sender<MeshEvent>,
    cmd_rx: Receiver<NetCommand>,
) {
    match run(identity, nickname, &events, cmd_rx).await {
        Ok(()) => {
            let _ = events.send(MeshEvent::Status("radio detenida".into()));
        }
        Err(e) => {
            let _ = events.send(MeshEvent::Status(format!("sin radio: {e}")));
        }
    }
}

/// Puente: el canal estándar de comandos (que usa la TUI) al canal async.
fn bridge_commands(cmd_rx: Receiver<NetCommand>) -> tokio_mpsc::Receiver<NetCommand> {
    let (tx, rx) = tokio_mpsc::channel(32);
    std::thread::spawn(move || {
        while let Ok(cmd) = cmd_rx.recv() {
            if tx.blocking_send(cmd).is_err() {
                break;
            }
        }
    });
    rx
}

/// Enlace activo con un vecino: peripheral + characteristic del wire format.
struct PeerLink {
    peripheral: Peripheral,
    characteristic: Characteristic,
    /// Peer ID del vecino (16 hex) tomado del service data del anuncio;
    /// permite avisar a la TUI con el mismo id que usa el announce.
    peer_hex: Option<String>,
}

async fn run(
    identity: Identity,
    mut nickname: String,
    events: &Sender<MeshEvent>,
    cmd_rx: Receiver<NetCommand>,
) -> Result<(), String> {
    let mut cmds = bridge_commands(cmd_rx);

    // ── Conexión D-Bus del sistema ──────────────────────────────────────
    let conn = zbus::Connection::system()
        .await
        .map_err(|e| format!("bus del sistema: {e}"))?;

    // ── Adaptador y energía ─────────────────────────────────────────────
    let adapter_path = find_adapter(&conn).await?;
    if let Some(why) = crate::deps::rfkill_blocked() {
        return Err(format!(
            "{why} — desbloquéala con: {}",
            crate::deps::unblock_command()
        ));
    }
    ensure_powered(&conn, &adapter_path).await?;
    let _ = events.send(MeshEvent::Status("radio BLE activa".into()));

    // ── Servidor GATT + anuncio ─────────────────────────────────────────
    let (incoming_tx, mut incoming_rx) = tokio_mpsc::channel::<Vec<u8>>(64);
    let char = CharHandle(Arc::new(CharShared {
        incoming: incoming_tx,
        value: Mutex::new(Vec::new()),
        notifying: Mutex::new(false),
    }));

    conn.object_server()
        .at(SERVICE_PATH, ServiceIface)
        .await
        .map_err(|e| format!("servicio GATT: {e}"))?;
    conn.object_server()
        .at(CHAR_PATH, char.clone())
        .await
        .map_err(|e| format!("characteristic: {e}"))?;

    conn.object_server()
        .at(
            ADV_PATH,
            AdvIface {
                nickname: nickname.clone(),
                peer_id: identity.peer_id().to_vec(),
            },
        )
        .await
        .map_err(|e| format!("anuncio: {e}"))?;

    // BlueZ recorre la app con GetManagedObjects en la raíz: sin este
    // ObjectManager no ve ningún objeto ("No object received").
    conn.object_server()
        .at(GATT_ROOT, zbus::fdo::ObjectManager)
        .await
        .map_err(|e| format!("ObjectManager: {e}"))?;

    register_application(&conn, &adapter_path).await?;
    register_advertisement(&conn, &adapter_path).await?;
    let _ = events.send(MeshEvent::Status("anunciando en la malla".into()));

    // ── Rol central: escanear y conectar ────────────────────────────────
    let manager = btleplug::platform::Manager::new()
        .await
        .map_err(|e| format!("gestor BLE: {e}"))?;
    let adapters = manager
        .adapters()
        .await
        .map_err(|e| format!("adaptadores: {e}"))?;
    let adapter: Adapter = adapters
        .into_iter()
        .next()
        .ok_or_else(|| "sin adaptador BLE".to_string())?;
    adapter
        .start_scan(ScanFilter {
            services: vec![Uuid::from_str(SERVICE_UUID).expect("uuid")],
        })
        .await
        .map_err(|e| format!("escaneo: {e}"))?;

    // ── Estado de la malla ──────────────────────────────────────────────
    let mut central_events = adapter.events().await.map_err(|e| e.to_string())?;
    let mut links: HashMap<PeripheralId, PeerLink> = HashMap::new();
    let mut seen: HashSet<[u8; 8]> = HashSet::new();
    let mut seen_order: VecDeque<[u8; 8]> = VecDeque::new();
    let mut reassembler = Reassembler::default();
    let (peer_tx, mut peer_rx) = tokio_mpsc::channel::<(PeripheralId, Vec<u8>)>(128);

    let mut announce_tick = tokio::time::interval(ANNOUNCE_INTERVAL);
    announce_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let my_peer_hex = hex::encode(identity.peer_id());
    let my_peer = identity.peer_id();

    // ── Bucle principal ─────────────────────────────────────────────────
    loop {
        tokio::select! {
            // Eventos del escáner: descubrimientos y cambios de estado.
            Some(ev) = central_events.next() => {
                use btleplug::api::CentralEvent;
                match ev {
                    CentralEvent::DeviceDiscovered(id) | CentralEvent::DeviceUpdated(id) => {
                        let before = links.len();
                        try_link(
                            &adapter, &id, &my_peer_hex, &mut links, &peer_tx, events,
                        ).await;
                        // Recién conectado: presentarnos al instante.
                        if links.len() > before {
                            send_announce(&identity, &nickname, &links, &char, &conn).await;
                        }
                    }
                    CentralEvent::DeviceDisconnected(id) => {
                        // Avisamos a la TUI para que el par no quede eterno.
                        if let Some(link) = links.remove(&id) {
                            if let Some(hex_id) = link.peer_hex {
                                let _ = events.send(MeshEvent::PeerLeft { id: hex_id });
                            }
                        }
                    }
                    _ => {}
                }
            }

            // Paquetes que otros nos escriben en nuestra characteristic.
            Some(bytes) = incoming_rx.recv() => {
                handle_packet(
                    bytes, None, &my_peer, &links, &mut seen, &mut seen_order,
                    &mut reassembler, events, &char, &conn,
                ).await;
            }

            // Notificaciones de los vecinos a los que nos conectamos.
            Some((id, bytes)) = peer_rx.recv() => {
                handle_packet(
                    bytes, Some(id), &my_peer, &links, &mut seen, &mut seen_order,
                    &mut reassembler, events, &char, &conn,
                ).await;
            }

            // Peticiones de la TUI.
            Some(cmd) = cmds.recv() => {
                match cmd {
                    NetCommand::SendPacket(bytes) => {
                        broadcast(&links, &bytes, &char, &conn, None).await;
                    }
                    NetCommand::SendDm { peer_id, bytes } => {
                        // Un MD sólo toca el enlace del destinatario: ni el
                        // resto de pares ni los suscritos a nuestro servicio.
                        for link in links.values() {
                            if link.peer_hex.as_deref() == Some(peer_id.as_str()) {
                                let _ = link
                                    .peripheral
                                    .write(&link.characteristic, &bytes, WriteType::WithoutResponse)
                                    .await;
                            }
                        }
                    }
                    NetCommand::SetNickname(new) => {
                        // El anuncio y el announce periódico usan este nick:
                        // sin actualizarlo, a los 30 s volvería el antiguo.
                        nickname = new.clone();
                        let _ = unregister_advertisement(&conn, &adapter_path).await;
                        let _ = conn.object_server().at(
                            ADV_PATH,
                            AdvIface {
                                nickname: new.clone(),
                                peer_id: identity.peer_id().to_vec(),
                            },
                        ).await;
                        let _ = register_advertisement(&conn, &adapter_path).await;
                        send_announce(&identity, &new, &links, &char, &conn).await;
                    }
                    NetCommand::Shutdown => break,
                }
            }

            // Anuncio periódico de identidad (el primero es inmediato).
            _ = announce_tick.tick() => {
                send_announce(&identity, &nickname, &links, &char, &conn).await;
            }
        }
    }

    let _ = unregister_advertisement(&conn, &adapter_path).await;
    Ok(())
}

// ── Lógica de red ──────────────────────────────────────────────────────

/// Intenta conectar y suscribirse a un vecino recién descubierto.
async fn try_link(
    adapter: &Adapter,
    id: &PeripheralId,
    my_peer_hex: &str,
    links: &mut HashMap<PeripheralId, PeerLink>,
    peer_tx: &tokio_mpsc::Sender<(PeripheralId, Vec<u8>)>,
    events: &Sender<MeshEvent>,
) {
    if links.contains_key(id) {
        return;
    }
    let Ok(peripheral) = adapter.peripheral(id).await else {
        return;
    };

    // Identidad del anuncio: el service data lleva el peer ID (8 bytes).
    let peer_hex = match peripheral.properties().await {
        Ok(Some(props)) => props
            .service_data
            .get(&Uuid::from_str(SERVICE_UUID).expect("uuid"))
            .map(hex::encode),
        _ => None,
    };
    if let Some(ref hex_id) = peer_hex {
        if hex_id == my_peer_hex {
            return; // nuestro propio anuncio
        }
    }

    if !peripheral.is_connected().await.unwrap_or(false) {
        let attempt = tokio::time::timeout(Duration::from_secs(5), peripheral.connect());
        match attempt.await {
            Ok(Ok(())) => {}
            _ => return,
        }
    }
    if peripheral.discover_services().await.is_err() {
        return;
    }

    let service_uuid = Uuid::from_str(SERVICE_UUID).expect("uuid");
    let char_uuid = Uuid::from_str(CHAR_UUID).expect("uuid");
    let characteristic = peripheral
        .characteristics()
        .iter()
        .find(|c| c.service_uuid == service_uuid && c.uuid == char_uuid)
        .cloned();
    let Some(characteristic) = characteristic else {
        let _ = peripheral.disconnect().await;
        return;
    };

    // Notificaciones (si no se pueden activar, se sigue pudiendo escribir).
    let _ = peripheral.subscribe(&characteristic).await;

    let listener = peripheral.clone();
    let sub_tx = peer_tx.clone();
    let sub_id = id.clone();
    tokio::spawn(async move {
        let Ok(mut notifications) = listener.notifications().await else {
            return;
        };
        while let Some(note) = notifications.next().await {
            if sub_tx.send((sub_id.clone(), note.value)).await.is_err() {
                break;
            }
        }
    });

    let label = peer_hex
        .as_deref()
        .map(|h| h[..8].to_string())
        .unwrap_or_else(|| short_id(id));
    links.insert(
        id.clone(),
        PeerLink {
            peripheral,
            characteristic,
            peer_hex,
        },
    );
    let _ = events.send(MeshEvent::Status(format!("vecino conectado: {label}")));
}

/// Identificación corta para la barra de estado.
fn short_id(id: &PeripheralId) -> String {
    let s = format!("{id:?}");
    s.chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(8)
        .collect()
}

/// Transmite un packet a todos los enlaces (write) y a los suscritos a
/// nuestra characteristic (notify). `exclude` deja fuera a un vecino.
async fn broadcast(
    links: &HashMap<PeripheralId, PeerLink>,
    bytes: &[u8],
    char: &CharHandle,
    conn: &zbus::Connection,
    exclude: Option<&PeripheralId>,
) {
    for (id, link) in links {
        if Some(id) == exclude {
            continue;
        }
        let _ = link
            .peripheral
            .write(&link.characteristic, bytes, WriteType::WithoutResponse)
            .await;
    }
    // A los suscritos a nuestro servidor: el cambio de "Value" se
    // convierte en notificación GATT para todos ellos.
    if *char.0.notifying.lock().unwrap() {
        *char.0.value.lock().unwrap() = bytes.to_vec();
        emit_value_changed(conn, bytes).await;
    }
}

/// Emite `PropertiesChanged("Value", …)` para notificar a los suscritos.
async fn emit_value_changed(conn: &zbus::Connection, value: &[u8]) {
    let Ok(emitter) = zbus::object_server::SignalEmitter::new(conn, CHAR_PATH) else {
        return;
    };
    let mut changed: HashMap<&str, Value<'_>> = HashMap::new();
    changed.insert("Value", Value::from(value));
    let iface = match zbus::names::InterfaceName::try_from("org.bluez.GattCharacteristic1") {
        Ok(i) => i,
        Err(_) => return,
    };
    let _ = zbus::fdo::Properties::properties_changed(
        &emitter,
        iface,
        changed,
        std::borrow::Cow::Borrowed(&[]),
    )
    .await;
}

/// Construye y firma el ANNOUNCE de identidad.
fn build_announce(identity: &Identity, nickname: &str) -> Result<Packet, String> {
    let ann = Announcement::new(nickname, identity.noise_public(), identity.signing_public());
    let payload = ann.encode().map_err(|e| e.to_string())?;
    let mut pkt = Packet::new(msg_type::ANNOUNCE, identity.peer_id(), payload);
    pkt.recipient = Some(BROADCAST);
    pkt.sign(identity.signing_key())
        .map_err(|e| e.to_string())?;
    Ok(pkt)
}

/// Envía el ANNOUNCE a todos los vecinos conectados.
async fn send_announce(
    identity: &Identity,
    nickname: &str,
    links: &HashMap<PeripheralId, PeerLink>,
    char: &CharHandle,
    conn: &zbus::Connection,
) {
    if let Ok(pkt) = build_announce(identity, nickname) {
        // Sin padding en el anuncio: es corto y no necesita normalizar.
        if let Ok(bytes) = pkt.encode(true) {
            broadcast(links, &bytes, char, conn, None).await;
        }
    }
}

/// Procesa un packet entrante: decodifica, deduplica, interpreta y retransmite.
// El estado de la malla son ocho piezas; agruparlo enlazaría peor el select.
#[allow(clippy::too_many_arguments)]
async fn handle_packet(
    bytes: Vec<u8>,
    from: Option<PeripheralId>,
    my_peer: &[u8; 8],
    links: &HashMap<PeripheralId, PeerLink>,
    seen: &mut HashSet<[u8; 8]>,
    seen_order: &mut VecDeque<[u8; 8]>,
    reassembler: &mut Reassembler,
    events: &Sender<MeshEvent>,
    char: &CharHandle,
    conn: &zbus::Connection,
) {
    let Ok(mut packet) = Packet::decode(&bytes) else {
        return;
    };

    // Fragmentos: reensamblar antes de interpretar.
    if packet.msg_type == msg_type::FRAGMENT {
        let Ok(frag) = Fragment::decode(&packet.payload) else {
            return;
        };
        let ttl = packet.ttl;
        let sender = packet.sender;
        let Some((_original_type, data)) = reassembler.push(frag) else {
            return;
        };
        let Ok(mut inner) = Packet::decode(&data) else {
            return;
        };
        // El reensamblado conserva el TTL mínimo del juego.
        inner.ttl = ttl.min(inner.ttl);
        inner.sender = sender;
        packet = inner;
    }

    // Un packet con destinatario que no somos nosotros: ni se entrega ni
    // se retransmite (así viajan los MDs).
    if let Some(r) = packet.recipient {
        if r != BROADCAST && r != *my_peer {
            return;
        }
    }

    // Dedup: un eco con TTL ya decrementado tiene la misma clave.
    let key = dedup_key(&packet);
    if !seen.insert(key) {
        return;
    }
    seen_order.push_back(key);
    if seen_order.len() > DEDUP_CAP {
        if let Some(old) = seen_order.pop_front() {
            seen.remove(&old);
        }
    }

    match packet.msg_type {
        msg_type::ANNOUNCE => {
            let Ok(ann) = Announcement::decode(&packet.payload) else {
                return;
            };
            // El ID reclamado debe derivar de la clave pública del anuncio.
            let claimed = hex::encode(packet.sender);
            if !crate::identity::matches_claimed_peer_id(&claimed, &ann.noise_public) {
                return;
            }
            let _ = events.send(MeshEvent::PeerJoined {
                id: claimed,
                nickname: ann.nickname,
                noise_public: hex::encode(ann.noise_public),
                signing_public: hex::encode(ann.signing_public),
            });
        }
        msg_type::MESSAGE => {
            // Texto plano (broadcast público) o payload binario con canal.
            let decoded = ChatMessage::decode(&packet.payload)
                .ok()
                .filter(|m| !m.content.is_empty() && !m.sender.is_empty());
            let (text, channel, claimed) = match decoded {
                Some(m) => (m.content, m.channel, Some(m.sender)),
                None => {
                    // El binario que no se entiende (o que la app de
                    // referencia reenvía mezclado con U+FFFD) jamás es un
                    // mensaje: no se cuela en el general.
                    let text = String::from_utf8_lossy(&packet.payload);
                    if !is_plain_chat(&text) {
                        wire_log(&format!(
                            "drop MESSAGE {} B de {} {}",
                            packet.payload.len(),
                            hex::encode(packet.sender),
                            hex::encode(&packet.payload[..packet.payload.len().min(200)])
                        ));
                        return;
                    }
                    (text.to_string(), None, None)
                }
            };
            if text.is_empty() {
                return;
            }
            wire_log(&format!(
                "MESSAGE de {} canal={:?} texto={:?}",
                hex::encode(packet.sender),
                channel,
                text.chars().take(60).collect::<String>()
            ));
            // El id completo (8 bytes en hex) deja a la TUI resolver el apodo.
            let from = hex::encode(packet.sender);
            let _ = events.send(MeshEvent::Message {
                from,
                nick: claimed,
                channel,
                text,
            });
        }
        other => {
            wire_log(&format!(
                "tipo {other:#04x} ignorado de {}",
                hex::encode(packet.sender)
            ));
        }
    }

    // Retransmisión (flood) mientras quede TTL. Los MDs van directos al
    // destinatario: reenviarlos a la malla sería ruido (y una fuga).
    if packet.ttl > 1 && packet.recipient.is_none_or(|r| r == BROADCAST) {
        packet.ttl -= 1;
        if let Ok(relayed) = packet.encode(true) {
            broadcast(links, &relayed, char, conn, from.as_ref()).await;
        }
    }
}

/// ¿Es texto plano de chat — y no binario ni texto mezclado por la app al
/// reenviarlo? Los U+FFFD y los controles (salvo salto de línea) delatan
/// un payload que no debería mostrarse como mensaje.
fn is_plain_chat(s: &str) -> bool {
    !s.is_empty()
        && !s.contains('\u{FFFD}')
        && s.chars()
            .all(|c| !c.is_control() || c == '\n' || c == '\t' || c == '\r')
}

/// Registro de diagnóstico del wire (best-effort, en /tmp): sirve para
/// ver qué llega de verdad sin ensuciar la interfaz.
fn wire_log(msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/bluebl_wire.log")
    {
        let _ = writeln!(f, "[{}] {msg}", crate::app::now_hm());
    }
}

// ── Ayudantes D-Bus ────────────────────────────────────────────────────

/// Busca el primer adaptador BlueZ en el bus del sistema.
async fn find_adapter(conn: &zbus::Connection) -> Result<OwnedObjectPath, String> {
    let proxy = zbus::Proxy::new(conn, "org.bluez", "/", "org.freedesktop.DBus.ObjectManager")
        .await
        .map_err(|e| e.to_string())?;
    let objects: HashMap<OwnedObjectPath, HashMap<String, HashMap<String, OwnedValue>>> = proxy
        .call_method("GetManagedObjects", &())
        .await
        .map_err(|e| format!("GetManagedObjects: {e}"))?
        .body()
        .deserialize()
        .map_err(|e| format!("cuerpo: {e}"))?;
    for (path, ifaces) in objects {
        if ifaces.contains_key("org.bluez.Adapter1") {
            return Ok(path);
        }
    }
    Err("BlueZ no ve ningún adaptador".into())
}

/// Enciende el adaptador si estaba apagado.
async fn ensure_powered(
    conn: &zbus::Connection,
    adapter_path: &OwnedObjectPath,
) -> Result<(), String> {
    // El proxy lleva la interfaz del adaptador: get/set_property envían
    // Get/Set con esa interfaz a org.freedesktop.DBus.Properties.
    let proxy = zbus::Proxy::new(
        conn,
        "org.bluez",
        adapter_path.as_str(),
        "org.bluez.Adapter1",
    )
    .await
    .map_err(|e| e.to_string())?;
    let powered: bool = proxy
        .get_property("Powered")
        .await
        .map_err(|e| e.to_string())?;
    if !powered {
        proxy
            .set_property("Powered", &true)
            .await
            .map_err(|e| format!("no se pudo encender la radio: {e}"))?;
    }
    Ok(())
}

/// Registra la aplicación GATT (servicio + characteristic).
async fn register_application(
    conn: &zbus::Connection,
    adapter_path: &OwnedObjectPath,
) -> Result<(), String> {
    let proxy = zbus::Proxy::new(
        conn,
        "org.bluez",
        adapter_path.as_str(),
        "org.bluez.GattManager1",
    )
    .await
    .map_err(|e| e.to_string())?;
    // El primer argumento es de tipo `o` (object path), no `s`.
    let root = OwnedObjectPath::try_from(GATT_ROOT).map_err(|e| e.to_string())?;
    let opts: HashMap<&str, Value<'_>> = HashMap::new();
    proxy
        .call_method("RegisterApplication", &(root, opts))
        .await
        .map_err(|e| format!("RegisterApplication: {e}"))?;
    Ok(())
}

/// Registra el anuncio LE.
async fn register_advertisement(
    conn: &zbus::Connection,
    adapter_path: &OwnedObjectPath,
) -> Result<(), String> {
    let proxy = zbus::Proxy::new(
        conn,
        "org.bluez",
        adapter_path.as_str(),
        "org.bluez.LEAdvertisingManager1",
    )
    .await
    .map_err(|e| e.to_string())?;
    let adv = OwnedObjectPath::try_from(ADV_PATH).map_err(|e| e.to_string())?;
    let opts: HashMap<&str, Value<'_>> = HashMap::new();
    proxy
        .call_method("RegisterAdvertisement", &(adv, opts))
        .await
        .map_err(|e| format!("RegisterAdvertisement: {e}"))?;
    Ok(())
}

async fn unregister_advertisement(
    conn: &zbus::Connection,
    adapter_path: &OwnedObjectPath,
) -> Result<(), String> {
    let proxy = zbus::Proxy::new(
        conn,
        "org.bluez",
        adapter_path.as_str(),
        "org.bluez.LEAdvertisingManager1",
    )
    .await
    .map_err(|e| e.to_string())?;
    let adv = OwnedObjectPath::try_from(ADV_PATH).map_err(|e| e.to_string())?;
    proxy
        .call_method("UnregisterAdvertisement", &(adv,))
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_plain_chat;

    #[test]
    fn texto_plano_pasa() {
        assert!(is_plain_chat("hola a todos"));
        assert!(is_plain_chat("ñandú ✦ lluvia\nsegunda línea"));
    }

    #[test]
    fn binario_o_mezcla_no_pasa() {
        // El garbled que devolvía la app: U+FFFD de sus trascodificaciones.
        assert!(!is_plain_chat(
            "\u{FFFD}\u{FFFD}d?\"f92f4cfe18e90153Kaomiho@dm"
        ));
        // Payload binario de verdad (bytes de control).
        assert!(!is_plain_chat("hola\u{1}mundo"));
        assert!(!is_plain_chat(""));
    }
}
