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

The API never includes QQ cookies, raw upstream responses containing credentials, absolute server paths, or stack traces.
