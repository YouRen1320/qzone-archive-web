# Verification matrix

| Requirement | Automated evidence | Production evidence |
| --- | --- | --- |
| Owner isolation | API test rejects a wrong 256-bit owner token | Two-browser access test |
| No shared database | Path and database tests use one file per random job | Inspect `/data/jobs/<id>/` layout |
| Cookie memory only | Login state has no serialization implementation; export tests inspect ZIP entries | Search task files and logs after real QR login |
| Resumable pages | SQLite transaction/checkpoint tests | Interrupt after a page, rescan, continue |
| SSRF resistance | Media URL allowlist unit tests | Review redirects and egress logs |
| Portable export | ZIP v2 test requires records JSON, HTML fallback, JSONL, SQLite and manifest | Open on desktop, iOS, and Android |
| Ready reader | Frontend tests open the current task automatically and page/filter records through private APIs | Finish a real task, browse it in place, then save the ZIP separately |
| Ready reader privacy | API tests reject missing owner cookies and verify private range responses | Open media in the owning browser; confirm another browser receives 401 |
| Automatic deletion | Job manager TTL tests | Observe idle, ready, and post-download cleanup |
| Fair slot release | Queue-position and watchdog tests cover active counts, stalls, packaging, and run limits | Start two isolated browsers and verify an auto-paused first task releases the second |
| Responsive UI | Type check, 11-phase component tests, axe checks, production build | 1440×900, 1366×768, 375×812, 320×720 and short-landscape browser checks |
| Scene safety | Phase mapping tests; static scan rejects API/storage use and inline styles in the scene layer | WebGL, static fallback, reduced-motion and production-CSP console checks |
| Rollback isolation | Container build and health check | Roll back tag; verify existing sites |

Tests deliberately do not make QQ network requests or contain real account data. A real account smoke test is a separate manual deployment gate.
