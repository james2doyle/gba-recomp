//! WebRTC party sessions (SPEC-067, phase 1: room, connection, text chat).
//!
//! Star topology: the host accepts up to [`MAX_GUESTS`] guests. Signaling
//! is a small TCP service (JSON lines) running inside the host's own process
//! on a background runtime thread; after the offer/answer exchange the
//! guests talk to the host over WebRTC data channels (DTLS, ICE/STUN), and
//! the host relays chat between guests. UIs use the synchronous [`Session`]
//! facade: send [`Command`]s, poll [`Event`]s.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc, oneshot};
use webrtc::data_channel::{DataChannel, DataChannelEvent, RTCDataChannelInit};
use webrtc::peer_connection::{
    MediaEngine, PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler,
    RTCConfigurationBuilder, RTCIceGatheringState, RTCIceServer, RTCPeerConnectionState,
    RTCSessionDescription, Registry, register_default_interceptors,
};
use webrtc::runtime::default_runtime;

pub const PROTOCOL_VERSION: u32 = 1;
pub const DEFAULT_PORT: u16 = 7373;
pub const MAX_GUESTS: usize = 3;
pub const DEFAULT_STUN: &str = "stun:stun.l.google.com:19302";
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);

/// What the UI asks the session to do.
#[derive(Debug, Clone)]
pub enum Command {
    Chat(String),
    Leave,
}

/// What the session reports to the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Free-form progress line for the status bar.
    Status(String),
    /// The local side is connected and can chat.
    Connected,
    Joined {
        id: u32,
        name: String,
    },
    Left {
        id: u32,
        name: String,
    },
    Chat {
        from: String,
        text: String,
    },
    Error(String),
    Closed,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub name: String,
    /// STUN/TURN URLs; empty for LAN/loopback only.
    pub ice_servers: Vec<String>,
}

/// Synchronous handle owned by a UI thread.
pub struct Session {
    commands: mpsc::UnboundedSender<Command>,
    events: std_mpsc::Receiver<Event>,
    /// Room code (host) — share it with guests.
    pub code: Option<String>,
    /// Signaling address the host listens on.
    pub listen: Option<SocketAddr>,
    _thread: std::thread::JoinHandle<()>,
}

impl Session {
    /// Host a room: binds the signaling port (0 = any) on all interfaces.
    pub fn host(options: Options, port: u16) -> Result<Self, String> {
        let code = room_code();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (ev_tx, ev_rx) = std_mpsc::channel();
        let (ready_tx, ready_rx) = std_mpsc::channel();
        let room = code.clone();
        let thread = std::thread::Builder::new()
            .name("gba-net-host".into())
            .spawn(move || {
                let rt = match runtime() {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                rt.block_on(async move {
                    let listener = match TcpListener::bind(("0.0.0.0", port)).await {
                        Ok(l) => l,
                        Err(e) => {
                            let _ =
                                ready_tx.send(Err(format!("cannot listen on port {port}: {e}")));
                            return;
                        }
                    };
                    let addr = listener.local_addr().ok();
                    let _ = ready_tx.send(Ok(addr));
                    host_main(listener, room, options, cmd_rx, ev_tx).await;
                });
            })
            .map_err(|e| e.to_string())?;
        let listen = ready_rx
            .recv()
            .map_err(|_| "network thread stopped".to_owned())??;
        Ok(Self {
            commands: cmd_tx,
            events: ev_rx,
            code: Some(code),
            listen,
            _thread: thread,
        })
    }

    /// Join a room at `addr` ("host:port") with its code.
    pub fn join(options: Options, addr: String, code: String) -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (ev_tx, ev_rx) = std_mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("gba-net-guest".into())
            .spawn(move || match runtime() {
                Ok(rt) => rt.block_on(guest_main(addr, code, options, cmd_rx, ev_tx)),
                Err(e) => {
                    let _ = ev_tx.send(Event::Error(e));
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            commands: cmd_tx,
            events: ev_rx,
            code: None,
            listen: None,
            _thread: thread,
        })
    }

    pub fn send(&self, cmd: Command) {
        let _ = self.commands.send(cmd);
    }

    /// Non-blocking: all events received since the last call.
    pub fn poll(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }

    /// Blocking wait for the next event (tests, scripts).
    pub fn wait(&self, timeout: Duration) -> Option<Event> {
        self.events.recv_timeout(timeout).ok()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Leave);
    }
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| format!("cannot start network runtime: {e}"))
}

/// Six unambiguous uppercase letters, from the per-process random hasher
/// seed mixed with the clock.
fn room_code() -> String {
    use std::hash::{BuildHasher, Hasher};
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    let mut x = h.finish() | 1;
    (0..6)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            ALPHABET[(x % ALPHABET.len() as u64) as usize] as char
        })
        .collect()
}

// ------------------------------------------------------------------ wire

/// Signaling messages (TCP, one JSON object per line).
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "t", rename_all = "snake_case")]
enum Signal {
    Hello {
        version: u32,
        code: String,
        name: String,
    },
    Welcome {
        id: u32,
    },
    Reject {
        reason: String,
    },
    Offer {
        sdp: String,
    },
    Answer {
        sdp: String,
    },
}

/// Application messages on the `chat` data channel.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "t", rename_all = "snake_case")]
enum Wire {
    Chat { from: String, text: String },
    Joined { id: u32, name: String },
    Left { id: u32, name: String },
    Roster { peers: Vec<(u32, String)> },
}

async fn send_line(stream: &mut (impl AsyncWriteExt + Unpin), msg: &Signal) -> Result<(), String> {
    let mut line = serde_json::to_string(msg).map_err(|e| e.to_string())?;
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .await
        .map_err(|e| e.to_string())
}

async fn read_line(reader: &mut (impl AsyncBufReadExt + Unpin)) -> Result<Signal, String> {
    let mut line = String::new();
    let n = tokio::time::timeout(HANDSHAKE_TIMEOUT, reader.read_line(&mut line))
        .await
        .map_err(|_| "signaling timed out".to_owned())?
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("connection closed".into());
    }
    serde_json::from_str(line.trim()).map_err(|e| format!("invalid signaling message: {e}"))
}

// ------------------------------------------------------------------ WebRTC

struct Handler {
    gathered: std::sync::Mutex<Option<oneshot::Sender<()>>>,
    state: mpsc::UnboundedSender<RTCPeerConnectionState>,
    incoming: mpsc::UnboundedSender<Arc<dyn DataChannel>>,
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for Handler {
    async fn on_ice_gathering_state_change(&self, state: RTCIceGatheringState) {
        if state == RTCIceGatheringState::Complete
            && let Some(tx) = self.gathered.lock().ok().and_then(|mut g| g.take())
        {
            let _ = tx.send(());
        }
    }
    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        let _ = self.state.send(state);
    }
    async fn on_data_channel(&self, dc: Arc<dyn DataChannel>) {
        let _ = self.incoming.send(dc);
    }
}

struct Peer {
    pc: Arc<dyn PeerConnection>,
    gathered: oneshot::Receiver<()>,
    states: mpsc::UnboundedReceiver<RTCPeerConnectionState>,
    incoming: mpsc::UnboundedReceiver<Arc<dyn DataChannel>>,
}

async fn new_peer(options: &Options) -> Result<Peer, String> {
    let (g_tx, g_rx) = oneshot::channel();
    let (s_tx, s_rx) = mpsc::unbounded_channel();
    let (i_tx, i_rx) = mpsc::unbounded_channel();
    let handler = Arc::new(Handler {
        gathered: std::sync::Mutex::new(Some(g_tx)),
        state: s_tx,
        incoming: i_tx,
    });
    let mut media = MediaEngine::default();
    media.register_default_codecs().map_err(|e| e.to_string())?;
    let registry =
        register_default_interceptors(Registry::new(), &mut media).map_err(|e| e.to_string())?;
    let config = RTCConfigurationBuilder::new()
        .with_ice_servers(if options.ice_servers.is_empty() {
            Vec::new()
        } else {
            vec![RTCIceServer {
                urls: options.ice_servers.clone(),
                ..Default::default()
            }]
        })
        .build();
    let pc = PeerConnectionBuilder::new()
        .with_configuration(config)
        .with_media_engine(media)
        .with_interceptor_registry(registry)
        .with_handler(handler)
        .with_runtime(default_runtime().ok_or("no WebRTC runtime")?)
        .with_udp_addrs(vec!["0.0.0.0:0".to_owned()])
        .build()
        .await
        .map_err(|e| format!("cannot create peer connection: {e}"))?;
    Ok(Peer {
        pc: Arc::new(pc),
        gathered: g_rx,
        states: s_rx,
        incoming: i_rx,
    })
}

/// Complete local description after ICE gathering (non-trickle).
async fn local_sdp(peer: &mut Peer) -> Result<String, String> {
    let _ = tokio::time::timeout(Duration::from_secs(10), &mut peer.gathered).await;
    let desc = peer
        .pc
        .local_description()
        .await
        .ok_or("no local description")?;
    serde_json::to_string(&desc).map_err(|e| e.to_string())
}

fn chat_init() -> RTCDataChannelInit {
    RTCDataChannelInit {
        ordered: true,
        ..Default::default()
    }
}

async fn send_wire(dc: &Arc<dyn DataChannel>, msg: &Wire) {
    if let Ok(text) = serde_json::to_string(msg) {
        let _ = dc.send_text(&text).await;
    }
}

fn parse_wire(data: &[u8]) -> Option<Wire> {
    serde_json::from_slice(data).ok()
}

// ------------------------------------------------------------------ host

struct Guest {
    name: String,
    dc: Option<Arc<dyn DataChannel>>,
    pc: Arc<dyn PeerConnection>,
}

type Guests = Arc<Mutex<HashMap<u32, Guest>>>;

async fn broadcast(guests: &Guests, msg: &Wire, except: Option<u32>) {
    let targets: Vec<Arc<dyn DataChannel>> = guests
        .lock()
        .await
        .iter()
        .filter(|(id, _)| Some(**id) != except)
        .filter_map(|(_, g)| g.dc.clone())
        .collect();
    for dc in targets {
        send_wire(&dc, msg).await;
    }
}

async fn host_main(
    listener: TcpListener,
    code: String,
    options: Options,
    mut commands: mpsc::UnboundedReceiver<Command>,
    events: std_mpsc::Sender<Event>,
) {
    let _ = events.send(Event::Status(format!(
        "Sala {code} aberta; aguardando convidados"
    )));
    let _ = events.send(Event::Connected);
    let guests: Guests = Arc::new(Mutex::new(HashMap::new()));
    let mut next_id = 2u32;
    let host_name = options.name.clone();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let Ok((stream, addr)) = accepted else { continue };
                let id = next_id;
                next_id += 1;
                let guests = guests.clone();
                let events = events.clone();
                let options = options.clone();
                let code = code.clone();
                let host_name = host_name.clone();
                tokio::spawn(async move {
                    if let Err(e) = admit(stream, id, code, options, host_name, guests.clone(), events.clone()).await {
                        let _ = events.send(Event::Status(format!("Conexão de {addr} recusada: {e}")));
                    }
                });
            }
            cmd = commands.recv() => match cmd {
                Some(Command::Chat(text)) => {
                    let _ = events.send(Event::Chat { from: host_name.clone(), text: text.clone() });
                    broadcast(&guests, &Wire::Chat { from: host_name.clone(), text }, None).await;
                }
                Some(Command::Leave) | None => {
                    for (_, g) in guests.lock().await.drain() {
                        let _ = g.pc.close().await;
                    }
                    let _ = events.send(Event::Closed);
                    return;
                }
            }
        }
    }
}

/// Handshake with one guest and keep relaying its chat until it leaves.
async fn admit(
    stream: TcpStream,
    id: u32,
    code: String,
    options: Options,
    host_name: String,
    guests: Guests,
    events: std_mpsc::Sender<Event>,
) -> Result<(), String> {
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let Signal::Hello {
        version,
        code: given,
        name,
    } = read_line(&mut reader).await?
    else {
        return Err("expected hello".into());
    };
    let reject = if version != PROTOCOL_VERSION {
        Some(format!(
            "versão de protocolo {version} incompatível (anfitrião usa {PROTOCOL_VERSION})"
        ))
    } else if !given.eq_ignore_ascii_case(&code) {
        Some("código da sala incorreto".to_owned())
    } else if guests.lock().await.len() >= MAX_GUESTS {
        Some(format!("sala cheia ({} participantes)", MAX_GUESTS + 1))
    } else {
        None
    };
    if let Some(reason) = reject {
        send_line(
            &mut write,
            &Signal::Reject {
                reason: reason.clone(),
            },
        )
        .await?;
        return Err(reason);
    }
    let name = if name.trim().is_empty() {
        format!("Jogador {id}")
    } else {
        name.trim().to_owned()
    };
    send_line(&mut write, &Signal::Welcome { id }).await?;
    let mut peer = new_peer(&options).await?;
    let dc = peer
        .pc
        .create_data_channel("chat", Some(chat_init()))
        .await
        .map_err(|e| e.to_string())?;
    let offer = peer
        .pc
        .create_offer(None)
        .await
        .map_err(|e| e.to_string())?;
    peer.pc
        .set_local_description(offer)
        .await
        .map_err(|e| e.to_string())?;
    let sdp = local_sdp(&mut peer).await?;
    send_line(&mut write, &Signal::Offer { sdp }).await?;
    let Signal::Answer { sdp } = read_line(&mut reader).await? else {
        return Err("expected answer".into());
    };
    let answer: RTCSessionDescription = serde_json::from_str(&sdp).map_err(|e| e.to_string())?;
    peer.pc
        .set_remote_description(answer)
        .await
        .map_err(|e| e.to_string())?;
    guests.lock().await.insert(
        id,
        Guest {
            name: name.clone(),
            dc: None,
            pc: peer.pc.clone(),
        },
    );
    let mut opened = false;
    loop {
        tokio::select! {
            ev = dc.poll() => match ev {
                Some(DataChannelEvent::OnOpen) => {
                    opened = true;
                    let roster: Vec<(u32, String)> = std::iter::once((1, host_name.clone()))
                        .chain(guests.lock().await.iter().filter(|(gid, _)| **gid != id).map(|(gid, g)| (*gid, g.name.clone())))
                        .collect();
                    send_wire(&dc, &Wire::Roster { peers: roster }).await;
                    broadcast(&guests, &Wire::Joined { id, name: name.clone() }, Some(id)).await;
                    if let Some(g) = guests.lock().await.get_mut(&id) {
                        g.dc = Some(dc.clone());
                    }
                    let _ = events.send(Event::Joined { id, name: name.clone() });
                }
                Some(DataChannelEvent::OnMessage(msg)) => {
                    if let Some(Wire::Chat { text, .. }) = parse_wire(&msg.data) {
                        // The host stamps the sender's name (guests cannot spoof).
                        let _ = events.send(Event::Chat { from: name.clone(), text: text.clone() });
                        broadcast(&guests, &Wire::Chat { from: name.clone(), text }, Some(id)).await;
                    }
                }
                Some(DataChannelEvent::OnClose) | None => break,
                Some(_) => {}
            },
            st = peer.states.recv() => match st {
                Some(RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed | RTCPeerConnectionState::Disconnected) | None => break,
                Some(_) => {}
            },
        }
    }
    let removed = guests.lock().await.remove(&id);
    if let Some(g) = removed {
        let _ = g.pc.close().await;
    }
    if opened {
        broadcast(
            &guests,
            &Wire::Left {
                id,
                name: name.clone(),
            },
            None,
        )
        .await;
        let _ = events.send(Event::Left { id, name });
    }
    Ok(())
}

// ------------------------------------------------------------------ guest

async fn guest_main(
    addr: String,
    code: String,
    options: Options,
    mut commands: mpsc::UnboundedReceiver<Command>,
    events: std_mpsc::Sender<Event>,
) {
    if let Err(e) = guest_session(&addr, code, &options, &mut commands, &events).await {
        let _ = events.send(Event::Error(e));
    }
    let _ = events.send(Event::Closed);
}

async fn guest_session(
    addr: &str,
    code: String,
    options: &Options,
    commands: &mut mpsc::UnboundedReceiver<Command>,
    events: &std_mpsc::Sender<Event>,
) -> Result<(), String> {
    let _ = events.send(Event::Status(format!("Conectando a {addr}…")));
    let stream = tokio::time::timeout(HANDSHAKE_TIMEOUT, TcpStream::connect(addr))
        .await
        .map_err(|_| format!("tempo esgotado ao conectar em {addr}"))?
        .map_err(|e| format!("não foi possível conectar em {addr}: {e}"))?;
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    send_line(
        &mut write,
        &Signal::Hello {
            version: PROTOCOL_VERSION,
            code,
            name: options.name.clone(),
        },
    )
    .await?;
    match read_line(&mut reader).await? {
        Signal::Welcome { .. } => {}
        Signal::Reject { reason } => return Err(format!("o anfitrião recusou: {reason}")),
        _ => return Err("resposta inesperada do anfitrião".into()),
    }
    let Signal::Offer { sdp } = read_line(&mut reader).await? else {
        return Err("expected offer".into());
    };
    let mut peer = new_peer(options).await?;
    let offer: RTCSessionDescription = serde_json::from_str(&sdp).map_err(|e| e.to_string())?;
    peer.pc
        .set_remote_description(offer)
        .await
        .map_err(|e| e.to_string())?;
    let answer = peer
        .pc
        .create_answer(None)
        .await
        .map_err(|e| e.to_string())?;
    peer.pc
        .set_local_description(answer)
        .await
        .map_err(|e| e.to_string())?;
    let sdp = local_sdp(&mut peer).await?;
    send_line(&mut write, &Signal::Answer { sdp }).await?;
    let _ = events.send(Event::Status("Negociando conexão WebRTC…".into()));
    let dc = tokio::time::timeout(HANDSHAKE_TIMEOUT, peer.incoming.recv())
        .await
        .map_err(|_| "o canal de dados não abriu (NAT/firewall?)".to_owned())?
        .ok_or("conexão encerrada")?;
    let mut opened = false;
    loop {
        tokio::select! {
            ev = dc.poll() => match ev {
                Some(DataChannelEvent::OnOpen) => {
                    opened = true;
                    let _ = events.send(Event::Connected);
                }
                Some(DataChannelEvent::OnMessage(msg)) => match parse_wire(&msg.data) {
                    Some(Wire::Chat { from, text }) => { let _ = events.send(Event::Chat { from, text }); }
                    Some(Wire::Joined { id, name }) => { let _ = events.send(Event::Joined { id, name }); }
                    Some(Wire::Left { id, name }) => { let _ = events.send(Event::Left { id, name }); }
                    Some(Wire::Roster { peers }) => {
                        for (id, name) in peers {
                            let _ = events.send(Event::Joined { id, name });
                        }
                    }
                    None => {}
                },
                Some(DataChannelEvent::OnClose) | None => break,
                Some(_) => {}
            },
            st = peer.states.recv() => match st {
                Some(RTCPeerConnectionState::Failed) => return Err("a conexão WebRTC falhou".into()),
                Some(RTCPeerConnectionState::Closed | RTCPeerConnectionState::Disconnected) | None => break,
                Some(_) => {}
            },
            cmd = commands.recv() => match cmd {
                Some(Command::Chat(text)) => {
                    let _ = events.send(Event::Chat { from: options.name.clone(), text: text.clone() });
                    send_wire(&dc, &Wire::Chat { from: options.name.clone(), text }).await;
                }
                Some(Command::Leave) | None => break,
            },
        }
    }
    let _ = peer.pc.close().await;
    if !opened {
        return Err("a sala fechou antes da conexão".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(name: &str) -> Options {
        Options {
            name: name.into(),
            ice_servers: Vec::new(),
        }
    }

    fn wait_for(s: &Session, pred: impl Fn(&Event) -> bool) -> Event {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while std::time::Instant::now() < deadline {
            if let Some(e) = s.wait(Duration::from_millis(200)) {
                if pred(&e) {
                    return e;
                }
                if let Event::Error(msg) = &e {
                    panic!("session error: {msg}");
                }
            }
        }
        panic!("timed out waiting for event");
    }

    #[test]
    fn host_and_guests_chat_over_webrtc_on_loopback() {
        let host = Session::host(opts("Ana"), 0).expect("host");
        let port = host.listen.unwrap().port();
        let code = host.code.clone().unwrap();
        let addr = format!("127.0.0.1:{port}");

        let bob = Session::join(opts("Bia"), addr.clone(), code.clone()).expect("join");
        wait_for(&bob, |e| *e == Event::Connected);
        wait_for(
            &host,
            |e| matches!(e, Event::Joined { name, .. } if name == "Bia"),
        );

        let caio = Session::join(opts("Caio"), addr.clone(), code.to_lowercase()).expect("join 2");
        wait_for(&caio, |e| *e == Event::Connected);
        wait_for(
            &bob,
            |e| matches!(e, Event::Joined { name, .. } if name == "Caio"),
        );

        host.send(Command::Chat("oi pessoal".into()));
        for s in [&bob, &caio] {
            wait_for(
                s,
                |e| matches!(e, Event::Chat { from, text } if from == "Ana" && text == "oi pessoal"),
            );
        }
        // Guest → host → other guest relay with host-stamped sender.
        bob.send(Command::Chat("bora trocar?".into()));
        wait_for(
            &host,
            |e| matches!(e, Event::Chat { from, .. } if from == "Bia"),
        );
        wait_for(
            &caio,
            |e| matches!(e, Event::Chat { from, text } if from == "Bia" && text == "bora trocar?"),
        );

        drop(caio);
        wait_for(
            &host,
            |e| matches!(e, Event::Left { name, .. } if name == "Caio"),
        );
    }

    #[test]
    fn wrong_code_is_rejected_with_a_clear_message() {
        let host = Session::host(opts("Ana"), 0).expect("host");
        let addr = format!("127.0.0.1:{}", host.listen.unwrap().port());
        let guest = Session::join(opts("Bia"), addr, "ZZZZZZ".into()).expect("join");
        let mut error = None;
        while let Some(e) = guest.wait(Duration::from_secs(10)) {
            match e {
                Event::Error(msg) => error = Some(msg),
                Event::Closed => break,
                _ => {}
            }
        }
        let msg = error.expect("an error is reported");
        assert!(msg.contains("código da sala incorreto"), "{msg}");
    }

    #[test]
    fn room_codes_are_six_unambiguous_letters() {
        let c = room_code();
        assert_eq!(c.len(), 6);
        assert!(
            c.chars()
                .all(|ch| ch.is_ascii_uppercase() && ch != 'I' && ch != 'O')
        );
    }
}
