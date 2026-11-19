# Maršál a špión

A two-player hidden-army board game in Rust, with a native **Bevy 0.18** client and an authoritative TCP server. Rules and original scanned graphics come from [`mas.pdf`](mas.pdf), especially pages 4–5. The complete 10×11 board and sharper piece symbols come from [`tmp/mas-plan.pdf`](tmp/mas-plan.pdf). No database, accounts, save files, or persistent matches: restarting the server removes everything.

## Run

Install a current stable Rust toolchain (tested with Rust 1.95). The graphical client needs a desktop session and a Vulkan/Metal/DX12-capable graphics driver. On Linux, install Bevy's [native build dependencies](https://bevy.org/learn/quick-start/getting-started/setup/).

From this directory, start the server:

```sh
cargo run --release --no-default-features --bin mas-server -- config.toml
```

Start a client on each player's computer:

```sh
cargo run --release --bin mas-client
```

Enter the server address (`127.0.0.1:4000` for local play, or the server machine's address for LAN/remote play), enter a game name, and press Enter. Tab switches fields. The first player creates the room; the second joins by entering **exactly the same name**. Names are case-sensitive and surrounding whitespace is trimmed. The room starts deployment as soon as both players connect. A third player must choose another name.

You can also prefill and join directly:

```sh
cargo run --release --bin mas-client -- --server 192.168.1.20:4000 --room "Prague battle"
```

Allow incoming TCP on port 4000 on the server machine when playing remotely. `config.toml` controls the bind address and limits. The server uses plaintext TCP; this version has no accounts or reconnect tokens.

## Play

- Choose a rank in the right panel, then left-click an empty square in your four home rows. Right-click removes a piece during deployment. The board rotates so your army always starts at the bottom.
- **Auto-fill** randomly places your remaining pieces while preserving the ones you already placed. **Lock army** confirms a complete deployment. Both players can place pieces simultaneously; the opponent's setup is concealed.
- Setup defaults to **120 seconds**, starting when player two joins. When the clock expires, both armies are filled and locked automatically.
- One randomly selected commander starts. Select one of your moving pieces and click a highlighted destination. Green squares are legal moves. Your pieces are ivory; enemy pieces are red and show `?`.
- Moves default to **5 seconds** each. Expiry forfeits that turn. Three consecutive missed personal turns lose the match; making a valid move resets your counter. Invalid commands never reset the clock.
- Capture the enemy flag to win. F1 opens the field manual. Resign or Escape/disconnect awards the other player the match. After a match, both players should leave and choose a new name for a rematch.

Both time limits, the missed-turn allowance, quiet-move draw threshold, waiting-room lifetime and capacity limits are configurable in [`config.toml`](config.toml). The server validates configuration at startup. Pass a different TOML path as its first argument.

## Rules and interpretation

See [`RULES.md`](RULES.md) for the translated rules, exact army inventory and decisions made where the scan is ambiguous. This follows the Czech game rather than assuming ordinary Stratego rules: **diagonal movement is allowed, diagonal attacks are forbidden, and a spy beats a marshal in either direction**. Only the attacker learns the defender's rank; a surviving attacking piece stays secret from the defender.

## Check the implementation

```sh
cargo test --locked --no-default-features
cargo check --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
```

The 33 rules/session tests include randomized matches, private views through the actual JSON codec, custom server clocks, inventory conservation, combat exceptions, legal move highlights, room isolation, capacity, and cleanup. They do not need graphics or sockets.

The live TCP integration test starts the actual server and connects several clients. It checks matchmaking, third-player rejection, private deployment, moves, automatic setup, timeout loss, and disconnect wins:

```sh
cargo test --locked --no-default-features --features network-tests --test network
```

This test requires permission to bind localhost sockets. CI runs it separately from the client build. The restricted development environment could compile this test but could not execute sockets or render the UI; those runtime checks need a normal host.

Artwork preview and optional screenshots (require a working graphics driver):

```sh
cargo run --bin mas-client -- --preview
cargo run --bin mas-client -- --preview --screenshot /tmp/mas-preview.png
# Render without a desktop window, on a machine with a GPU or software Vulkan driver:
cargo run --bin mas-client -- --headless --preview --screenshot /tmp/mas-preview.png
```

`--preview` is an offline artwork preview, not a local opponent.

## Layout

- `src/lib.rs`: deterministic rule validation, combat, player-specific views and wire types; random deployment and starting player.
- `src/session.rs`: in-memory named-room coordinator shared by tests and server.
- `src/net.rs`: bounded newline-delimited JSON framing.
- `src/bin/server.rs`: single game-state owner with bounded I/O queues, per-connection readers/writers, monotonic clocks, limits and disconnect cleanup.
- `src/bin/client.rs`: Bevy 2D interface, game-name entry, deployment, board interaction, clocks and field reports. Network work runs off the render thread.
- `assets/`: the complete printed board, transparent high-resolution rank symbols, cover artwork and a bundled DejaVu font with its license. Bevy loads this directory relative to the compiled source location; retain it when running the binaries.
- `tools/extract_assets.py`: reproducible extraction from the supplied PDFs, requiring Poppler (`pdftoppm`) and Python Pillow.

The wire protocol is one JSON object per newline: actions use a `type` field (`join`, `place`, `remove`, `auto_place`, `ready`, `move`, `resign`); replies are `state` or `error`. Coordinates are indices `0..109`, with index `y * 10 + x` on the 10-column, 11-row board; A11 is index 0 and J1 is index 109. The six blocked squares are D5–D7 and G5–G7; each commander deploys in their four back rows, leaving three neutral rows. The server never sends another player's live ranks, even after the match. It records captured ranks and keeps only eight private field reports per player. Updates arrive at least every 250 ms; displayed countdowns interpolate locally, while deadlines are enforced by the server.

Rooms disappear when their last connection leaves. A waiting room expires after its configured lifetime; its occupant sees the result and can leave. Finished rooms stay available to their occupants until they leave. Unjoined connections time out after 30 seconds. Oversized/malformed frames, excessive command rates and stalled output consumers are disconnected. No game state is written to disk.
