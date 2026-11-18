//! Pintado de la interfaz: paneles tipo consola que colapsan a vista única
//! en pantallas estrechas (móvil / Termux).

use crate::app::{App, LineKind, Overlay, Panel};
use crate::commands;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

/// Dibuja el frame completo.
pub fn draw(f: &mut Frame, app: &mut App) {
    let pal = app.theme().palette.clone();
    let area = f.area();
    app.last_width = area.width;

    // Fondo del tema en toda la pantalla.
    f.render_widget(
        Block::default().style(Style::default().bg(pal.bg).fg(pal.fg)),
        area,
    );

    let wide = area.width >= 100;
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(4),
        Constraint::Length(app.density.input_height()),
        Constraint::Length(1),
    ])
    .split(area);

    draw_header(f, app, rows[0]);
    if wide {
        draw_wide(f, app, rows[1]);
    } else {
        draw_narrow(f, app, rows[1]);
    }
    draw_input(f, app, rows[2]);
    draw_status(f, app, rows[3]);

    if app.overlay.is_some() {
        draw_overlay(f, app, area);
    }
}

fn style(color: Color) -> Style {
    Style::default().fg(color)
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let pal = &app.theme().palette;
    let wide = area.width >= 100;
    let left = if wide {
        format!(" BlueBl · {} ", app.channel())
    } else {
        format!(" {} ", app.channel())
    };
    let right = format!(
        " {} · {} · {} ",
        app.density.display(),
        app.theme().name,
        app.panel.title()
    );
    let line = Line::from(vec![
        Span::styled(
            left,
            Style::default()
                .fg(pal.bg)
                .bg(pal.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            "⌁ mesh ble · ",
            Style::default()
                .fg(pal.accent2)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(right, Style::default().fg(pal.dim)),
    ]);
    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(pal.bg)),
        area,
    );
}

/// Pantalla ancha: canales + pares a la izquierda, chat a la derecha.
fn draw_wide(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::horizontal([Constraint::Length(26), Constraint::Min(40)]).split(area);

    let side =
        Layout::vertical([Constraint::Percentage(45), Constraint::Percentage(55)]).split(cols[0]);
    draw_channels(f, app, side[0]);
    draw_peers(f, app, side[1]);
    draw_chat(f, app, cols[1], true);
}

/// Pantalla estrecha: un solo panel; Tab lo cambia.
fn draw_narrow(f: &mut Frame, app: &App, area: Rect) {
    match app.panel {
        Panel::Chat => draw_chat(f, app, area, false),
        Panel::Canales => draw_channels(f, app, area),
        Panel::Pares => draw_peers(f, app, area),
    }
}

fn panel_block<'a>(title: &'a str, app: &App, bordered: bool) -> Block<'a> {
    let pal = &app.theme().palette;
    let mut b = Block::default()
        .title(format!(" {title} "))
        .style(Style::default().fg(pal.fg).bg(pal.input_bg));
    if bordered {
        b = b.borders(Borders::ALL).border_style(style(pal.border));
    }
    b
}

fn draw_channels(f: &mut Frame, app: &App, area: Rect) {
    let pal = &app.theme().palette;
    let bordered = area.width >= 40;
    let lines: Vec<Line> = app
        .channels
        .iter()
        .enumerate()
        .map(|(i, ch)| {
            let selected = i == app.channel_idx;
            let (symbol, st) = if selected {
                (
                    "▸ ",
                    Style::default()
                        .fg(pal.bg)
                        .bg(pal.accent)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                ("  ", Style::default().fg(pal.fg))
            };
            Line::from(vec![Span::styled(format!("{symbol}{ch}"), st)])
        })
        .collect();
    f.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Canales", app, bordered))
            .style(Style::default().fg(pal.fg).bg(pal.bg)),
        area,
    );
}

fn draw_peers(f: &mut Frame, app: &App, area: Rect) {
    let pal = &app.theme().palette;
    let bordered = area.width >= 40;
    let lines: Vec<Line> = if app.peers.is_empty() {
        vec![Line::from(Span::styled(
            " (nadie cerca aún)",
            style(pal.dim),
        ))]
    } else {
        app.peers
            .iter()
            .map(|p| {
                Line::from(vec![
                    Span::styled("● ", style(pal.accent2)),
                    Span::styled(p.nickname.clone(), style(pal.fg)),
                    Span::styled(format!(" {}", &p.id[..8]), style(pal.dim)),
                ])
            })
            .collect()
    };
    f.render_widget(
        Paragraph::new(lines)
            .block(panel_block("Pares", app, bordered))
            .style(Style::default().fg(pal.fg).bg(pal.bg)),
        area,
    );
}

fn draw_chat(f: &mut Frame, app: &App, area: Rect, bordered: bool) {
    let pal = &app.theme().palette;
    let block = panel_block(app.channel(), app, bordered);
    let inner = if bordered { block.inner(area) } else { area };
    f.render_widget(block.style(Style::default().fg(pal.fg).bg(pal.bg)), area);

    let pad = app.density.chat_padding();
    let inner = Rect {
        x: inner.x + 1,
        y: inner.y + pad,
        width: inner.width.saturating_sub(2),
        height: inner.height.saturating_sub(pad * 2),
    };
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let mut lines: Vec<Line> = Vec::with_capacity(app.lines().len());
    for msg in app.lines() {
        let ts = Span::styled(format!("[{}] ", msg.time), style(pal.dim));
        match msg.kind {
            LineKind::Own => lines.push(Line::from(vec![
                ts,
                Span::styled(
                    format!("{}: ", msg.sender),
                    Style::default()
                        .fg(pal.self_msg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(msg.text.clone(), Style::default().fg(pal.fg)),
            ])),
            LineKind::Peer => lines.push(Line::from(vec![
                ts,
                Span::styled(
                    format!("{}: ", msg.sender),
                    Style::default()
                        .fg(pal.accent2)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(msg.text.clone(), Style::default().fg(pal.peer_msg)),
            ])),
            LineKind::System => lines.push(Line::from(vec![
                Span::styled(" ", Style::default().fg(pal.bg)),
                ts,
                Span::styled(msg.text.clone(), style(pal.system)),
            ])),
            LineKind::Error => lines.push(Line::from(vec![
                ts,
                Span::styled(msg.text.clone(), style(pal.error)),
            ])),
            LineKind::Action => lines.push(Line::from(vec![
                ts,
                Span::styled(
                    format!("* {} ", msg.sender),
                    Style::default()
                        .fg(pal.accent)
                        .add_modifier(Modifier::ITALIC),
                ),
                Span::styled(
                    msg.text.clone(),
                    Style::default()
                        .fg(pal.accent)
                        .add_modifier(Modifier::ITALIC),
                ),
            ])),
        }
    }

    // scroll desde el final (0 = anclado abajo).
    let visible = lines.len().saturating_sub(app.scroll);
    let start = visible.saturating_sub(inner.height as usize);
    let slice: Vec<Line> = lines
        .into_iter()
        .skip(start)
        .take(visible - start)
        .collect();

    f.render_widget(
        Paragraph::new(slice)
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(pal.fg).bg(pal.bg)),
        inner,
    );
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let pal = &app.theme().palette;
    let bordered = area.height >= 2;
    let mut block = Block::default()
        .title(" mensaje ")
        .style(Style::default().fg(pal.fg).bg(pal.input_bg));
    if bordered {
        block = block
            .borders(Borders::ALL)
            .border_style(Style::default().fg(pal.accent).bg(pal.input_bg));
    }
    let inner = if bordered { block.inner(area) } else { area };
    f.render_widget(block, area);

    // Ventana de texto que contiene al cursor (entrada larga no rompe nada).
    let chars: Vec<char> = app.input.as_str().chars().collect();
    let cursor = app.input.cursor().min(chars.len());
    let width = inner.width as usize;
    let start = cursor.saturating_sub(width.saturating_sub(1));
    let end = (start + width).min(chars.len());
    let visible: String = chars[start..end].iter().collect();
    let col = (cursor - start) as u16;

    f.render_widget(
        Paragraph::new(Span::styled(
            visible,
            Style::default().fg(pal.fg).bg(pal.input_bg),
        )),
        inner,
    );
    // Cursor real del terminal sobre la posición del texto.
    f.set_cursor_position((inner.x + col, inner.y));
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let pal = &app.theme().palette;
    let id = app.peer_id_hex();
    let left = format!(" ⌬ {} · {} pares ", &id[..8], app.peers.len());
    let right = format!(" {} ", app.status);
    let line = Line::from(vec![
        Span::styled(
            left,
            Style::default()
                .fg(pal.status_bg)
                .bg(pal.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {}", app.channel()),
            Style::default().fg(pal.dim).bg(pal.status_bg),
        ),
        Span::styled(right, Style::default().fg(pal.accent2).bg(pal.status_bg)),
    ]);
    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(pal.status_bg)),
        area,
    );
}

fn draw_overlay(f: &mut Frame, app: &App, area: Rect) {
    let pal = &app.theme().palette;
    let w = (area.width * 7 / 10).max(40).min(area.width);
    let h = match &app.overlay {
        Some(Overlay::Help) => (commands::help_text().len() as u16 + 4).min(area.height),
        Some(Overlay::Themes { .. }) => (area.height * 8 / 10).max(10),
        None => return,
    };
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    let rect = Rect::new(x, y, w, h);

    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(style(pal.accent))
        .style(Style::default().fg(pal.fg).bg(pal.bg));

    match &app.overlay {
        Some(Overlay::Help) => {
            let lines: Vec<Line> = commands::help_text()
                .into_iter()
                .map(|l| Line::from(Span::styled(l, style(pal.fg))))
                .collect();
            let inner = block.inner(rect);
            f.render_widget(block.title(" ayuda "), rect);
            f.render_widget(Paragraph::new(lines), inner);
        }
        Some(Overlay::Themes { selected }) => {
            let inner = block.inner(rect);
            f.render_widget(block.title(" temas · intro aplica · esc cierra "), rect);
            let mut lines: Vec<Line> = Vec::new();
            let mut idx = 0usize;
            for cat in crate::themes::Category::ALL {
                lines.push(Line::from(Span::styled(
                    format!("── {} ", cat.name()),
                    Style::default()
                        .fg(pal.accent2)
                        .add_modifier(Modifier::BOLD),
                )));
                for theme in app.themes.iter().filter(|t| t.category == cat) {
                    let is_sel = idx == *selected;
                    let (sym, st) = if is_sel {
                        (
                            "▸ ",
                            Style::default()
                                .fg(pal.bg)
                                .bg(pal.accent)
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        ("  ", Style::default().fg(pal.fg))
                    };
                    lines.push(Line::from(vec![
                        Span::styled(sym, st),
                        Span::styled(format!("{:20}", theme.name), st),
                        Span::styled(theme.id, Style::default().fg(pal.dim)),
                    ]));
                    idx += 1;
                }
            }
            // Ajusta el desplazamiento para que la selección sea visible.
            let sel_line = lines
                .iter()
                .take_while(|l| {
                    !l.spans.iter().any(|s| s.content.starts_with('▸'))
                        || !l
                            .spans
                            .iter()
                            .any(|s| s.content.contains(app.themes[*selected].name))
                })
                .count();
            let offset = (sel_line as u16).saturating_sub(inner.height.saturating_sub(3));
            f.render_widget(
                Paragraph::new(lines)
                    .scroll((offset, 0))
                    .style(Style::default().fg(pal.fg)),
                inner,
            );
        }
        None => {}
    }
}
