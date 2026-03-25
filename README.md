# DAL — Download, Assess, Label

A self-hosted content review pipeline: queue video downloads from any browser, review them from any device, and rate them with a filename-based scoring system.

## Prerequisites

- [Rust](https://rustup.rs/) (1.75+)
- [Node.js](https://nodejs.org/) (20+)
- [yt-dlp](https://github.com/yt-dlp/yt-dlp)
- SQLite 3

## Quick Start

### 1. Rust API Server

```bash
cargo build --release
./target/release/dal --config dal.toml
```

The API server starts on port 8550 by default.

### 2. Next.js Frontend

```bash
cd web
npm install
npm run dev
```

The web UI starts on port 3000 and proxies API requests to the Rust server.

### 3. Firefox Extension

1. Open Firefox and navigate to `about:debugging`
2. Click "This Firefox" > "Load Temporary Add-on"
3. Select `clients/firefox/manifest.json`
4. Configure the server URL in the extension popup

## Configuration

Edit `dal.toml`:

```toml
[server]
host = "0.0.0.0"
port = 8550

[storage]
download_dir = "/mnt/media/videos"
database = "/var/lib/dal/dal.db"

[ytdlp]
binary = "yt-dlp"
max_concurrent = 1
extra_args = ["--merge-output-format", "mp4"]
```

## File Naming Convention

- Unrated: `Some Great Video.mp4`
- Rated: `85 - Some Great Video.mp4` (10-99 scale)
- Incomplete: `INCOMPLETE - Some Great Video.mp4`

Files are rated via the web UI. The rating prefix means any file browser sorting by name produces a rating-sorted list.

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/health` | Health check |
| POST | `/api/queue` | Queue a URL for download |
| GET | `/api/queue` | List queue entries |
| DELETE | `/api/queue/:id` | Cancel a pending download |
| GET | `/api/files` | List downloaded files |
| POST | `/api/files/:id/rate` | Rate a file (10-99) |
| DELETE | `/api/files/:id` | Delete a file |
| GET | `/api/ws` | WebSocket for download progress |

## Architecture

Two processes:

- **Rust server** (port 8550): API, SQLite, yt-dlp download worker, WebSocket
- **Next.js** (port 3000): SSR web UI with shadcn/ui + Tailwind, proxies `/api/*` to Rust

Files are stored on a Samba share and accessed directly by clients — no video streaming through the server.

## Docker Compose

```yaml
services:
  dal-api:
    build: .
    ports: ["8550:8550"]
    volumes:
      - /mnt/media/videos:/mnt/media/videos
      - ./data:/var/lib/dal
  dal-web:
    build: ./web
    ports: ["3000:3000"]
    environment:
      - NEXT_TELEMETRY_DISABLED=1
      - DAL_API_URL=http://dal-api:8550
```
