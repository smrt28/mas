#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]
#[path = "client/assets.rs"]
mod client_assets;

use bevy::{
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
    sprite::Anchor,
};
use marsal_a_spion::{net, plan, *};
use std::{
    net::{Shutdown, TcpStream, ToSocketAddrs},
    sync::{
        Mutex,
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

struct LoginLayout {
    x: f32,
    game_y: f32,
    server_y: f32,
    enter_y: f32,
    width: f32,
    scale: f32,
}
impl LoginLayout {
    fn connection_button(&self) -> plan::Rect {
        plan::Rect {
            left: self.x + self.width / 2. - 104. * self.scale,
            top: self.server_y - 18. * self.scale,
            width: 104. * self.scale,
            height: 36. * self.scale,
        }
    }
}
fn login_layout(surface: plan::Layout) -> LoginLayout {
    let scale = ((surface.width - 32.) / 382.).clamp(0.1, 1.);
    let margin = 64_f32.min((surface.width - 382. * scale) / 2.);
    let x = surface.width / 2. - margin - 191. * scale;
    let game_y = -surface.height / 2. + 66. + 82. * scale;
    LoginLayout {
        x,
        game_y,
        server_y: game_y + 44. * scale,
        enter_y: game_y - 44. * scale,
        width: 350. * scale,
        scale,
    }
}
struct WaitingLayout {
    rows: usize,
    header_y: f32,
    pager_y: f32,
    login: LoginLayout,
}
impl WaitingLayout {
    fn new(surface: plan::Layout) -> Self {
        let login = login_layout(surface);
        let rows = (((surface.height / 2. - 36. - login.game_y - 112. * login.scale)
            / (32. * login.scale))
            .floor() as usize)
            .clamp(1, 6);
        Self {
            rows,
            header_y: login.game_y + (112. + rows as f32 * 32.) * login.scale,
            pager_y: login.game_y + 88. * login.scale,
            login,
        }
    }
    fn positioned(surface: plan::Layout, offset: Vec2) -> (Self, Vec2) {
        let mut panel = Self::new(surface);
        let bounds = panel.bounds();
        let offset = Vec2::new(
            offset.x.clamp(
                -surface.width / 2. - bounds.left,
                surface.width / 2. - bounds.left - bounds.width,
            ),
            offset.y.clamp(
                -surface.height / 2. - bounds.top,
                surface.height / 2. - bounds.top - bounds.height,
            ),
        );
        panel.login.x += offset.x;
        panel.login.game_y += offset.y;
        panel.login.server_y += offset.y;
        panel.login.enter_y += offset.y;
        panel.header_y += offset.y;
        panel.pager_y += offset.y;
        (panel, offset)
    }
    fn bounds(&self) -> plan::Rect {
        let bottom = self.login.game_y - 82. * self.login.scale;
        plan::Rect {
            left: self.login.x - 191. * self.login.scale,
            top: bottom,
            width: 382. * self.login.scale,
            height: self.header_y + 24. * self.login.scale - bottom,
        }
    }
    fn title_bar(&self) -> plan::Rect {
        plan::Rect {
            left: self.bounds().left,
            top: self.header_y - 16. * self.login.scale,
            width: self.bounds().width,
            height: 40. * self.login.scale,
        }
    }
    fn row(&self, i: usize) -> plan::Rect {
        plan::Rect {
            left: self.login.x - self.login.width / 2.,
            top: self.header_y - (40. + i as f32 * 32.) * self.login.scale,
            width: self.login.width,
            height: 28. * self.login.scale,
        }
    }
    fn pager(&self, next: bool) -> plan::Rect {
        plan::Rect {
            left: self.login.x
                + if next {
                    self.login.width / 2. - 72. * self.login.scale
                } else {
                    -self.login.width / 2.
                },
            top: self.pager_y - 14. * self.login.scale,
            width: 72. * self.login.scale,
            height: 28. * self.login.scale,
        }
    }
    fn page(&self, lobby: &Lobby) -> usize {
        lobby
            .page
            .min(lobby.games.len().saturating_sub(1) / self.rows)
    }
}
fn client_window(preview: bool) -> Window {
    Window {
        title: "Maršál a špión | Field command".into(),
        resolution: if preview {
            (plan::WIDTH, plan::WINDOW_HEIGHT).into()
        } else {
            (1280, 900).into()
        },
        // Enable mouse resizing at creation and keep it enabled on every screen.
        resizable: true,
        resize_constraints: WindowResizeConstraints {
            min_width: 360.,
            min_height: 480.,
            ..default()
        },
        ..default()
    }
}
#[derive(Component)]
struct Painted;
#[derive(Component)]
struct InputCaret(Instant);

fn position_input_caret(
    texts: Query<(&bevy::text::TextLayoutInfo, &GlobalTransform), Without<InputCaret>>,
    mut carets: Query<(
        &InputCaret,
        &ChildOf,
        &mut Transform,
        &mut GlobalTransform,
        &mut Sprite,
    )>,
) {
    for (caret, parent, mut transform, mut global, mut sprite) in &mut carets {
        let Ok((layout, parent_global)) = texts.get(parent.parent()) else {
            continue;
        };
        // Use the exact shaped advance, including spaces, rather than a caret
        // glyph's ink bearing or the rounded text bounding box.
        transform.translation.x = layout
            .run_geometry
            .iter()
            .map(|run| run.bounds.max.x)
            .fold(0., f32::max)
            / layout.scale_factor.max(f32::EPSILON);
        *global = parent_global.mul_transform(*transform);
        sprite.color = if caret.0.elapsed().as_millis() % 1000 < 500 {
            Color::srgb(0.94, 0.88, 0.74)
        } else {
            Color::NONE
        };
    }
}
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum Clock {
    Yours,
    Opponent,
    Setup,
}
#[derive(Component)]
struct ActiveClock;
#[derive(Component)]
struct ReserveCount;
#[derive(Component)]
struct RemainingCount;
#[derive(Resource, Default)]
struct Surface {
    layout: plan::Layout,
    playing: Option<bool>,
}
#[derive(Resource)]
struct Offscreen(Handle<Image>);
#[derive(Resource)]
struct Art {
    font: Handle<Font>,
    bold_font: Handle<Font>,
    ranks: Vec<Handle<Image>>,
    board: Handle<Image>,
    cover: Handle<Image>,
}
#[derive(Default)]
struct Lobby {
    address: String,
    games: Vec<String>,
    loading: bool,
    loaded: bool,
    page: usize,
}
enum Event {
    ConnectingSocket(TcpStream),
    Connected(SyncSender<Action>, TcpStream),
    Reply(Reply),
    Failed(String),
}
const MOVE_TRAVEL_MS: f32 = 180.;
const CAPTURE_FADE_MS: f32 = 220.;
const CAPTURE_DURATION_MS: f32 = MOVE_TRAVEL_MS + CAPTURE_FADE_MS;
struct MovePlayback {
    event: MoveView,
    received: Instant,
}
impl MovePlayback {
    fn elapsed_ms(&self) -> f32 {
        self.event.elapsed_ms as f32 + self.received.elapsed().as_secs_f32() * 1000.
    }
}
fn motion_duration_ms(event: &MoveView) -> f32 {
    if event.defender.is_some() {
        CAPTURE_DURATION_MS
    } else {
        MOVE_TRAVEL_MS
    }
}
fn motion_sample(elapsed_ms: f32) -> (f32, f32) {
    let travel = (elapsed_ms / MOVE_TRAVEL_MS).clamp(0., 1.);
    let travel = travel * travel * (3. - 2. * travel);
    let opacity = 1. - ((elapsed_ms - MOVE_TRAVEL_MS) / CAPTURE_FADE_MS).clamp(0., 1.);
    (travel, opacity)
}
#[derive(Resource)]
struct Client {
    address: String,
    room: String,
    field: usize,
    connecting: bool,
    tx: Option<SyncSender<Action>>,
    socket: Option<TcpStream>,
    events: Mutex<Receiver<(u64, Event)>>,
    sender: SyncSender<(u64, Event)>,
    view: Option<View>,
    received: Instant,
    error: String,
    selected: Option<usize>,
    rank: Rank,
    dirty: bool,
    ready_sent: bool,
    preview: bool,
    session: u64,
    broken: bool,
    lobby: Lobby,
    join_waiting: bool,
    auto_join: bool,
    lobby_offset: Vec2,
    lobby_drag: Option<Vec2>,
    motion: Option<MovePlayback>,
}
impl Client {
    fn send(&mut self, a: Action) -> bool {
        self.error.clear();
        let sent = if let Some(tx) = &self.tx {
            if tx.try_send(a).is_err() {
                self.error = "Connection busy or closed".into();
                false
            } else {
                true
            }
        } else {
            self.error = if self.preview {
                "Offline preview. Restart without --preview to play.".into()
            } else {
                "Disconnected. Click the info bar to return.".into()
            };
            false
        };
        self.dirty = true;
        sent
    }
    fn leave(&mut self) {
        self.session += 1;
        if let Some(s) = self.socket.take() {
            let _ = s.shutdown(Shutdown::Both);
        }
        self.tx = None;
        self.view = None;
        self.selected = None;
        self.motion = None;
        self.ready_sent = false;
        self.broken = false;
        self.error.clear();
        self.connecting = false;
        self.auto_join = false;
        self.lobby_drag = None;
        self.lobby = Lobby::default();
        self.dirty = true;
    }
    fn break_connection(&mut self) {
        // Retain the board until the dialog is acknowledged. Invalidate queued
        // network events so they cannot replace or dismiss the interruption.
        self.session += 1;
        if let Some(socket) = self.socket.take() {
            let _ = socket.shutdown(Shutdown::Both);
        }
        self.tx = None;
        self.connecting = false;
        self.selected = None;
        self.error.clear();
        self.broken = true;
        self.lobby = Lobby::default();
        self.motion = None;
        self.dirty = true;
    }
    fn connect(&mut self) {
        self.connect_game(false);
    }
    fn join_action(&self) -> Action {
        if self.join_waiting {
            Action::JoinWaiting {
                game: self.room.clone(),
            }
        } else {
            Action::Join {
                game: self.room.clone(),
            }
        }
    }
    fn connect_game(&mut self, waiting: bool) {
        self.join_waiting = waiting;
        if self.room.trim().is_empty() {
            self.error = "Enter a game name".into();
            self.dirty = true;
            return;
        }
        if self.tx.is_some() {
            self.send(self.join_action());
        }
    }
    fn toggle_connection(&mut self) {
        if self.connecting || self.tx.is_some() {
            self.leave();
        } else {
            self.start_connection();
        }
    }
    fn start_connection(&mut self) {
        if self.connecting || self.tx.is_some() {
            return;
        }
        self.lobby = Lobby {
            address: self.address.clone(),
            loading: true,
            ..default()
        };
        self.connecting = true;
        self.error.clear();
        self.dirty = true;
        self.session += 1;
        let session = self.session;
        let address = self.address.clone();
        let events = self.sender.clone();
        thread::spawn(move || {
            let result = (|| -> std::io::Result<TcpStream> {
                let addresses = address.to_socket_addrs()?;
                for a in addresses {
                    if let Ok(s) = TcpStream::connect_timeout(&a, Duration::from_secs(3)) {
                        return Ok(s);
                    }
                }
                Err(std::io::Error::other("Could not connect to server"))
            })();
            let mut stream = match result {
                Ok(s) => s,
                Err(e) => {
                    let _ = events.send((session, Event::Failed(e.to_string())));
                    return;
                }
            };
            // Expose the socket before authentication so cancel can interrupt
            // an in-flight handshake as well as an established connection.
            match stream.try_clone() {
                Ok(socket) => {
                    if events
                        .send((session, Event::ConnectingSocket(socket)))
                        .is_err()
                    {
                        let _ = stream.shutdown(Shutdown::Both);
                        return;
                    }
                }
                Err(error) => {
                    let _ = events.send((session, Event::Failed(error.to_string())));
                    return;
                }
            }
            let (mut outgoing, mut incoming) = match net::client_handshake(&mut stream) {
                Ok(states) => states,
                Err(_) => {
                    let _ = stream.shutdown(Shutdown::Both);
                    let _ = events.send((
                        session,
                        Event::Failed("Server authentication failed".into()),
                    ));
                    return;
                }
            };
            let _ = stream.set_nodelay(true);
            let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
            let mut writer = match stream.try_clone() {
                Ok(s) => s,
                Err(e) => {
                    let _ = events.send((session, Event::Failed(e.to_string())));
                    return;
                }
            };
            let reader = match stream.try_clone() {
                Ok(s) => s,
                Err(e) => {
                    let _ = events.send((session, Event::Failed(e.to_string())));
                    return;
                }
            };
            let (tx, rx) = mpsc::sync_channel(32);
            if events
                .send((session, Event::Connected(tx, stream)))
                .is_err()
            {
                return;
            }
            thread::spawn(move || {
                while let Ok(a) = rx.recv() {
                    if outgoing.send(&mut writer, &a).is_err() {
                        break;
                    }
                }
                let _ = writer.shutdown(Shutdown::Both);
            });
            let mut reader = reader;
            let reason = loop {
                match incoming.receive::<Reply>(&mut reader, net::MAX_FRAME) {
                    Ok(reply) => {
                        if events.send((session, Event::Reply(reply))).is_err() {
                            return;
                        }
                    }
                    Err(e) => break e.to_string(),
                }
            };
            let _ = events.send((session, Event::Failed(reason)));
        });
    }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--licenses") {
        println!("{}", client_assets::FONT_LICENSE);
        return;
    }
    let (sender, events) = mpsc::sync_channel(128);
    let preview = args.iter().any(|a| a == "--preview");
    let mut view = None;
    if preview {
        let cfg = Config::default();
        let mut g = Game::new(0, &cfg);
        g.start(0, &cfg);
        view = Some(g.view(0, "Artwork preview", 0));
    }
    let address = args
        .windows(2)
        .find(|a| a[0] == "--server")
        .map(|a| a[1].clone())
        .unwrap_or("127.0.0.1:4000".into());
    let room = args
        .windows(2)
        .find(|a| a[0] == "--room")
        .map(|a| a[1].clone())
        .unwrap_or_default();
    let autojoin = !room.is_empty() && !preview;
    let mut client = Client {
        address,
        room,
        field: 1,
        connecting: false,
        tx: None,
        socket: None,
        events: Mutex::new(events),
        sender,
        view,
        received: Instant::now(),
        error: String::new(),
        selected: None,
        rank: Rank::Flag,
        dirty: true,
        ready_sent: false,
        preview,
        session: 0,
        broken: false,
        lobby: Lobby::default(),
        join_waiting: false,
        auto_join: false,
        lobby_offset: Vec2::ZERO,
        lobby_drag: None,
        motion: None,
    };
    if autojoin {
        client.auto_join = true;
        client.start_connection();
    }
    let headless = args.iter().any(|a| a == "--headless");
    let mut app = App::new();
    app.insert_resource(client)
        .init_resource::<Surface>()
        .insert_resource(ClearColor(Color::srgb(0.055, 0.065, 0.07)));
    let plugins = DefaultPlugins.set(AssetPlugin {
        meta_check: bevy::asset::AssetMetaCheck::Never,
        ..default()
    });
    if headless {
        app.add_plugins(
            plugins
                .set(WindowPlugin {
                    primary_window: None,
                    exit_condition: bevy::window::ExitCondition::DontExit,
                    ..default()
                })
                .disable::<bevy::winit::WinitPlugin>(),
        )
        .add_plugins(bevy::app::ScheduleRunnerPlugin::run_loop(
            Duration::from_secs_f64(1. / 60.),
        ));
    } else {
        app.add_plugins(plugins.set(WindowPlugin {
            primary_window: Some(client_window(preview)),
            ..default()
        }));
    }
    app.add_plugins(client_assets::EmbeddedArtPlugin)
        .add_systems(Startup, setup)
        .add_systems(
            PostUpdate,
            position_input_caret
                .after(bevy::text::Text2dUpdateSystems)
                .before(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
        )
        .add_systems(
            Update,
            (
                receive,
                fit_surface,
                animate_motion,
                input,
                paint,
                clock_text,
                screenshot,
            )
                .chain(),
        )
        .run();
}
fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    c: Res<Client>,
) {
    if std::env::args().any(|a| a == "--headless") {
        let mut image = Image::new_target_texture(
            if c.view.is_some() { plan::WIDTH } else { 1280 },
            if c.view.is_some() {
                plan::WINDOW_HEIGHT
            } else {
                900
            },
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            None,
        );
        image.texture_descriptor.usage |= bevy::render::render_resource::TextureUsages::COPY_SRC;
        let target = images.add(image);
        commands.spawn((
            Camera2d,
            bevy::camera::RenderTarget::Image(target.clone().into()),
        ));
        commands.insert_resource(Offscreen(target));
    } else {
        commands.spawn(Camera2d);
    }
    commands.insert_resource(Art {
        font: assets.load(client_assets::path("font.ttf")),
        bold_font: assets.load(client_assets::path("font-bold.ttf")),
        ranks: Rank::ALL
            .into_iter()
            .map(|r| assets.load(client_assets::path(&format!("{}.png", r.asset()))))
            .collect(),
        board: assets.load(client_assets::path("board.png")),
        cover: assets.load(client_assets::path("cover.png")),
    });
}

fn fit_surface(
    mut c: ResMut<Client>,
    mut surface: ResMut<Surface>,
    mut window: Option<Single<&mut Window>>,
    offscreen: Option<Res<Offscreen>>,
    mut images: ResMut<Assets<Image>>,
) {
    let playing = c.view.is_some();
    let initial_size = if playing {
        (plan::WIDTH, plan::WINDOW_HEIGHT)
    } else {
        (1280, 900)
    };
    // Set a sensible initial size only when entering/leaving a game. Continuous
    // updates must observe the user's resize, never reset it on the next frame.
    if surface.playing != Some(playing) {
        if let Some(window) = window.as_mut() {
            window
                .resolution
                .set(initial_size.0 as f32, initial_size.1 as f32);
        }
        if let Some(target) = &offscreen
            && let Some(image) = images.get_mut(&target.0)
        {
            image.resize(bevy::render::render_resource::Extent3d {
                width: initial_size.0,
                height: initial_size.1,
                depth_or_array_layers: 1,
            });
        }
        surface.playing = Some(playing);
        c.dirty = true;
    }
    let (w, h) = if let Some(window) = window.as_ref() {
        (window.width(), window.height())
    } else if let Some(target) = &offscreen
        && let Some(image) = images.get(&target.0)
    {
        (image.width() as f32, image.height() as f32)
    } else {
        (initial_size.0 as f32, initial_size.1 as f32)
    };
    let layout = plan::Layout::new(w, h);
    if surface.layout != layout {
        surface.layout = layout;
        c.dirty = true;
    }
}

fn receive(mut c: ResMut<Client>) {
    let events: Vec<_> = c.events.lock().unwrap().try_iter().collect();
    for (session, event) in events {
        if session != c.session {
            if let Event::Connected(_, socket) | Event::ConnectingSocket(socket) = event {
                let _ = socket.shutdown(Shutdown::Both);
            }
            continue;
        }
        match event {
            Event::ConnectingSocket(socket) => c.socket = Some(socket),
            Event::Reply(Reply::Games { games }) => {
                if c.view.is_none() && c.tx.is_some() {
                    c.lobby.games = games;
                    c.lobby.loaded = true;
                    c.lobby.loading = false;
                    c.dirty = true;
                }
            }
            Event::Connected(tx, socket) => {
                c.tx = Some(tx);
                c.socket = Some(socket);
                c.connecting = false;
                c.field = 1;
                c.dirty = true;
                if c.auto_join {
                    c.auto_join = false;
                    c.connect();
                }
            }
            Event::Reply(Reply::State { view }) => {
                if matches!(&view.phase, Phase::Finished { reason, .. } if reason == "Opponent disconnected")
                {
                    c.break_connection();
                    continue;
                }
                let lock = matches!(view.phase, Phase::Placement)
                    && !view.ready[view.you]
                    && view.reserve.iter().all(|(_, n)| *n == 0)
                    && !c.ready_sent;
                if matches!(view.phase, Phase::Placement)
                    && view
                        .reserve
                        .iter()
                        .find(|(r, _)| *r == c.rank)
                        .is_some_and(|(_, n)| *n == 0)
                    && let Some(rank) = plan::STRIP
                        .into_iter()
                        .find(|r| view.reserve.iter().any(|(vr, n)| vr == r && *n > 0))
                {
                    c.rank = rank;
                }
                if c.view
                    .as_ref()
                    .is_some_and(|old| old.turn != view.turn || old.phase != view.phase)
                {
                    c.selected = None;
                    c.error.clear();
                }
                let new_ply = c
                    .view
                    .as_ref()
                    .is_some_and(|old| old.game == view.game && view.ply > old.ply);
                if new_ply {
                    c.motion = view
                        .motion
                        .as_ref()
                        .filter(|event| {
                            event.ply == view.ply
                                && (event.elapsed_ms as f32) < motion_duration_ms(event)
                        })
                        .cloned()
                        .map(|event| MovePlayback {
                            event: *event,
                            received: Instant::now(),
                        });
                    c.selected = None;
                }
                c.view = Some(view);
                c.received = Instant::now();
                c.dirty = true;
                if lock {
                    c.ready_sent = c.send(Action::Ready);
                }
            }
            Event::Reply(Reply::Error { message }) => {
                c.error = message;
                c.connecting = false;
                c.dirty = true;
            }
            Event::Failed(message) => {
                if c.view
                    .as_ref()
                    .is_some_and(|v| !matches!(v.phase, Phase::Finished { .. }))
                {
                    c.break_connection();
                    continue;
                }
                c.leave();
                c.error = format!("Disconnected: {message}");
                c.dirty = true;
            }
        }
    }
}
fn animate_motion(mut c: ResMut<Client>) {
    if let Some(capture) = &c.motion {
        if capture.elapsed_ms() >= motion_duration_ms(&capture.event) {
            c.motion = None;
        }
        c.dirty = true;
    }
}
fn input(
    mut c: ResMut<Client>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut text: MessageReader<KeyboardInput>,
    window: Option<Single<&Window>>,
    surface: Res<Surface>,
) {
    if c.broken {
        text.clear();
        let button = broken_ok(surface.layout);
        let clicked_ok = mouse.just_pressed(MouseButton::Left)
            && window
                .as_ref()
                .and_then(|w| w.cursor_position())
                .is_some_and(|cursor| {
                    let p = surface.layout.cursor(cursor.x, cursor.y);
                    button.contains(p)
                });
        if clicked_ok
            || keys.just_pressed(KeyCode::Enter)
            || keys.just_pressed(KeyCode::Space)
            || keys.just_pressed(KeyCode::Escape)
        {
            c.leave();
        }
        return;
    }
    if c.view.as_ref().and_then(flag_result).is_some()
        && (keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space))
    {
        c.leave();
        text.clear();
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        c.leave();
    }
    if c.view.is_none() {
        for event in text.read() {
            if !event.state.is_pressed() {
                continue;
            }
            match &event.logical_key {
                Key::Tab => {
                    c.field = 1 - c.field;
                    c.dirty = true;
                }
                Key::Backspace => {
                    if c.field == 0 && !c.connecting && c.tx.is_none() {
                        c.address.pop();
                    } else if c.field == 1 {
                        c.room.pop();
                    }
                    c.dirty = true;
                }
                Key::Enter if c.tx.is_some() => c.connect(),
                Key::Character(s)
                    if (c.field == 1 || (!c.connecting && c.tx.is_none()))
                        && !s.chars().any(char::is_control) =>
                {
                    let field = if c.field == 0 {
                        &mut c.address
                    } else {
                        &mut c.room
                    };
                    if field.chars().count() < 48 {
                        field.push_str(s);
                    }
                    c.dirty = true;
                }
                _ => {}
            }
        }
    } else {
        text.clear();
    }
    if !mouse.pressed(MouseButton::Left) || c.view.is_some() {
        c.lobby_drag = None;
    }
    let Some(window) = window else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    // Use the current logical window size, rather than a camera projection
    // cached before this frame's resize. The UI camera stays at the origin.
    let cursor_point = surface.layout.cursor(cursor.x, cursor.y);
    let pos = Vec2::new(cursor_point.x, cursor_point.y);
    let clicked = mouse.just_pressed(MouseButton::Left);
    let right = mouse.just_pressed(MouseButton::Right);
    if c.view.is_none() {
        let (waiting, offset) = WaitingLayout::positioned(surface.layout, c.lobby_offset);
        if clicked && waiting.title_bar().contains(cursor_point) {
            c.lobby_drag = Some(pos - offset);
        }
        if let Some(anchor) = c.lobby_drag {
            let (_, offset) = WaitingLayout::positioned(surface.layout, pos - anchor);
            if c.lobby_offset != offset {
                c.lobby_offset = offset;
                c.dirty = true;
            }
            // Dragging the header must never activate a game or form control.
            return;
        }
        let login = &waiting.login;
        if clicked && c.tx.is_some() {
            let page = waiting.page(&c.lobby);
            if c.lobby.address == c.address {
                for i in 0..waiting.rows {
                    if waiting.row(i).contains(cursor_point)
                        && let Some(game) = c.lobby.games.get(page * waiting.rows + i).cloned()
                    {
                        c.room = game;
                        c.connect_game(true);
                        return;
                    }
                }
                if waiting.pager(false).contains(cursor_point) && page > 0 {
                    c.lobby.page = page - 1;
                    c.dirty = true;
                }
                if waiting.pager(true).contains(cursor_point)
                    && (page + 1) * waiting.rows < c.lobby.games.len()
                {
                    c.lobby.page = page + 1;
                    c.dirty = true;
                }
            }
        }
        if clicked {
            if login.connection_button().contains(cursor_point) {
                c.toggle_connection();
                return;
            }
            if hit(
                pos,
                login.x - 55. * login.scale,
                login.server_y,
                login.width - 110. * login.scale,
                36. * login.scale,
            ) && !c.connecting
                && c.tx.is_none()
            {
                c.field = 0;
                c.dirty = true;
            }
            if hit(pos, login.x, login.game_y, login.width, 36. * login.scale) {
                c.field = 1;
                c.dirty = true;
            }
            if hit(pos, login.x, login.enter_y, login.width, 34. * login.scale) && c.tx.is_some() {
                c.connect();
            }
        }
        return;
    }
    if !clicked && !right {
        return;
    }
    let v = c.view.as_ref().unwrap().clone();
    let point = plan::Point { x: pos.x, y: pos.y };
    if flag_result(&v).is_some() {
        if clicked && result_ok(surface.layout).contains(point) {
            c.leave();
        }
        return;
    }
    if clicked && v.can_claim_victory() && claim_button(surface.layout).contains(point) {
        c.send(Action::ClaimVictory { turn_id: v.ply });
        return;
    }
    if c.motion.is_some() {
        return;
    }
    if surface.layout.in_bar(point) {
        if clicked && (matches!(v.phase, Phase::Finished { .. }) || c.tx.is_none() && !c.preview) {
            c.leave();
        } else if right && !matches!(v.phase, Phase::Finished { .. }) {
            c.send(Action::Resign);
        }
        return;
    }
    if matches!(v.phase, Phase::Placement) && !v.ready[v.you] && !c.ready_sent {
        if clicked && let Some(rank) = surface.layout.pick_rank(point, v.you) {
            if v.reserve.iter().any(|(r, n)| *r == rank && *n > 0) {
                c.rank = rank;
                c.error.clear();
            } else {
                c.error = format!("All {} pieces are already deployed.", rank.name());
            }
            c.dirty = true;
            return;
        }
        if let Some(at) = surface.layout.pick_cell(point, v.you) {
            if !home(v.you, at) {
                c.error = "Deploy in your bottom four rows.".into();
                c.dirty = true;
                return;
            }
            if right {
                c.send(Action::Remove { at });
            } else if v.board[at].is_some() {
                c.error = "Right-click a placed piece to return it to the strip.".into();
                c.dirty = true;
            } else {
                let rank = c.rank;
                c.send(Action::Place { at, rank });
            }
        }
    } else if matches!(v.phase, Phase::Playing) && clicked {
        let Some(at) = surface.layout.pick_cell(point, v.you) else {
            return;
        };
        if v.turn != v.you {
            c.error = "Waiting for your opponent.".into();
            c.dirty = true;
            return;
        }
        if c.selected == Some(at) {
            c.selected = None;
            c.dirty = true;
            return;
        }
        if v.board[at]
            .as_ref()
            .is_some_and(|p| p.owner == v.you && p.rank.is_some_and(Rank::mobile))
        {
            c.error.clear();
            c.selected = Some(at);
            c.dirty = true;
        } else if let Some(from) = c.selected {
            if v.legal_targets(from).contains(&at) {
                c.selected = None;
                c.send(Action::Move { from, to: at });
            } else {
                c.error = "Choose a highlighted destination.".into();
                c.dirty = true;
            }
        }
    }
}
fn hit(p: Vec2, x: f32, y: f32, w: f32, h: f32) -> bool {
    (p.x - x).abs() < w / 2. && (p.y - y).abs() < h / 2.
}
fn rect(cmd: &mut Commands, x: f32, y: f32, w: f32, h: f32, z: f32, color: Color) {
    cmd.spawn((
        Painted,
        Sprite::from_color(color, Vec2::new(w, h)),
        Transform::from_xyz(x, y, z),
    ));
}
fn picture(cmd: &mut Commands, image: Handle<Image>, x: f32, y: f32, w: f32, h: f32, z: f32) {
    cmd.spawn((
        Painted,
        Sprite {
            image,
            custom_size: Some(Vec2::new(w, h)),
            ..default()
        },
        Transform::from_xyz(x, y, z),
    ));
}
fn label(
    cmd: &mut Commands,
    art: &Art,
    s: impl Into<String>,
    x: f32,
    y: f32,
    size: f32,
    color: Color,
) -> Entity {
    cmd.spawn((
        Painted,
        Text2d::new(s),
        TextFont {
            font: art.font.clone(),
            font_size: size,
            ..default()
        },
        TextColor(color),
        Anchor::CENTER_LEFT,
        Transform::from_xyz(x, y, 8.),
    ))
    .id()
}
fn border(cmd: &mut Commands, x: f32, y: f32, w: f32, h: f32, z: f32, color: Color) {
    border_with_stroke(cmd, x, y, w, h, z, color, w.min(h) / 25.);
}
fn border_with_stroke(
    cmd: &mut Commands,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    z: f32,
    color: Color,
    stroke: f32,
) {
    for (dx, dy, width, height) in [
        (0., h / 2. - stroke / 2., w, stroke),
        (0., -h / 2. + stroke / 2., w, stroke),
        (-w / 2. + stroke / 2., 0., stroke, h),
        (w / 2. - stroke / 2., 0., stroke, h),
    ] {
        rect(cmd, x + dx, y + dy, width, height, z, color);
    }
}
fn printed_rect(
    cmd: &mut Commands,
    area: plan::Rect,
    you: usize,
    z: f32,
    color: Color,
    outline: bool,
    layout: plan::Layout,
) {
    let point = layout.world(area.center(), you);
    let w = area.width / plan::PAGE_WIDTH * layout.board_width;
    let h = area.height / plan::PAGE_HEIGHT * layout.board_height;
    if outline {
        border(cmd, point.x, point.y, w, h, z, color);
    } else {
        rect(cmd, point.x, point.y, w, h, z, color);
    }
}
fn shortened(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.into()
    } else {
        s.chars()
            .take(max - 1)
            .chain(std::iter::once('…'))
            .collect()
    }
}
struct PieceVisual {
    point: plan::Point,
    size: Vec2,
    scale: f32,
    opacity: f32,
    layer: f32,
}
fn paint_piece(cmd: &mut Commands, art: &Art, piece: &VisiblePiece, visual: PieceVisual) {
    let PieceVisual {
        point,
        size,
        scale,
        opacity,
        layer,
    } = visual;
    let face = if piece.owner == 0 {
        Color::srgb(0.87, 0.36, 0.28)
    } else {
        Color::srgb(0.96, 0.94, 0.87)
    };
    rect(
        cmd,
        point.x + 1.5 * scale,
        point.y - 1.5 * scale,
        size.x - 10. * scale,
        size.y - 8. * scale,
        2.5 + layer,
        Color::srgba(0.1, 0.07, 0.04, 0.4 * opacity),
    );
    rect(
        cmd,
        point.x,
        point.y,
        size.x - 10. * scale,
        size.y - 8. * scale,
        3. + layer,
        face.with_alpha(opacity),
    );
    border(
        cmd,
        point.x,
        point.y,
        size.x - 10. * scale,
        size.y - 8. * scale,
        3.5 + layer,
        Color::srgba(0.42, 0.29, 0.17, opacity),
    );
    if let Some(rank) = piece.rank {
        let side = size.x.min(size.y) - 15. * scale;
        cmd.spawn((
            Painted,
            Sprite {
                image: art.ranks[rank as usize].clone(),
                custom_size: Some(Vec2::splat(side)),
                color: Color::srgba(1., 1., 1., opacity),
                ..default()
            },
            Transform::from_xyz(point.x, point.y, 4. + layer),
        ));
    }
}
fn count_label(
    cmd: &mut Commands,
    art: &Art,
    rank: Rank,
    owner: usize,
    you: usize,
    count: usize,
    layout: plan::Layout,
) -> Entity {
    let point = layout.world(plan::count_position(rank, owner), you);
    cmd.spawn((
        Painted,
        Text2d::new(format!("{count}×")),
        TextFont {
            font: art.font.clone(),
            font_size: 12. * layout.scale,
            ..default()
        },
        TextColor(Color::BLACK),
        Anchor::CENTER,
        Transform::from_xyz(point.x, point.y, 8.),
    ))
    .id()
}
fn clock_panels(cmd: &mut Commands, art: &Art, view: &View, layout: plan::Layout) {
    let top = layout.height / 2.;
    let width = layout.width / 2.;
    let playing = matches!(view.phase, Phase::Playing);
    for (i, role) in [Clock::Yours, Clock::Opponent].into_iter().enumerate() {
        let left = -layout.width / 2. + i as f32 * width;
        let active = playing
            && match role {
                Clock::Yours => view.turn == view.you,
                Clock::Opponent => view.turn != view.you,
                _ => false,
            };
        let panel = cmd
            .spawn((
                Painted,
                Sprite::from_color(
                    if active {
                        Color::srgb(0.08, 0.30, 0.40)
                    } else {
                        Color::srgb(0.10, 0.13, 0.15)
                    },
                    Vec2::new(width - 4., 44.),
                ),
                Transform::from_xyz(left + width / 2., top - 24., 6.),
            ))
            .id();
        if active {
            cmd.entity(panel).insert(ActiveClock);
        }
        let caption = match (role, active) {
            (Clock::Yours, true) => "Your time · turn delay",
            (Clock::Opponent, true) => "Opponent's time · turn delay",
            (Clock::Yours, false) => "Your time",
            _ => "Opponent's time",
        };
        label(
            cmd,
            art,
            caption,
            left + 8.,
            top - 12.,
            10.,
            Color::srgb(0.85, 0.89, 0.91),
        );
        cmd.spawn((
            Painted,
            role,
            Text2d::new(""),
            TextFont {
                font: if active {
                    art.bold_font.clone()
                } else {
                    art.font.clone()
                },
                font_size: (width / 9.).clamp(14., 22.),
                ..default()
            },
            TextColor(if active {
                Color::srgb(0.45, 0.91, 1.)
            } else {
                Color::WHITE
            }),
            Anchor::CENTER_LEFT,
            Transform::from_xyz(left + 8., top - 32., 8.),
        ));
    }
}
fn claim_button(layout: plan::Layout) -> plan::Rect {
    plan::Rect {
        left: layout.width / 2. - 148.,
        top: layout.height / 2. - 80.,
        width: 136.,
        height: 32.,
    }
}
fn paint(
    mut cmd: Commands,
    mut c: ResMut<Client>,
    art: Res<Art>,
    old: Query<Entity, With<Painted>>,
    surface: Res<Surface>,
) {
    if !c.dirty {
        return;
    }
    c.dirty = false;
    for e in &old {
        cmd.entity(e).despawn();
    }
    let ink = Color::srgb(0.94, 0.88, 0.74);
    let gold = Color::srgb(0.94, 0.67, 0.32);
    let Some(v) = &c.view else {
        let (waiting, _) = WaitingLayout::positioned(surface.layout, c.lobby_offset);
        let login = &waiting.login;
        let scale = login.scale;
        let cover_width =
            (surface.layout.width - 90.).min((surface.layout.height - 90.) * 1532. / 1027.);
        picture(
            &mut cmd,
            art.cover.clone(),
            0.,
            0.,
            cover_width,
            cover_width * 1027. / 1532.,
            0.,
        );
        let panel_bottom = login.game_y - 82. * scale;
        let panel_top = waiting.header_y + 24. * scale;
        rect(
            &mut cmd,
            login.x,
            (panel_top + panel_bottom) / 2.,
            382. * scale,
            panel_top - panel_bottom,
            1.,
            Color::srgba(0.035, 0.05, 0.055, 0.90),
        );
        for (i, (name, value, y)) in [
            ("Server:", c.address.as_str(), login.server_y),
            ("Game:", c.room.as_str(), login.game_y),
        ]
        .into_iter()
        .enumerate()
        {
            rect(
                &mut cmd,
                login.x - if i == 0 { 55. * scale } else { 0. },
                y,
                login.width - if i == 0 { 110. * scale } else { 0. },
                36. * scale,
                3.,
                if c.field == i {
                    Color::srgb(0.24, 0.29, 0.29)
                } else {
                    Color::srgb(0.12, 0.16, 0.17)
                },
            );
            label(
                &mut cmd,
                &art,
                name,
                login.x - login.width / 2. + 12. * scale,
                y,
                16. * scale,
                ink,
            );
            let tail: String = value
                .chars()
                .rev()
                .take(if i == 0 { 15 } else { 28 })
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let display = if value.chars().count() > if i == 0 { 15 } else { 28 } {
                format!("…{tail}")
            } else {
                tail
            };
            let value_label = label(
                &mut cmd,
                &art,
                display,
                login.x - login.width / 2. + 80. * scale,
                y,
                16. * scale,
                ink,
            );
            if c.field == i && (i == 1 || (!c.connecting && c.tx.is_none())) {
                cmd.entity(value_label).with_children(|parent| {
                    parent.spawn((
                        Sprite::from_color(ink, Vec2::new(scale, 16. * scale)),
                        Anchor::CENTER_LEFT,
                        Transform::from_xyz(0., 0., 0.1),
                        InputCaret(Instant::now()),
                    ));
                });
            }
        }
        let button = login.connection_button();
        let center = button.center();
        rect(
            &mut cmd,
            center.x,
            center.y,
            button.width,
            button.height,
            5.,
            if c.tx.is_some() {
                Color::srgb(0.08, 0.48, 0.2)
            } else {
                Color::srgb(0.65, 0.12, 0.1)
            },
        );
        label(
            &mut cmd,
            &art,
            if c.connecting {
                "Connecting…"
            } else if c.tx.is_some() {
                "Connected"
            } else {
                "Connect"
            },
            button.left + 5. * scale,
            center.y,
            13. * scale,
            ink,
        );
        rect(
            &mut cmd,
            login.x,
            login.enter_y,
            login.width,
            34. * scale,
            5.,
            if c.tx.is_some() {
                Color::srgb(0.6, 0.27, 0.18)
            } else {
                Color::srgb(0.2, 0.2, 0.2)
            },
        );
        label(
            &mut cmd,
            &art,
            "Enter",
            login.x - login.width / 2. + 12. * scale,
            login.enter_y,
            15. * scale,
            ink,
        );
        if !c.error.is_empty() {
            label(
                &mut cmd,
                &art,
                shortened(&c.error, 44),
                login.x - login.width / 2.,
                login.game_y - 70. * scale,
                13. * scale,
                gold,
            );
        }
        label(
            &mut cmd,
            &art,
            "Waiting games",
            login.x - login.width / 2.,
            waiting.header_y,
            18. * scale,
            ink,
        );
        let page = waiting.page(&c.lobby);
        let games = if c.lobby.address == c.address {
            c.lobby.games.as_slice()
        } else {
            &[]
        };
        for (i, game) in games
            .iter()
            .skip(page * waiting.rows)
            .take(waiting.rows)
            .enumerate()
        {
            let area = waiting.row(i);
            let center = area.center();
            rect(
                &mut cmd,
                center.x,
                center.y,
                area.width,
                area.height,
                3.,
                Color::srgb(0.12, 0.23, 0.29),
            );
            label(
                &mut cmd,
                &art,
                shortened(game, 34),
                area.left + 10. * scale,
                center.y,
                14. * scale,
                ink,
            );
            label(
                &mut cmd,
                &art,
                "Join",
                area.left + area.width - 40. * scale,
                center.y,
                13. * scale,
                gold,
            );
        }
        if games.is_empty() {
            let message = if c.tx.is_none() && !c.connecting {
                "Connect to see waiting games"
            } else if c.lobby.address != c.address || !c.lobby.loaded || c.lobby.loading {
                "Looking for games…"
            } else {
                "No waiting games. Create one below."
            };
            label(
                &mut cmd,
                &art,
                message,
                login.x - login.width / 2.,
                waiting.row(0).center().y,
                13. * scale,
                ink,
            );
        }
        if games.len() > waiting.rows {
            for (next, caption) in [(false, "Previous"), (true, "Next")] {
                if (!next && page == 0) || (next && (page + 1) * waiting.rows >= games.len()) {
                    continue;
                }
                let area = waiting.pager(next);
                let center = area.center();
                rect(
                    &mut cmd,
                    center.x,
                    center.y,
                    area.width,
                    area.height,
                    3.,
                    Color::srgb(0.12, 0.23, 0.29),
                );
                label(
                    &mut cmd,
                    &art,
                    caption,
                    area.left + 6. * scale,
                    center.y,
                    13. * scale,
                    ink,
                );
            }
            label(
                &mut cmd,
                &art,
                format!("{} / {}", page + 1, games.len().div_ceil(waiting.rows)),
                login.x - 24. * scale,
                waiting.pager_y,
                13. * scale,
                ink,
            );
        }
        if c.broken {
            broken_dialog(&mut cmd, &art, surface.layout);
        }
        return;
    };
    let layout = surface.layout;
    let scale = layout.scale;
    // The entire plan is the content: both printed legends, the grid and flags.
    let mut transform = Transform::from_xyz(0., -(plan::BAR_HEIGHT as f32) / 2., 0.);
    if v.you == 1 {
        transform.rotation = Quat::from_rotation_z(std::f32::consts::PI);
    }
    cmd.spawn((
        Painted,
        Sprite {
            image: art.board.clone(),
            custom_size: Some(Vec2::new(layout.board_width, layout.board_height)),
            ..default()
        },
        transform,
    ));
    let targets = c
        .selected
        .map(|from| v.legal_targets(from))
        .unwrap_or_default();
    for at in 0..CELLS {
        if blocked(at) {
            continue;
        }
        let area = plan::cell(at);
        let point = layout.world(area.center(), v.you);
        let w = area.width / plan::PAGE_WIDTH * layout.board_width;
        let h = area.height / plan::PAGE_HEIGHT * layout.board_height;
        if targets.contains(&at) {
            border(
                &mut cmd,
                point.x,
                point.y,
                w - 6. * scale,
                h - 6. * scale,
                2.,
                Color::srgb(0.2, 0.85, 0.5),
            );
        }
        if c.selected == Some(at) {
            border(
                &mut cmd,
                point.x,
                point.y,
                w - 4. * scale,
                h - 4. * scale,
                2.,
                gold,
            );
        }
        if c.motion
            .as_ref()
            .is_some_and(|capture| at == capture.event.from || at == capture.event.to)
        {
            continue;
        }
        if let Some(piece) = &v.board[at] {
            paint_piece(
                &mut cmd,
                &art,
                piece,
                PieceVisual {
                    point,
                    size: Vec2::new(w, h),
                    scale,
                    opacity: 1.,
                    layer: 0.,
                },
            );
        }
    }
    if let Some(capture) = &c.motion {
        let event = &capture.event;
        let (travel, opacity) = motion_sample(capture.elapsed_ms());
        let from = layout.world(plan::cell(event.from).center(), v.you);
        let to = layout.world(plan::cell(event.to).center(), v.you);
        let size = Vec2::new(
            plan::STEP / plan::PAGE_WIDTH * layout.board_width,
            plan::STEP / plan::PAGE_HEIGHT * layout.board_height,
        );
        if let Some(defender) = &event.defender {
            paint_piece(
                &mut cmd,
                &art,
                defender,
                PieceVisual {
                    point: to,
                    size,
                    scale,
                    opacity: if event.defender_lost { opacity } else { 1. },
                    layer: if travel >= 1. && event.defender_lost {
                        5.
                    } else {
                        0.
                    },
                },
            );
        }
        paint_piece(
            &mut cmd,
            &art,
            &event.piece,
            PieceVisual {
                point: plan::Point {
                    x: from.x + (to.x - from.x) * travel,
                    y: from.y + (to.y - from.y) * travel,
                },
                size,
                scale,
                opacity: if event.piece_lost { opacity } else { 1. },
                layer: if travel < 1. || event.piece_lost {
                    7.
                } else {
                    0.
                },
            },
        );
    }
    let remaining_counts = matches!(v.phase, Phase::Playing)
        || matches!(v.phase, Phase::Finished { .. }) && v.ready.iter().all(|ready| *ready);
    for (rank, count) in &v.reserve {
        let card = plan::card(*rank, v.you);
        let deploying = matches!(v.phase, Phase::Placement);
        if deploying && *count == 0 {
            printed_rect(
                &mut cmd,
                card,
                v.you,
                1.,
                Color::srgba(0.04, 0.03, 0.02, 0.4),
                false,
                layout,
            );
        }
        if deploying && *rank == c.rank && *count > 0 && !v.ready[v.you] {
            let center = layout.world(card.center(), v.you);
            let width = card.width / plan::PAGE_WIDTH * layout.board_width;
            let height = card.height / plan::PAGE_HEIGHT * layout.board_height;
            let stroke = width * 0.07;
            // Draw outside the card so the stronger outline cannot cover its
            // illustration, label or reserve count. Scale with the sheet.
            border_with_stroke(
                &mut cmd,
                center.x,
                center.y,
                width + 2. * stroke,
                height + 2. * stroke,
                2.,
                Color::srgb(0.02, 0.18, 0.95),
                stroke,
            );
        }
        if !remaining_counts {
            let entity = count_label(&mut cmd, &art, *rank, v.you, v.you, *count, layout);
            cmd.entity(entity).insert(ReserveCount);
        }
    }
    if remaining_counts {
        for owner in 0..2 {
            for rank in Rank::ALL {
                let entity = count_label(
                    &mut cmd,
                    &art,
                    rank,
                    owner,
                    v.you,
                    v.remaining(owner, rank),
                    layout,
                );
                cmd.entity(entity).insert(RemainingCount);
            }
        }
    }
    // This is the only UI outside the original plan.
    let top = layout.height / 2.;
    rect(
        &mut cmd,
        0.,
        top - plan::BAR_HEIGHT as f32 / 2.,
        layout.width,
        plan::BAR_HEIGHT as f32,
        5.,
        Color::srgb(0.07, 0.085, 0.085),
    );
    clock_panels(&mut cmd, &art, v, layout);
    if let Some(result) = flag_result(v) {
        let button = result_ok(layout);
        let center = button.center();
        rect(
            &mut cmd,
            center.x,
            center.y,
            button.width,
            button.height,
            6.,
            Color::srgb(0.04, 0.28, 0.78),
        );
        for (text, x, anchor) in [
            (result, -layout.width / 2. + 12., Anchor::CENTER_LEFT),
            ("OK", center.x, Anchor::CENTER),
        ] {
            cmd.spawn((
                Painted,
                Text2d::new(text),
                TextFont {
                    font: art.bold_font.clone(),
                    font_size: 22.,
                    ..default()
                },
                TextColor(Color::WHITE),
                anchor,
                Transform::from_xyz(x, center.y, 8.),
            ));
        }
        return;
    }
    let status = if c.tx.is_none() && !c.preview {
        "Disconnected · Click here to return".into()
    } else {
        match &v.phase {
            Phase::Waiting => format!("Waiting for opponent · {}", shortened(&v.game, 25)),
            Phase::Placement => {
                if v.ready[v.you] || c.ready_sent {
                    "Army ready · Waiting for opponent".into()
                } else {
                    format!("Deploying · {}", shortened(&v.game, 25))
                }
            }
            Phase::Playing => format!(
                "{} · {}",
                if v.turn == v.you {
                    "Your move"
                } else {
                    "Opponent's move"
                },
                shortened(&v.game, 25)
            ),
            Phase::Finished { winner, .. } => format!(
                "{} · Click here to return",
                match winner {
                    Some(p) if *p == v.you => "Victory",
                    Some(_) => "Defeat",
                    None => "Draw",
                }
            ),
        }
    };
    let detail = if !c.error.is_empty() {
        c.error.clone()
    } else {
        match &v.phase {
            Phase::Waiting => "Your game starts when the second player joins.".into(),
            Phase::Placement => {
                if v.ready[v.you] || c.ready_sent {
                    "Both armies begin as soon as deployment is complete.".into()
                } else {
                    let count = v
                        .reserve
                        .iter()
                        .find(|(r, _)| *r == c.rank)
                        .map_or(0, |(_, n)| *n);
                    format!(
                        "{}: {count} left · Bottom four rows · Right-click removes",
                        c.rank.name()
                    )
                }
            }
            Phase::Playing => v
                .messages
                .last()
                .cloned()
                .unwrap_or_else(|| "Select your piece, then a highlighted square.".into()),
            Phase::Finished { reason, .. } => reason.clone(),
        }
    };
    let reserved = if v.can_claim_victory() {
        160.
    } else if matches!(v.phase, Phase::Placement) {
        136.
    } else {
        12.
    };
    label(
        &mut cmd,
        &art,
        shortened(
            &status,
            ((layout.width - reserved - 24.) / 7.).max(8.) as usize,
        ),
        -layout.width / 2. + 12.,
        top - 58.,
        12.,
        ink,
    );
    label(
        &mut cmd,
        &art,
        shortened(
            &detail,
            ((layout.width - reserved - 24.) / 6.).max(8.) as usize,
        ),
        -layout.width / 2. + 12.,
        top - 75.,
        11.,
        if c.error.is_empty() {
            Color::srgb(0.76, 0.78, 0.72)
        } else {
            gold
        },
    );
    if matches!(v.phase, Phase::Placement) {
        cmd.spawn((
            Painted,
            Clock::Setup,
            Text2d::new(""),
            TextFont {
                font: art.font.clone(),
                font_size: 13.,
                ..default()
            },
            TextColor(gold),
            Anchor::CENTER_RIGHT,
            Transform::from_xyz(layout.width / 2. - 12., top - 58., 8.),
        ));
    }
    if v.can_claim_victory() {
        let button = claim_button(layout);
        let center = button.center();
        rect(
            &mut cmd,
            center.x,
            center.y,
            button.width,
            button.height,
            6.,
            Color::srgb(0.12, 0.34, 0.63),
        );
        cmd.spawn((
            Painted,
            Text2d::new("Claim victory"),
            TextFont {
                font: art.bold_font.clone(),
                font_size: 14.,
                ..default()
            },
            TextColor(Color::WHITE),
            Anchor::CENTER,
            Transform::from_xyz(center.x, center.y, 8.),
        ));
    }
    if c.broken {
        broken_dialog(&mut cmd, &art, layout);
    }
}
fn flag_result(view: &View) -> Option<&'static str> {
    match &view.phase {
        Phase::Finished {
            winner: Some(winner),
            reason,
        } if reason == "Enemy flag captured" || reason == "Victory claimed on time" => {
            Some(if *winner == view.you {
                "You win!"
            } else {
                "You lose!"
            })
        }
        _ => None,
    }
}
fn result_ok(layout: plan::Layout) -> plan::Rect {
    plan::Rect {
        left: layout.width / 2. - 76.,
        top: layout.height / 2. - 80.,
        width: 64.,
        height: 32.,
    }
}
fn broken_ok(layout: plan::Layout) -> plan::Rect {
    let width = 100_f32.min(layout.width - 32.);
    // World coordinates, shared by rendering and mouse picking.
    plan::Rect {
        left: -width / 2.,
        top: -48.,
        width,
        height: 36.,
    }
}
fn broken_dialog(cmd: &mut Commands, art: &Art, layout: plan::Layout) {
    rect(
        cmd,
        0.,
        0.,
        layout.width,
        layout.height,
        20.,
        Color::srgba(0., 0., 0., 0.75),
    );
    rect(
        cmd,
        0.,
        0.,
        300_f32.min(layout.width - 24.),
        144.,
        21.,
        Color::srgb(0.10, 0.13, 0.16),
    );
    let button = broken_ok(layout);
    let center = button.center();
    rect(
        cmd,
        center.x,
        center.y,
        button.width,
        button.height,
        22.,
        Color::srgb(0.04, 0.28, 0.78),
    );
    for (text, y, size) in [("Broken pike", 28., 26.), ("OK", center.y, 20.)] {
        cmd.spawn((
            Painted,
            Text2d::new(text),
            TextFont {
                font: art.font.clone(),
                font_size: size,
                ..default()
            },
            TextColor(Color::WHITE),
            Anchor::CENTER,
            Transform::from_xyz(0., y, 23.),
        ));
    }
}
fn format_main_time(ms: u64) -> String {
    let tenths = ms.div_ceil(100);
    format!("{}:{:02}.{}", tenths / 600, (tenths / 10) % 60, tenths % 10)
}
fn player_clock_text(clocks: ClockView, owner: usize, active: bool) -> String {
    let main = format_main_time(clocks.main_ms[owner]);
    if active {
        let tenths = clocks.delay_ms.div_ceil(100);
        format!("{main} ({}.{:01}s)", tenths / 10, tenths % 10)
    } else {
        main
    }
}
fn clock_text(c: Res<Client>, mut texts: Query<(&Clock, &mut Text2d)>) {
    if let Some(v) = &c.view {
        let elapsed = if c.broken {
            0
        } else {
            c.received.elapsed().as_millis() as u64
        };
        let clocks = v.clock_after(elapsed);
        for (role, mut text) in &mut texts {
            text.0 = match role {
                Clock::Yours => player_clock_text(
                    clocks,
                    v.you,
                    matches!(v.phase, Phase::Playing) && v.turn == v.you,
                ),
                Clock::Opponent => player_clock_text(
                    clocks,
                    1 - v.you,
                    matches!(v.phase, Phase::Playing) && v.turn != v.you,
                ),
                Clock::Setup => format!(
                    "Setup {}",
                    format_main_time(v.remaining_ms.saturating_sub(elapsed))
                ),
            };
        }
    }
}
fn screenshot(
    mut cmd: Commands,
    mut frame: Local<u32>,
    mut exit: MessageWriter<AppExit>,
    offscreen: Option<Res<Offscreen>>,
) {
    let args: Vec<_> = std::env::args().collect();
    let Some(pair) = args.windows(2).find(|a| a[0] == "--screenshot") else {
        return;
    };
    *frame += 1;
    if *frame == 90 {
        cmd.spawn(if let Some(target) = offscreen {
            bevy::render::view::screenshot::Screenshot::image(target.0.clone())
        } else {
            bevy::render::view::screenshot::Screenshot::primary_window()
        })
        .observe(bevy::render::view::screenshot::save_to_disk(
            pair[1].clone(),
        ));
    }
    if *frame == 150 {
        exit.write(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Client, SyncSender<(u64, Event)>, Receiver<Action>) {
        let (events_tx, events_rx) = mpsc::sync_channel(16);
        let (actions_tx, actions_rx) = mpsc::sync_channel(16);
        let client = Client {
            address: String::new(),
            room: "test".into(),
            field: 1,
            connecting: false,
            tx: Some(actions_tx),
            socket: None,
            events: Mutex::new(events_rx),
            sender: events_tx.clone(),
            view: None,
            received: Instant::now(),
            error: String::new(),
            selected: None,
            rank: Rank::Flag,
            dirty: true,
            ready_sent: false,
            preview: false,
            session: 0,
            broken: false,
            lobby: Lobby::default(),
            join_waiting: false,
            auto_join: false,
            lobby_offset: Vec2::ZERO,
            lobby_drag: None,
            motion: None,
        };
        (client, events_tx, actions_rx)
    }
    #[cfg(feature = "network-tests")]
    #[test]
    fn client_network_worker_authenticates_browses_and_joins_with_embedded_key() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let (mut outgoing, mut incoming) = net::server_handshake(&mut stream).unwrap();
            outgoing
                .send(
                    &mut stream,
                    &Reply::Games {
                        games: vec!["Friends".into()],
                    },
                )
                .unwrap();
            assert_eq!(
                incoming.receive::<Action>(&mut stream, 4096).unwrap(),
                Action::JoinWaiting {
                    game: "Friends".into()
                }
            );
            let view = Game::new(0, &Config::default()).view(0, "Friends", 0);
            outgoing.send(&mut stream, &Reply::State { view }).unwrap();
            // Keep the connection alive until the test client leaves normally.
            assert!(incoming.receive::<Action>(&mut stream, 4096).is_err());
        });
        let (mut client, _, _) = fixture();
        client.tx = None;
        client.address = address.clone();
        client.lobby.address = address;
        client.start_connection();
        let mut app = App::new();
        app.insert_resource(client).add_systems(Update, receive);
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.world().resource::<Client>().lobby.games.is_empty() {
            assert!(
                Instant::now() < deadline,
                "Authenticated lobby request timed out"
            );
            app.update();
            thread::sleep(Duration::from_millis(10));
        }
        {
            let mut client = app.world_mut().resource_mut::<Client>();
            client.room = client.lobby.games[0].clone();
            client.connect_game(true);
        }
        while app.world().resource::<Client>().view.is_none() {
            assert!(Instant::now() < deadline, "Authenticated join timed out");
            app.update();
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            app.world().resource::<Client>().view.as_ref().unwrap().game,
            "Friends"
        );
        app.world_mut().resource_mut::<Client>().leave();
        server.join().unwrap();
    }
    #[test]
    fn ordinary_and_scout_moves_slide_without_fading_on_both_boards() {
        for (rank, to) in [(Rank::Cadet, 45), (Rank::Scout, 74)] {
            for mover in 0..2 {
                for you in 0..2 {
                    let cfg = Config::default();
                    let mut game = Game::new(0, &cfg);
                    game.phase = Phase::Playing;
                    game.deadline = 5000;
                    game.turn = mover;
                    game.board[44] = Some(Piece { owner: mover, rank });
                    game.board[90] = Some(Piece {
                        owner: 0,
                        rank: Rank::Miner,
                    });
                    game.board[9] = Some(Piece {
                        owner: 1,
                        rank: Rank::Miner,
                    });
                    let (mut client, events, _) = fixture();
                    client.view = Some(game.view(you, "motion", 0));
                    let mut app = App::new();
                    app.insert_resource(client)
                        .init_resource::<Surface>()
                        .insert_resource(Art {
                            font: default(),
                            bold_font: default(),
                            ranks: vec![Handle::default(); 11],
                            board: default(),
                            cover: default(),
                        })
                        .add_systems(Update, (receive, animate_motion, paint).chain());
                    app.update();
                    game.apply(mover, Action::Move { from: 44, to }, 1, &cfg)
                        .unwrap();
                    let view = game.view(you, "motion", 1);
                    events
                        .send((0, Event::Reply(Reply::State { view: view.clone() })))
                        .unwrap();
                    app.update();
                    for ((w, h), age) in [(360., 480.), (1200., 900.), (400., 1600.)]
                        .into_iter()
                        .zip([0_u64, 90, 150])
                    {
                        let layout = plan::Layout::new(w, h);
                        app.world_mut().resource_mut::<Surface>().layout = layout;
                        let received = Instant::now();
                        {
                            let mut client = app.world_mut().resource_mut::<Client>();
                            let motion = client.motion.as_mut().unwrap();
                            assert_eq!(motion_duration_ms(&motion.event), 180.);
                            motion.event.elapsed_ms = age;
                            motion.received = received;
                        }
                        events
                            .send((0, Event::Reply(Reply::State { view: view.clone() })))
                            .unwrap();
                        app.update();
                        assert_eq!(
                            app.world()
                                .resource::<Client>()
                                .motion
                                .as_ref()
                                .unwrap()
                                .received,
                            received
                        );
                        let travel = motion_sample(age as f32).0;
                        let from = layout.world(plan::cell(44).center(), you);
                        let target = layout.world(plan::cell(to).center(), you);
                        let expected = Vec2::new(
                            from.x + (target.x - from.x) * travel,
                            from.y + (target.y - from.y) * travel,
                        );
                        let mut sprites = app.world_mut().query::<(&Sprite, &Transform)>();
                        let (sprite, transform) = sprites
                            .iter(app.world())
                            .find(|(_, t)| t.translation.z == 10.)
                            .unwrap();
                        assert!(transform.translation.truncate().distance(expected) < 3.);
                        assert_eq!(sprite.color.alpha(), 1.);
                    }
                    app.world_mut()
                        .resource_mut::<Client>()
                        .motion
                        .as_mut()
                        .unwrap()
                        .event
                        .elapsed_ms = 181;
                    app.update();
                    assert!(app.world().resource::<Client>().motion.is_none());
                    events
                        .send((0, Event::Reply(Reply::State { view })))
                        .unwrap();
                    app.update();
                    assert!(app.world().resource::<Client>().motion.is_none());
                    let layout = app.world().resource::<Surface>().layout;
                    let target = layout.world(plan::cell(to).center(), you);
                    let mut sprites = app.world_mut().query::<(&Sprite, &Transform)>();
                    assert!(sprites.iter(app.world()).any(|(sprite, transform)| {
                        transform.translation.z == 3.
                            && (transform.translation.x - target.x).abs() < 0.001
                            && (transform.translation.y - target.y).abs() < 0.001
                            && sprite.color.alpha() == 1.
                    }));
                }
            }
        }
    }
    #[test]
    fn captures_move_then_fade_the_losers_on_both_rotated_boards_without_restarting() {
        for (attacker, defender) in [
            (Rank::General, Rank::Cadet),
            (Rank::Cadet, Rank::General),
            (Rank::General, Rank::General),
            (Rank::Spy, Rank::Marshal),
            (Rank::Miner, Rank::Mine),
            (Rank::Cadet, Rank::Flag),
        ] {
            for you in 0..2 {
                let cfg = Config::default();
                let mut game = Game::new(0, &cfg);
                game.phase = Phase::Playing;
                game.deadline = 5000;
                game.board[44] = Some(Piece {
                    owner: 0,
                    rank: attacker,
                });
                game.board[45] = Some(Piece {
                    owner: 1,
                    rank: defender,
                });
                game.board[90] = Some(Piece {
                    owner: 0,
                    rank: Rank::Miner,
                });
                game.board[9] = Some(Piece {
                    owner: 1,
                    rank: Rank::Miner,
                });
                let (mut client, events, _) = fixture();
                client.view = Some(game.view(you, "capture", 0));
                let mut app = App::new();
                app.insert_resource(client)
                    .init_resource::<Surface>()
                    .insert_resource(Art {
                        font: default(),
                        bold_font: default(),
                        ranks: vec![Handle::default(); 11],
                        board: default(),
                        cover: default(),
                    })
                    .add_systems(Update, (receive, animate_motion, paint).chain());
                app.update();
                game.apply(0, Action::Move { from: 44, to: 45 }, 1, &cfg)
                    .unwrap();
                let view = game.view(you, "capture", 1);
                events
                    .send((0, Event::Reply(Reply::State { view: view.clone() })))
                    .unwrap();
                app.update();
                assert!(app.world().resource::<Client>().motion.is_some());
                for ((w, h), age) in [(600., 881.), (1200., 900.), (400., 1600.), (360., 480.)]
                    .into_iter()
                    .zip([0_u64, 90, 180, 290])
                {
                    let layout = plan::Layout::new(w, h);
                    app.world_mut().resource_mut::<Surface>().layout = layout;
                    let received = Instant::now();
                    {
                        let mut client = app.world_mut().resource_mut::<Client>();
                        let capture = client.motion.as_mut().unwrap();
                        capture.event.elapsed_ms = age;
                        capture.received = received;
                    }
                    // Repeated state broadcasts must not reset an ongoing animation.
                    events
                        .send((0, Event::Reply(Reply::State { view: view.clone() })))
                        .unwrap();
                    app.update();
                    assert_eq!(
                        app.world()
                            .resource::<Client>()
                            .motion
                            .as_ref()
                            .unwrap()
                            .received,
                        received
                    );
                    let (travel, opacity) = motion_sample(age as f32);
                    let from = layout.world(plan::cell(44).center(), you);
                    let to = layout.world(plan::cell(45).center(), you);
                    let attack_point = plan::Point {
                        x: from.x + (to.x - from.x) * travel,
                        y: from.y + (to.y - from.y) * travel,
                    };
                    let result = combat(attacker, defender);
                    let attacker_layer = if travel < 1. || result <= 0 { 7. } else { 0. };
                    let defender_layer = if travel >= 1. && result >= 0 { 5. } else { 0. };
                    let mut sprites = app.world_mut().query::<(&Sprite, &Transform)>();
                    for (point, layer, alpha) in [
                        (
                            attack_point,
                            attacker_layer,
                            if result <= 0 { opacity } else { 1. },
                        ),
                        (to, defender_layer, if result >= 0 { opacity } else { 1. }),
                    ] {
                        let (sprite, _) = sprites
                            .iter(app.world())
                            .find(|(_, t)| {
                                (t.translation.z - (3. + layer)).abs() < 0.001
                                    && (t.translation.x - point.x).abs() < 2.
                                    && (t.translation.y - point.y).abs() < 2.
                            })
                            .unwrap();
                        assert!((sprite.color.alpha() - alpha).abs() < 0.03);
                    }
                }
                {
                    let mut client = app.world_mut().resource_mut::<Client>();
                    client.motion.as_mut().unwrap().event.elapsed_ms = 401;
                }
                app.update();
                assert!(app.world().resource::<Client>().motion.is_none());
                events
                    .send((0, Event::Reply(Reply::State { view })))
                    .unwrap();
                app.update();
                assert!(app.world().resource::<Client>().motion.is_none());
                let mut sprites = app.world_mut().query_filtered::<&Transform, With<Sprite>>();
                assert!(!sprites.iter(app.world()).any(|t| t.translation.z > 9.));
            }
        }
    }
    #[test]
    fn waiting_games_can_be_joined_from_every_page_at_different_sizes_and_dpi() {
        for (w, h) in [(360., 480.), (600., 881.), (1280., 900.), (400., 1600.)] {
            for dpi in [1., 2.] {
                let (mut client, _, actions) = fixture();
                client.lobby.games = (0..19).map(|i| format!("Game {i:02} — Praha")).collect();
                client.lobby.loaded = true;
                let layout = plan::Layout::new(w, h);
                let waiting = WaitingLayout::new(layout);
                let mut app = App::new();
                app.insert_resource(client)
                    .insert_resource(Surface {
                        layout,
                        playing: Some(false),
                    })
                    .init_resource::<ButtonInput<KeyCode>>()
                    .init_resource::<ButtonInput<MouseButton>>()
                    .add_message::<KeyboardInput>()
                    .insert_resource(Art {
                        font: default(),
                        bold_font: default(),
                        ranks: vec![Handle::default(); 11],
                        board: default(),
                        cover: default(),
                    })
                    .add_systems(Update, (input, paint).chain());
                let mut window = Window::default();
                window.resolution.set_scale_factor_override(Some(dpi));
                window.resolution.set(w, h);
                let window = app.world_mut().spawn(window).id();
                app.update();
                for page in 0..19_usize.div_ceil(waiting.rows) {
                    assert_eq!(waiting.page(&app.world().resource::<Client>().lobby), page);
                    for i in 0..waiting.rows.min(19 - page * waiting.rows) {
                        let area = waiting.row(i);
                        assert!(area.left >= -w / 2. && area.left + area.width <= w / 2.);
                        assert!(area.top > waiting.login.server_y + 18. * waiting.login.scale);
                        assert!(area.top + area.height < h / 2.);
                        let expected = format!("Game {:02} — Praha", page * waiting.rows + i);
                        let mut labels = app.world_mut().query::<&Text2d>();
                        assert!(labels.iter(app.world()).any(|text| text.0 == expected));
                        let point = area.center();
                        app.world_mut()
                            .get_mut::<Window>(window)
                            .unwrap()
                            .set_cursor_position(Some(Vec2::new(
                                point.x + w / 2.,
                                h / 2. - point.y,
                            )));
                        app.insert_resource(ButtonInput::<MouseButton>::default());
                        app.world_mut()
                            .resource_mut::<ButtonInput<MouseButton>>()
                            .press(MouseButton::Left);
                        app.update();
                        assert_eq!(
                            actions.try_recv().unwrap(),
                            Action::JoinWaiting {
                                game: expected.clone()
                            }
                        );
                        assert_eq!(app.world().resource::<Client>().room, expected);
                        assert!(app.world().resource::<Client>().view.is_none());
                    }
                    if (page + 1) * waiting.rows < 19 {
                        let point = waiting.pager(true).center();
                        app.world_mut()
                            .get_mut::<Window>(window)
                            .unwrap()
                            .set_cursor_position(Some(Vec2::new(
                                point.x + w / 2.,
                                h / 2. - point.y,
                            )));
                        app.insert_resource(ButtonInput::<MouseButton>::default());
                        app.world_mut()
                            .resource_mut::<ButtonInput<MouseButton>>()
                            .press(MouseButton::Left);
                        app.update();
                        assert!(actions.try_recv().is_err());
                    }
                }
            }
        }
    }
    #[test]
    fn caret_uses_fractional_text_advance_without_glyph_padding_or_dpi_drift() {
        for dpi in [0.75, 1., 2.] {
            let mut app = App::new();
            app.add_systems(Update, position_input_caret);
            let parent = app
                .world_mut()
                .spawn(bevy::text::TextLayoutInfo {
                    scale_factor: dpi,
                    // The rounded bounding box must not determine the caret position.
                    size: Vec2::new(33., 20.),
                    run_geometry: vec![bevy::text::RunGeometry {
                        bounds: Rect::new(0., 0., 32.375 * dpi, 20. * dpi),
                        ..default()
                    }],
                    ..default()
                })
                .id();
            app.world_mut()
                .entity_mut(parent)
                .insert(GlobalTransform::default());
            let caret = app
                .world_mut()
                .spawn((
                    InputCaret(Instant::now()),
                    ChildOf(parent),
                    Transform::default(),
                    Sprite::from_color(Color::WHITE, Vec2::ONE),
                ))
                .id();
            for advance in [32.375, 38.125, 0.] {
                app.world_mut()
                    .get_mut::<bevy::text::TextLayoutInfo>(parent)
                    .unwrap()
                    .run_geometry[0]
                    .bounds
                    .max
                    .x = advance * dpi;
                app.update();
                let position = app.world().get::<Transform>(caret).unwrap();
                assert!((position.translation.x - advance).abs() < 0.001);
            }
        }
    }
    #[test]
    fn lobby_header_drags_clamps_and_keeps_join_targets_aligned_after_resize() {
        for (w, h) in [(360., 480.), (600., 925.), (1280., 900.)] {
            for dpi in [1., 2.] {
                let (mut client, _, actions) = fixture();
                client.lobby.games = vec!["Drag match".into()];
                let layout = plan::Layout::new(w, h);
                let mut app = App::new();
                app.insert_resource(client)
                    .insert_resource(Surface {
                        layout,
                        playing: Some(false),
                    })
                    .init_resource::<ButtonInput<KeyCode>>()
                    .init_resource::<ButtonInput<MouseButton>>()
                    .add_message::<KeyboardInput>()
                    .insert_resource(Art {
                        font: default(),
                        bold_font: default(),
                        ranks: vec![Handle::default(); 11],
                        board: default(),
                        cover: default(),
                    })
                    .add_systems(Update, (input, paint).chain());
                let mut window = Window::default();
                window.resolution.set_scale_factor_override(Some(dpi));
                window.resolution.set(w, h);
                let window = app.world_mut().spawn(window).id();
                let (panel, _) = WaitingLayout::positioned(layout, Vec2::ZERO);
                let header = panel.title_bar().center();
                app.world_mut()
                    .get_mut::<Window>(window)
                    .unwrap()
                    .set_cursor_position(Some(Vec2::new(header.x + w / 2., h / 2. - header.y)));
                app.world_mut()
                    .resource_mut::<ButtonInput<MouseButton>>()
                    .press(MouseButton::Left);
                app.update();
                assert!(app.world().resource::<Client>().lobby_drag.is_some());
                // Keep the button held without another press; move beyond every edge.
                app.world_mut()
                    .resource_mut::<ButtonInput<MouseButton>>()
                    .clear();
                for point in [Vec2::new(-1000., -1000.), Vec2::new(w + 1000., h + 1000.)] {
                    app.world_mut()
                        .get_mut::<Window>(window)
                        .unwrap()
                        .set_cursor_position(Some(point));
                    app.update();
                    let client = app.world().resource::<Client>();
                    let (panel, _) = WaitingLayout::positioned(layout, client.lobby_offset);
                    let bounds = panel.bounds();
                    assert!(bounds.left >= -w / 2. - 0.01);
                    assert!(bounds.top >= -h / 2. - 0.01);
                    assert!(bounds.left + bounds.width <= w / 2. + 0.01);
                    assert!(bounds.top + bounds.height <= h / 2. + 0.01);
                    assert!(actions.try_recv().is_err());
                }
                app.insert_resource(ButtonInput::<MouseButton>::default());
                app.update();
                assert!(app.world().resource::<Client>().lobby_drag.is_none());
                // A smaller window clamps the saved position for both paint and picking.
                let layout = plan::Layout::new(360., 480.);
                app.world_mut().resource_mut::<Surface>().layout = layout;
                app.world_mut().resource_mut::<Client>().dirty = true;
                app.world_mut()
                    .get_mut::<Window>(window)
                    .unwrap()
                    .resolution
                    .set(360., 480.);
                let (panel, _) = WaitingLayout::positioned(
                    layout,
                    app.world().resource::<Client>().lobby_offset,
                );
                let point = panel.row(0).center();
                app.world_mut()
                    .get_mut::<Window>(window)
                    .unwrap()
                    .set_cursor_position(Some(Vec2::new(point.x + 180., 240. - point.y)));
                app.world_mut()
                    .resource_mut::<ButtonInput<MouseButton>>()
                    .press(MouseButton::Left);
                app.update();
                assert_eq!(
                    actions.try_recv().unwrap(),
                    Action::JoinWaiting {
                        game: "Drag match".into()
                    }
                );
                let mut labels = app.world_mut().query::<(&Text2d, &Transform)>();
                let (_, position) = labels
                    .iter(app.world())
                    .find(|(text, _)| text.0 == "Waiting games")
                    .unwrap();
                assert_eq!(position.translation.y, panel.header_y);
                assert_eq!(
                    position.translation.x,
                    panel.login.x - panel.login.width / 2.
                );
            }
        }
    }
    #[test]
    fn connection_controls_lock_address_gate_enter_and_cancel_at_all_sizes() {
        for (w, h) in [(360., 480.), (600., 925.), (1280., 900.)] {
            for state in 0..3 {
                let (mut client, events, actions) = fixture();
                client.address = "localhost:4000".into();
                client.field = 0;
                client.connecting = state == 1;
                if state != 2 {
                    client.tx = None;
                }
                let layout = plan::Layout::new(w, h);
                let login = login_layout(layout);
                let mut app = App::new();
                app.insert_resource(client)
                    .insert_resource(Surface {
                        layout,
                        playing: Some(false),
                    })
                    .init_resource::<ButtonInput<KeyCode>>()
                    .init_resource::<ButtonInput<MouseButton>>()
                    .add_message::<KeyboardInput>()
                    .insert_resource(Art {
                        font: default(),
                        bold_font: default(),
                        ranks: vec![Handle::default(); 11],
                        board: default(),
                        cover: default(),
                    })
                    .add_systems(Update, (receive, input, paint).chain());
                let mut window = Window::default();
                window.resolution.set(w, h);
                let window = app.world_mut().spawn(window).id();
                app.update();
                let mut labels = app.world_mut().query::<&Text2d>();
                let expected = ["Connect", "Connecting…", "Connected"][state];
                assert!(labels.iter(app.world()).any(|t| t.0 == expected));
                let mut carets = app.world_mut().query_filtered::<Entity, With<InputCaret>>();
                assert_eq!(carets.iter(app.world()).count(), usize::from(state == 0));
                for key in [Key::Character("x".into()), Key::Backspace, Key::Enter] {
                    app.world_mut().write_message(KeyboardInput {
                        key_code: KeyCode::KeyX,
                        logical_key: key,
                        state: bevy::input::ButtonState::Pressed,
                        text: None,
                        repeat: false,
                        window,
                    });
                    app.update();
                }
                assert_eq!(app.world().resource::<Client>().address, "localhost:4000");
                if state == 2 {
                    assert!(matches!(actions.try_recv().unwrap(), Action::Join { .. }));
                } else {
                    assert!(actions.try_recv().is_err());
                }
                if state != 0 {
                    let point = login.connection_button().center();
                    app.world_mut()
                        .get_mut::<Window>(window)
                        .unwrap()
                        .set_cursor_position(Some(Vec2::new(point.x + w / 2., h / 2. - point.y)));
                    app.world_mut()
                        .resource_mut::<ButtonInput<MouseButton>>()
                        .press(MouseButton::Left);
                    app.update();
                    app.insert_resource(ButtonInput::<MouseButton>::default());
                    events.send((0, Event::Failed("late".into()))).unwrap();
                    app.update();
                    let client = app.world().resource::<Client>();
                    assert!(!client.connecting && client.tx.is_none());
                    assert!(client.error.is_empty());
                    assert!(client.lobby.games.is_empty());
                }
            }
        }
    }
    #[test]
    fn pushed_lobby_updates_and_canceled_results_are_session_bound() {
        let (client, events, _) = fixture();
        let mut app = App::new();
        app.insert_resource(client).add_systems(Update, receive);
        for games in [vec!["Available".to_string()], vec![]] {
            events
                .send((
                    0,
                    Event::Reply(Reply::Games {
                        games: games.clone(),
                    }),
                ))
                .unwrap();
            app.update();
            assert_eq!(app.world().resource::<Client>().lobby.games, games);
        }
        app.world_mut().resource_mut::<Client>().toggle_connection();
        events
            .send((
                0,
                Event::Reply(Reply::Games {
                    games: vec!["stale".into()],
                }),
            ))
            .unwrap();
        events
            .send((0, Event::Failed("late failure".into())))
            .unwrap();
        app.update();
        let client = app.world().resource::<Client>();
        assert!(client.tx.is_none());
        assert!(client.lobby.games.is_empty());
        assert!(client.error.is_empty());
    }
    #[test]
    fn flag_capture_reveals_board_and_shows_bold_result_until_ok() {
        let mut fonts = Assets::<Font>::default();
        let bold = fonts.add(
            Font::try_from_bytes(include_bytes!("../../assets/font-bold.ttf").to_vec()).unwrap(),
        );
        for you in 0..2 {
            for winner in 0..2 {
                for (w, h) in [(360., 480.), (600., 881.), (1200., 900.), (400., 1600.)] {
                    for keyboard_ok in [false, true] {
                        let cfg = Config::default();
                        let mut game = Game::new(0, &cfg);
                        game.phase = Phase::Playing;
                        game.turn = winner;
                        game.deadline = 5000;
                        game.board[44] = Some(Piece {
                            owner: winner,
                            rank: Rank::Miner,
                        });
                        game.board[45] = Some(Piece {
                            owner: 1 - winner,
                            rank: Rank::Flag,
                        });
                        game.board[90] = Some(Piece {
                            owner: 1 - winner,
                            rank: Rank::Spy,
                        });
                        let (mut client, events, actions) = fixture();
                        client.view = Some(game.view(you, "test", 0));
                        let layout = plan::Layout::new(w, h);
                        let mut app = App::new();
                        app.insert_resource(client)
                            .insert_resource(Surface {
                                layout,
                                playing: Some(true),
                            })
                            .init_resource::<ButtonInput<KeyCode>>()
                            .init_resource::<ButtonInput<MouseButton>>()
                            .add_message::<KeyboardInput>()
                            .insert_resource(Art {
                                font: default(),
                                bold_font: bold.clone(),
                                ranks: vec![Handle::default(); 11],
                                board: default(),
                                cover: default(),
                            })
                            .add_systems(Update, (receive, input, paint).chain());
                        let window = app
                            .world_mut()
                            .spawn(Window {
                                resolution: (w as u32, h as u32).into(),
                                ..default()
                            })
                            .id();
                        app.update();
                        game.apply(winner, Action::Move { from: 44, to: 45 }, 1, &cfg)
                            .unwrap();
                        events
                            .send((
                                0,
                                Event::Reply(Reply::State {
                                    view: game.view(you, "test", 1),
                                }),
                            ))
                            .unwrap();
                        app.update();
                        let expected = if you == winner {
                            "You win!"
                        } else {
                            "You lose!"
                        };
                        let mut texts = app.world_mut().query::<(&Text2d, &TextFont, &Transform)>();
                        for text in [expected, "OK"] {
                            let (_, font, transform) = texts
                                .iter(app.world())
                                .find(|(t, _, _)| t.0 == text)
                                .unwrap();
                            assert_eq!(font.font, bold);
                            assert!(layout.in_bar(plan::Point {
                                x: transform.translation.x,
                                y: transform.translation.y
                            }));
                        }
                        assert!(!texts.iter(app.world()).any(|(t, _, _)| t.0 == "?"));
                        assert!(
                            app.world()
                                .resource::<Client>()
                                .view
                                .as_ref()
                                .unwrap()
                                .board
                                .iter()
                                .flatten()
                                .all(|piece| piece.rank.is_some())
                        );
                        // Clicking the result text leaves the revealed board available.
                        app.world_mut()
                            .get_mut::<Window>(window)
                            .unwrap()
                            .set_cursor_position(Some(Vec2::new(20., 20.)));
                        app.world_mut()
                            .resource_mut::<ButtonInput<MouseButton>>()
                            .press(MouseButton::Left);
                        app.update();
                        assert!(app.world().resource::<Client>().view.is_some());
                        assert!(actions.try_recv().is_err());
                        app.insert_resource(ButtonInput::<MouseButton>::default());
                        if keyboard_ok {
                            app.world_mut()
                                .resource_mut::<ButtonInput<KeyCode>>()
                                .press(KeyCode::Enter);
                        } else {
                            let p = result_ok(layout).center();
                            app.world_mut()
                                .get_mut::<Window>(window)
                                .unwrap()
                                .set_cursor_position(Some(Vec2::new(p.x + w / 2., h / 2. - p.y)));
                            app.world_mut()
                                .resource_mut::<ButtonInput<MouseButton>>()
                                .press(MouseButton::Left);
                        }
                        app.update();
                        assert!(app.world().resource::<Client>().view.is_none());
                    }
                }
            }
        }
    }

    #[test]
    fn disconnect_dialog_blocks_board_input_and_ok_returns_to_main_screen() {
        for phase in [Phase::Waiting, Phase::Placement, Phase::Playing] {
            for transport_failed in [false, true] {
                for keyboard_ok in [false, true] {
                    let (mut client, events, actions) = fixture();
                    let mut game = Game::new(0, &Config::default());
                    game.phase = phase.clone();
                    client.view = Some(game.view(0, "test", 0));
                    let mut app = App::new();
                    app.insert_resource(client)
                        .init_resource::<Surface>()
                        .init_resource::<Assets<Image>>()
                        .init_resource::<ButtonInput<KeyCode>>()
                        .init_resource::<ButtonInput<MouseButton>>()
                        .add_message::<KeyboardInput>()
                        .insert_resource(Art {
                            font: default(),
                            bold_font: default(),
                            ranks: vec![Handle::default(); 11],
                            board: default(),
                            cover: default(),
                        })
                        .add_systems(Update, (receive, fit_surface, input, paint).chain());
                    let window = app.world_mut().spawn(Window::default()).id();
                    app.update();
                    let event = if transport_failed {
                        Event::Failed("connection closed".into())
                    } else {
                        game.phase = Phase::Finished {
                            winner: None,
                            reason: "Opponent disconnected".into(),
                        };
                        Event::Reply(Reply::State {
                            view: game.view(0, "test", 0),
                        })
                    };
                    events.send((0, event)).unwrap();
                    // A queued old state must not undo the dialog.
                    events
                        .send((
                            0,
                            Event::Reply(Reply::State {
                                view: game.view(0, "test", 0),
                            }),
                        ))
                        .unwrap();
                    app.update();
                    assert!(app.world().resource::<Client>().broken);
                    assert!(app.world().resource::<Client>().view.is_some());
                    let mut labels = app.world_mut().query::<&Text2d>();
                    assert!(labels.iter(app.world()).any(|text| text.0 == "Broken pike"));
                    assert!(labels.iter(app.world()).any(|text| text.0 == "OK"));
                    let layout = app.world().resource::<Surface>().layout;
                    let board_point = layout.world(plan::card(Rank::Miner, 0).center(), 0);
                    app.world_mut()
                        .get_mut::<Window>(window)
                        .unwrap()
                        .set_cursor_position(Some(Vec2::new(
                            board_point.x + layout.width / 2.,
                            layout.height / 2. - board_point.y,
                        )));
                    app.world_mut()
                        .resource_mut::<ButtonInput<MouseButton>>()
                        .press(MouseButton::Left);
                    app.update();
                    assert!(app.world().resource::<Client>().broken);
                    assert_eq!(app.world().resource::<Client>().rank, Rank::Flag);
                    assert!(actions.try_recv().is_err());
                    app.insert_resource(ButtonInput::<MouseButton>::default());
                    if keyboard_ok {
                        app.world_mut()
                            .resource_mut::<ButtonInput<KeyCode>>()
                            .press(KeyCode::Enter);
                    } else {
                        let p = broken_ok(layout).center();
                        app.world_mut()
                            .get_mut::<Window>(window)
                            .unwrap()
                            .set_cursor_position(Some(Vec2::new(
                                p.x + layout.width / 2.,
                                layout.height / 2. - p.y,
                            )));
                        app.world_mut()
                            .resource_mut::<ButtonInput<MouseButton>>()
                            .press(MouseButton::Left);
                    }
                    app.update();
                    let client = app.world().resource::<Client>();
                    assert!(!client.broken);
                    assert!(client.view.is_none());
                    assert!(client.tx.is_none());
                    assert!(client.error.is_empty());
                    app.insert_resource(ButtonInput::<KeyCode>::default());
                    app.insert_resource(ButtonInput::<MouseButton>::default());
                    app.update();
                    assert_eq!(app.world().resource::<Surface>().playing, Some(false));
                    assert!(!labels.iter(app.world()).any(|text| text.0 == "Broken pike"));
                }
            }
        }
    }

    #[test]
    fn completing_deployment_sends_ready_once_without_needing_a_ready_button() {
        let cfg = Config::default();
        let mut game = Game::new(0, &cfg);
        game.start(0, &cfg);
        game.apply(0, Action::AutoPlace, 0, &cfg).unwrap();
        let (client, events_tx, actions_rx) = fixture();
        let mut app = App::new();
        app.insert_resource(client).add_systems(Update, receive);
        for _ in 0..3 {
            events_tx
                .send((
                    0,
                    Event::Reply(Reply::State {
                        view: game.view(0, "test", 0),
                    }),
                ))
                .unwrap();
            app.update();
        }
        assert_eq!(actions_rx.try_recv().unwrap(), Action::Ready);
        assert!(actions_rx.try_recv().is_err());
        game.apply(0, Action::Ready, 0, &cfg).unwrap();
        events_tx
            .send((
                0,
                Event::Reply(Reply::State {
                    view: game.view(0, "test", 1),
                }),
            ))
            .unwrap();
        app.update();
        assert!(actions_rx.try_recv().is_err());
        assert!(
            app.world()
                .resource::<Client>()
                .view
                .as_ref()
                .unwrap()
                .ready[0]
        );
    }
    #[test]
    fn joining_and_leaving_fit_the_window_to_the_sheet_and_then_restore_login() {
        let (client, _, _) = fixture();
        let mut app = App::new();
        app.insert_resource(client)
            .init_resource::<Assets<Image>>()
            .init_resource::<Surface>()
            .add_systems(Update, fit_surface);
        let window = app.world_mut().spawn(client_window(false)).id();
        assert!(app.world().get::<Window>(window).unwrap().resizable);
        app.update();
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set(1000., 700.);
        app.update();
        let login = app.world().get::<Window>(window).unwrap();
        assert!(login.resizable);
        assert_eq!((login.width(), login.height()), (1000., 700.));
        app.world_mut().resource_mut::<Client>().view =
            Some(Game::new(0, &Config::default()).view(0, "test", 0));
        app.update();
        let size = app.world().get::<Window>(window).unwrap();
        assert_eq!(
            (size.width(), size.height()),
            (plan::WIDTH as f32, plan::WINDOW_HEIGHT as f32)
        );
        assert!(size.resizable);
        for factor in [1., 2.] {
            let mut size = app.world_mut().get_mut::<Window>(window).unwrap();
            size.resolution.set_scale_factor_override(Some(factor));
            size.resolution.set(1100., 650.);
            app.update();
            app.update();
            let size = app.world().get::<Window>(window).unwrap();
            assert_eq!((size.width(), size.height()), (1100., 650.));
            let layout = app.world().resource::<Surface>().layout;
            assert!((layout.board_width / layout.board_height - plan::ASPECT).abs() < 0.00001);
            assert_eq!((layout.width, layout.height), (1100., 650.));
        }
        app.world_mut().resource_mut::<Client>().leave();
        app.update();
        let size = app.world().get::<Window>(window).unwrap();
        assert_eq!((size.width(), size.height()), (1280., 900.));
        assert!(size.resizable);
    }
    #[test]
    fn every_game_phase_contains_the_full_plan_and_fixed_bar_at_any_window_size() {
        let cfg = Config::default();
        let mut game = Game::new(0, &cfg);
        game.start(0, &cfg);
        game.apply(0, Action::AutoPlace, 0, &cfg).unwrap();
        game.apply(1, Action::AutoPlace, 0, &cfg).unwrap();
        for (w, h) in [(600., 881.), (1200., 900.), (400., 1600.), (1600., 400.)] {
            let layout = plan::Layout::new(w, h);
            for you in 0..2 {
                for phase in [
                    Phase::Waiting,
                    Phase::Placement,
                    Phase::Playing,
                    Phase::Finished {
                        winner: Some(you),
                        reason: "Flag captured".into(),
                    },
                ] {
                    let (mut client, _, _) = fixture();
                    game.phase = phase;
                    game.turn = you;
                    client.view = Some(game.view(you, "test", 0));
                    let mut app = App::new();
                    app.insert_resource(client)
                        .insert_resource(Surface {
                            layout,
                            playing: Some(true),
                        })
                        .insert_resource(Art {
                            font: default(),
                            bold_font: default(),
                            ranks: vec![Handle::default(); 11],
                            board: default(),
                            cover: default(),
                        })
                        .add_systems(Update, (paint, clock_text).chain());
                    app.update();
                    let mut sheets = 0;
                    let mut bars = 0;
                    let mut query = app.world_mut().query::<(&Sprite, &Transform)>();
                    for (sprite, transform) in query.iter(app.world()) {
                        let size = sprite.custom_size.unwrap();
                        if size == Vec2::new(layout.board_width, layout.board_height) {
                            sheets += 1;
                        }
                        if size == Vec2::new(layout.width, plan::BAR_HEIGHT as f32) {
                            bars += 1;
                        }
                        assert!(
                            transform.translation.x.abs() + size.x / 2. <= layout.width / 2. + 0.01
                        );
                        assert!(
                            transform.translation.y.abs() + size.y / 2.
                                <= layout.height / 2. + 0.01
                        );
                    }
                    assert_eq!((sheets, bars), (1, 1));
                    let mut clocks = app.world_mut().query::<(&Clock, &Transform)>();
                    let clocks: Vec<_> = clocks.iter(app.world()).collect();
                    assert_eq!(
                        clocks.len(),
                        if matches!(
                            app.world()
                                .resource::<Client>()
                                .view
                                .as_ref()
                                .unwrap()
                                .phase,
                            Phase::Placement
                        ) {
                            3
                        } else {
                            2
                        }
                    );
                    for (role, clock) in clocks {
                        assert_eq!(
                            clock.translation.y,
                            layout.height / 2. - if *role == Clock::Setup { 58. } else { 32. }
                        );
                    }
                    let count = app.world().entities().len();
                    app.update();
                    assert_eq!(app.world().entities().len(), count);
                }
            }
        }
    }
    #[test]
    fn resized_window_cursor_selects_bottom_cards_and_places_pieces_without_a_stale_camera() {
        for you in 0..2 {
            for dpi in [1., 2.] {
                let cfg = Config::default();
                let mut game = Game::new(0, &cfg);
                game.start(0, &cfg);
                let (mut client, _, actions) = fixture();
                client.view = Some(game.view(you, "test", 0));
                let mut app = App::new();
                app.insert_resource(client)
                    .init_resource::<Surface>()
                    .init_resource::<Assets<Image>>()
                    .init_resource::<ButtonInput<KeyCode>>()
                    .init_resource::<ButtonInput<MouseButton>>()
                    .add_message::<KeyboardInput>()
                    .insert_resource(Art {
                        font: default(),
                        bold_font: default(),
                        ranks: vec![Handle::default(); 11],
                        board: default(),
                        cover: default(),
                    })
                    .add_systems(Update, (fit_surface, input, paint).chain());
                let window = app.world_mut().spawn(Window::default()).id();
                app.update();
                for (w, h, target, rank) in
                    [(360., 480.), (600., 881.), (1200., 900.), (400., 1600.)]
                        .into_iter()
                        .flat_map(|(w, h)| {
                            plan::STRIP.into_iter().map(move |rank| (w, h, None, rank))
                        })
                        .chain(std::iter::once((
                            1600.,
                            500.,
                            Some(if you == 0 { 109 } else { 0 }),
                            Rank::Mine,
                        )))
                {
                    let layout = plan::Layout::new(w, h);
                    let position = if let Some(at) = target {
                        plan::cell(at).center()
                    } else {
                        plan::card(rank, you).center()
                    };
                    let point = layout.world(position, you);
                    let mut win = app.world_mut().get_mut::<Window>(window).unwrap();
                    win.resolution.set_scale_factor_override(Some(dpi));
                    win.resolution.set(w, h);
                    win.set_cursor_position(Some(Vec2::new(point.x + w / 2., h / 2. - point.y)));
                    app.insert_resource(ButtonInput::<MouseButton>::default());
                    app.world_mut()
                        .resource_mut::<ButtonInput<MouseButton>>()
                        .press(MouseButton::Left);
                    app.update();
                    assert_eq!(app.world().resource::<Client>().rank, rank);
                    let area = plan::card(rank, you);
                    let center = layout.world(area.center(), you);
                    let width = area.width / plan::PAGE_WIDTH * layout.board_width;
                    let height = area.height / plan::PAGE_HEIGHT * layout.board_height;
                    let mut sprites = app.world_mut().query::<(&Sprite, &Transform)>();
                    let edges: Vec<_> = sprites
                        .iter(app.world())
                        .filter(|(sprite, transform)| {
                            transform.translation.z == 2.
                                && sprite.color == Color::srgb(0.02, 0.18, 0.95)
                        })
                        .collect();
                    assert_eq!(edges.len(), 4);
                    let stroke = width * 0.07;
                    for (sprite, _) in &edges {
                        let size = sprite.custom_size.unwrap();
                        assert!((size.x.min(size.y) - stroke).abs() < 0.001);
                    }
                    let width = width + 2. * stroke;
                    let height = height + 2. * stroke;
                    let left = edges
                        .iter()
                        .map(|(s, t)| t.translation.x - s.custom_size.unwrap().x / 2.)
                        .fold(f32::INFINITY, f32::min);
                    let right = edges
                        .iter()
                        .map(|(s, t)| t.translation.x + s.custom_size.unwrap().x / 2.)
                        .fold(f32::NEG_INFINITY, f32::max);
                    let bottom = edges
                        .iter()
                        .map(|(s, t)| t.translation.y - s.custom_size.unwrap().y / 2.)
                        .fold(f32::INFINITY, f32::min);
                    let top = edges
                        .iter()
                        .map(|(s, t)| t.translation.y + s.custom_size.unwrap().y / 2.)
                        .fold(f32::NEG_INFINITY, f32::max);
                    assert!((left - (center.x - width / 2.)).abs() < 0.001);
                    assert!((right - (center.x + width / 2.)).abs() < 0.001);
                    assert!((bottom - (center.y - height / 2.)).abs() < 0.001);
                    assert!((top - (center.y + height / 2.)).abs() < 0.001);
                    let count = layout.world(plan::count_position(rank, you), you);
                    let mut counts = app
                        .world_mut()
                        .query_filtered::<&Transform, With<ReserveCount>>();
                    assert!(
                        counts
                            .iter(app.world())
                            .any(|t| (t.translation.x - count.x).abs() < 0.001
                                && (t.translation.y - count.y).abs() < 0.001)
                    );
                    if let Some(at) = target {
                        assert_eq!(actions.try_recv().unwrap(), Action::Place { at, rank });
                    } else {
                        assert!(actions.try_recv().is_err());
                    }
                }
            }
        }
    }
    #[test]
    fn army_counters_reset_after_deployment_and_track_captures_on_both_strips() {
        for (attacker, defender) in [
            (Rank::General, Rank::Cadet),
            (Rank::Cadet, Rank::General),
            (Rank::General, Rank::General),
            (Rank::Spy, Rank::Marshal),
            (Rank::Miner, Rank::Mine),
            (Rank::Cadet, Rank::Flag),
        ] {
            for you in 0..2 {
                let cfg = Config::default();
                let mut game = Game::new(0, &cfg);
                game.start(0, &cfg);
                for owner in 0..2 {
                    game.apply(owner, Action::AutoPlace, 0, &cfg).unwrap();
                }
                let (mut client, _, _) = fixture();
                client.view = Some(game.view(you, "counts", 0));
                let mut app = App::new();
                app.insert_resource(client)
                    .init_resource::<Surface>()
                    .insert_resource(Art {
                        font: default(),
                        bold_font: default(),
                        ranks: vec![Handle::default(); 11],
                        board: default(),
                        cover: default(),
                    })
                    .add_systems(Update, paint);
                app.update();
                let mut reserves = app
                    .world_mut()
                    .query_filtered::<&Text2d, With<ReserveCount>>();
                assert_eq!(reserves.iter(app.world()).count(), 11);
                assert!(reserves.iter(app.world()).all(|text| text.0 == "0×"));
                for owner in 0..2 {
                    game.apply(owner, Action::Ready, 0, &cfg).unwrap();
                }
                for captured in [false, true] {
                    if captured {
                        let from = game
                            .board
                            .iter()
                            .position(|piece| {
                                *piece
                                    == Some(Piece {
                                        owner: 0,
                                        rank: attacker,
                                    })
                            })
                            .unwrap();
                        let to = game
                            .board
                            .iter()
                            .position(|piece| {
                                *piece
                                    == Some(Piece {
                                        owner: 1,
                                        rank: defender,
                                    })
                            })
                            .unwrap();
                        game.board[44] = game.board[from].take();
                        game.board[45] = game.board[to].take();
                        game.turn = 0;
                        game.apply(0, Action::Move { from: 44, to: 45 }, 1, &cfg)
                            .unwrap();
                    }
                    for (w, h) in [(600., 881.), (1200., 900.), (400., 1600.)] {
                        let layout = plan::Layout::new(w, h);
                        app.world_mut().resource_mut::<Surface>().layout = layout;
                        {
                            let mut client = app.world_mut().resource_mut::<Client>();
                            client.view = Some(game.view(you, "counts", 1));
                            client.dirty = true;
                        }
                        app.update();
                        assert_eq!(reserves.iter(app.world()).count(), 0);
                        let mut query = app
                            .world_mut()
                            .query_filtered::<(&Text2d, &Transform), With<RemainingCount>>();
                        let counts: Vec<_> = query.iter(app.world()).collect();
                        assert_eq!(counts.len(), 22);
                        for owner in 0..2 {
                            for rank in Rank::ALL {
                                let expected = game
                                    .board
                                    .iter()
                                    .flatten()
                                    .filter(|piece| piece.owner == owner && piece.rank == rank)
                                    .count();
                                let position = layout.world(plan::count_position(rank, owner), you);
                                let (text, _) = counts
                                    .iter()
                                    .find(|(_, t)| {
                                        (t.translation.x - position.x).abs() < 0.001
                                            && (t.translation.y - position.y).abs() < 0.001
                                    })
                                    .unwrap();
                                assert_eq!(text.0, format!("{expected}×"));
                                if !captured {
                                    assert_eq!(expected, rank.count());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn cleaned_cards_show_initial_placed_and_returned_counts_on_both_sides() {
        for you in 0..2 {
            let cfg = Config::default();
            let mut game = Game::new(0, &cfg);
            game.start(0, &cfg);
            let (mut client, _, _) = fixture();
            client.view = Some(game.view(you, "test", 0));
            let mut app = App::new();
            app.insert_resource(client)
                .init_resource::<Surface>()
                .insert_resource(Art {
                    font: default(),
                    bold_font: default(),
                    ranks: vec![Handle::default(); 11],
                    board: default(),
                    cover: default(),
                })
                .add_systems(Update, paint);
            let at = if you == 0 { 109 } else { 0 };
            for expected_flag_count in [1, 0, 1] {
                if expected_flag_count == 0 {
                    game.apply(
                        you,
                        Action::Place {
                            at,
                            rank: Rank::Flag,
                        },
                        0,
                        &cfg,
                    )
                    .unwrap();
                } else if game.board[at].is_some() {
                    game.apply(you, Action::Remove { at }, 0, &cfg).unwrap();
                }
                let mut client = app.world_mut().resource_mut::<Client>();
                client.view = Some(game.view(you, "test", 0));
                client.dirty = true;
                app.update();
                let mut query = app
                    .world_mut()
                    .query_filtered::<(&Text2d, &Transform, &TextColor), With<ReserveCount>>();
                let counts: Vec<_> = query.iter(app.world()).collect();
                assert_eq!(counts.len(), 11);
                for rank in plan::STRIP {
                    let p = plan::Layout::default().world(plan::count_position(rank, you), you);
                    let (text, _, color) = counts
                        .iter()
                        .find(|(_, transform, _)| {
                            (transform.translation.x - p.x).abs() < 0.01
                                && (transform.translation.y - p.y).abs() < 0.01
                        })
                        .unwrap();
                    let expected = if rank == Rank::Flag {
                        expected_flag_count
                    } else {
                        rank.count()
                    };
                    assert_eq!(text.0, format!("{expected}×"));
                    assert_eq!(color.0, Color::BLACK);
                }
            }
        }
    }
    #[test]
    fn clocks_highlight_the_active_player_and_claim_button_is_opponent_only_and_turn_bound() {
        for (w, h) in [(360., 480.), (600., 925.), (1200., 900.), (400., 1600.)] {
            for you in 0..2 {
                for turn in 0..2 {
                    let cfg = Config::default();
                    let mut game = Game::new(0, &cfg);
                    game.phase = Phase::Playing;
                    game.turn = turn;
                    game.ply = 17;
                    let (mut client, _, actions) = fixture();
                    let mut view = game.view(you, "clocks", 0);
                    view.clocks.main_ms[turn] = 0;
                    view.clocks.delay_ms = 0;
                    view.clocks.claimable = true;
                    client.view = Some(view.clone());
                    let layout = plan::Layout::new(w, h);
                    let mut app = App::new();
                    app.insert_resource(client)
                        .insert_resource(Surface {
                            layout,
                            playing: Some(true),
                        })
                        .init_resource::<ButtonInput<KeyCode>>()
                        .init_resource::<ButtonInput<MouseButton>>()
                        .add_message::<KeyboardInput>()
                        .insert_resource(Art {
                            font: default(),
                            bold_font: default(),
                            ranks: vec![Handle::default(); 11],
                            board: default(),
                            cover: default(),
                        })
                        .add_systems(Update, (input, paint, clock_text).chain());
                    let window = app
                        .world_mut()
                        .spawn(Window {
                            resolution: (w as u32, h as u32).into(),
                            ..default()
                        })
                        .id();
                    app.update();
                    let mut labels = app.world_mut().query::<&Text2d>();
                    assert!(
                        labels
                            .iter(app.world())
                            .any(|text| text.0.starts_with("Your time"))
                    );
                    assert!(
                        labels
                            .iter(app.world())
                            .any(|text| text.0.starts_with("Opponent's time"))
                    );
                    let delay_label = if turn == you {
                        "Your time · turn delay"
                    } else {
                        "Opponent's time · turn delay"
                    };
                    assert!(labels.iter(app.world()).any(|text| text.0 == delay_label));
                    let mut clocks = app.world_mut().query::<&Clock>();
                    assert_eq!(clocks.iter(app.world()).count(), 2);
                    assert_eq!(
                        labels
                            .iter(app.world())
                            .any(|text| text.0 == "Claim victory"),
                        turn != you
                    );
                    let mut highlights = app
                        .world_mut()
                        .query_filtered::<&Transform, With<ActiveClock>>();
                    let active = highlights.single(app.world()).unwrap();
                    assert!(
                        (active.translation.x - if turn == you { -w / 4. } else { w / 4. }).abs()
                            < 0.001
                    );
                    let point = claim_button(layout).center();
                    assert!(layout.in_bar(point));
                    app.world_mut()
                        .get_mut::<Window>(window)
                        .unwrap()
                        .set_cursor_position(Some(Vec2::new(point.x + w / 2., h / 2. - point.y)));
                    app.world_mut()
                        .resource_mut::<ButtonInput<MouseButton>>()
                        .press(MouseButton::Left);
                    app.update();
                    if turn != you {
                        assert_eq!(
                            actions.try_recv().unwrap(),
                            Action::ClaimVictory { turn_id: 17 }
                        );
                    } else {
                        assert!(actions.try_recv().is_err());
                    }
                    // A fresh authoritative turn removes the old button even if
                    // the new active player also has no main time remaining.
                    view.turn = 1 - turn;
                    view.ply = 18;
                    view.clocks.delay_ms = 10_000;
                    view.clocks.claimable = false;
                    {
                        let mut client = app.world_mut().resource_mut::<Client>();
                        client.view = Some(view);
                        client.dirty = true;
                        client.received = Instant::now();
                    }
                    app.insert_resource(ButtonInput::<MouseButton>::default());
                    app.world_mut()
                        .resource_mut::<ButtonInput<MouseButton>>()
                        .press(MouseButton::Left);
                    app.update();
                    assert!(actions.try_recv().is_err());
                    assert!(
                        !labels
                            .iter(app.world())
                            .any(|text| text.0 == "Claim victory")
                    );
                    let mut sprites = app.world_mut().query::<(&Sprite, &Transform)>();
                    for (sprite, transform) in sprites.iter(app.world()) {
                        let size = sprite.custom_size.unwrap();
                        assert!(transform.translation.x.abs() + size.x / 2. <= w / 2. + 0.01);
                        assert!(transform.translation.y.abs() + size.y / 2. <= h / 2. + 0.01);
                    }
                }
            }
        }
    }
    #[test]
    fn time_bar_interpolates_both_main_clocks_and_delay_from_server_state() {
        for you in 0..2 {
            for turn in 0..2 {
                let cfg = Config::default();
                let mut game = Game::new(0, &cfg);
                game.phase = Phase::Playing;
                game.turn = turn;
                let (mut client, _, _) = fixture();
                let mut view = game.view(you, "clock", 0);
                view.clocks.main_ms = [90_000, 120_000];
                view.clocks.delay_ms = 500;
                client.view = Some(view);
                client.received = Instant::now() - Duration::from_millis(1500);
                let mut app = App::new();
                app.insert_resource(client).add_systems(Update, clock_text);
                let yours = app.world_mut().spawn((Clock::Yours, Text2d::new(""))).id();
                let opponent = app
                    .world_mut()
                    .spawn((Clock::Opponent, Text2d::new("")))
                    .id();
                app.update();
                let elapsed = app
                    .world()
                    .resource::<Client>()
                    .received
                    .elapsed()
                    .as_millis() as u64;
                let clocks = app
                    .world()
                    .resource::<Client>()
                    .view
                    .as_ref()
                    .unwrap()
                    .clock_after(elapsed);
                // The inactive clock is unchanged; only the active main clock
                // pays the elapsed portion beyond the snapshot's 500ms delay.
                assert_eq!(
                    app.world().get::<Text2d>(yours).unwrap().0,
                    player_clock_text(clocks, you, turn == you)
                );
                assert_eq!(
                    app.world().get::<Text2d>(opponent).unwrap().0,
                    player_clock_text(clocks, 1 - you, turn != you)
                );
                let active = if turn == you { yours } else { opponent };
                let inactive = if turn == you { opponent } else { yours };
                assert!(
                    app.world()
                        .get::<Text2d>(active)
                        .unwrap()
                        .0
                        .ends_with(" (0.0s)")
                );
                assert!(!app.world().get::<Text2d>(inactive).unwrap().0.contains('('));
                assert_eq!(clocks.main_ms[1 - turn], [90_000, 120_000][1 - turn]);
                assert!(clocks.main_ms[turn] <= [89_000, 119_000][turn]);
                app.world_mut().resource_mut::<Client>().broken = true;
                app.update();
                assert_eq!(
                    app.world().get::<Text2d>(yours).unwrap().0,
                    player_clock_text(
                        *app.world()
                            .resource::<Client>()
                            .view
                            .as_ref()
                            .unwrap()
                            .clocks,
                        you,
                        turn == you
                    )
                );
            }
        }
    }
}
