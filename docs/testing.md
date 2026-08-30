# Verification matrix

| Requirement | Automated evidence | Production evidence |
| --- | --- | --- |
| Owner isolation | API test rejects a wrong 256-bit owner token | Two-browser access test |
| No shared database | Path and database tests use one file per random job | Inspect `/data/jobs/<id>/` layout |
| Cookie memory only | Login state has no serialization implementation; export tests inspect ZIP entries | Search task files and logs after real QR login |
| Resumable pages | SQLite transaction/checkpoint tests | Interrupt after a page, rescan, continue |
| SSRF resistance | Media URL allowlist unit tests | Review redirects and egress logs |
| Portable export | ZIP test requires HTML, JS, JSONL, SQLite | Open on desktop, iOS, and Android |
| Automatic deletion | Job manager TTL tests | Observe expiry and post-download cleanup |
| Responsive UI | Type check, component utility tests, production build | Browser viewport and device checks |
| Rollback isolation | Container build and health check | Roll back tag; verify existing sites |

Tests deliberately do not make QQ network requests or contain real account data. A real account smoke test is a separate manual deployment gate.
