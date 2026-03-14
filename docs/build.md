# Build & Deploy

## Desktop app

```bash
# Dev run (builds if needed)
cargo run

# Release build
cargo build --release
./target/release/dj-rs
```

The server starts automatically on `0.0.0.0:7879` when the app launches.

## Tizen TV app

Prerequisites: Tizen Studio installed at `~/tizen-studio`, cert profile `djrs-dev` active.

```bash
cd tizen-app

# 1. Build & sign the .wgt package
bash build.sh

# 2. Make sure the TV is connected via sdb
sdb connect 192.168.1.44
sdb devices   # should show QE55Q8DNA

# 3. Install on TV
~/tizen-studio/tools/ide/bin/tizen install -n dj-rs.wgt -t QE55Q8DNA
```

The cert password is in `tizen-app/.env.tizen` (gitignored).

## Cert profile (one-time setup)

If the `djrs-dev` profile or password files are missing:

```bash
# Author cert password file
echo -n "<your-author-cert-password>" > ~/tizen-studio-data/keystore/author/djrs-author.pwd

# Distributor cert password file
mkdir -p ~/tizen-studio-data/tools/certificate-generator/certificates/distributor
echo -n "<tizen-distributor-signer-password>" \
  > ~/tizen-studio-data/tools/certificate-generator/certificates/distributor/tizen-distributor-signer.pwd
```

## Server endpoints

| Endpoint | Description |
|---|---|
| `GET /ping` | Discovery — returns `{"service":"dj-rs"}` |
| `GET /ws` | WebSocket — metadata/position/state/stream events |
| `GET /stream/:id?seek=N` | Audio stream — AAC 256k via ffmpeg |
