# Feature Parity Roadmap

## Foundation

- [x] Fixed 320x240 logical canvas with integer scaling
- [x] Keyboard navigation
- [x] Versionable runtime profiles
- [x] Tape counter modes: static, linear, manual, auto
- [x] Leader gap, track gap, capacity, and timeline calculations

## Audio Workflow

- [x] Scan and probe MP3, WAV, FLAC, WebM, M4A, AAC, and OGG files
- [x] Preview, pause, stop, seek, and explicit track switching
- [x] Peak and LUFS normalization with deterministic cache keys
- [x] Dedicated post-normalization review screen
- [ ] ALSA output-device selection for the iBasso D-Zero
- [ ] Real-time stereo VU meters synchronized to output
- [ ] 400 Hz, 1 kHz, 10 kHz, 15 kHz, and sweep test signals

## Recording Session

- [x] Track ordering and reusable selections
- [x] Dedicated track/queue workspace
- [x] Capacity validation
- [x] Preparation countdown and leader timing
- [x] Sequential playback with configured track gaps
- [x] Dedicated recording view with linear counter and progress
- [x] Timestamped tracklist generation
- [x] Profile-driven counter modes and manual calibration file loading
- [x] Save active deck profile snapshots
- [x] Guided deck profile creation wizard
- [x] In-app keyboard editor for timing, normalization, counter, and fullscreen settings
- [x] In-app text editing for profile name, paths, audio device, and calibration file
- [ ] Recovery and clear error reporting after interrupted playback

## Deployment

- [ ] Raspberry Pi 4 release build
- [ ] Fullscreen DRM/KMS validation
- [ ] ALSA USB DAC validation
- [ ] systemd launch and clean shutdown
- [ ] GPIO RGBS/SCART implementation after adapter selection
- [ ] Sony PVM 240p60 geometry and overscan calibration

## Deferred

- [ ] DSF/DFF decoding to PCM
- [ ] Native DSD output evaluation