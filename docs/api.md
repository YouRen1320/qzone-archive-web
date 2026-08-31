# HTTP API

All endpoints are same-origin under `/api`. Successful job creation sets private job cookies; subsequent endpoints use those cookies and do not accept credentials in URLs.

| Method | Path | Purpose |
| --- | --- | --- |
| `POST` | `/api/jobs` | Create a new isolated job or replace the browser's expired job |
| `GET` | `/api/job` | Read authorized job status |
| `DELETE` | `/api/job` | Cancel and immediately delete the authorized job |
| `POST` | `/api/login/qr` | Start a fresh QR login and return the QR image |
| `POST` | `/api/login/poll` | Poll QQ's QR confirmation state |
| `POST` | `/api/archive` | Queue an archive using the requested media and pacing options |
| `POST` | `/api/archive/cancel` | Request cooperative cancellation |
| `GET` | `/api/archive/viewer/manifest` | Read the completed job's private viewer manifest |
| `GET` | `/api/archive/viewer/records?offset=&limit=&q=&category=&year=` | Page and filter the completed job's structured viewer records |
| `GET` | `/api/archive/viewer/media/{path}` | Stream one completed-job media file with byte-range support |
| `GET` | `/api/events` | Receive status updates as server-sent events |
| `GET` | `/api/download` | Download the completed ZIP |
| `GET` | `/api/health` | Liveness and capacity status without private data |

Errors use a stable envelope:

```json
{
  "error": {
    "code": "job_not_found",
    "message": "任务不存在或已经过期"
  }
}
```

Viewer endpoints require both owner cookies and a ready job. Record pages are capped at 60 items and read only the frozen viewer projection in that job's SQLite. Media paths are canonicalized inside the job directory, and responses are private, `no-store`, and range-capable for video seeking.

The API never includes QQ cookies, raw upstream responses containing credentials, absolute server paths, or stack traces.
