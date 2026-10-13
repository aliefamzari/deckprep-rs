# DeckPrep RS

DeckPrep RS is a Raspberry Pi 4 cassette-recording workstation, rebuilt in Rust from
the original Python `deckpreprec` application. Its interface is designed on a fixed
320x240 canvas for eventual 240p60 output to a Sony PVM.

The current milestone provides:

- SDL2 rendering with integer scaling
- FFprobe-backed track discovery and FFplay preview
- Scrolling track browser with long-name marquee and scrollbar
- Keyboard navigation and track selection
- Desktop, Pi, and inactive GPIO/SCART runtime profiles
- Static, linear, calibrated, and reel-physics tape counters
- Session capacity and track timing calculations

Audio normalization and recording-session playback are the next milestone. DSF/DFF
support is intentionally deferred.

## Development on WSL2

Install Rust, SDL2 development files, and FFmpeg. Then run:

```bash
cargo run -- profiles/desktop-dev.json
```

Create a reusable profile interactively:

```bash
cargo run -- --create-profile profiles/my-deck.json
```

Place MP3, WAV, FLAC, WebM, M4A, AAC, or OGG files in `tracks/`, or change
`tracks_folder` in the selected runtime profile.

Controls:

- `Up`/`Down` or `K`/`J`: move through tracks
- `Page Up`/`Page Down`, `Home`/`End`: move through long libraries
- `Space`: select or deselect
- `C`: clear all selected tracks
- `P`: play or pause the highlighted track; navigation does not change playback
- `Tab`/`V`: open the dedicated track/queue workspace
- `Left`/`Right` or `H`/`L`: seek backward/forward 10 seconds
- `X`: stop preview
- `N`: normalize all selected tracks into the configured cache folder
- `Ctrl+S`: save the selected queue under `queues/`
- `Ctrl+O`: browse saved queues; use `Up`/`Down` and `Enter` to load
- `S`: save the active runtime settings as a reusable profile snapshot
- `W`: open the in-app profile editor; `Ctrl+S` saves changes and `Escape` closes it
- `Enter`: validate, normalize, countdown, and start the prep session
- After normalization: review the prepared tracks, total session timing, tape side capacity, and remaining headroom (`REMAINING +/-S` in seconds); press `Enter` to continue or `Escape` to go back
- In review: `Up`/`Down`, `Page Up`/`Page Down`, `Home`/`End` navigate; `P` previews the highlighted track
- `Q` during prep: cancel the countdown or playback session
- Tracklists are written beside the source tracks as `deck_tracklist_<timestamp>.txt`
- `Enter`: toggle preparation state
- `Q` or `Escape`: quit
- `?`: open the contextual control legend; `Escape` or `?` closes it

Counter settings live in the runtime profile: `linear` uses elapsed seconds,
`static` uses `counter_rate`, `auto` simulates reel-radius changes, and `manual`
interpolates checkpoints from `counter_config`.

Press `W` in the application to open the profile editor. Use `Up`/`Down` to select
a field, `Left`/`Right` to change numeric and enum values, and type to edit text
fields. `Backspace` deletes text. Press `Ctrl+S` to save and close, or `Escape` to close
without saving.

Validate changes with:

```bash
cargo fmt --check
cargo test
cargo check
```

## Runtime Profiles

- `profiles/desktop-dev.json`: 3x window for WSLg and ordinary monitors
- `profiles/pi-production.json`: fullscreen DRM/KMS target with ALSA audio
- `profiles/pi-gpio-scart.json`: inactive design placeholder only

The GPIO/SCART profile does not configure GPIO pins, DPI overlays, sync, or 15 kHz
timings. Those settings must be added and tested against the chosen interface hardware.

Preview playback and prep playback use normalized cached WAV files. The current prep
workflow validates tape capacity, runs the countdown and leader gap, then plays the
selected tracks with configured gaps. A dedicated counter/VU recording screen and
tracklist export are included. The recording view shows a linear counter, progress,
current track, queue, and stereo waveform-synchronized VU data.