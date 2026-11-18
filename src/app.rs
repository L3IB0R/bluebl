//! Estado de la aplicación: canales, pares, mensajes, entrada y overlays.

use crate::commands::{self, Command};
use crate::config::Config;
use crate::identity::Identity;
use crate::mesh::MeshEvent;
use crate::themes::{self, Theme};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use std::sync::mpsc::Receiver;

/// Densidad de la interfaz (equivalente al ajuste de tamaño de letra del
/// terminal: controla el aire entre líneas y la altura de la entrada).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Density {
    Compacto,
    Comodo,
    Grande,
}

impl Density {
    pub fn parse(s: &str) -> Option<Density> {
        match s.trim().to_ascii_lowercase().as_str() {
            "compacto" | "compact" => Some(Density::Compacto),
            "comodo" | "cómodo" | "comfortable" => Some(Density::Comodo),
            "grande" | "large" => Some(Density::Grande),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Density::Compacto => "compacto",
            Density::Comodo => "comodo",
            Density::Grande => "grande",
        }
    }

    /// Nombre para mostrar (con acento).
    pub fn display(&self) -> &'static str {
        match self {
            Density::Compacto => "compacto",
            Density::Comodo => "cómodo",
            Density::Grande => "grande",
        }
    }

    /// Altura del cuadro de entrada en líneas.
    pub fn input_height(&self) -> u16 {
        match self {
            Density::Compacto => 1,
            Density::Comodo => 3,
            Density::Grande => 4,
        }
    }

    /// Margen vertical alrededor del chat.
    pub fn chat_padding(&self) -> u16 {
        match self {
            Density::Compacto => 0,
            Density::Comodo => 0,
            Density::Grande => 1,
        }
    }
}

/// Panel activo (para pantallas estrechas: un solo panel visible, Tab
/// lo cambia).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Panel {
    Canales,
    Chat,
    Pares,
}

impl Panel {
    pub fn next(&self) -> Panel {
        match self {
            Panel::Canales => Panel::Chat,
            Panel::Chat => Panel::Pares,
            Panel::Pares => Panel::Canales,
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            Panel::Canales => "Canales",
            Panel::Chat => "Chat",
            Panel::Pares => "Pares",
        }
    }
}

/// Capa superpuesta (ayuda o selector de temas).
#[derive(Clone, Debug)]
pub enum Overlay {
    Help,
    Themes {
        selected: usize,
    },
    /// Código de emparejamiento con su QR.
    Pair {
        code: String,
        qr: Vec<String>,
    },
}

/// Contacto verificado por emparejamiento: no es un random de la malla.
#[derive(Clone, Debug)]
pub struct Contact {
    pub id: String,
    pub nickname: String,
    pub noise_public: String,
    pub signing_public: String,
}

/// Clase de una línea del chat.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LineKind {
    Own,
    Peer,
    /// Mensaje privado (MD) entrante.
    Direct,
    System,
    Error,
    Action,
}

/// Una línea del chat.
#[derive(Clone, Debug)]
pub struct ChatLine {
    pub time: String,
    pub kind: LineKind,
    pub sender: String,
    pub text: String,
}

/// Un par visto en la malla.
#[derive(Clone, Debug)]
pub struct Peer {
    pub id: String,
    pub nickname: String,
    /// Contacto verificado por emparejamiento (★): no es un random.
    pub verified: bool,
    /// Claves públicas del announce (hex): con ellas se calcula la huella
    /// para compararla con la del teléfono y se guarda el contacto.
    pub noise_public: String,
    pub signing_public: String,
}

/// Caja de texto con cursor por caracteres (UTF-8 seguro).
#[derive(Clone, Debug, Default)]
pub struct Input {
    text: Vec<char>,
    cursor: usize,
}

impl Input {
    pub fn as_str(&self) -> String {
        self.text.iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn insert(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.text.remove(self.cursor);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.text.len() {
            self.text.remove(self.cursor);
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        if self.cursor < self.text.len() {
            self.cursor += 1;
        }
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.text.len();
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }
}

/// Estado completo de BlueBl.
pub struct App {
    pub running: bool,
    pub identity: Identity,
    pub nickname: String,
    pub config: Config,
    pub themes: Vec<Theme>,
    pub theme_idx: usize,
    pub density: Density,
    pub channels: Vec<String>,
    pub channel_idx: usize,
    pub messages: HashMap<String, Vec<ChatLine>>,
    pub peers: Vec<Peer>,
    /// Selección dentro del panel Pares (↑↓ con el foco ahí).
    pub peer_sel: usize,
    /// MDs abiertos: canal `@nick` → peer id del otro extremo.
    pub dm_ids: HashMap<String, String>,
    /// Contactos verificados (emparejados): peer id → contacto.
    pub contacts: HashMap<String, Contact>,
    pub input: Input,
    pub status: String,
    pub panel: Panel,
    pub overlay: Option<Overlay>,
    /// Líneas contadas desde abajo; 0 = anclado al final.
    pub scroll: usize,
    pub rx: Receiver<MeshEvent>,
    /// Emisor hacia la radio (None = sin backend: sólo mensajes locales).
    pub net: Option<crate::net::NetTx>,
    /// Último ancho alto para decidir el layout (lo escribe ui).
    pub last_width: u16,
}

impl App {
    pub fn new(config: Config, rx: Receiver<MeshEvent>, identity: Identity) -> App {
        let themes = themes::all_themes();
        let theme_idx = themes
            .iter()
            .position(|t| t.id == config.theme)
            .unwrap_or(0);
        let density = Density::parse(&config.density).unwrap_or(Density::Comodo);
        let contacts = load_contacts(&config);
        let mut messages = HashMap::new();
        messages.insert(
            "#general".to_string(),
            vec![ChatLine {
                time: now_hm(),
                kind: LineKind::System,
                sender: String::new(),
                text: "bienvenida a BlueBl · escribe /help para los comandos".into(),
            }],
        );
        App {
            running: true,
            identity,
            nickname: config.nick.clone(),
            config,
            themes,
            theme_idx,
            density,
            channels: vec!["#general".to_string()],
            channel_idx: 0,
            messages,
            peers: Vec::new(),
            peer_sel: 0,
            dm_ids: HashMap::new(),
            contacts,
            input: Input::default(),
            status: "sin radio (fase de protocolo)".into(),
            panel: Panel::Chat,
            overlay: None,
            scroll: 0,
            rx,
            net: None,
            last_width: 0,
        }
    }

    /// Conecta la radio: todo lo que se envíe saldrá por la malla.
    pub fn attach_net(&mut self, tx: crate::net::NetTx) {
        self.net = Some(tx);
        self.status = "conectando la radio…".into();
    }

    pub fn theme(&self) -> &Theme {
        &self.themes[self.theme_idx]
    }

    pub fn channel(&self) -> &str {
        &self.channels[self.channel_idx]
    }

    pub fn lines(&self) -> &[ChatLine] {
        self.messages
            .get(self.channel())
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// peer id local en hex (16 chars).
    pub fn peer_id_hex(&self) -> String {
        hex::encode(self.identity.peer_id())
    }

    fn push_line(&mut self, line: ChatLine) {
        let ch = self.channel().to_string();
        let list = self.messages.entry(ch).or_default();
        list.push(line);
        // Límite de memoria por canal.
        if list.len() > 2_000 {
            let drain = list.len() - 2_000;
            list.drain(0..drain);
        }
        self.scroll = 0;
    }

    pub fn system(&mut self, text: impl Into<String>) {
        self.push_line(ChatLine {
            time: now_hm(),
            kind: LineKind::System,
            sender: String::new(),
            text: text.into(),
        });
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.push_line(ChatLine {
            time: now_hm(),
            kind: LineKind::Error,
            sender: String::new(),
            text: text.into(),
        });
    }

    /// Desplaza hacia arriba (hacia líneas más antiguas).
    pub fn scroll_up(&mut self, n: usize) {
        let max = self.lines().len().saturating_sub(1);
        self.scroll = (self.scroll + n).min(max);
    }

    /// Desplaza hacia abajo; al llegar al final se ancla.
    pub fn scroll_down(&mut self, n: usize) {
        self.scroll = self.scroll.saturating_sub(n);
    }

    pub fn scroll_bottom(&mut self) {
        self.scroll = 0;
    }

    /// Mueve la selección de canal (foco en el panel Canales).
    fn move_channel(&mut self, delta: isize) {
        if self.channels.is_empty() {
            return;
        }
        let len = self.channels.len() as isize;
        let next = (self.channel_idx as isize + delta).rem_euclid(len) as usize;
        if next != self.channel_idx {
            self.channel_idx = next;
            self.scroll = 0;
        }
    }

    /// Mueve la selección de par (foco en el panel Pares).
    fn move_peer(&mut self, delta: isize) {
        if self.peers.is_empty() {
            return;
        }
        let last = self.peers.len() - 1;
        let cur = self.peer_sel.min(last) as isize;
        self.peer_sel = (cur + delta).clamp(0, last as isize) as usize;
    }

    /// Abre (o reabre) el canal privado con un par y lo enfoca.
    fn open_dm(&mut self, nick: &str, peer_id: &str) {
        let ch = format!("@{nick}");
        if !self.channels.contains(&ch) {
            self.channels.push(ch.clone());
        }
        self.dm_ids.insert(ch.clone(), peer_id.to_string());
        if let Some(idx) = self.channels.iter().position(|c| *c == ch) {
            self.channel_idx = idx;
            self.scroll = 0;
        }
    }

    /// Muestra una huella digital para comparar con la del teléfono
    /// (la app la muestra bajo el icono de candado del contacto).
    /// Sin apodo: la nuestra; con apodo: la de ese par de la malla.
    fn print_fingerprint(&mut self, nick: Option<&str>) {
        match nick {
            None => {
                let fp = crate::pairing::fingerprint(&self.identity.noise_public());
                self.system("tu huella (compárala con la del teléfono):");
                for line in crate::pairing::format_fingerprint(&fp).lines() {
                    self.system(format!("  {line}"));
                }
            }
            Some(name) => {
                let Some(p) = self
                    .peers
                    .iter()
                    .find(|p| p.nickname.eq_ignore_ascii_case(name))
                    .cloned()
                else {
                    self.error(format!("no veo a «{name}» en la malla — prueba /who"));
                    return;
                };
                let Some(fp) = crate::pairing::fingerprint_from_hex(&p.noise_public) else {
                    self.error("la clave pública de ese par es ilegible");
                    return;
                };
                self.system(format!("huella de {} ({}):", p.nickname, &p.id[..8]));
                for line in crate::pairing::format_fingerprint(&fp).lines() {
                    self.system(format!("  {line}"));
                }
                self.system(format!(
                    "si coincide con la del teléfono: /accept {}",
                    p.nickname
                ));
            }
        }
    }

    /// Procesa los eventos que llegan del backend de malla.
    pub fn drain_mesh(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                MeshEvent::PeerJoined {
                    id,
                    nickname,
                    noise_public,
                    signing_public,
                } => {
                    // Un contacto se verifica por sus claves: si cambian,
                    // no es (ya no es) la persona que emparejaste.
                    let known = self.contacts.get(&id);
                    let verified = known.is_some_and(|c| {
                        c.noise_public == noise_public && c.signing_public == signing_public
                    });
                    match self.peers.iter().position(|p| p.id == id) {
                        None => {
                            self.peers.push(Peer {
                                id: id.clone(),
                                nickname: nickname.clone(),
                                verified,
                                noise_public: noise_public.clone(),
                                signing_public: signing_public.clone(),
                            });
                            if verified {
                                self.system(format!("★ {nickname} — contacto verificado"));
                            } else if known.is_some() {
                                self.system(format!(
                                    "⚠ {nickname} está en tus contactos pero su clave cambió"
                                ));
                            } else {
                                self.system(format!("• {nickname} se une a la malla"));
                            }
                        }
                        Some(pos) => {
                            // Anuncio repetido: se actualiza el apodo, las
                            // claves y el estado de contacto sin spamear el
                            // chat.
                            let p = &mut self.peers[pos];
                            if p.nickname != nickname {
                                p.nickname = nickname;
                            }
                            p.verified = verified;
                            p.noise_public = noise_public;
                            p.signing_public = signing_public;
                        }
                    }
                }
                MeshEvent::PeerLeft { id } => {
                    if let Some(pos) = self.peers.iter().position(|p| p.id == id) {
                        let p = self.peers.remove(pos);
                        self.system(format!("• {} deja la malla", p.nickname));
                    }
                }
                MeshEvent::Message {
                    from,
                    nick,
                    channel,
                    text,
                } => {
                    // Nuestro propio eco (malla con saltos): ya está en el chat.
                    if from == self.peer_id_hex() {
                        continue;
                    }
                    // Apodo: announce validado > declaración del payload > id.
                    let sender = self
                        .peers
                        .iter()
                        .find(|p| p.id == from)
                        .map(|p| p.nickname.clone())
                        .or(nick.filter(|n| !n.is_empty()))
                        .unwrap_or_else(|| from.chars().take(8).collect());

                    // Un MD se abre como "@nick de quien escribe"; el resto
                    // va a su canal (#general si el texto es plano).
                    let ch = match &channel {
                        Some(c) if c.starts_with('@') => format!("@{sender}"),
                        other => other.clone().unwrap_or_else(|| "#general".to_string()),
                    };
                    let is_dm = ch.starts_with('@');
                    let was_new = !self.channels.contains(&ch);
                    if was_new {
                        self.channels.push(ch.clone());
                    }
                    let kind = if is_dm {
                        LineKind::Direct
                    } else {
                        LineKind::Peer
                    };
                    self.messages.entry(ch.clone()).or_default().push(ChatLine {
                        time: now_hm(),
                        kind,
                        sender: sender.clone(),
                        text,
                    });
                    if is_dm {
                        // Un MD manda la vista a su canal: se abre solo y se
                        // ve al momento, como si fuera el general.
                        if let Some(idx) = self.channels.iter().position(|c| *c == ch) {
                            self.channel_idx = idx;
                        }
                        self.panel = Panel::Chat;
                        self.scroll = 0;
                        self.system(format!("✉ md de {sender}"));
                    } else {
                        if was_new {
                            self.system(format!("nuevo canal: {ch}"));
                        }
                        if ch != self.channel() {
                            self.system(format!("mensaje nuevo en {ch}"));
                        }
                        self.scroll = 0;
                    }
                }
                MeshEvent::Status(s) => self.status = s,
            }
        }
    }

    /// Procesa una tecla pulsada.
    pub fn on_key(&mut self, key: KeyEvent) {
        // Salida: ctrl+q (y ctrl+c, que en modo crudo no llega como señal).
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('q') | KeyCode::Char('c') => {
                    self.running = false;
                    return;
                }
                KeyCode::Char('t') => {
                    self.overlay = Some(Overlay::Themes {
                        selected: self.theme_idx,
                    });
                    return;
                }
                _ => {}
            }
        }

        // Overlays: se cierran o se navegan antes que nada.
        if let Some(overlay) = self.overlay.take() {
            match overlay {
                Overlay::Help => match key.code {
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::F(1) => {}
                    _ => self.overlay = Some(Overlay::Help),
                },
                Overlay::Pair { code, qr } => match key.code {
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {}
                    _ => self.overlay = Some(Overlay::Pair { code, qr }),
                },
                Overlay::Themes { mut selected } => match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => {}
                    KeyCode::Up => {
                        selected = selected.saturating_sub(1);
                        self.overlay = Some(Overlay::Themes { selected });
                    }
                    KeyCode::Down => {
                        selected = (selected + 1).min(self.themes.len() - 1);
                        self.overlay = Some(Overlay::Themes { selected });
                    }
                    KeyCode::PageUp => {
                        selected = selected.saturating_sub(10);
                        self.overlay = Some(Overlay::Themes { selected });
                    }
                    KeyCode::PageDown => {
                        selected = (selected + 10).min(self.themes.len() - 1);
                        self.overlay = Some(Overlay::Themes { selected });
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        self.theme_idx = selected;
                        self.config.theme = self.themes[selected].id.to_string();
                        self.config.save();
                        self.system(format!(
                            "tema: {} ({})",
                            self.themes[selected].name,
                            self.themes[selected].category.name()
                        ));
                    }
                    KeyCode::Home => {
                        selected = 0;
                        self.overlay = Some(Overlay::Themes { selected });
                    }
                    KeyCode::End => {
                        selected = self.themes.len() - 1;
                        self.overlay = Some(Overlay::Themes { selected });
                    }
                    _ => self.overlay = Some(Overlay::Themes { selected }),
                },
            }
            return;
        }

        match key.code {
            KeyCode::F(1) => self.overlay = Some(Overlay::Help),
            KeyCode::Tab => self.panel = self.panel.next(),
            KeyCode::Enter => self.send(),
            KeyCode::Esc => self.input.clear(),
            // Con el foco en un panel lateral, ↑↓ seleccionan ahí; en el
            // chat siguen desplazando la conversación.
            KeyCode::Up => match self.panel {
                Panel::Canales => self.move_channel(-1),
                Panel::Pares => self.move_peer(-1),
                Panel::Chat => self.scroll_up(3),
            },
            KeyCode::Down => match self.panel {
                Panel::Canales => self.move_channel(1),
                Panel::Pares => self.move_peer(1),
                Panel::Chat => self.scroll_down(3),
            },
            KeyCode::PageUp => self.scroll_up(20),
            KeyCode::PageDown => self.scroll_down(20),
            KeyCode::Home if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_up(usize::MAX / 2)
            }
            KeyCode::End if key.modifiers.contains(KeyModifiers::CONTROL) => self.scroll_bottom(),
            KeyCode::Backspace => self.input.backspace(),
            KeyCode::Delete => self.input.delete(),
            KeyCode::Left => self.input.left(),
            KeyCode::Right => self.input.right(),
            KeyCode::Home => self.input.home(),
            KeyCode::End => self.input.end(),
            KeyCode::Char(c) => self.input.insert(c),
            _ => {}
        }
    }

    /// Envía lo que hay en la entrada (comando o mensaje).
    pub fn send(&mut self) {
        let text = self.input.as_str();
        self.input.clear();
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        if text.starts_with('/') {
            match commands::parse(&text) {
                Some(cmd) => self.run_command(cmd),
                None => self.error("comando incompleto — /help"),
            }
        } else {
            // Con el foco en Pares, escribir es un MD al par seleccionado:
            // ni se pasa por el general ni lo ven los demás.
            if self.panel == Panel::Pares {
                if self.peers.is_empty() {
                    self.error("no hay pares cerca todavía");
                    return;
                }
                let idx = self.peer_sel.min(self.peers.len() - 1);
                let nick = self.peers[idx].nickname.clone();
                let id = self.peers[idx].id.clone();
                self.open_dm(&nick, &id);
                self.panel = Panel::Chat;
            }
            let line = ChatLine {
                time: now_hm(),
                kind: LineKind::Own,
                sender: self.nickname.clone(),
                text: text.clone(),
            };
            self.push_line(line);
            self.transmit(&text);
        }
    }

    /// Codifica, firma y transmite un mensaje por la malla (si hay radio).
    fn transmit(&mut self, text: &str) {
        use crate::net::{fragment_packet, NetCommand};
        use crate::proto::packet::msg_type;
        use crate::proto::{ChatMessage, Packet, BROADCAST};

        let Some(net) = self.net.clone() else {
            return; // sin radio: el mensaje se quedó en pantalla
        };

        // Un MD lleva el id del otro extremo en el `recipient` de la
        // cabecera: sólo el destinatario lo descifra y nadie lo reenvía.
        let mut target: Option<String> = None;
        if let Some(dm_nick) = self.channel().strip_prefix('@') {
            let found = self.dm_ids.get(self.channel()).cloned().or_else(|| {
                self.peers
                    .iter()
                    .find(|p| p.nickname == dm_nick)
                    .map(|p| p.id.clone())
            });
            let Some(id) = found else {
                self.error(format!("«{dm_nick}» ya no está en la malla"));
                return;
            };
            target = Some(id);
        }

        // En el canal público viaja texto plano (como la app de referencia);
        // en el resto, payload binario con el canal marcado. Los MDs usan
        // el marcador fijo "@dm": el receptor abre el canal con el nick
        // del emisor, no con el nombre local que le pusimos aquí.
        let channel = self.channel().to_string();
        let payload = if channel == "#general" {
            text.as_bytes().to_vec()
        } else {
            let mut m = ChatMessage::new(self.nickname.clone(), text);
            m.channel = Some(if target.is_some() {
                "@dm".to_string()
            } else {
                channel
            });
            m.encode()
        };

        let mut pkt = Packet::new(msg_type::MESSAGE, self.identity.peer_id(), payload);
        pkt.recipient = match &target {
            Some(id) => parse_peer_id(id),
            None => Some(BROADCAST),
        };
        if let Err(e) = pkt.sign(self.identity.signing_key()) {
            self.error(format!("no se pudo firmar: {e}"));
            return;
        }

        // El envío por enlace: MD sólo al destinatario, el resto a todos.
        let send_bytes = |bytes: Vec<u8>| -> bool {
            match &target {
                Some(id) => net
                    .send(NetCommand::SendDm {
                        peer_id: id.clone(),
                        bytes,
                    })
                    .is_ok(),
                None => net.send(NetCommand::SendPacket(bytes)).is_ok(),
            }
        };

        let sent = match fragment_packet(&pkt) {
            Ok(Some(frags)) => frags
                .into_iter()
                .filter_map(|f| f.encode(true).ok())
                .all(send_bytes),
            Ok(None) => pkt.encode(true).map(send_bytes).unwrap_or(false),
            Err(e) => {
                self.error(format!("no se pudo enviar: {e}"));
                return;
            }
        };
        if !sent && self.net.is_some() {
            self.error("la radio no acepta el paquete");
        }
    }

    fn run_command(&mut self, cmd: Command) {
        match cmd {
            Command::Help => self.overlay = Some(Overlay::Help),
            Command::Quit => self.running = false,
            Command::Nick(new) => {
                if new.is_empty() {
                    self.system(format!("tu apodo es {}", self.nickname));
                    return;
                }
                let old = std::mem::replace(&mut self.nickname, new.clone());
                self.config.nick = new.clone();
                self.config.save();
                if let Some(net) = self.net.clone() {
                    let _ = net.send(crate::net::NetCommand::SetNickname(new.clone()));
                }
                self.system(format!("{old} ahora es {new}"));
            }
            Command::Join(channel) => {
                if channel.is_empty() {
                    self.error("uso: /join #canal");
                    return;
                }
                let channel = if channel.starts_with('#') {
                    channel
                } else {
                    format!("#{channel}")
                };
                if !self.channels.contains(&channel) {
                    self.channels.push(channel.clone());
                    self.system(format!("entras en {channel}"));
                }
                if let Some(idx) = self.channels.iter().position(|c| *c == channel) {
                    self.channel_idx = idx;
                    self.scroll = 0;
                }
            }
            Command::Part => {
                if self.channels.len() == 1 {
                    self.error("es el único canal; /join #otro para cambiar");
                    return;
                }
                let old = self.channels.remove(self.channel_idx);
                if self.channel_idx >= self.channels.len() {
                    self.channel_idx = self.channels.len() - 1;
                }
                self.system(format!("sales de {old}"));
                self.scroll = 0;
            }
            Command::Msg { to, text } => {
                if to.is_empty() || text.is_empty() {
                    self.error("uso: /msg apodo texto");
                    return;
                }
                let Some(peer) = self
                    .peers
                    .iter()
                    .find(|p| p.nickname.eq_ignore_ascii_case(&to))
                    .cloned()
                else {
                    self.error(format!("no veo a «{to}» en la malla — prueba /who"));
                    return;
                };
                self.open_dm(&peer.nickname, &peer.id);
                self.panel = Panel::Chat;
                self.push_line(ChatLine {
                    time: now_hm(),
                    kind: LineKind::Own,
                    sender: self.nickname.clone(),
                    text: text.clone(),
                });
                self.transmit(&text);
            }
            Command::Me(action) => {
                if action.is_empty() {
                    self.error("uso: /me acción");
                    return;
                }
                self.push_line(ChatLine {
                    time: now_hm(),
                    kind: LineKind::Action,
                    sender: self.nickname.clone(),
                    text: action,
                });
            }
            Command::Who | Command::Peers => {
                if self.peers.is_empty() {
                    self.system("malla vacía: aún no hay nadie cerca");
                } else {
                    let names: Vec<String> = self
                        .peers
                        .iter()
                        .map(|p| {
                            let star = if p.verified { "★ " } else { "" };
                            format!("{star}{} ({})", p.nickname, &p.id[..8])
                        })
                        .collect();
                    self.system(format!(
                        "{} en la malla: {}",
                        self.peers.len(),
                        names.join(" · ")
                    ));
                }
            }
            Command::Theme(None) => {
                self.overlay = Some(Overlay::Themes {
                    selected: self.theme_idx,
                });
            }
            Command::Theme(Some(id)) => match themes::find(&id) {
                Some(t) => {
                    self.theme_idx = self.themes.iter().position(|x| x.id == t.id).unwrap();
                    self.config.theme = t.id.to_string();
                    self.config.save();
                    self.system(format!("tema: {}", t.name));
                }
                None => self.error(format!("tema desconocido: {id} — /theme")),
            },
            Command::Density(name) => match Density::parse(&name) {
                Some(d) => {
                    self.density = d;
                    self.config.density = d.name().to_string();
                    self.config.save();
                    self.system(format!(
                        "densidad: {} (la fuente se cambia en tu terminal)",
                        d.name()
                    ));
                }
                None => self.error("uso: /density compacto|comodo|grande"),
            },
            Command::Clear => {
                let ch = self.channel().to_string();
                if let Some(list) = self.messages.get_mut(&ch) {
                    list.clear();
                }
                self.scroll = 0;
            }
            Command::Deps => {
                for line in crate::deps::report() {
                    if line.starts_with(' ') {
                        self.system(line);
                    } else if line.contains("✗") {
                        self.error(line);
                    } else {
                        self.system(line);
                    }
                }
            }
            Command::Pair => {
                let ts = now_unix();
                let mut nonce = [0u8; 16];
                rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut nonce);
                let url =
                    crate::pairing::build_verify_url(&self.identity, &self.nickname, ts, &nonce);
                let qr = qr_lines(&url);
                if qr.is_empty() {
                    self.error("no se pudo dibujar el QR — copia la URL de abajo");
                } else {
                    self.system("QR listo: escanéalo con la app (dura 5 min)");
                }
                self.print_fingerprint(None);
                self.system(format!("tu QR: {url}"));
                self.overlay = Some(Overlay::Pair { code: url, qr });
            }
            Command::Accept(raw) => {
                let raw = raw.trim();
                if raw.is_empty() {
                    self.error("uso: /accept APODO · /accept BB1|… · /accept bitchat://verify?…");
                    return;
                }
                // Tres formas de dar de alta: apodo de un par en la malla
                // (huella ya comparada a ojo), código BB1 o QR de la app.
                let parsed: Result<Contact, String> = if raw.starts_with("bitchat://") {
                    crate::pairing::parse_verify_url(raw, now_unix()).map(|s| Contact {
                        id: s.id,
                        nickname: s.nickname,
                        noise_public: s.noise_public,
                        signing_public: s.signing_public,
                    })
                } else if raw.starts_with("BB1|") {
                    parse_pair_code(raw)
                } else {
                    self.peers
                        .iter()
                        .find(|p| p.nickname.eq_ignore_ascii_case(raw))
                        .map(|p| Contact {
                            id: p.id.clone(),
                            nickname: p.nickname.clone(),
                            noise_public: p.noise_public.clone(),
                            signing_public: p.signing_public.clone(),
                        })
                        .ok_or_else(|| {
                            format!("no veo a «{raw}» en la malla — prueba /fp {raw} o /who")
                        })
                };
                match parsed {
                    Ok(c) => {
                        let id = c.id.clone();
                        let msg = if self.contacts.contains_key(&id) {
                            format!("★ {} ya era tu contacto", c.nickname)
                        } else {
                            format!("★ {} añadido a contactos verificados", c.nickname)
                        };
                        self.contacts.insert(id.clone(), c);
                        save_contacts(&self.config, &self.contacts);
                        if let Some(p) = self.peers.iter_mut().find(|p| p.id == id) {
                            p.verified = true;
                        }
                        self.system(msg);
                    }
                    Err(e) => self.error(format!("contacto rechazado: {e}")),
                }
            }
            Command::Fp(nick) => {
                self.print_fingerprint(nick.as_deref());
            }
            Command::Unknown(name) => {
                self.error(format!("comando desconocido: /{name} — prueba /help"));
            }
        }
    }
}

/// Hora local como "HH:MM".
pub fn now_hm() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // UTC fiable en cualquier plataforma; el ajuste lo pone el terminal.
    format!("{:02}:{:02}", (secs / 3600) % 24, (secs / 60) % 60)
}

/// Segundos Unix (para la frescura del QR de emparejamiento).
fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Peer ID en hex de 16 caracteres → 8 bytes. Formato inválido: None.
fn parse_peer_id(hex_id: &str) -> Option<crate::proto::PeerId> {
    let bytes = hex::decode(hex_id).ok()?;
    bytes.try_into().ok()
}

/// Ruta del archivo de contactos (hermano del config y de la identidad).
fn contacts_path(config: &Config) -> std::path::PathBuf {
    config.identity_path().with_file_name("contacts")
}

/// Carga los contactos: una línea `id|nick|noise|signing` por contacto.
fn load_contacts(config: &Config) -> HashMap<String, Contact> {
    let mut out = HashMap::new();
    let Ok(text) = std::fs::read_to_string(contacts_path(config)) else {
        return out;
    };
    for line in text.lines() {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() == 4 && parts[0].len() == 16 {
            out.insert(
                parts[0].to_string(),
                Contact {
                    id: parts[0].to_string(),
                    nickname: parts[1].to_string(),
                    noise_public: parts[2].to_string(),
                    signing_public: parts[3].to_string(),
                },
            );
        }
    }
    out
}

/// Reescribe el archivo de contactos completo (es pequeño).
fn save_contacts(config: &Config, contacts: &HashMap<String, Contact>) {
    let path = contacts_path(config);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut body = String::from("# contactos verificados de BlueBl\n");
    for c in contacts.values() {
        body.push_str(&format!(
            "{}|{}|{}|{}\n",
            c.id, c.nickname, c.noise_public, c.signing_public
        ));
    }
    let _ = std::fs::write(path, body);
}

/// Validación de un código BB1: el id debe derivar de la clave pública
/// ruidosa (la misma regla canónica que el announce).
fn parse_pair_code(raw: &str) -> Result<Contact, String> {
    let code = raw.trim();
    let parts: Vec<&str> = code.split('|').collect();
    if parts.len() != 5 || parts[0] != "BB1" {
        return Err("no es un código BlueBl (empieza por BB1|)".into());
    }
    if parts[2].len() != 64 || parts[3].len() != 64 {
        return Err("las claves del código tienen un tamaño inválido".into());
    }
    let noise: [u8; 32] = hex::decode(parts[2])
        .map_err(|_| "clave pública ilegible".to_string())?
        .try_into()
        .map_err(|_| "clave pública con tamaño distinto de 32 bytes".to_string())?;
    let _: [u8; 32] = hex::decode(parts[3])
        .map_err(|_| "clave de firma ilegible".to_string())?
        .try_into()
        .map_err(|_| "clave de firma con tamaño distinto de 32 bytes".to_string())?;
    if !crate::identity::matches_claimed_peer_id(parts[1], &noise) {
        return Err("el id no corresponde a esa clave pública".into());
    }
    if parts[4].is_empty() {
        return Err("el código no trae apodo".into());
    }
    Ok(Contact {
        id: parts[1].to_string(),
        nickname: parts[4].to_string(),
        noise_public: parts[2].to_string(),
        signing_public: parts[3].to_string(),
    })
}

/// Dibuja un QR como bloques Unicode para el overlay. Corrección de
/// error baja (L): la URL de la app es larga y menos redundancia = menos
/// módulos = cabe en la terminal.
fn qr_lines(data: &str) -> Vec<String> {
    let Ok(qr) = qrcode::QrCode::with_error_correction_level(data.as_bytes(), qrcode::EcLevel::L)
    else {
        return Vec::new();
    };
    qr.render::<qrcode::render::unicode::Dense1x2>()
        .quiet_zone(true)
        .build()
        .split('\n')
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    fn test_app() -> (App, std::sync::mpsc::Sender<MeshEvent>) {
        let (tx, rx) = channel();
        // Subdirectorio propio: los tests no pisan la config real ni se
        // pisan entre sí al persistir identidad/contactos.
        let cfg = Config {
            nick: "prueba".into(),
            path: Some(
                std::env::temp_dir()
                    .join(format!("bluebl-tests-{}", std::process::id()))
                    .join("config"),
            ),
            ..Config::default()
        };
        (App::new(cfg, rx, Identity::generate()), tx)
    }

    #[test]
    fn arranca_con_un_canal_y_bienvenida() {
        let (app, _tx) = test_app();
        assert_eq!(app.channels, vec!["#general"]);
        assert_eq!(app.nickname, "prueba");
        assert!(!app.lines().is_empty());
    }

    #[test]
    fn densidad_parseada() {
        assert_eq!(Density::parse("compacto"), Some(Density::Compacto));
        assert_eq!(Density::parse("Comodo"), Some(Density::Comodo));
        assert_eq!(Density::parse("grande"), Some(Density::Grande));
        assert_eq!(Density::parse("gigante"), None);
        assert_eq!(Density::Comodo.input_height(), 3);
    }

    #[test]
    fn enviar_texto_y_comandos() {
        let (mut app, _tx) = test_app();
        app.input.insert('h');
        app.input.insert('o');
        app.input.insert('l');
        app.input.insert('a');
        app.send();
        let last = app.lines().last().unwrap();
        assert_eq!(last.kind, LineKind::Own);
        assert_eq!(last.text, "hola");

        for c in "/nick luna".chars() {
            app.input.insert(c);
        }
        app.send();
        assert_eq!(app.nickname, "luna");
    }

    #[test]
    fn join_y_part_de_canales() {
        let (mut app, _tx) = test_app();
        for c in "/join neon".chars() {
            app.input.insert(c);
        }
        app.send();
        assert!(app.channels.contains(&"#neon".to_string()));
        assert_eq!(app.channel(), "#neon");
        // /part en el único canal extraído vuelve a #general.
        for c in "/part".chars() {
            app.input.insert(c);
        }
        app.send();
        assert_eq!(app.channel(), "#general");
    }

    #[test]
    fn tema_por_comando() {
        let (mut app, _tx) = test_app();
        for c in "/theme dracula".chars() {
            app.input.insert(c);
        }
        app.send();
        assert_eq!(app.theme().id, "dracula");
    }

    #[test]
    fn teclas_de_entrada_utf8() {
        let mut input = Input::default();
        for c in "ñandú".chars() {
            input.insert(c);
        }
        assert_eq!(input.as_str(), "ñandú");
        input.backspace();
        assert_eq!(input.as_str(), "ñand");
        input.home();
        input.delete();
        assert_eq!(input.as_str(), "and");
    }

    #[test]
    fn eventos_de_malla_llegan_al_chat() {
        let (mut app, tx) = test_app();
        tx.send(MeshEvent::PeerJoined {
            id: "aabbccdd00112233".into(),
            nickname: "luna".into(),
            noise_public: String::new(),
            signing_public: String::new(),
        })
        .unwrap();
        tx.send(MeshEvent::Message {
            from: "luna".into(),
            nick: None,
            channel: None,
            text: "¿alguien ahí?".into(),
        })
        .unwrap();
        app.drain_mesh();
        assert_eq!(app.peers.len(), 1);
        let last = app.lines().last().unwrap();
        assert_eq!(last.kind, LineKind::Peer);
        assert_eq!(last.text, "¿alguien ahí?");
    }

    #[test]
    fn mensaje_resuelve_apodo_del_announce() {
        let (mut app, tx) = test_app();
        tx.send(MeshEvent::PeerJoined {
            id: "aabbccdd00112233".into(),
            nickname: "luna".into(),
            noise_public: String::new(),
            signing_public: String::new(),
        })
        .unwrap();
        // El announce está validado por clave pública: manda sobre lo que
        // el payload declare.
        tx.send(MeshEvent::Message {
            from: "aabbccdd00112233".into(),
            nick: Some("otro_apodo".into()),
            channel: None,
            text: "hola".into(),
        })
        .unwrap();
        app.drain_mesh();
        assert_eq!(app.lines().last().unwrap().sender, "luna");
    }

    #[test]
    fn broadcast_publico_va_a_general() {
        let (mut app, tx) = test_app();
        for c in "/join #neon".chars() {
            app.input.insert(c);
        }
        app.send();
        assert_eq!(app.channel(), "#neon");

        tx.send(MeshEvent::Message {
            from: "1122334455667788".into(),
            nick: Some("luna".into()),
            channel: None,
            text: "para todos".into(),
        })
        .unwrap();
        app.drain_mesh();

        let gen = app.messages.get("#general").expect("broadcast en #general");
        assert_eq!(gen.last().unwrap().text, "para todos");
        // No secuestra el canal que estás mirando.
        assert_eq!(app.channel(), "#neon");
    }

    #[test]
    fn canal_entrante_se_abre_solo() {
        let (mut app, tx) = test_app();
        tx.send(MeshEvent::Message {
            from: "1122334455667788".into(),
            nick: Some("luna".into()),
            channel: Some("#soporte".into()),
            text: "¿hay alguien?".into(),
        })
        .unwrap();
        app.drain_mesh();
        assert!(app.channels.iter().any(|c| c == "#soporte"));
        assert!(app.messages.contains_key("#soporte"));
    }

    #[test]
    fn md_wire_round_trip_completo() {
        // El camino exacto de un MD: transmit() construye, el receptor
        // decodifica. Si esto falla, el MD no funciona en la malla.
        use crate::proto::packet::msg_type;
        use crate::proto::Packet;
        let id = Identity::generate();
        let target: crate::proto::PeerId = [0x55; 8];
        let mut m = crate::proto::ChatMessage::new("kaomi", "hola en privado");
        m.channel = Some("@dm".to_string());
        let mut pkt = Packet::new(msg_type::MESSAGE, id.peer_id(), m.encode());
        pkt.recipient = Some(target);
        pkt.sign(id.signing_key()).unwrap();
        let bytes = pkt.encode(true).unwrap();

        let dec = Packet::decode(&bytes).expect("el receptor debe leer el packet");
        assert_eq!(dec.recipient, Some(target));
        let m2 = crate::proto::ChatMessage::decode(&dec.payload).expect("payload legible");
        assert_eq!(m2.content, "hola en privado");
        assert_eq!(m2.channel.as_deref(), Some("@dm"));
        assert_eq!(m2.sender, "kaomi");
    }

    #[test]
    fn eco_propio_no_se_duplica() {
        let (mut app, tx) = test_app();
        let mine = app.peer_id_hex();
        tx.send(MeshEvent::Message {
            from: mine,
            nick: Some(app.nickname.clone()),
            channel: None,
            text: "hola".into(),
        })
        .unwrap();
        app.drain_mesh();
        assert!(app.lines().iter().all(|l| l.text != "hola"));
    }

    #[test]
    fn foco_en_canales_cambia_el_canal() {
        let (mut app, _tx) = test_app();
        for c in "/join #neon".chars() {
            app.input.insert(c);
        }
        app.send();
        assert_eq!(app.channel(), "#neon");

        app.panel = Panel::Canales;
        app.on_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.channel(), "#general"); // envuelve hacia atrás
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.channel(), "#neon");
        // En el chat, ↑↓ siguen desplazando la conversación.
        app.panel = Panel::Chat;
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.channel(), "#neon");
    }

    #[test]
    fn escribir_con_foco_en_pares_abre_un_md() {
        let (mut app, tx) = test_app();
        tx.send(MeshEvent::PeerJoined {
            id: "aabbccdd00112233".into(),
            nickname: "luna".into(),
            noise_public: String::new(),
            signing_public: String::new(),
        })
        .unwrap();
        app.drain_mesh();

        app.panel = Panel::Pares;
        for c in "hola en privado".chars() {
            app.input.insert(c);
        }
        app.send();

        // El canal pasa a ser el MD con luna y el foco vuelve al chat.
        assert_eq!(app.channel(), "@luna");
        assert_eq!(app.panel, Panel::Chat);
        let last = app.lines().last().unwrap();
        assert_eq!(last.kind, LineKind::Own);
        assert_eq!(last.text, "hola en privado");
    }

    #[test]
    fn md_entrante_abre_canal_con_el_emisor() {
        let (mut app, tx) = test_app();
        tx.send(MeshEvent::PeerJoined {
            id: "aabbccdd00112233".into(),
            nickname: "luna".into(),
            noise_public: String::new(),
            signing_public: String::new(),
        })
        .unwrap();
        tx.send(MeshEvent::Message {
            from: "aabbccdd00112233".into(),
            nick: Some("luna".into()),
            channel: Some("@dm".into()),
            text: "para tus ojos solos".into(),
        })
        .unwrap();
        app.drain_mesh();

        assert!(app.channels.iter().any(|c| c == "@luna"));
        let dm = app.messages.get("@luna").expect("canal del MD");
        let direct = dm
            .iter()
            .find(|l| l.kind == LineKind::Direct)
            .expect("línea Direct");
        assert_eq!(direct.sender, "luna");
        assert_eq!(direct.text, "para tus ojos solos");
        // No se cuela en el general.
        assert!(app
            .messages
            .get("#general")
            .unwrap()
            .iter()
            .all(|l| l.text != "para tus ojos solos"));
    }

    #[test]
    fn msg_abre_el_md_directo() {
        let (mut app, tx) = test_app();
        tx.send(MeshEvent::PeerJoined {
            id: "aabbccdd00112233".into(),
            nickname: "luna".into(),
            noise_public: String::new(),
            signing_public: String::new(),
        })
        .unwrap();
        app.drain_mesh();

        for c in "/msg luna ¿por aquí?".chars() {
            app.input.insert(c);
        }
        app.send();

        assert_eq!(app.channel(), "@luna");
        let dm = app.messages.get("@luna").unwrap();
        assert_eq!(dm.last().unwrap().kind, LineKind::Own);
        assert_eq!(dm.last().unwrap().text, "¿por aquí?");
    }

    /// Código de un "otro" node, como lo produciría /pair.
    fn codigo_de_otros(nick: &str) -> String {
        let other = Identity::generate();
        format!(
            "BB1|{}|{}|{}|{}",
            hex::encode(other.peer_id()),
            hex::encode(other.noise_public()),
            hex::encode(other.signing_public()),
            nick
        )
    }

    #[test]
    fn pair_genera_qr_de_bitchat() {
        let (mut app, _tx) = test_app();
        app.run_command(Command::Pair);
        let Some(Overlay::Pair { code, qr }) = &app.overlay else {
            panic!("debería abrirse el overlay de emparejamiento");
        };
        assert!(code.starts_with("bitchat://verify?"));
        assert!(!qr.is_empty(), "el QR debe dibujarse");
        // El QR es el nuestro y la app podría leerlo: firma válida, id
        // derivado de la clave y frescura en ventana.
        let sc = crate::pairing::parse_verify_url(code, now_unix()).expect("QR propio legible");
        assert_eq!(sc.id, app.peer_id_hex());
        // Y la huella impresa es la de nuestra clave pública Noise.
        let fp = crate::pairing::fingerprint(&app.identity.noise_public());
        assert!(crate::pairing::format_fingerprint(&fp)
            .lines()
            .all(|l| app.lines().iter().any(|line| line.text.contains(l))));
    }

    #[test]
    fn md_entrante_salta_a_su_canal() {
        let (mut app, tx) = test_app();
        // Estamos en #general leyendo, no en el MD.
        assert_eq!(app.channel(), "#general");
        tx.send(MeshEvent::PeerJoined {
            id: "aabbccdd00112233".into(),
            nickname: "luna".into(),
            noise_public: String::new(),
            signing_public: String::new(),
        })
        .unwrap();
        tx.send(MeshEvent::Message {
            from: "aabbccdd00112233".into(),
            nick: Some("luna".into()),
            channel: Some("@dm".into()),
            text: "¿sigues ahí?".into(),
        })
        .unwrap();
        app.drain_mesh();
        // La vista cambia sola al canal del MD y el foco va al chat.
        assert_eq!(app.channel(), "@luna");
        assert_eq!(app.panel, Panel::Chat);
        let dm = app.messages.get("@luna").unwrap();
        let direct = dm
            .iter()
            .find(|l| l.kind == LineKind::Direct)
            .expect("línea Direct");
        assert_eq!(direct.text, "¿sigues ahí?");
        // El aviso de llegada queda en el propio canal del MD.
        assert!(dm.iter().any(|l| l.text == "✉ md de luna"));
    }

    #[test]
    fn accept_por_apodo_de_un_par_en_la_malla() {
        let (mut app, tx) = test_app();
        let other = Identity::generate();
        tx.send(MeshEvent::PeerJoined {
            id: hex::encode(other.peer_id()),
            nickname: "luna".into(),
            noise_public: hex::encode(other.noise_public()),
            signing_public: hex::encode(other.signing_public()),
        })
        .unwrap();
        app.drain_mesh();

        app.run_command(Command::Accept("luna".into()));
        assert_eq!(app.contacts.len(), 1);
        let c = app.contacts.values().next().unwrap();
        assert_eq!(c.noise_public, hex::encode(other.noise_public()));
        assert!(app.peers[0].verified);

        // Apodo desconocido: no inventa contactos.
        app.run_command(Command::Accept("nadie".into()));
        assert_eq!(app.contacts.len(), 1);
        let _ = std::fs::remove_file(app.config.identity_path().with_file_name("contacts"));
    }

    #[test]
    fn fp_de_un_par_es_su_announce() {
        let (mut app, tx) = test_app();
        let other = Identity::generate();
        tx.send(MeshEvent::PeerJoined {
            id: hex::encode(other.peer_id()),
            nickname: "luna".into(),
            noise_public: hex::encode(other.noise_public()),
            signing_public: hex::encode(other.signing_public()),
        })
        .unwrap();
        app.drain_mesh();

        app.run_command(Command::Fp(Some("luna".into())));
        let expected =
            crate::pairing::format_fingerprint(&crate::pairing::fingerprint(&other.noise_public()));
        let head = format!("huella de luna ({}):", &hex::encode(other.peer_id())[..8]);
        assert!(app.lines().iter().any(|l| l.text == head));
        for line in expected.lines() {
            let shown = format!("  {line}");
            assert!(
                app.lines().iter().any(|l| l.text == shown),
                "falta la línea {shown:?}"
            );
        }

        // La nuestra también se muestra sin argumentos.
        app.run_command(Command::Fp(None));
        let mine = crate::pairing::format_fingerprint(&crate::pairing::fingerprint(
            &app.identity.noise_public(),
        ));
        for line in mine.lines() {
            let shown = format!("  {line}");
            assert!(app.lines().iter().any(|l| l.text == shown));
        }
    }

    #[test]
    fn accept_verifica_y_marca_contacto() {
        let (mut app, tx) = test_app();
        let code = codigo_de_otros("luna");
        app.run_command(Command::Accept(code.clone()));
        assert_eq!(app.contacts.len(), 1);

        // El par en la malla queda marcado con ★ sólo si sus claves del
        // announce coinciden con las del código.
        let parts: Vec<&str> = code.split('|').collect();
        tx.send(MeshEvent::PeerJoined {
            id: parts[1].to_string(),
            nickname: "luna".into(),
            noise_public: parts[2].to_string(),
            signing_public: parts[3].to_string(),
        })
        .unwrap();
        app.drain_mesh();
        assert!(app.peers[0].verified, "claves coincidentes → ★");

        // Un announce con otras claves (mismo id) NO es tu contacto.
        tx.send(MeshEvent::PeerJoined {
            id: parts[1].to_string(),
            nickname: "luna".into(),
            noise_public: "00".repeat(32),
            signing_public: "11".repeat(32),
        })
        .unwrap();
        app.drain_mesh();
        assert!(!app.peers[0].verified, "claves distintas → no ★");

        // Un código falsificado (id que no deriva de la clave) no entra.
        let bad = codigo_de_otros("estafa");
        let mut parts: Vec<String> = bad.split('|').map(str::to_string).collect();
        parts[1] = "0000000000000000".into();
        app.run_command(Command::Accept(parts.join("|")));
        assert_eq!(app.contacts.len(), 1);

        // No dejamos contactos de prueba en /tmp.
        let _ = std::fs::remove_file(app.config.identity_path().with_file_name("contacts"));
    }

    #[test]
    fn scroll_limitado() {
        let (mut app, _tx) = test_app();
        app.scroll_up(1000);
        assert_eq!(app.scroll, 0); // sólo hay1 línea
        app.system("otra");
        app.scroll_up(1000);
        assert_eq!(app.scroll, 1);
        app.scroll_down(100);
        assert_eq!(app.scroll, 0);
    }
}
