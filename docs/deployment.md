# Production deployment

This guide assumes one Linux x86_64 host, Docker Compose v2, Nginx, a dedicated subdomain, and HTTPS. The initial capacity target is one active archive and a bounded queue.

## 1. Prepare DNS and compliance

Point the dedicated subdomain to the server. Do not reuse an existing application's server block. For a public service on a mainland China host, confirm ICP filing and the privacy/legal requirements that apply to the operator before launch.

## 2. Prepare the application directory

```bash
sudo install -d -m 0755 /opt/qzone-archive-web
sudo install -d -m 0700 -o 10001 -g 10001 /opt/qzone-archive-web/data
```

Place `compose.yml` and `.env` in `/opt/qzone-archive-web`. The data directory is deliberately private and belongs to the unprivileged container user.

Recommended `.env` for the initial 2 GiB host:

```dotenv
QZONE_BIND=0.0.0.0:8091
QZONE_DATA_DIR=/data/jobs
QZONE_PUBLIC_ORIGIN=https://qzone.iyouren.top
QZONE_SECURE_COOKIES=true
QZONE_JOB_TTL_SECONDS=21600
QZONE_POST_DOWNLOAD_TTL_SECONDS=600
QZONE_MAX_JOBS=8
QZONE_MAX_ACTIVE_ARCHIVES=1
QZONE_MAX_JOB_BYTES=5368709120
QZONE_MIN_FREE_BYTES=5368709120
RUST_LOG=qzone_archive_web=info,tower_http=info
```

Do not put QQ cookies or user credentials in this file.

## 3. Start the pinned image

Set `QZONE_IMAGE_TAG` to a tested release tag instead of relying on `latest`:

```bash
export QZONE_IMAGE_TAG=v0.1.0
docker compose pull
docker compose up -d
docker compose ps
curl --fail http://127.0.0.1:8091/api/health
```

The container binds only to loopback, runs without Linux capabilities, has a read-only root filesystem, and is limited to one CPU and 768 MiB RAM.

## 4. Add the Nginx site and TLS

Copy `deploy/nginx/qzone.iyouren.top.conf` to its own file under `/etc/nginx/conf.d/`, validate the complete Nginx configuration, then reload it.

```bash
sudo nginx -t
sudo systemctl reload nginx
```

Obtain a certificate with the host's existing ACME/Certbot workflow. After HTTPS is active, verify that HTTP redirects to HTTPS and that the site includes HSTS. Never enable `QZONE_SECURE_COOKIES=false` in production.

## 5. Acceptance checks

Run these checks before inviting users:

1. `/api/health` returns 200 through HTTPS.
2. A new browser creates one task and receives two `HttpOnly`, `Secure`, `SameSite=Strict` cookies.
3. A second browser cannot access the first task, even if it knows only the job ID.
4. A QR login never creates files containing `p_skey`, `skey`, or full Cookie headers.
5. One-page real-account smoke test succeeds from the server IP without QQ rate limiting.
6. Desktop, iOS, and Android browsers can download and open the ZIP.
7. The ZIP contains the viewer, JSONL, SQLite, manifest, and expected media, but no cookies.
8. Explicit deletion and TTL cleanup remove the complete task directory.
9. The existing sites on the host still return their previous content.

## Updates

```bash
export QZONE_IMAGE_TAG=v0.2.0
docker compose pull
docker compose up -d
curl --fail http://127.0.0.1:8091/api/health
```

Do not update while an archive is active: a restart intentionally destroys in-memory QQ sessions. Completed pages remain in each task's SQLite and can resume after a new scan while the task is unexpired.

## Rollback

Keep the previous immutable tag. If health or smoke checks fail:

```bash
export QZONE_IMAGE_TAG=v0.1.0
docker compose up -d
curl --fail http://127.0.0.1:8091/api/health
```

If the service must be removed, stop its Compose project and remove only its dedicated Nginx server block. Preserve `/opt/qzone-archive-web/data` long enough to let the operator decide whether active temporary jobs should be securely deleted; never recursively target `/opt` or another broad directory.

## Observability

Application logs contain methods, routes, coarse error classes, short random job prefixes, and counters only. They must not contain QQ numbers, nicknames, feed text, media URLs, QR images, or cookies. Monitor:

- container health and restart count;
- available disk space;
- queue saturation and active jobs;
- upstream error rate by coarse class;
- automatic cleanup failures.
