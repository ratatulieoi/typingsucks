# typingsucks

Hold-to-talk voice transcription for Linux. Press a hotkey, speak, release — text appears at your cursor.

## Install

Download the latest binary from [Releases](https://github.com/ratatulieoi/typingsucks/releases), then:

```bash
chmod +x typingsucks
./typingsucks
```

Your user must be in the `input` group for hotkey detection (the app can fix this for you on first launch).

## Usage

```bash
typingsucks          # Opens settings GUI
typingsucks daemon   # Runs headless (after configuring)
```

### Transcription modes

- **Local** — Downloads a Whisper model (tiny to small) and runs offline. No API key needed.
- **API** — Sends audio to any OpenAI-compatible speech-to-text endpoint. Works with [Groq](https://console.groq.com/) (free tier, use `https://api.groq.com/openai` as the API URL).

## Build from source

Requires Rust toolchain and system libs:

```bash
# Debian/Ubuntu
sudo apt install cmake pkg-config libclang-dev libasound2-dev \
  libwayland-dev wayland-protocols libxkbcommon-dev \
  libx11-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libudev-dev

# Arch
sudo pacman -S cmake clang alsa-lib wayland wayland-protocols libxkbcommon libx11 libxcb

cargo build --release
# Binary at target/release/typingsucks (~16MB stripped)
```

## License

MIT
