# DeckPrep RS Project Status

Last updated: 2026-09-18

## Project

DeckPrep RS is the Rust replacement for the original Python `deckpreprec` cassette preparation tool. The target platform is Raspberry Pi 4 with keyboard input, an iBasso D-Zero USB DAC, and eventual 320x240p60 output to a Sony PVM 14M4E through a GPIO RGBS/SCART setup.

The repository is published at:

`https://github.com/aliefamzari/deckprep-rs`

The original Python implementation remains the behavioral reference in the sibling `deckpreprec` repository.

## Implemented

### UI and navigation

- Fixed 320x240 logical SDL2 canvas with integer scaling.
- Desktop development profile for WSL2/WSLg.
- Keyboard track navigation with Up/Down, J/K, Page Up/Down, Home, and End.
- Track selection and deselection with Space, and clear selection with C.
- Dynamic six-row track viewport with auto-scroll and scrollbar.
- Long-title marquee for the highlighted track.
- Dedicated track/queue workspace opened with Tab or V.
- Queue workspace with library and selected queue columns, synchronized highlighting, fixed title/duration columns, capacity totals, and queue scrolling.
- Contextual `?` legend popup for the main UI, review screen, and profile editor.
- Main-screen preview monitor with output meters and preview status.
- Main-screen footer is intentionally text-only and pixel-aligned after several icon/font experiments.

### Audio library and preview

- FFprobe-backed discovery for MP3, WAV, FLAC, WebM, M4A, AAC, and OGG.
- FFplay preview with P play/pause, X stop, and H/L or Left/Right seek by 10 seconds.
- Preview does not change automatically while navigating; P explicitly previews the highlighted track.
- Preview uses normalized cached WAV output.
- Stereo RMS analysis with 50 ms windows for main-screen and recording-mode meters.

### Normalization and recording workflow

The four intended stages are implemented:

1. Main menu and track selection.
2. Dedicated normalization review with durations, cache status, totals, capacity, queue navigation, and preview controls.
3. Cancellable preparation countdown and leader gap.
4. Dedicated recording mode with current track, queue, progress, configured counter, and stereo VU levels.

Additional workflow features:

- Peak normalization and LUFS normalization through FFmpeg.
- SHA-256 normalization cache keys based on source metadata and settings.
- Atomic normalized WAV cache writes under the configured normalized folder.
- Tape capacity validation before preparation.
- Sequential normalized playback with leader and track gaps.
- Timestamped tracklist export containing timing and counter positions.
- Profile-driven linear, static, auto reel, and manual checkpoint counter modes.

### Profiles and persistence

- JSON runtime profiles for desktop, Pi production, and inactive GPIO/SCART placeholder modes.
- Terminal profile wizard:

  ```bash
  cargo run -- --create-profile profiles/my-deck.json
  ```

- In-app profile editor opened with W.
- Numeric/enum profile editing with Left/Right.
- Text editing for profile name, track paths, normalized folder, audio device, and calibration file.
- Ctrl+S saves profile-editor changes; S saves an active profile snapshot outside the editor.
- Queue save dialog with typed queue names.
- Ctrl+S saves a named queue under `queues/`.
- Ctrl+O opens a queue browser; users choose a named queue, and missing tracks are reported.
- Queue selection order is preserved when loading and used for the prep session.

Local media and generated queue files are ignored by Git. Audio must be supplied locally in `tracks/` or via the profile's `tracks_folder`.

## Validation

The project currently passes:

- `cargo fmt --check`
- `cargo check`
- `cargo test` with 26 tests
- Desktop SDL GUI startup smoke test under WSLg
- Temporary WAV tests for FFprobe scanning, FFplay playback, FFmpeg normalization/cache reuse, meter analysis, and tracklist generation

The test environment does not validate actual Pi display takeover, GPIO timings, SCART signal integrity, or the iBasso D-Zero hardware path.

## Next work, in priority order

### 1. Fix workflow hardening

- Add overwrite confirmation/versioning when saving an existing queue name.
- Handle duplicate filenames/paths robustly when loading queues.
- Add stronger recovery when FFmpeg, FFplay, or a recording session is interrupted.
- Separate preparation, playback, and cancellation state from the main event loop.
- Add regression tests for UI state transitions and queue load/save behavior.

### 2. Finish audio hardware behavior

- Select and test the ALSA device string for the iBasso D-Zero on Raspberry Pi OS.
- Add test-tone generation for 400 Hz, 1 kHz, 10 kHz, 15 kHz, and sweep signals.
- Verify output latency and VU synchronization on the physical DAC.
- Add user-facing audio-device diagnostics.

### 3. Raspberry Pi deployment

- Produce a Raspberry Pi 4 release build, preferably for 64-bit Raspberry Pi OS Lite.
- Make the KMS/DRM backend explicit for the Pi production profile.
- Validate fullscreen display ownership through `/dev/dri` on a normal HDMI monitor first.
- Add a systemd service, permissions for DRM/input/audio devices, clean shutdown, and logging.
- Keep GPIO RGBS/SCART disabled until the exact adapter/pinout is confirmed.

### 4. CRT output

- Confirm the GPIO-to-SCART interface hardware and pinout.
- Validate RGB bit depth, sync polarity, blanking/switching voltages, and signal levels.
- Add and test a 240p60 timing/modeline around 15 kHz horizontal frequency.
- Calibrate overscan and geometry on the Sony PVM 14M4E.
- Do not assume DRM/KMS alone provides GPIO RGBS output.

### 5. Deferred formats

- Add DSF/DFF decoding to PCM only after the core Pi workflow is stable.
- Native DSD output is not a priority because the target workflow records analog audio to cassette.

## Useful commands

```bash
cd /home/almaz/deckprep-rs
source "$HOME/.cargo/env"
cargo run -- profiles/desktop-dev.json
cargo fmt --check
cargo check
cargo test
```

For a new profile:

```bash
cargo run -- --create-profile profiles/my-deck.json
```
