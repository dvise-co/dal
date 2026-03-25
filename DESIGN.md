# DAL — Download, Assess, Label

A self-hosted content review pipeline: queue video downloads from any browser, review them from any device, and rate them with a filename-based scoring system.

## Architecture Overview

```
┌──────────────┐       POST /queue        ┌──────────────────────────┐
│   Firefox     │ ────────────────────────→│                          │
│   Extension   │                          │     DAL Server (Rust)    │
└──────────────┘                          │                          │
                                          │  ┌────────────────────┐  │
┌──────────────┐       GET /ui            │  │   HTTP API (Axum)  │  │
│  Any Device   │ ←──────────────────────→│  └────────┬───────────┘  │
│  Browser      │   (review + rate UI)    │           │              │
└──────────────┘                          │  ┌────────▼───────────┐  │
                                          │  │   SQLite Database  │  │
                                          │  └────────┬───────────┘  │
                                          │           │              │
                                          │  ┌────────▼───────────┐  │
                                          │  │  Download Worker   │  │
                                          │  │  (yt-dlp subprocess│) │
                                          │  └────────┬───────────┘  │
                                          │           │              │
                                          └───────────┼──────────────┘
                                                      │
                                                      ▼
                                          ┌──────────────────────┐
                                          │   Samba Share Mount   │
                                          │   /mnt/media/videos   │
                                          └──────────────────────┘
```

## Components

### 1. DAL Server (Rust Binary)

A single binary that runs everything: HTTP API, download worker, and web UI.

**Tech stack:**
- Axum — HTTP framework
- SQLite (sqlx) — persistent state
- Tokio — async runtime + process spawning
- Tower — middleware (CORS, logging)

#### HTTP API

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/queue` | Add a URL to the download queue |
| GET | `/api/queue` | List queued/active/completed downloads |
| DELETE | `/api/queue/:id` | Cancel a queued download |
| GET | `/api/files` | List downloaded files (unrated, or all) |
| POST | `/api/files/:id/rate` | Assign a rating (renames the file) |
| DELETE | `/api/files/:id` | Delete a file |
| GET | `/api/files/:id/stream` | Stream video for in-browser preview |
| GET | `/api/health` | Health check |

#### Download Worker

- Background Tokio task that polls the queue
- Runs `yt-dlp` as a subprocess with `--newline` for progress parsing
- Downloads to configured output directory (the Samba mount)
- One download at a time (configurable concurrency)
- On completion: inserts file record into SQLite, updates queue entry status
- On failure: marks queue entry as failed with error message, supports retry

#### Database Schema

```sql
CREATE TABLE queue (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    url         TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'pending',  -- pending, downloading, completed, failed
    progress    TEXT,                               -- yt-dlp progress string
    error       TEXT,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE files (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    queue_id    INTEGER REFERENCES queue(id),
    filename    TEXT NOT NULL,                      -- current filename on disk
    original    TEXT NOT NULL,                      -- original filename from yt-dlp
    path        TEXT NOT NULL,                      -- full path on disk
    rating      INTEGER,                            -- 10-99, NULL if unrated
    size_bytes  INTEGER,
    duration    TEXT,                                -- from yt-dlp metadata
    thumbnail   TEXT,                                -- thumbnail path if available
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    rated_at    DATETIME
);
```

### 2. Web UI

Served by the same Axum binary as static files. Plain HTML + vanilla JS (no build step).

**Two views:**

#### Queue View
- Shows pending, downloading (with progress), completed, and failed downloads
- Input field to manually add a URL
- Cancel/retry buttons

#### Review View
- Grid or list of unrated files with thumbnails
- Click to open video preview (in-browser streaming via `/api/files/:id/stream`)
- Rating controls: click a number (10–99 scale) or use a slider
- Delete button for bad content
- Filter: unrated / all / by rating range
- Sort: by date, by rating, by name

### 3. Firefox Extension

Minimal Manifest V3 extension.

**Features:**
- Adds "Queue in DAL" to the right-click context menu for links
- Popup with: server URL configuration, queue depth badge, manual URL input
- Sends `POST /api/queue` with the link URL

**Files:**
```
extension/
├── manifest.json
├── background.js    -- context menu handler, API calls
├── popup.html       -- config + manual input
├── popup.js
└── icons/
    └── icon-48.png
```

**Permissions:** `contextMenus`, `storage`, `host_permissions` for the server URL.

## File Naming Convention

Downloaded files start with their original `yt-dlp` title. Upon rating, the filename is prefixed:

```
Before:  Some Great Video.mp4
After:   85 - Some Great Video.mp4
```

- Rating range: **10–99** (integer, two digits, always sorts correctly as string)
- Separator: ` - ` (space-dash-space)
- Higher number = better content
- Unrated files have no prefix
- Re-rating updates the prefix in place

This means any file browser on any device, sorting by name, produces a rating-sorted list.

## Configuration

Single config file (`dal.toml`):

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

## Deployment

Single binary + config file. Can be run directly, as a systemd service, or in a container.

```
dal --config /etc/dal.toml
```

## Project Structure

```
dal/
├── Cargo.toml
├── dal.toml                  -- default config
├── src/
│   ├── main.rs               -- entry point, config loading
│   ├── config.rs             -- config parsing
│   ├── db.rs                 -- database setup + queries
│   ├── api/
│   │   ├── mod.rs            -- router setup
│   │   ├── queue.rs          -- queue endpoints
│   │   └── files.rs          -- file endpoints
│   ├── worker.rs             -- yt-dlp download worker
│   └── rename.rs             -- file rating/renaming logic
├── static/                   -- web UI files
│   ├── index.html
│   ├── app.js
│   └── style.css
└── extension/                -- Firefox extension
    ├── manifest.json
    ├── background.js
    ├── popup.html
    ├── popup.js
    └── icons/
```

## Future Considerations

These are explicitly out of scope for v1 but noted for later:

- **Authentication** — if the server is ever exposed beyond the local network
- **Tags/categories** — beyond the numeric rating
- **Bulk operations** — rate or delete multiple files at once
- **Mobile-optimized UI** — responsive design in v1, dedicated mobile UX later
- **Download presets** — different quality/format profiles
