//! Party window (SPEC-067 phase 1): host or join a room and chat over
//! WebRTC. The signaling server runs inside this process when hosting.

use std::net::UdpSocket;
use std::time::Duration;

use eframe::egui;
use gba_desktop::sdl;
use gba_desktop::settings::Settings;
use gba_i18n::tr;
use gba_net::{Command, DEFAULT_PORT, DEFAULT_STUN, Event, Options, Session};

struct Line {
    from: Option<String>,
    text: String,
}

enum Mode {
    Lobby,
    InRoom {
        session: Session,
        host: bool,
        connected: bool,
    },
}

struct App {
    name: String,
    port: String,
    address: String,
    code: String,
    use_stun: bool,
    mode: Mode,
    log: Vec<Line>,
    peers: Vec<(u32, String)>,
    input: String,
    error: Option<String>,
}

/// Best-effort LAN address (no packet is sent: UDP "connect" only picks a
/// route).
fn lan_ip() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    socket.local_addr().ok().map(|a| a.ip().to_string())
}

impl App {
    fn new() -> Self {
        Self {
            name: std::env::var("USER").unwrap_or_else(|_| "Jogador".into()),
            port: DEFAULT_PORT.to_string(),
            address: String::new(),
            code: String::new(),
            use_stun: true,
            mode: Mode::Lobby,
            log: Vec::new(),
            peers: Vec::new(),
            input: String::new(),
            error: None,
        }
    }

    fn options(&self) -> Options {
        Options {
            name: self.name.trim().to_owned(),
            ice_servers: if self.use_stun {
                vec![DEFAULT_STUN.to_owned()]
            } else {
                Vec::new()
            },
        }
    }

    fn system(&mut self, text: impl Into<String>) {
        self.log.push(Line {
            from: None,
            text: text.into(),
        });
    }

    fn pump(&mut self) {
        let Mode::InRoom {
            session, connected, ..
        } = &mut self.mode
        else {
            return;
        };
        let mut closed = false;
        let mut system = Vec::new();
        for ev in session.poll() {
            match ev {
                Event::Status(s) => system.push(s),
                Event::Connected => {
                    *connected = true;
                    system.push(tr!("party.connected_log"));
                }
                Event::Joined { id, name } => {
                    if !self.peers.iter().any(|(i, _)| *i == id) {
                        self.peers.push((id, name.clone()));
                        system.push(tr!("party.joined", name = name));
                    }
                }
                Event::Left { id, name } => {
                    self.peers.retain(|(i, _)| *i != id);
                    system.push(tr!("party.left", name = name));
                }
                Event::Chat { from, text } => self.log.push(Line {
                    from: Some(from),
                    text,
                }),
                Event::Error(e) => {
                    self.error = Some(e.clone());
                    system.push(tr!("party.error", error = e));
                }
                Event::Closed => closed = true,
            }
        }
        for s in system {
            self.system(s);
        }
        if closed {
            self.mode = Mode::Lobby;
            self.peers.clear();
            self.system(tr!("party.session_ended"));
        }
    }

    fn lobby(&mut self, ui: &mut egui::Ui) {
        ui.heading(tr!("party.title"));
        ui.label(tr!("party.tagline"));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(tr!("party.name"));
            ui.text_edit_singleline(&mut self.name);
        });
        ui.checkbox(&mut self.use_stun, tr!("party.stun"));
        ui.add_space(10.0);
        ui.columns(2, |cols| {
            cols[0].group(|ui| {
                ui.strong(tr!("party.host"));
                ui.horizontal(|ui| {
                    ui.label(tr!("party.port"));
                    ui.text_edit_singleline(&mut self.port);
                });
                ui.weak(tr!("party.port_hint"));
                if ui.button(tr!("party.create")).clicked() {
                    match self.port.trim().parse::<u16>() {
                        Ok(port) => match Session::host(self.options(), port) {
                            Ok(session) => {
                                self.error = None;
                                self.log.clear();
                                self.peers.clear();
                                self.mode = Mode::InRoom {
                                    session,
                                    host: true,
                                    connected: false,
                                };
                            }
                            Err(e) => self.error = Some(e),
                        },
                        Err(_) => self.error = Some(tr!("party.invalid_port")),
                    }
                }
            });
            cols[1].group(|ui| {
                ui.strong(tr!("party.join"));
                ui.horizontal(|ui| {
                    ui.label(tr!("party.address"));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.address)
                            .hint_text("192.168.0.10:7373"),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label(tr!("party.code"));
                    ui.add(egui::TextEdit::singleline(&mut self.code).hint_text("ABCDEF"));
                });
                if ui.button(tr!("party.connect")).clicked() {
                    let mut addr = self.address.trim().to_owned();
                    if !addr.contains(':') {
                        addr.push_str(&format!(":{DEFAULT_PORT}"));
                    }
                    match Session::join(self.options(), addr, self.code.trim().to_owned()) {
                        Ok(session) => {
                            self.error = None;
                            self.log.clear();
                            self.peers.clear();
                            self.mode = Mode::InRoom {
                                session,
                                host: false,
                                connected: false,
                            };
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
            });
        });
        if let Some(e) = &self.error {
            ui.add_space(8.0);
            ui.colored_label(egui::Color32::LIGHT_RED, e);
        }
    }

    fn room(&mut self, ui: &mut egui::Ui) {
        let Mode::InRoom {
            session,
            host,
            connected,
        } = &self.mode
        else {
            return;
        };
        let (host, connected) = (*host, *connected);
        let invite = if host {
            let port = session.listen.map_or(0, |a| a.port());
            Some((
                session.code.clone().unwrap_or_default(),
                format!("{}:{port}", lan_ip().unwrap_or_else(|| "127.0.0.1".into())),
            ))
        } else {
            None
        };
        let mut leave = false;
        egui::Panel::top("room_top").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if let Some((code, addr)) = &invite {
                    ui.strong(tr!("party.code_line", code = code));
                    ui.label(tr!("party.address_line", address = addr));
                    if ui.button(tr!("party.copy_invite")).clicked() {
                        ui.ctx().copy_text(format!("{addr} {code}"));
                    }
                } else {
                    ui.strong(if connected {
                        tr!("party.connected")
                    } else {
                        tr!("party.connecting")
                    });
                }
                if ui.button(tr!("party.leave")).clicked() {
                    leave = true;
                }
            });
            ui.add_space(4.0);
        });
        egui::Panel::right("peers").show(ui, |ui| {
            ui.strong(tr!("party.participants"));
            ui.label(format!("- {}", tr!("party.you", name = self.name)));
            for (_, n) in &self.peers {
                ui.label(format!("- {n}"));
            }
        });
        let mut send = None;
        egui::Panel::bottom("compose").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.input)
                        .hint_text(tr!("party.message"))
                        .desired_width(ui.available_width() - 80.0),
                );
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (ui.button(tr!("party.send")).clicked() || enter)
                    && !self.input.trim().is_empty()
                {
                    send = Some(self.input.trim().to_owned());
                    self.input.clear();
                    field.request_focus();
                }
            });
            ui.add_space(4.0);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for line in &self.log {
                        match &line.from {
                            Some(from) => {
                                ui.horizontal_wrapped(|ui| {
                                    ui.strong(format!("{from}:"));
                                    ui.label(&line.text);
                                });
                            }
                            None => {
                                ui.weak(&line.text);
                            }
                        }
                    }
                });
        });
        if let Mode::InRoom { session, .. } = &self.mode {
            if let Some(text) = send {
                session.send(Command::Chat(text));
            }
            if leave {
                session.send(Command::Leave);
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.pump();
        ui.ctx().request_repaint_after(Duration::from_millis(100));
        match self.mode {
            Mode::Lobby => {
                egui::CentralPanel::default().show(ui, |ui| self.lobby(ui));
            }
            Mode::InRoom { .. } => self.room(ui),
        }
    }
}

fn main() -> Result<(), String> {
    // Same language rule as the other frontends: a saved choice wins,
    // otherwise the OS language does (SPEC-069 L10N-69-04).
    let path = gba_desktop::settings::default_path();
    let loaded = path.as_deref().map(gba_desktop::settings::load);
    let saved = loaded
        .as_ref()
        .map_or_else(Settings::default, |l| l.settings.clone());
    let (lang, _) =
        gba_desktop::settings::initial_language(path.as_deref(), &saved, &sdl::preferred_locales());
    gba_i18n::set_lang(lang);
    let icon = gba_desktop::sdl::Surface::from_png(gba_desktop::assets::ICON_PNG)
        .and_then(|s| s.to_rgba())
        .map(|(width, height, rgba)| egui::IconData {
            rgba,
            width,
            height,
        });
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(tr!("party.title"))
        .with_inner_size([760.0, 560.0]);
    if let Ok(icon) = icon {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    eframe::run_native(
        "gba-party",
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        Box::new(|_cc| Ok(Box::new(App::new()))),
    )
    .map_err(|e| e.to_string())
}
