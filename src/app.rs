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
    Themes { selected: usize },
}

/// Clase de una línea del chat.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LineKind {
    Own,
    Peer,
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
    pub fn new(config: Config, rx: Receiver<MeshEvent>) -> App {
        let themes = themes::all_themes();
        let theme_idx = themes
            .iter()
            .position(|t| t.id == config.theme)
            .unwrap_or(0);
        let density = Density::parse(&config.density).unwrap_or(Density::Comodo);
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
            identity: Identity::generate(),
            nickname: config.nick.clone(),
            config,
            themes,
            theme_idx,
            density,
            channels: vec!["#general".to_string()],
            channel_idx: 0,
            messages,
            peers: Vec::new(),
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

    /// Procesa los eventos que llegan del backend de malla.
    pub fn drain_mesh(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                MeshEvent::PeerJoined { id, nickname } => {
                    if !self.peers.iter().any(|p| p.id == id) {
                        self.peers.push(Peer {
                            id: id.clone(),
                            nickname: nickname.clone(),
                        });
                        self.system(format!("• {nickname} se une a la malla"));
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
                    channel,
                    text,
                } => {
                    let ch = channel.unwrap_or_else(|| self.channel().to_string());
                    self.messages.entry(ch.clone()).or_default().push(ChatLine {
                        time: now_hm(),
                        kind: LineKind::Peer,
                        sender: from,
                        text,
                    });
                    if ch != self.channel() {
                        self.system(format!("mensaje nuevo en {ch}"));
                    }
                    self.scroll = 0;
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
            KeyCode::Up => self.scroll_up(3),
            KeyCode::Down => self.scroll_down(3),
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

        // En el canal público viaja texto plano (como la app de referencia);
        // en el resto, payload binario con el canal marcado.
        let channel = self.channel().to_string();
        let payload = if channel == "#general" {
            text.as_bytes().to_vec()
        } else {
            let mut m = ChatMessage::new(self.nickname.clone(), text);
            m.channel = Some(channel);
            m.encode()
        };

        let mut pkt = Packet::new(msg_type::MESSAGE, self.identity.peer_id(), payload);
        pkt.recipient = Some(BROADCAST);
        if let Err(e) = pkt.sign(self.identity.signing_key()) {
            self.error(format!("no se pudo firmar: {e}"));
            return;
        }

        let sent = match fragment_packet(&pkt) {
            Ok(Some(frags)) => frags
                .into_iter()
                .filter_map(|f| f.encode(true).ok())
                .map(|bytes| net.send(NetCommand::SendPacket(bytes)))
                .all(|r| r.is_ok()),
            Ok(None) => pkt
                .encode(true)
                .map(|bytes| net.send(NetCommand::SendPacket(bytes)).is_ok())
                .unwrap_or(false),
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
                self.push_line(ChatLine {
                    time: now_hm(),
                    kind: LineKind::System,
                    sender: String::new(),
                    text: format!("→ {to}: {text} (sin radio: pendiente de entrega)"),
                });
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
                        .map(|p| format!("● {} ({})", p.nickname, &p.id[..8]))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    fn test_app() -> (App, std::sync::mpsc::Sender<MeshEvent>) {
        let (tx, rx) = channel();
        let cfg = Config {
            nick: "prueba".into(),
            path: Some(std::env::temp_dir().join("bluebl-test-config")),
            ..Config::default()
        };
        (App::new(cfg, rx), tx)
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
        })
        .unwrap();
        tx.send(MeshEvent::Message {
            from: "luna".into(),
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
