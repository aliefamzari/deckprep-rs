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

The app remembers the last successfully loaded or saved profile in the local,
Git-ignored `.deckprep-state.json` file. Run `cargo run` without a profile argument
to restore it. An explicit profile argument overrides the remembered choice and
becomes the new default for next time. If the remembered profile is missing or
invalid, the app falls back to `profiles/desktop-dev.json`.

Create a reusable profile interactively:

```bash
cargo run -- --create-profile profiles/my-deck.json
```

Inspect and diagnose audio hardware, ALSA sound cards, and USB DACs:

```bash
cargo run -- --audio-diagnostics profiles/pi-production.json
```

Place MP3, WAV, FLAC, WebM, M4A, AAC, or OGG files in `tracks/`, or change
`tracks_folder` in the selected runtime profile.

Controls:

- `Up`/`Down` or `K`/`J`: move through tracks
- `Page Up`/`Page Down`, `Home`/`End`: move through long libraries
- `Space`: select or deselect
- `C`: clear all selected tracks
- `1` / `2` / `3` / `4` / `5`: play calibration test tones (400 Hz level, 1 kHz ref, 10 kHz bias, 15 kHz azimuth, 20 Hz-20 kHz sweep)
- `P`: play or pause the highlighted track; navigation does not change playback
- `Tab`/`V`: open the dedicated track/queue workspace
- `Left`/`Right` or `H`/`L`: seek backward/forward 10 seconds
- `X`: stop preview
- `N`: prepare selected tracks using the profile's normalization mode, with live progress
- `Ctrl+S`: save the selected queue under `queues/`
- `Ctrl+O`: browse saved queues; use `Up`/`Down` and `Enter` to load
- `L`: browse and load deck profiles from `profiles/`; dynamically updates settings, tracks, and audio backend
- `S`: save the active runtime settings as a reusable profile snapshot
- `W`: open the in-app profile editor; `Ctrl+S` saves changes and `Escape` closes it
- `F2` in the profile editor: browse for the tracks folder
- `B` when the main library has no tracks: browse for a tracks folder and reload immediately
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

The normalization field cycles through `Peak`, `Lufs`, and `None`. `None` uses
the original files directly for preview and recording, without changing their
gain, converting them, or creating normalized cache files. Profiles store this
as `"normalization": "none"`; the terminal wizard also offers `none`.

Both `N` and preparation with `Enter` display the current track, normalization
mode, completed/total tracks, cache hits, and a track-based progress bar while
working in the background. `Escape`/`Q` cancels after the current track finishes.
Review identifies original tracks as `ORIG` and normalized tracks as `NEW` or
`CACHE`. Preparation failures return to the main page with an error message.

The tracks folder browser uses `Up`/`Down`, `Page Up`/`Page Down`, and `Home`/`End`
to navigate folders, `Enter`/`Right` to open one, `Backspace`/`Left` to go to its
parent, `Space` to use the current folder, and `Escape` to cancel.
Hold `Up`/`Down`, `J`/`K`, or
`Page Up`/`Page Down` to keep scrolling in folder, profile, and queue browsers;
opening and selecting remain single-press actions. Missing paths
start at their nearest existing parent. Audio files are loaded directly from the
chosen folder, not recursively. In the profile editor, the chosen path remains
unsaved until `Ctrl+S`, which also reloads the library. On the main page, press
`S` after browsing to save the new location as a profile snapshot.

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

Preview playback and prep playback use normalized cached WAV files unless normalization
is `none`, which plays the original files. The current prep
workflow validates tape capacity, runs the countdown and leader gap, then plays the
selected tracks with configured gaps. A dedicated counter/VU recording screen and
tracklist export are included. The recording view shows a linear counter, progress,
current track, queue, and stereo waveform-synchronized VU data.