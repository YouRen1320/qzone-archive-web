# Architecture

## Goal and scope

Qzone Archive Web is a responsive, self-hosted web service for archiving QQ Zone interaction history that the logged-in QQ account is authorized to access. It produces a portable ZIP download and automatically removes server-side task data after a bounded lifetime.

The service does not provide user accounts, long-term cloud storage, multi-node scheduling, or a guarantee that QQ can return permanently deleted content. Feeds that never appeared in QQ's interaction list cannot be reconstructed by this service.

## Chosen design

The deployment is a single Rust process behind Nginx. It serves the compiled Vue application and a same-origin JSON API.

Each browser session receives two opaque, `HttpOnly`, `SameSite=Strict` cookies: a random job identifier and an independent 256-bit owner secret. The server hashes the secret before writing task metadata. A successful authorization check is required before any task state, cancellation, export, download, or deletion operation.

Each job is physically isolated:

```text
QZONE_DATA_DIR/
└── <random-job-id>/
    ├── status.json
    ├── archive.sqlite3
    ├── media/
    └── export/
        ├── viewer-manifest.json
        ├── viewer-records.json
        └── qzone-archive.zip
```

`archive.sqlite3` is not a shared application database. It belongs to one job, is included in that user's export, and is deleted with the job. QQ login credentials exist only in the in-memory runtime associated with that job.

## Runtime flow

1. The browser creates or resumes its isolated job.
2. The backend obtains a QQ QR code and keeps the QQ cookie jar in memory.
3. After login confirmation, the user starts an archive with an optional media-download choice.
4. A global semaphore permits one active archive on the initial 2 GiB server; additional authenticated jobs wait in FIFO order.
5. Each page is committed transactionally to the task-local SQLite database, together with a resumable cursor.
6. The service optionally downloads bounded media, then writes JSON, HTML, a manifest, and the SQLite database into a ZIP.
7. The ready page can stream private records and range-capable media directly into the reader, or download the complete ZIP.
8. A saved ZIP can later be opened by the same Vue reader entirely in the browser. ZIP entries are treated as data, records are parsed as JSON, and media is expanded only when it approaches the viewport.
9. The task is deleted after the configured post-download delay or the absolute TTL, whichever comes first.

## Presentation state

The Vue application maps the authoritative backend phase onto six visual spaces: entrance, QR login, options, archive progress, media/package processing, and download. Visual navigation cannot advance the job, and background scene code cannot call APIs. Failed or interrupted jobs return visually to QR login; paused or cancelled jobs return to QR or options according to the backend's current `loggedIn` value.

The Jiangnan stage is progressive enhancement. Static desktop and mobile images remain usable when WebGL is unavailable. One animation scheduler owns rain, lens transitions, parallax, and progress ripples; it stops for hidden pages, reduced-motion preferences, and component disposal. The eight “雨路取景” stills are bundled interface atmosphere, explicitly labelled so they cannot be mistaken for recovered user media.

## Failure and restart behavior

- A process restart deliberately destroys all QQ cookies and active login sessions.
- Existing job directories are scanned at startup. Interrupted jobs become resumable after the user scans a new QR code.
- SQLite transactions protect completed pages. A checkpoint is advanced only after its page commit succeeds.
- Expired and malformed job directories are removed without loading their contents into another session.

## Capacity guardrails

- One running archive; a small bounded queue.
- Per-job and global disk quotas are checked before work and media writes.
- Page requests have bounded retries and a configurable delay of at least two seconds.
- Individual images and videos have size limits and download timeouts.
- The production container has explicit CPU and memory limits.

## Rollback

The web service uses a dedicated container, data directory, loopback port, and Nginx server block. Rollback stops the new container and removes only its Nginx configuration and data directory. Existing sites on the host are not modified.
