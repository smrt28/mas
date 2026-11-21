# Maršál a špión

A two-player hidden-army board game in Rust, with a native **Bevy 0.18** client and an authoritative TCP server. Rules and original scanned graphics come from [`mas.pdf`](mas.pdf), especially pages 4–5. The complete 10×11 board and original piece references come from [`tmp/mas-plan.pdf`](tmp/mas-plan.pdf). The game uses a cleaned restoration with blank count areas; a [restored PDF copy](tmp/mas-plan-restored.pdf) preserves the original page size. See [restoration details and prompt](assets/board-restoration.md). No database, accounts, save files, or persistent matches: restarting the server removes everything.

## Run

Install a current stable Rust toolchain (tested with Rust 1.95). The graphical client needs a desktop session and a Vulkan/Metal/DX12-capable graphics driver. On Linux, install Bevy's [native build dependencies](https://bevy.org/learn/quick-start/getting-started/setup/).

Generate the private build key once, then build both friend-distribution binaries from the same checkout:

```sh
python3 tools/generate_shared_key.py
cargo build --release --bins
```

The generator creates `.secrets/multiplayer.key` (32 OS-random bytes, owner-only permissions), refuses to overwrite an existing key, and never prints it. Optionally, run `python3 tools/generate_shared_key.py 'your-password'` to derive the 32-byte key as SHA-256 of the password's exact UTF-8 bytes instead. The same password produces the same key. Remove the existing key first when regenerating, then rebuild both binaries. Git ignores `.secrets/` and `target/`. `build.rs` embeds the same key in both binaries; builds fail if it is missing or not exactly 32 bytes. Distribute `target/release/mas-client` to friends and run the matching `target/release/mas-server`; no separate key file is needed at runtime. Keep the local key for matching rebuilds. For builds elsewhere, set `MAS_SHARED_KEY_FILE` to a securely supplied 32-byte file outside source control. CI generates its own disposable test key, not a distribution key.

From this directory, start the server:

```sh
cargo run --release --no-default-features --bin mas-server -- config.toml
```

Start a client on each player's computer:

```sh
cargo run --release --bin mas-client
```

Enter the server address (`127.0.0.1` for local play, or the server machine's address for LAN/remote play). Port 4000 is used when omitted; append `:port` to use a different port. **Waiting games** lists rooms with one player; click a game to join directly, without typing its name. Click the red **Connect** button to authenticate and keep a lobby connection open. **Connecting…** cancels an attempt when clicked; the green **Connected** button disconnects. The server immediately sends the current games and pushes changes over that same connection, without polling. Disconnecting clears the list. The address is editable only while disconnected, and **Enter** is enabled only while connected. Drag the **Waiting games** header to move the panel within the window. Previous/Next buttons page through longer lists. To create a game, enter a new name below and press Enter. Tab switches fields. You can also join by entering an existing game name. Names are case-sensitive and surrounding whitespace is trimmed. The room starts deployment as soon as both players connect. Games that start, expire or disappear cannot be joined from an outdated list.

You can also prefill and join directly:

```sh
cargo run --release --bin mas-client -- --server 192.168.1.20:4000 --room "Prague battle"
```

Allow incoming TCP on port 4000 on the server machine when playing remotely. `config.toml` controls the bind address and limits. The server uses plaintext TCP; this version has no accounts or reconnect tokens.

## Play

The game window starts at 600×925 and is **resizable**. The info bar stays 84 logical pixels high. Below it, the **complete printed plan** scales uniformly at the exact aspect ratio of `tmp/mas-plan.pdf` (841.68:1178.88). Extra space becomes margins, so the artwork is always fully visible with no cropping or stretching. Pieces, counts, highlights, and mouse targets scale together; the bar text and timer retain their size. Both original piece legends, all eleven rows, and both flag panels retain their positions and proportions. There is no side panel. The entire plan rotates for commander two, so each player has their own upright piece strip below the board.

- During deployment, click a piece **in the printed bottom strip**, then left-click an empty square in your bottom four rows. The selected card has a thick, vivid blue border outside its artwork. Remaining counts update on the cards; exhausted ranks dim. Right-click a placed piece to return it to the strip.
- Deployment locks automatically when your fortieth piece is placed. Play begins once both armies are ready. Both players deploy simultaneously; the opponent's setup remains hidden. At the start of play, the counters reset to the full army counts: your remaining pieces appear on the bottom strip and your opponent's on the top strip. Each captured piece reduces its owner's rank count; equal ranks reduce both armies. Counts stay visible at the end of the game.
- Setup defaults to **120 seconds**, starting when player two joins. At expiry, remaining pieces are placed and both armies are locked automatically.
- Select one of your moving pieces on the board, then click a destination outlined in green. Click the selected piece again to deselect. Red and ivory correspond to the original armies; the opponent's hidden pieces have blank card backs. All moves animate on both boards, including long scout moves: the piece slides to the destination in 180 ms. During captures, the losing piece fades out over 220 ms. Equal ranks both fade. Both players use the same server move event and elapsed time; repeated updates do not restart it. Board clicks pause during the short animation.
- The top bar shows two clock boxes: **Your time** and **Opponent's time**. The active player's delay appears in parentheses immediately beside their main time, for example `3:00.0 (10.0s)`. That player's box is highlighted and its heading identifies the turn delay. Each player starts with **3 minutes** of main time; every turn starts with a **10-second delay**. Both main clocks pause during the delay, then only the active player's main clock runs. Unused main time carries over. The separate setup clock remains visible during deployment.
- When the active player's main time reaches zero, their opponent gets **Claim victory**. Play continues until a valid claim or legal move reaches the server first. A legal move removes that turn's claim opportunity and starts the next turn with a full delay. A player already at zero main time still receives the full delay on every later turn; the opponent can claim again only after that delay expires. Invalid moves do not reset either clock or delay.
- Capture the enemy flag to win. All remaining pieces are revealed, and the top bar shows **You win!** or **You lose!** with an **OK** button. Click OK (or press Enter/Space) to return to the main screen. Right-click the info bar to resign; Escape also leaves the game. For other completed results, click the info bar to return to game-name entry. Choose a new name for a rematch. Everything needed for deployment, moves and the result is available on the board and its info bar.

Setup time, `turn_delay_seconds` (default 10), `main_time_seconds` (default 180 per player), the quiet-move draw threshold, waiting-room lifetime and capacity limits are configurable in [`config.toml`](config.toml). The server validates configuration at startup. Pass a different TOML path as its first argument.

## Rules and interpretation

See [`RULES.md`](RULES.md) for the translated rules, exact army inventory and decisions made where the scan is ambiguous. This follows the Czech game rather than assuming ordinary Stratego rules: **diagonal movement is allowed, diagonal attacks are forbidden, and a spy beats a marshal in either direction**. Only the attacker learns the defender's rank; a surviving attacking piece stays secret from the defender.

## Check the implementation

```sh
cargo test --locked --no-default-features
cargo test --locked --bin mas-client
cargo check --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
```

Rules, session, clock and geometry tests include randomized matches, private JSON views, server clocks, inventory conservation, combat exceptions, room isolation and cleanup, exact printed-cell and card picking in both orientations, and a complete deployment/move through those coordinates. Client tests verify automatic Ready, join/leave window sizing, both main clocks, live turn delays, turn-bound claim controls, initial/placed/returned reserve counts, preserved user resizing, every printed card’s clicks, highlight bounds and count anchors after wide/tall and high-DPI resizes, that every game phase fits inside the full sheet and fixed-height clock bar, the disconnect dialog’s input blocking and return to the main screen, bold flag-capture results with OK acknowledgement, paginated one-click lobby joins with stale-response handling, ordinary/scout movement plus capture fading across outcomes, perspectives and resizes, all embedded images/fonts loading without an asset directory, and both armies' remaining counters after deployment and captures. Geometry checks cover the PDF aspect ratio, uniform scaling, rotated card/cell picking at several sizes, and ignored clicks in the margins. These tests do not need a GPU, display, or sockets.

The live TCP integration test starts the actual server and connects several clients. It checks waiting-game discovery and direct joining, matchmaking, third-player rejection, private deployment, moves, matching capture events for both players, automatic setup, clock expiry, legal moves after expiry, stale-claim rejection, claimed time wins, and interrupted games on disconnect:

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

## Standalone Windows client

The client embeds the board, cover, all eleven piece images, and both fonts into the executable. It loads these from Bevy's in-memory asset source on every platform. No `assets` directory, configuration file, font installation, or separate key file is needed beside the Windows EXE. `mas-client.exe --licenses` displays the embedded font license. The release build uses the Windows GUI subsystem, so opening the client does not open a console window.

To cross-build **only the client** on x86_64 Linux:

```sh
./tools/build_windows_client.sh
```

On first use, the script downloads a pinned official [LLVM-MinGW UCRT toolchain](https://github.com/mstorsjo/llvm-mingw/releases), verifies its SHA-256, and caches it under the ignored `target/toolchains/` directory. It also installs the Rust Windows target if missing. Subsequent builds reuse the cached tools. No system packages or shell environment setup are needed. To use an existing toolchain instead, set `LLVM_MINGW_ROOT=/path/to/llvm-mingw`; a compiler already on `PATH` is also supported.

Use the same `.secrets/multiplayer.key` (or `MAS_SHARED_KEY_FILE`) as the server build. The script builds with static runtime libraries and strips the distribution copy. Give friends only **`target/windows/mas-client.exe`**, intended for 64-bit Windows 10/11 with a compatible graphics driver. No MinGW or Visual C++ runtime installation is required; Windows system libraries and GPU drivers are supplied by Windows/the graphics vendor.

To build natively on 64-bit Windows, install Rust's MSVC toolchain and the Visual Studio C++ build tools, provide the matching build key, and run in PowerShell:

```powershell
$env:RUSTFLAGS = '-C target-feature=+crt-static'
cargo build --locked --release --no-default-features --features client --bin mas-client
```

The native output is `target\release\mas-client.exe`. CI builds and tests the Windows client with a disposable test key; those builds do not use the distribution key. The embedded-asset test decodes every image and font with a deliberately missing filesystem asset directory.

## Server sandbox

The server requires 64-bit Linux (x86_64, aarch64 or riscv64), Linux 5.9+ with seccomp filtering enabled. It reads and validates configuration and binds the listening socket first, then irreversibly drops filesystem access before accepting players. Startup fails if sandbox setup fails; there is no unsandboxed fallback or disable switch. The graphical client is unaffected.

The server uses the `seccompiler` library to install a syscall allowlist with `no_new_privs` and thread synchronization. Current and future worker threads are confined. File opens, directory listing, path metadata, file creation/modification/deletion, file-backed memory mappings, process execution, new sockets, descriptor imports and io_uring are blocked. Only the existing TCP listener, accepted connections, runtime memory/thread operations, clocks and OS randomness remain available.

Inherited descriptors are closed except for the listener and safe logging streams. Standard input becomes `/dev/null`; stdout/stderr remain usable when they are terminals, pipes or sockets. If either is a regular file, it is replaced with `/dev/null`, so even redirected disk logs cannot retain filesystem access. For file logging, pipe output to a separate process that owns the file, for example `target/release/mas-server config.toml 2>&1 | tee server.log`.

The subprocess sandbox tests verify denied file reads/writes, creation, metadata, directory listing, rename and deletion, closed inherited disk descriptors, neutralized file-backed standard streams, blocked executable/async-I/O bypasses, and enforcement on both existing and newly spawned threads. The full TCP integration test exercises authentication, lobby discovery, gameplay, capture events, timers and disconnects against the sandboxed server.

## Authenticated transport

All TCP connections, including waiting-game discovery, authenticate before joining a room or receiving game data. The server sends only a plaintext `MAS\x01` marker and a cryptographically random 64-bit session number before authentication. Both sides enforce a three-second total authentication deadline. Unauthenticated peers count toward connection limits but never occupy a player slot.

Each binary frame contains, in order: protocol version (`u16`), session (`u64`), direction (`u8`, 1=client→server, 2=server→client), sequence (`u64`), message type (`u8`, 1=proof, 2=acknowledgement, 3=action, 4=reply), payload length (`u32`), exact JSON payload bytes, and a full 32-byte HMAC-SHA-256 tag. Integers use big-endian encoding. The MAC covers `MAS-HMAC\0`, the entire 24-byte header, and the exact payload bytes; it uses the crypto library’s constant-time `verify_slice` API. Payloads remain plaintext.

Client proof and server acknowledgement have empty payloads and use sequence 1 in their respective directions. Application traffic continues at sequence 2, with independent ordered counters. Receivers verify version, session, direction, type, exact next sequence, length and HMAC before deserializing data; counters advance only after successful verification and never wrap. Reconnects use a new OS-random session and fresh counters. Invalid authentication, malformed frames, tampering and replays close the connection. Legacy unauthenticated clients cannot join.

The live TCP test checks valid authenticated discovery/joining, wrong-key and unauthenticated admission rejection, admission timeout, altered payloads, duplicate proofs and actions, and proofs/actions replayed on later connections. Unit tests cover every authenticated header/payload/tag byte, truncation, size limits, direction reflection and counter exhaustion. With `network-tests`, the actual client networking worker also browses and joins using the embedded key.

## Layout

- `src/lib.rs`: deterministic rule validation, combat, player-specific views and wire types; random deployment and starting player.
- `src/session.rs`: in-memory named-room coordinator shared by tests and server.
- `src/sandbox.rs`: irreversible Linux server confinement and inherited-descriptor cleanup.
- `src/plan.rs`: shared source-PDF geometry, exact-size window layout, rotated cell and printed-piece picking.
- `src/net.rs`: bounded newline-delimited JSON framing.
- `src/bin/server.rs`: single game-state owner with bounded I/O queues, per-connection readers/writers, monotonic clocks, limits and disconnect cleanup.
- `src/bin/client.rs`: Bevy 2D interface, game-name entry, deployment, board interaction, clocks and field reports. Network work runs off the render thread.
- `assets/`: the complete printed PDF sheet in `board.png` (including both piece strips), solid-black transparent piece PNGs with traced SVG masters in `assets/vectors/`, cover artwork and a bundled DejaVu font with its license. Bevy loads this directory relative to the compiled source location; retain it when running the binaries.
- `tools/extract_assets.py`: preserves the restored board (recovering its exact pixels from the restored PDF if missing) and exports piece PNGs from the committed SVG masters. It preserves the upright, AI-restored cover, with an upright raw-PDF fallback if that asset is missing. See [cover restoration and prompts](assets/cover-restoration.md). Requires Poppler (`pdftoppm`), librsvg (`rsvg-convert`), and Python Pillow.
- `tools/export_board_pdf.py`: exports the cleaned board losslessly at the original PDF page size.
- `tools/check_board_alignment.py`: verifies grid registration against the original PDF; the report is in `assets/board-alignment.json`.
- `tools/trace_pieces.py`: reproduces the SVG traces from the PDF's original embedded scan, using Potrace, Poppler (`pdfimages`), librsvg, and Pillow. The corporal uses the intact red-side reference. See [`assets/vectors/README.md`](assets/vectors/README.md).

The authenticated binary frames described above carry JSON payloads: actions use a `type` field (`join`, `join_waiting`, `list_games`, `place`, `remove`, `auto_place`, `ready`, `move`, `claim_victory`, `resign`); replies are `state`, `games` or `error`. Coordinates are indices `0..109`, with index `y * 10 + x` on the 10-column, 11-row board; A11 is index 0 and J1 is index 109. The six blocked squares are D5–D7 and G5–G7; each commander deploys in their four back rows, leaving three neutral rows. During deployment, placements and removals are visible to the opponent as unrevealed pieces. Enemy ranks remain hidden during deployment and play except for the existing combat reports and captured pieces; all remaining pieces are revealed at match end. It records captured ranks and keeps only eight private field reports per player. Updates arrive at least every 250 ms, with immediate broadcasts for state changes such as claim availability. Clock snapshots contain both main balances, the active delay and authoritative claim eligibility; displays interpolate locally. `claim_victory` carries `turn_id`, which must equal the current state's `ply`. The server settles time before processing each action and rejects claims for any earlier turn. Expiry never automatically passes a turn or ends a game. Rebuild and distribute matching server/client binaries for this clock protocol.

Rooms disappear when their last connection leaves. A waiting room expires after its configured lifetime; its occupant sees the result and can leave. A disconnect interrupts an unfinished game without a winner. The remaining player sees a “Broken pike” dialog; OK returns to the main screen. Losing the server connection during a game shows the same dialog. Finished rooms stay available to their occupants until they leave. Unjoined connections time out after 30 seconds. Oversized/malformed frames, excessive command rates and stalled output consumers are disconnected. No game state is written to disk.

### Headless LLM player (Linux)

Build the headless client without Bevy or graphics dependencies:

```sh
cargo build --release --no-default-features --features ai-client --bin mas-ai-client
cp config-ai.example.toml config-ai.toml
chmod 600 config-ai.toml
# Edit hostname, port, game, openai_api_key and model in config-ai.toml.
./target/release/mas-ai-client config-ai.toml
```

The configuration path is the only command-line argument. For OpenAI, set `model` to a Responses API model that supports structured JSON output and supply `openai_api_key`. The implementation follows the [Responses API](https://developers.openai.com/api/reference/resources/responses/methods/create) and [structured outputs](https://developers.openai.com/api/docs/guides/structured-outputs) formats. The selected model decides deployment placements, readiness, moves, resignations and available time claims from numbered legal actions. OpenAI API usage is billed to the configured API key. No real API request is needed for the automated tests.

To use a private OpenAI-compatible server, set the **top-level** `model = "local"` and add:

```toml
# These fields stay at the top level, before [local].
hostname = "127.0.0.1"
port = 4000
game = "Local AI match"
model = "local"

[local]
baseUrl = "http://localhost:8000/v1"
apiKey = "YOUR_PRIVATE_API_KEY"
model = "recommended-coder"
timeoutSeconds = 180
```

`local.model` is the actual model ID; top-level `model = "local"` only selects the provider. The local API key replaces the OpenAI key, so `openai_api_key` can be omitted in local mode. Use `apiKey = ""` for an unauthenticated server. `baseUrl` includes the API prefix (such as `/v1`); the client appends `/chat/completions`. `timeoutSeconds` sets the local request deadline (default 180 seconds, range 1–3600); connection establishment is limited to 10 seconds. OpenAI requests retain their 60-second deadline. Transport failures print elapsed time, configured limits and the redacted error chain, distinguishing timeouts from connection failures instead of merging them into one message. Private requests use [Chat Completions](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create) system/user messages, non-streaming responses, and a JSON-only action instruction, without requiring optional server-side schema extensions. Returned IDs still undergo strict JSON parsing, legality and turn checks, with invalid answers retried. The same filtered view and observation history are used for both providers. HTTPS uses the system's trusted certificate authorities, including installed private CAs, and redirects are disabled. Selecting an OpenAI model again restores the OpenAI endpoint; there is no automatic fallback between providers.

The AI uses the same embedded multiplayer key, authentication and persistent connection worker as the graphical client. Rebuild the server too: the protocol adds a create-only action and public match settings. It creates the configured game, refuses an existing name, and waits for server-pushed opponent/game updates. It keeps receiving updates during API requests, rechecks decisions before sending them, discards stale turns, and retries invalid answers or API errors with a delay. There are no local strategic fallback moves. On completion it logs win/loss/draw; connection errors use fixed diagnostics; AI requests and response bodies are logged to the terminal with API credentials redacted. A disconnected client exits; it does not reconnect into another player's slot.

If authentication succeeds but the connection closes immediately after the game-creation request, check that the server on the configured host has been rebuilt **and restarted**. A server built before the AI client does not recognize `create_game` and closes its authenticated connection. Rebuilding only the client, or rebuilding the server without restarting its existing process, is insufficient. The client distinguishes sending the request from receiving confirmation and reports the connection failure category without logging raw protocol data.

Every model request is built from a separate observation object, never a serialized server reply. Both clients share an explicit rank-visibility filter. Opponent deployment positions are visible as hidden pieces, unrevealed opponent ranks remain `null`, and undisplayed deployment counters remain `"?"`. Public remaining counters are derived from captures in the same way as the GUI. The AI receives the public rules, actual match settings, own ranks, visible clocks and filtered combat observations. Combat ranks enter memory only when the GUI would animate them, or when the latest visible combat-detail line legitimately identifies a rank. Completed/missed animations cannot reveal new ranks. The client respects the GUI's animation input pause. History records observations at the time/square they occurred; it does not turn remembered ranks into guaranteed current board information.

AI requests use a compact board array, numeric rank IDs and compact actions with explicit IDs. Fixed rules and the rank/encoding legend are sent in a stable first developer message (system message for the private provider), ahead of changing game state. The OpenAI output schema is also stable between turns. This makes common prefixes eligible for [prompt caching](https://developers.openai.com/api/docs/guides/prompt-caching), subject to the provider's minimum prefix length and cache availability; the client does not pad prompts just to trigger caching. It sends a fresh filtered state rather than growing an increasingly expensive conversation. Ready and time claims are sent without an API call when they are the only available interface action; strategic moves and resignations remain model-controlled.

OpenAI cost controls are **top-level fields before `[local]`**:

```toml
max_output_tokens = 4096
# Optional override for a reasoning-capable OpenAI model:
reasoning_effort = "low"
```

GPT-6 models default to `low` reasoning; other models retain their provider default unless an explicit override is configured. The output-token limit defaults to 4096 and accepts 64–32768. It includes hidden reasoning, so excessively small limits can produce incomplete, billable responses and retries. Lower reasoning can reduce playing strength; raise it to `medium` or `high` if desired. These two settings are not forwarded to private providers, whose extension support varies. Responses with usage data print per-request and cumulative session input, cached-input, output and reasoning counts. Reasoning is a subset of output, and cached input is a subset of input: do not add those subsets twice. Dollar cost depends on the chosen model and caching, so no fixed per-game price is claimed. For the deployment/play fixtures, compact prompts reduced prompt bytes by roughly 65–71%; actual token and monetary savings must be measured with provider usage.

`config-ai.toml` is ignored by Git. Keep any other configuration filename private as well. The API key is used only in the HTTPS authorization header; it is never included in prompts, logs, configuration diagnostics or server messages. Every selected-provider request and response body (including API error responses) is printed to the terminal with labeled boundaries and HTTP response status. Credentials echoed in a response are redacted, and authorization headers are never printed. Free-form server text is not logged or replayed into model requests. OpenAI requests set `store = false` and use the public OpenAI HTTPS endpoint. Local requests go only to `local.baseUrl`; neither configuration section is included in model prompts.

Run privacy and deterministic tests without sockets, then the Linux TCP/HTTP mock tests:

```sh
cargo test --no-default-features --features ai-client
cargo test --no-default-features --features ai-client,network-tests
```

The tests inject unrevealed enemy ranks and compare the complete observation/action payloads and decision identities, verify short-lived legitimate reveals and unknown counters, exercise stale moves/claims, drive deployment and a move through the authenticated connection, and inspect an actual HTTP request against a local mock of the OpenAI endpoint. Mock error bodies and malformed configuration contain a sentinel credential to check that diagnostics do not echo it.
