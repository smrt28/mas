use bevy::{
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
    sprite::Anchor,
};
use marsal_a_spion::{net, *};
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

const CELL: f32 = 60.;
const BOARD_WIDTH: f32 = COLS as f32 * CELL;
const BOARD_HEIGHT: f32 = ROWS as f32 * CELL;
const LEFT: f32 = -570.;
const TOP: f32 = 290.;
#[derive(Component)]
struct Painted;
#[derive(Component)]
struct Clock;
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
    help: bool,
    preview: bool,
    session: u64,
}
impl Client {
    fn send(&mut self, a: Action) {
        self.error.clear();
        if let Some(tx) = &self.tx {
            if tx.try_send(a).is_err() {
                self.error = "Connection busy or closed".into();
            }
        } else if self.preview {
            self.error = "Artwork preview. Restart without --preview to play online.".into();
        }
        self.dirty = true;
    }
    fn leave(&mut self) {
        self.session += 1;
        if let Some(s) = self.socket.take() {
            let _ = s.shutdown(Shutdown::Both);
        }
        self.tx = None;
        self.view = None;
        self.selected = None;
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
        let _ = g.apply(0, Action::AutoPlace, 0, &cfg);
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
        rank: Rank::Marshal,
        dirty: true,
        help: false,
        preview,
        session: 0,
    };
    if autojoin {
        client.connect();
    }
    let headless = args.iter().any(|a| a == "--headless");
    let mut app = App::new();
    app.insert_resource(client)
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
            primary_window: Some(Window {
                title: "Maršál a špión | Field command".into(),
                resolution: (1280, 900).into(),
                resizable: false,
                ..default()
            }),
            ..default()
        }));
    }
    app.add_systems(Startup, setup)
        .add_systems(
            Update,
            (receive, input, paint, clock_text, screenshot).chain(),
        )
        .run();
}
fn setup(mut commands: Commands, assets: Res<AssetServer>, mut images: ResMut<Assets<Image>>) {
    if std::env::args().any(|a| a == "--headless") {
        let mut image = Image::new_target_texture(
            1280,
            900,
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
                c.view = Some(view);
                c.received = Instant::now();
                c.dirty = true;
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
    camera: Single<(&Camera, &GlobalTransform)>,
) {
    if keys.just_pressed(KeyCode::F1) {
        c.help = !c.help;
        c.dirty = true;
    }
    if keys.just_pressed(KeyCode::Escape) {
        if c.help {
            c.help = false;
            c.dirty = true;
        } else {
            c.leave();
        }
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
                Key::Enter if !c.connecting => {
                    c.connect();
                }
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
    if c.help {
        return;
    }
    let Some(window) = window else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok(pos) = camera.0.viewport_to_world_2d(camera.1, cursor) else {
        return;
    };
    let clicked = mouse.just_pressed(MouseButton::Left);
    let right = mouse.just_pressed(MouseButton::Right);
    if c.view.is_none() {
        if clicked {
            if hit(pos, 0., 110., 600., 56.) {
                c.field = 0;
                c.dirty = true;
            }
            if hit(pos, 0., 20., 600., 56.) {
                c.field = 1;
                c.dirty = true;
            }
            if hit(pos, 0., -90., 600., 56.) && !c.connecting {
                c.connect();
            }
        }
        return;
    }
    let v = c.view.as_ref().unwrap().clone();
    if clicked && hit(pos, 380., -375., 330., 36.) {
        c.help = true;
        c.dirty = true;
        return;
    }
    if clicked && hit(pos, 380., -420., 330., 30.) {
        c.send(Action::Resign);
        return;
    }
    if matches!(v.phase, Phase::Placement) && clicked {
        for (i, r) in Rank::ALL.into_iter().enumerate() {
            if hit(pos, 380., 208. - i as f32 * 35., 330., 32.) {
                c.rank = r;
                c.dirty = true;
                return;
            }
        }
        if hit(pos, 292., -207., 155., 40.) {
            c.send(Action::AutoPlace);
            return;
        }
        if hit(pos, 468., -207., 155., 40.) {
            c.send(Action::Ready);
            return;
        }
    }
    let col = ((pos.x - LEFT) / CELL).floor() as i32;
    let row = ((TOP + CELL / 2. - pos.y) / CELL).floor() as i32;
    if !(0..COLS as i32).contains(&col) || !(0..ROWS as i32).contains(&row) {
        return;
    }
    let screen = row as usize * COLS + col as usize;
    let at = orient(screen, v.you);
    if matches!(v.phase, Phase::Placement) {
        if right {
            c.send(Action::Remove { at });
        } else if clicked {
            let rank = c.rank;
            c.send(Action::Place { at, rank });
        }
    } else if matches!(v.phase, Phase::Playing) && clicked {
        if v.board[at]
            .as_ref()
            .is_some_and(|p| p.owner == v.you && p.rank.is_some_and(Rank::mobile))
        {
            c.error.clear();
            c.selected = Some(at);
            c.dirty = true;
        } else if let Some(from) = c.selected.take() {
            c.error.clear();
            c.send(Action::Move { from, to: at });
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
fn button(cmd: &mut Commands, art: &Art, s: &str, x: f32, y: f32, w: f32, active: bool) {
    rect(
        cmd,
        x,
        y,
        w,
        34.,
        5.,
        if active {
            Color::srgb(0.6, 0.27, 0.18)
        } else {
            Color::srgb(0.16, 0.20, 0.22)
        },
    );
    label(
        cmd,
        art,
        s,
        x - w / 2. + 12.,
        y,
        15.,
        Color::srgb(0.96, 0.90, 0.75),
    );
}
fn paint(
    mut cmd: Commands,
    mut c: ResMut<Client>,
    art: Res<Art>,
    old: Query<Entity, With<Painted>>,
) {
    if !c.dirty {
        return;
    }
    c.dirty = false;
    for e in &old {
        cmd.entity(e).despawn();
    }
    let ink = Color::srgb(0.94, 0.88, 0.74);
    let muted = Color::srgb(0.61, 0.66, 0.64);
    let gold = Color::srgb(0.94, 0.67, 0.32);
    label(&mut cmd, &art, "MARŠÁL A ŠPIÓN", -575., 402., 36., ink);
    label(
        &mut cmd,
        &art,
        "HIDDEN ARMIES  /  OPEN POSSIBILITIES",
        -572.,
        365.,
        13.,
        muted,
    );
    label(&mut cmd, &art, "F1  Field manual", 395., 402., 15., muted);
    if c.view.is_none() {
        picture(&mut cmd, art.cover.clone(), 0., -40., 1190., 740., 0.);
        rect(
            &mut cmd,
            0.,
            -20.,
            740.,
            610.,
            1.,
            Color::srgba(0.035, 0.05, 0.055, 0.95),
        );
        label(&mut cmd, &art, "Take command.", -300., 235., 34., ink);
        label(
            &mut cmd,
            &art,
            "Choose the same game name as your opponent.",
            -300.,
            190.,
            17.,
            muted,
        );
        for (i, (s, y)) in [
            (format!("Server   {}", c.address), 110.),
            (format!("Game     {}", c.room), 20.),
        ]
        .into_iter()
        .enumerate()
        {
            rect(
                &mut cmd,
                0.,
                y,
                600.,
                56.,
                3.,
                if c.field == i {
                    Color::srgb(0.24, 0.29, 0.29)
                } else {
                    Color::srgb(0.12, 0.16, 0.17)
                },
            );
            label(&mut cmd, &art, s, -280., y, 21., ink);
        }
        button(
            &mut cmd,
            &art,
            if c.connecting {
                "Connecting…"
            } else {
                "ENTER  /  Join or create game"
            },
            0.,
            -90.,
            600.,
            true,
        );
        label(
            &mut cmd,
            &art,
            "Tab switches fields. Enter connects.",
            -300.,
            -150.,
            16.,
            muted,
        );
        label(
            &mut cmd,
            &art,
            "Two commanders · 40 pieces each · one hidden flag",
            -300.,
            -195.,
            15.,
            muted,
        );
        label(
            &mut cmd,
            &art,
            "All matches live in server memory only.",
            -300.,
            -230.,
            15.,
            muted,
        );
        label(&mut cmd, &art, wrap(&c.error, 68), -300., -285., 14., gold);
    } else if let Some(v) = &c.view {
        label(
            &mut cmd,
            &art,
            format!("{}  /  Commander {}", v.game, v.you + 1),
            -570.,
            329.,
            17.,
            gold,
        );
        let mut board_transform = Transform::from_xyz(
            LEFT + BOARD_WIDTH / 2.,
            TOP + CELL / 2. - BOARD_HEIGHT / 2.,
            0.,
        );
        if v.you == 1 {
            board_transform.rotation = Quat::from_rotation_z(std::f32::consts::PI);
        }
        cmd.spawn((
            Painted,
            Sprite {
                image: art.board.clone(),
                custom_size: Some(Vec2::new(BOARD_WIDTH, BOARD_HEIGHT)),
                ..default()
            },
            board_transform,
        ));
        let targets = c
            .selected
            .map(|from| v.legal_targets(from))
            .unwrap_or_default();
        for screen in 0..CELLS {
            let at = orient(screen, v.you);
            let x = LEFT + (screen % COLS) as f32 * CELL + CELL / 2.;
            let y = TOP - (screen / COLS) as f32 * CELL;
            // The printed flag panels span three rows each. Leave their art
            // uninterrupted; neither cells nor piece hit targets can enter them.
            if blocked(at) {
                continue;
            }
            if targets.contains(&at) {
                rect(
                    &mut cmd,
                    x,
                    y,
                    56.,
                    56.,
                    2.,
                    Color::srgba(0.25, 0.65, 0.52, 0.8),
                );
            }
            if c.selected == Some(at) {
                rect(&mut cmd, x, y, 58., 58., 2., gold);
            }
            if let Some(p) = &v.board[at] {
                let friendly = p.owner == v.you;
                rect(
                    &mut cmd,
                    x + 2.,
                    y - 2.,
                    50.,
                    54.,
                    2.5,
                    Color::srgba(0.10, 0.07, 0.04, 0.45),
                );
                rect(&mut cmd, x, y, 50., 54., 3., Color::srgb(0.52, 0.37, 0.19));
                rect(
                    &mut cmd,
                    x,
                    y,
                    47.,
                    51.,
                    3.5,
                    if friendly {
                        Color::srgb(0.96, 0.92, 0.81)
                    } else {
                        Color::srgb(0.65, 0.24, 0.18)
                    },
                );
                if let Some(rank) = p.rank {
                    picture(
                        &mut cmd,
                        art.ranks[rank as usize].clone(),
                        x,
                        y + 5.,
                        40.,
                        40.,
                        4.,
                    );
                    label(
                        &mut cmd,
                        &art,
                        rank.short(),
                        x - 17.,
                        y - 18.,
                        11.,
                        Color::srgb(0.12, 0.13, 0.12),
                    );
                } else {
                    label(&mut cmd, &art, "?", x - 9., y + 1., 27., ink);
                }
            }
        }
        for col in 0..COLS {
            let at = orient(col, v.you);
            label(
                &mut cmd,
                &art,
                ((b'A' + (at % COLS) as u8) as char).to_string(),
                LEFT + col as f32 * CELL + CELL / 2. - 5.,
                TOP + CELL / 2. - BOARD_HEIGHT - 17.,
                12.,
                muted,
            );
        }
        for row in 0..ROWS {
            let at = orient(row * COLS, v.you);
            label(
                &mut cmd,
                &art,
                (ROWS - at / COLS).to_string(),
                LEFT - 28.,
                TOP - row as f32 * CELL,
                12.,
                muted,
            );
        }
        let status: String = match &v.phase {
            Phase::Waiting => "AWAITING OPPONENT".into(),
            Phase::Placement => {
                if v.ready[v.you] {
                    "DEPLOYMENT LOCKED".into()
                } else {
                    "DEPLOY YOUR ARMY".into()
                }
            }
            Phase::Playing => {
                if v.turn == v.you {
                    "YOUR MOVE".into()
                } else {
                    "OPPONENT'S MOVE".into()
                }
            }
            Phase::Finished { winner, .. } => match winner {
                Some(p) if *p == v.you => "VICTORY".into(),
                Some(_) => "DEFEAT".into(),
                None => "DRAW".into(),
            },
        };
        label(&mut cmd, &art, status, 215., 330., 23., gold);
        let clock = cmd
            .spawn((
                Painted,
                Clock,
                Text2d::new(""),
                TextFont {
                    font: art.font.clone(),
                    font_size: 20.,
                    ..default()
                },
                TextColor(ink),
                Anchor::CENTER_LEFT,
                Transform::from_xyz(215., 288., 8.),
            ))
            .id();
        let _ = clock;
        match &v.phase {
            Phase::Waiting => {
                label(
                    &mut cmd,
                    &art,
                    "Share this game name:",
                    215.,
                    212.,
                    18.,
                    ink,
                );
                label(&mut cmd, &art, wrap(&v.game, 24), 215., 165., 24., gold);
                label(
                    &mut cmd,
                    &art,
                    "Deployment starts when the\nsecond commander joins.\n\nYour army stays hidden\nuntil the match begins.",
                    215.,
                    45.,
                    18.,
                    muted,
                );
            }
            Phase::Placement => {
                label(
                    &mut cmd,
                    &art,
                    "Select a rank, then click a home square.",
                    215.,
                    250.,
                    15.,
                    muted,
                );
                for (i, (rank, count)) in v.reserve.iter().enumerate() {
                    let y = 208. - i as f32 * 35.;
                    button(
                        &mut cmd,
                        &art,
                        &format!("       {:<15} ×{}", rank.name(), count),
                        380.,
                        y,
                        330.,
                        c.rank == *rank,
                    );
                    picture(
                        &mut cmd,
                        art.ranks[*rank as usize].clone(),
                        238.,
                        y,
                        26.,
                        26.,
                        7.,
                    );
                }
                button(&mut cmd, &art, "Auto-fill", 292., -207., 155., false);
                button(&mut cmd, &art, "Lock army", 468., -207., 155., true);
                label(
                    &mut cmd,
                    &art,
                    "Right-click to remove.\nAuto-fill preserves your placed pieces.\nThe clock fills and locks both armies.",
                    215.,
                    -270.,
                    14.,
                    muted,
                );
            }
            Phase::Playing => {
                label(
                    &mut cmd,
                    &art,
                    format!("Turn {}  /  missed turns: {}", v.ply + 1, v.missed[v.you]),
                    215.,
                    235.,
                    15.,
                    ink,
                );
                label(
                    &mut cmd,
                    &art,
                    "Click your piece, then its destination.\nMove in 8 directions, attack in 4.\nScouts travel on clear straight lines.\nGreen squares are legal destinations.",
                    215.,
                    172.,
                    15.,
                    muted,
                );
                label(&mut cmd, &art, "FIELD REPORTS", 215., 84., 15., gold);
                let reports = v
                    .messages
                    .iter()
                    .rev()
                    .take(4)
                    .map(|s| wrap(s, 39))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                label(&mut cmd, &art, reports, 215., -45., 14., ink);
                label(
                    &mut cmd,
                    &art,
                    format!(
                        "Captured: {}   /   Lost: {}",
                        v.captured.iter().filter(|p| p.owner != v.you).count(),
                        v.captured.iter().filter(|p| p.owner == v.you).count()
                    ),
                    215.,
                    -238.,
                    17.,
                    muted,
                );
            }
            Phase::Finished { reason, .. } => {
                label(&mut cmd, &art, wrap(reason, 28), 215., 196., 23., ink);
                label(
                    &mut cmd,
                    &art,
                    format!(
                        "{} turns played\n{} pieces captured\n\nEscape returns to the lobby.\nUse a new name for a rematch.",
                        v.ply,
                        v.captured.len()
                    ),
                    215.,
                    55.,
                    17.,
                    muted,
                );
            }
        }
        label(&mut cmd, &art, wrap(&c.error, 43), 215., -330., 14., gold);
        label(
            &mut cmd,
            &art,
            "Your army is ivory. Enemy ranks remain hidden.",
            -570.,
            -395.,
            16.,
            ink,
        );
        label(
            &mut cmd,
            &art,
            "Board and pieces from mas-plan.pdf",
            -570.,
            -424.,
            13.,
            muted,
        );
        button(
            &mut cmd,
            &art,
            "F1  /  Field manual",
            380.,
            -375.,
            330.,
            false,
        );
        button(
            &mut cmd,
            &art,
            "Resign  /  Escape to leave",
            380.,
            -420.,
            330.,
            false,
        );
    }
    if c.help {
        rect(
            &mut cmd,
            0.,
            0.,
            1190.,
            730.,
            10.,
            Color::srgb(0.08, 0.11, 0.12),
        );
        // Overlay text must be above the panel.
        let start=cmd.spawn((Painted,Text2d::new("FIELD MANUAL  /  F1 to close\n\nDeploy all 40 pieces in your four home rows. Auto-fill can help.\nYour opponent cannot see your ranks or your deployment.\n\nMove one square in any of eight directions. Attacks are orthogonal only.\nThe scout may travel any distance orthogonally, stopping at the first piece.\nYou cannot cross pieces or the two central flag gaps. Mines and flags stay put.\n\nStrength: Marshal > General > Captain > Cadet > Corporal > Rifleman\n             > Scout > Miner > Spy. Equal ranks remove both pieces.\nSpy defeats Marshal in either direction. Miner alone clears mines.\nEvery moving piece can capture the enemy flag to win.\n\nOnly the attacker learns the defender's rank. A surviving attacker's rank\nis never disclosed. Captured pieces are recorded by the server.\n\nSetup defaults to 2 minutes; moves default to 5 seconds.\nA missed move forfeits the turn; repeated misses lose the match.\nNo legal moves, or too many moves without a capture, means a draw.\nDisconnecting or resigning awards the opponent the match.\n\nSelect pieces with left click. Remove setup pieces with right click.\nF1 opens this manual. Escape leaves the match."),TextFont{font:art.font.clone(),font_size:20.,..default()},TextColor(ink),Anchor::CENTER_LEFT,Transform::from_xyz(-535.,0.,12.))).id();
        let _ = start;
    }
}
fn wrap(s: &str, width: usize) -> String {
    let mut result = String::new();
    let mut n = 0;
    for word in s.split_whitespace() {
        let len = word.chars().count();
        if n > 0 && n + 1 + len > width {
            result.push('\n');
            n = 0;
        }
        if n > 0 {
            result.push(' ');
            n += 1;
        }
        result.push_str(word);
        n += len;
    }
    result
}
fn clock_text(c: Res<Client>, mut texts: Query<&mut Text2d, With<Clock>>) {
    if let Some(v) = &c.view {
        let remain = v
            .remaining_ms
            .saturating_sub(c.received.elapsed().as_millis() as u64);
        for mut text in &mut texts {
            text.0 = if matches!(v.phase, Phase::Finished { .. }) {
                "Match complete".into()
            } else {
                format!(
                    "{:02}:{:02}.{} remaining",
                    remain / 60000,
                    (remain / 1000) % 60,
                    (remain / 100) % 10
                )
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
