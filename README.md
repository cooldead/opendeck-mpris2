# MPRIS2 Media for OpenDeck

Linux media controls for OpenDeck, with Strawberry selected by default and a
shared player selector for all plugin controls. Derived from OpenAction MPRIS 1.4.0.

The original plugin already uses MPRIS2, but chooses the first player returned
by D-Bus. When browsers or other players are running, that can control the wrong
application. This project uses explicit selection and periodically refreshes
state so players can start, stop, and restart while OpenDeck remains open.

## Controls

- Album Artwork: live cover display and shared player dropdown in its settings
- Play/Pause, Stop, Previous, Next
- Repeat: None → Playlist → Track → None
- Shuffle
- Seek backward/forward by 10 seconds
- Volume down/up by 5 percentage points, clamped to 0–100%

Play/Pause uses two states: **Playing** (showing the pause symbol)
and **Paused / Stopped** (showing the play symbol). Text is hidden by
default. Enable **Show playback status text** in that button's settings if wanted.
Repeat cycles through **Off → Playlist → One Song → Off**, using distinct
off, playlist, and single-song icons. Shuffle has separate on/off icons.
Both controls use icons only, without text labels, and follow the selected player. Other transport buttons
show `Offline` when the player is unavailable; failed commands show an alert.

The separate Album Artwork button reads `mpris:artUrl`, supports local file URLs
(including escaped paths), HTTP/HTTPS, and base64 image data. PNG, JPEG, GIF, and
WebP covers are supported up to 8 MiB. Covers are cached and refreshed when the
player, track, URL, or local file changes. Missing/unreadable artwork restores
the record icon. The artwork button does not trigger playback when pressed;
select it in OpenDeck to access the player dropdown.

## Large album cover: 2×2 or 3×3

Place four adjacent **Album Artwork** buttons for 2×2, or nine for 3×3, and leave
**Artwork layout** set to **Auto** (the default). The plugin reads each button's
deck position and assigns its tile from left to right, top to bottom.

```text
2×2          3×3
1 2          1 2 3
3 4          4 5 6
             7 8 9
```

Automatic detection works separately on each connected Stream Deck and supports
multiple adjacent grids. Incomplete groups remain full-cover buttons. Choose **Single button** to always show the full cover, or select a manual 2×2
or 3×3 layout and tile position if you want a specific arrangement.
Artwork buttons on different profiles are detected from the visible grid only.

Non-square artwork is cropped centrally to a square, then divided into equal
144×144 PNG tiles. Physical spaces between buttons remain visible; this version
does not compensate for button gaps. Missing artwork restores the fallback icon
on each button. Tiled covers use a static frame for animated images. Tile decoding
is bounded to 8192 pixels per dimension and 128 MiB of decoder allocations;
unsupported/oversized covers show the fallback icon. Tile images are cached per
cover and layout, and refreshed when settings or the source cover change.

## Build and install

Requires Linux, a current stable Rust toolchain, Python 3, and OpenDeck.
The plugin itself is a native executable: it does not need Python, Node.js,
playerctl, or Rust installed on the destination machine.

```sh
python3 scripts/package.py
```

The resulting `dist/opendeck-mpris2-0.4.0-<architecture>.streamDeckPlugin` is a
ZIP archive for OpenDeck's plugin import. Native x86_64 and aarch64 GNU/Linux
builds are supported; each archive contains one architecture. Local builds
require a destination with a compatible glibc. GitHub Actions builds on Ubuntu
24.04 for wider compatibility and uploads packages as workflow artifacts.

For manual installation, close OpenDeck, copy the generated
`dist/<architecture>/com.cooldead.mpris2.sdPlugin` directory into
`${XDG_CONFIG_HOME:-$HOME/.config}/opendeck/plugins/`, and reopen OpenDeck.
For a Flatpak installation, use OpenDeck's import flow and ensure its sandbox
can talk to your media player's session D-Bus service.

Find **MPRIS2 Media** in the action list and drag new buttons onto the deck.
The plugin has its own ID (`com.cooldead.mpris2`); existing **Linux Media**
buttons still belong to the original plugin and must be replaced to use this
one. Both plugins can be installed side by side.

## Player selection

Select an **Album Artwork** or **Play/Pause** button in OpenDeck, then choose
**Media player — all plugin controls** from its dropdown. It lists running MPRIS2
players using their reported names; instance names distinguish multiple copies.
The list refreshes automatically while the settings panel is open, and there is
a **Refresh players** button. Closed players reappear once started. The selected
player remains in the list as unavailable if it closes.

Changes take effect across **every control in this plugin**, on every profile,
and persist through OpenDeck/plugin restarts. Existing per-button player values
from version 0.1 are ignored. A new global configuration defaults to Strawberry.
Buttons keep their UUIDs, so existing MPRIS2 Media controls continue working.
Add an Album Artwork button to use the new artwork display.

**Automatic (prefer playing)** prefers a playing player, then Strawberry, then
alphabetical bus-name order. An explicit selection never falls back to another
application. Dropdown entries use exact running bus names; the default
`strawberry` selector also recognizes instance-suffixed Strawberry names.

Strawberry must be running with MPRIS2 enabled, in the same user's desktop
session as OpenDeck. Its documented service is
`org.mpris.MediaPlayer2.strawberry`:
https://wiki.strawberrymusicplayer.org/wiki/Using_MPRIS2

## Troubleshooting

Run these from a terminal in your desktop session:

```sh
./target/release/opendeck-mpris2 --list-players
./target/release/opendeck-mpris2 --diagnose strawberry
```

These commands only read player information. If Strawberry is missing, check
that it is running and its MPRIS2/D-Bus integration is enabled. If the diagnostic
works but buttons do not, check that you added **MPRIS2 Media** actions and
restart OpenDeck after installing. The plugin log normally lives at
`~/.local/share/opendeck/logs/plugins/com.cooldead.mpris2.sdPlugin.log`.

State refresh runs once per second when visible buttons exist. Individual
command and refresh operations have timeouts; a failed player does not terminate
the plugin. Optional repeat/shuffle properties may be unsupported by some
players, in which case their icons use the default state.

## Development and validation

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
dbus-run-session -- cargo test --locked -- --ignored
node --test tests/inspector.cjs
```

The ignored tests require local sockets and an isolated session bus. They test
all ten commands, volume bounds, player selection and restart recovery, plus
OpenDeck registration, restored/saved global selection, routing across existing
buttons, artwork display/fallback, play/pause symbols, optional status text, and
failure alerts. Unit tests cover escaped local artwork URLs and cache refresh.
The artwork settings default to automatic placement, with manual grid/position controls as a fallback.
Layout tests detect 2×2, 3×3, and adjacent grids from deck positions. Tile tests
reconstruct both grids pixel-for-pixel and check square cropping,
layout changes, cover changes, and missing covers. Tests do not
change desktop playback. Physical deck interaction and inspector appearance
still need a manual check.

## Publish to GitHub

The project is a local Git repository. Review and commit the source, then create
an empty GitHub repository and push:

```sh
git add .
git commit -m "Initial MPRIS2 media plugin"
git remote add origin https://github.com/YOUR_USERNAME/opendeck-mpris2.git
git push -u origin main
```

The included workflow builds and tests both supported architectures. Download
its artifacts and attach the `.streamDeckPlugin` packages to a GitHub release.
No repository or release is created automatically by the local build.

## License and attribution

MIT; see [LICENSE](LICENSE) and [NOTICE.md](NOTICE.md). The starting plugin distribution is by nekename / Aman Khanna; current button
icons were supplied by the project owner. This project is
independent of [OpenDeck](https://github.com/nekename/OpenDeck) and Strawberry.
