# Security and privacy model

## Protected data

QQ cookies are high-value credentials. They must never be written to disk, returned to the browser, included in errors, emitted through tracing, or placed in the exported archive. The backend exposes only a coarse login state and a masked QQ number.

Archive contents are private user data. Every route that reads or mutates a job requires both the job ID and owner secret supplied through secure cookies. A job ID alone is insufficient.

## Controls

- Cryptographically random 128-bit job IDs and 256-bit owner secrets.
- SHA-256 owner-secret hashes at rest and constant-time comparisons.
- `HttpOnly`, `Secure`, `SameSite=Strict` cookies in production.
- Same-origin checks on state-changing requests.
- Strict job-ID validation before constructing paths.
- No user-supplied filenames or filesystem paths.
- One job directory and SQLite file per user; no shared data tables.
- Response headers disable framing, MIME sniffing, referrer leakage, and browser caching for API responses.
- Download responses are private and non-cacheable.
- Structured logs contain job prefixes and counters only, never QQ UINs, content, URLs, or cookies.
- Automatic TTL cleanup, post-download cleanup, and explicit immediate deletion.

## Known risks

- QQ may rate-limit or block requests from a cloud IP.
- A compromised server process can access credentials held in its memory while a job is active.
- QQ media URLs can expire or return placeholders.
- Same-phone QR login depends on QQ's ability to recognize a saved QR image or deep link; this must be validated on real devices.
- This service is unofficial and is not affiliated with Tencent.

## Responsible use

Only archive accounts and content the user owns or is authorized to access. Do not use the service to evade access controls, scrape unrelated accounts, or switch QQ accounts during an active archive.
