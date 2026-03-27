# DAL — Download, Assess, Label

A self-hosted content review pipeline: queue video downloads from any browser, review them from any device, and rate them with a filename-based scoring system.

## Prerequisites

- [Rust](https://rustup.rs/) (1.75+)
- [Node.js](https://nodejs.org/) (20+)
- [yt-dlp](https://github.com/yt-dlp/yt-dlp)
- SQLite 3

## Quick Start

### Docker Compose (recommended)

```bash
docker compose up -d
```

All data lives in `.docker/dal/` (config, database, downloads). On first run, `dal.toml` is created from `dal.toml.example`. Edit `.docker/dal/dal.toml` to customize settings, then `docker compose restart dal-api`.

To mount an external volume (e.g. a Samba share) instead of the default downloads directory, create `docker-compose.override.yml`:

```yaml
services:
  dal-api:
    volumes:
      - /your/samba/share:/downloads
```

### Manual Setup

#### 1. Rust API Server

```bash
cargo build --release
./target/release/dal --config dal.toml
```

The API server starts on port 8550 by default.

#### 2. Next.js Frontend

```bash
cd web
npm install
npm run dev
```

The web UI starts on port 3000 and proxies API requests to the Rust server.

For production:

```bash
cd web
npm run build
npm start
```

#### 3. Firefox Extension

See [Installing the Firefox Extension](#installing-the-firefox-extension) below.

## Configuration

All server configuration lives in `dal.toml`. If the file is missing, sensible defaults are used. See `dal.toml.example` for a fully commented reference.

```toml
[server]
host = "0.0.0.0"
port = 8550

[storage]
download_dir = "/downloads"
database = "/data/dal.db"
segment_by_host = true
# Optional: rewrite displayed file paths (useful for Docker volume mounts)
# display_path_prefix = "/mnt/media"

[ytdlp]
binary = "yt-dlp"
max_concurrent = 1
default_args = [
  "--newline", "--no-colors", "--verbose",
  "--progress", "--continue",
  "--trim-filenames", "150"
]
# output_template = "%(uploader)s/%(title)s.%(ext)s"
extra_args = []
```

### Configuration Reference

| Section | Key | Default | Description |
|---------|-----|---------|-------------|
| `server` | `host` | `0.0.0.0` | Address to bind to |
| `server` | `port` | `8550` | API server port |
| `storage` | `download_dir` | `./downloads` | Where files are saved |
| `storage` | `database` | `./dal.db` | SQLite database path |
| `storage` | `segment_by_host` | `true` | Create subdirectories per source hostname |
| `storage` | `display_path_prefix` | *(none)* | Rewrite displayed paths with this prefix instead of `download_dir` |
| `ytdlp` | `binary` | `yt-dlp` | Path to yt-dlp binary |
| `ytdlp` | `max_concurrent` | `1` | Max simultaneous downloads |
| `ytdlp` | `default_args` | *(see dal.toml.example)* | Base arguments passed to every yt-dlp invocation |
| `ytdlp` | `output_template` | *(none)* | Custom yt-dlp output template (overrides default `%(title)s.%(ext)s`) |
| `ytdlp` | `extra_args` | `[]` | Additional yt-dlp arguments appended after defaults |

### Display Path Prefix

When running in Docker, the container's internal paths (e.g. `/downloads/movies/file.mkv`) don't match the host filesystem. Set `display_path_prefix` to show paths as they appear on the host:

```toml
[storage]
download_dir = "/downloads"
display_path_prefix = "/mnt/media"
# /downloads/movies/file.mkv → displays as /mnt/media/movies/file.mkv
```

File paths are stored as relative paths in the database (e.g. `movies/file.mkv`), making the database portable across environments.

### Common yt-dlp Extra Arguments

```toml
# Always output mp4
extra_args = ["--merge-output-format", "mp4"]

# Best quality
extra_args = ["--format", "bestvideo+bestaudio", "--merge-output-format", "mp4"]

# Use browser cookies (for sites requiring login)
extra_args = ["--cookies-from-browser", "firefox"]

# Limit download speed
extra_args = ["--ratelimit", "5M"]

# Combine multiple options
extra_args = [
  "--merge-output-format", "mp4",
  "--cookies-from-browser", "firefox",
  "--ratelimit", "10M"
]
```

### Next.js Frontend Configuration

The frontend is configured via environment variables in `web/.env.local`:

```
NEXT_TELEMETRY_DISABLED=1
DAL_API_URL=http://localhost:8550
```

| Variable | Default | Description |
|----------|---------|-------------|
| `NEXT_TELEMETRY_DISABLED` | `1` | Disable Next.js telemetry (privacy) |
| `DAL_API_URL` | `http://localhost:8550` | URL of the Rust API server |

## File Organization

### Host Segmentation

With `segment_by_host = true` (default), downloads are organized by source:

```
/mnt/media/videos/
├── youtube.com/
│   ├── 85 - Great Video.mp4
│   └── Tutorial.mp4
├── vimeo.com/
│   └── 72 - Art Film.mp4
└── reddit.com/
    └── INCOMPLETE - Funny Clip.mp4
```

Set `segment_by_host = false` for a flat directory.

### File Naming Convention

- Unrated: `Some Great Video.mp4`
- Rated: `85 - Some Great Video.mp4` (10-99 scale)
- Incomplete: `INCOMPLETE - Some Great Video.mp4`

Files are rated via the web UI. The rating prefix means any file browser sorting by name produces a rating-sorted list.

### Duplicate Detection

DAL tracks every URL ever queued. Submitting the same URL a second time returns a `409 Conflict` error. To re-download, send `"force": true` in the request body (or click "Force re-download" in the UI).

## Installing the Firefox Extension

The Firefox extension adds a right-click "Queue in DAL" option for any link.

### Temporary Installation (Development)

1. Open Firefox and navigate to `about:debugging#/runtime/this-firefox`
2. Click **Load Temporary Add-on...**
3. Navigate to `clients/firefox/` and select `manifest.json`
4. The DAL icon appears in the toolbar

> Temporary add-ons are removed when Firefox restarts.

### Permanent Installation

To install permanently, package the extension as an `.xpi` file:

```bash
cd clients/firefox
zip -r ../dal-extension.xpi .
```

Then either:
- **Self-host**: Go to `about:config`, set `xpinstall.signatures.required` to `false`, then install from `about:addons` > gear icon > "Install Add-on From File..."
- **Sign via AMO**: Upload to [addons.mozilla.org](https://addons.mozilla.org/) for signing (free, can be unlisted)

### Extension Configuration

1. Click the DAL icon in the Firefox toolbar
2. Set the **Server URL** to your DAL server (e.g., `http://192.168.1.100:8550`)
3. The URL is saved and persists across sessions

### Usage

- **Right-click any link** and select **"Queue in DAL"** to send it to the download queue
- **Click the DAL icon** to see the queue status badge and manually paste URLs
- The badge shows the number of active/pending downloads

## API Endpoints

### Health & WebSocket

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/health` | Health check |
| GET | `/api/ws` | WebSocket for real-time download progress |

### Queue

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/queue` | Queue a URL for download |
| GET | `/api/queue` | List queue entries (filter: `status`, `archived`, `include_archived`) |
| PUT | `/api/queue/reorder` | Reorder queue entries |
| POST | `/api/queue/archive-all` | Archive all completed/failed items |
| POST | `/api/queue/retry-all` | Retry all failed items |
| DELETE | `/api/queue/{id}` | Cancel a pending download |
| POST | `/api/queue/{id}/archive` | Archive a single entry |
| POST | `/api/queue/{id}/unarchive` | Unarchive a single entry |
| POST | `/api/queue/{id}/retry` | Retry a failed or completed entry |
| GET | `/api/queue/{id}/history` | Get retry/status history |

### Files

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/files` | List files (filter: `unrated`, `incomplete`, `category_id`; sort: `created_at`, `rating`, `name`) |
| POST | `/api/files/scan` | Scan download directory for untracked files |
| POST | `/api/files/{id}/rate` | Rate a file (10-99) |
| POST | `/api/files/{id}/requeue` | Requeue a provisional file (deletes file, resets queue entry) |
| DELETE | `/api/files/{id}` | Delete a file from disk and database |

### Categories

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/categories` | List all categories |
| POST | `/api/categories` | Create a category |
| PUT | `/api/categories/{id}` | Rename a category |
| DELETE | `/api/categories/{id}` | Delete a category |

### Queue a URL

```bash
# Basic
curl -X POST http://localhost:8550/api/queue \
  -H 'Content-Type: application/json' \
  -d '{"url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ"}'

# With a category
curl -X POST http://localhost:8550/api/queue \
  -H 'Content-Type: application/json' \
  -d '{"url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ", "category_id": 1}'

# Force re-download of a previously queued URL
curl -X POST http://localhost:8550/api/queue \
  -H 'Content-Type: application/json' \
  -d '{"url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ", "force": true}'
```

## Architecture

Two processes:

- **Rust server** (port 8550): API, SQLite, yt-dlp download worker, WebSocket
- **Next.js** (port 3000): SSR web UI with Tailwind, proxies `/api/*` to Rust

Files are stored on a Samba share and accessed directly by clients — no video streaming through the server.
