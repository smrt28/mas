use bevy::{
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
    sprite::Anchor,
};
use marsal_a_spion::{net, plan, *};
use std::{
    io::BufReader,
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
struct Clock;
#[derive(Component)]
struct ReserveCount;
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
    ranks: Vec<Handle<Image>>,
    board: Handle<Image>,
    cover: Handle<Image>,
}
enum Event {
    Connected(SyncSender<Action>, TcpStream),
    Reply(Reply),
    Failed(String),
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
        self.ready_sent = false;
        self.connecting = false;
        self.dirty = true;
    }
    fn connect(&mut self) {
        if self.room.trim().is_empty() {
            self.error = "Enter a game name".into();
            self.dirty = true;
            return;
        }
        if self.tx.is_some() {
            self.send(Action::Join {
                game: self.room.clone(),
            });
            return;
        }
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
            let stream = match result {
                Ok(s) => s,
                Err(e) => {
                    let _ = events.send((session, Event::Failed(e.to_string())));
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
                    if net::write_frame(&mut writer, &a).is_err() {
                        break;
                    }
                }
                let _ = writer.shutdown(Shutdown::Both);
            });
            let mut reader = BufReader::new(reader);
            let reason = loop {
                match net::read_frame(&mut reader, net::MAX_FRAME) {
                    Ok(line) => match serde_json::from_str(&line) {
                        Ok(reply) => {
                            if events.send((session, Event::Reply(reply))).is_err() {
                                return;
                            }
                        }
                        Err(e) => break format!("Invalid server reply: {e}"),
                    },
                    Err(e) => break e.to_string(),
                }
            };
            let _ = events.send((session, Event::Failed(reason)));
        });
    }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
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
    };
    if autojoin {
        client.connect();
    }
    let headless = args.iter().any(|a| a == "--headless");
    let mut app = App::new();
    app.insert_resource(client)
        .init_resource::<Surface>()
        .insert_resource(ClearColor(Color::srgb(0.055, 0.065, 0.07)));
    let plugins = DefaultPlugins.set(AssetPlugin {
        file_path: format!("{}/assets", env!("CARGO_MANIFEST_DIR")),
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
    app.add_systems(Startup, setup)
        .add_systems(
            Update,
            (receive, fit_surface, input, paint, clock_text, screenshot).chain(),
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
        font: assets.load("font.ttf"),
        ranks: Rank::ALL
            .into_iter()
            .map(|r| assets.load(format!("{}.png", r.asset())))
            .collect(),
        board: assets.load("board.png"),
        cover: assets.load("cover.png"),
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
            continue;
        }
        match event {
            Event::Connected(tx, socket) => {
                c.tx = Some(tx);
                c.socket = Some(socket);
                c.connecting = false;
                let name = c.room.clone();
                c.send(Action::Join { game: name });
            }
            Event::Reply(Reply::State { view }) => {
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
                c.tx = None;
                c.socket = None;
                c.connecting = false;
                c.error = format!("Disconnected: {message}");
                c.dirty = true;
            }
        }
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
                    if c.field == 0 {
                        c.address.pop();
                    } else {
                        c.room.pop();
                    }
                    c.dirty = true;
                }
                Key::Enter if !c.connecting => c.connect(),
                Key::Character(s) if !c.connecting && !s.chars().any(char::is_control) => {
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
        let login = login_layout(surface.layout);
        if clicked {
            if hit(pos, login.x, login.server_y, login.width, 36. * login.scale) {
                c.field = 0;
                c.dirty = true;
            }
            if hit(pos, login.x, login.game_y, login.width, 36. * login.scale) {
                c.field = 1;
                c.dirty = true;
            }
            if hit(pos, login.x, login.enter_y, login.width, 34. * login.scale) && !c.connecting {
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
) {
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
    ));
}
fn border(cmd: &mut Commands, x: f32, y: f32, w: f32, h: f32, z: f32, color: Color) {
    let stroke = w.min(h) / 25.;
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
        let login = login_layout(surface.layout);
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
        rect(
            &mut cmd,
            login.x,
            login.game_y,
            382. * scale,
            164. * scale,
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
                login.x,
                y,
                login.width,
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
                .take(28)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let display = if value.chars().count() > 28 {
                format!("…{tail}")
            } else {
                tail
            };
            label(
                &mut cmd,
                &art,
                display,
                login.x - login.width / 2. + 80. * scale,
                y,
                16. * scale,
                ink,
            );
        }
        rect(
            &mut cmd,
            login.x,
            login.enter_y,
            login.width,
            34. * scale,
            5.,
            Color::srgb(0.6, 0.27, 0.18),
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
                login.game_y - 112. * scale,
                13. * scale,
                gold,
            );
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
        if let Some(piece) = &v.board[at] {
            let face = if piece.owner == 0 {
                Color::srgb(0.87, 0.36, 0.28)
            } else {
                Color::srgb(0.96, 0.94, 0.87)
            };
            rect(
                &mut cmd,
                point.x + 1.5 * scale,
                point.y - 1.5 * scale,
                w - 10. * scale,
                h - 8. * scale,
                2.5,
                Color::srgba(0.1, 0.07, 0.04, 0.4),
            );
            rect(
                &mut cmd,
                point.x,
                point.y,
                w - 10. * scale,
                h - 8. * scale,
                3.,
                face,
            );
            border(
                &mut cmd,
                point.x,
                point.y,
                w - 10. * scale,
                h - 8. * scale,
                3.5,
                Color::srgb(0.42, 0.29, 0.17),
            );
            if let Some(rank) = piece.rank {
                let size = w.min(h) - 15. * scale;
                picture(
                    &mut cmd,
                    art.ranks[rank as usize].clone(),
                    point.x,
                    point.y,
                    size,
                    size,
                    4.,
                );
            } else {
                label(
                    &mut cmd,
                    &art,
                    "?",
                    point.x - 7. * scale,
                    point.y,
                    24. * scale,
                    Color::srgb(0.18, 0.13, 0.1),
                );
            }
        }
    }
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
            printed_rect(&mut cmd, card, v.you, 2., gold, true, layout);
        }
        // Both printed strips have clean, blank count areas. Draw every live
        // reserve value from the server, including the initial inventory, without
        // painting a flat patch over the restored paper beneath it.
        let count_position = plan::count_position(*rank, v.you);
        let point = layout.world(count_position, v.you);
        cmd.spawn((
            Painted,
            ReserveCount,
            Text2d::new(format!("{count}×")),
            TextFont {
                font: art.font.clone(),
                font_size: 12. * scale,
                ..default()
            },
            TextColor(Color::BLACK),
            Anchor::CENTER,
            Transform::from_xyz(point.x, point.y, 8.),
        ));
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
    label(
        &mut cmd,
        &art,
        shortened(&status, ((layout.width - 120.) / 7.).max(8.) as usize),
        -layout.width / 2. + 12.,
        top - 11.,
        13.,
        ink,
    );
    label(
        &mut cmd,
        &art,
        shortened(&detail, ((layout.width - 120.) / 6.).max(8.) as usize),
        -layout.width / 2. + 12.,
        top - 29.,
        11.,
        if c.error.is_empty() {
            Color::srgb(0.76, 0.78, 0.72)
        } else {
            gold
        },
    );
    label(
        &mut cmd,
        &art,
        "YOUR TIME",
        layout.width / 2. - 80.,
        top - 9.,
        8.,
        ink,
    );
    cmd.spawn((
        Painted,
        Clock,
        Text2d::new(""),
        TextFont {
            font: art.font.clone(),
            font_size: 18.,
            ..default()
        },
        TextColor(gold),
        Anchor::CENTER_RIGHT,
        Transform::from_xyz(layout.width / 2. - 12., top - 26., 8.),
    ));
}
fn clock_text(c: Res<Client>, mut texts: Query<&mut Text2d, With<Clock>>) {
    if let Some(v) = &c.view {
        let remaining = v
            .remaining_ms
            .saturating_sub(c.received.elapsed().as_millis() as u64);
        let running = matches!(v.phase, Phase::Placement)
            || matches!(v.phase, Phase::Playing) && v.turn == v.you;
        for mut text in &mut texts {
            text.0 = if running {
                format!(
                    "{:02}:{:02}.{}",
                    remaining / 60000,
                    (remaining / 1000) % 60,
                    (remaining / 100) % 10
                )
            } else {
                "—".into()
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
        };
        (client, events_tx, actions_rx)
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
                    let mut clocks = app.world_mut().query_filtered::<&Transform, With<Clock>>();
                    let clock = clocks.single(app.world()).unwrap();
                    assert_eq!(clock.translation.x, layout.width / 2. - 12.);
                    assert_eq!(clock.translation.y, layout.height / 2. - 26.);
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
                                && sprite.color == Color::srgb(0.94, 0.67, 0.32)
                        })
                        .collect();
                    assert_eq!(edges.len(), 4);
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
    fn time_bar_counts_only_setup_and_the_players_own_turn() {
        let (mut client, _, _) = fixture();
        let cfg = Config::default();
        let mut game = Game::new(0, &cfg);
        game.start(0, &cfg);
        client.view = Some(game.view(0, "test", 0));
        let mut app = App::new();
        app.insert_resource(client).add_systems(Update, clock_text);
        let clock = app.world_mut().spawn((Clock, Text2d::new(""))).id();
        for (phase, turn, running) in [
            (Phase::Placement, 0, true),
            (Phase::Playing, 0, true),
            (Phase::Playing, 1, false),
            (Phase::Waiting, 0, false),
            (
                Phase::Finished {
                    winner: None,
                    reason: "Draw".into(),
                },
                0,
                false,
            ),
        ] {
            game.phase = phase;
            game.turn = turn;
            game.deadline = 5000;
            app.world_mut().resource_mut::<Client>().view = Some(game.view(0, "test", 0));
            app.update();
            let text = &app.world().get::<Text2d>(clock).unwrap().0;
            assert_eq!(text != "—", running);
            if running {
                assert!(text.starts_with("00:0"));
            }
        }
    }
}
