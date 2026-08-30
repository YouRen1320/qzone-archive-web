# Project collaboration notes

- This repository is an unofficial web edition derived in part from QzoneArchive.
- Never log, persist, serialize, or return QQ cookies or login credentials.
- Every recovery job must stay inside its own validated directory below `QZONE_DATA_DIR`.
- Do not add a shared user database. Task-local SQLite files are disposable artifacts.
- New or changed Vue and TypeScript files need concise intent-level comments for responsibilities, data sources, non-obvious mappings, and side effects.
- Use Conventional Commits and keep architecture, API, privacy, and deployment docs synchronized with behavior.
- Run frontend build/tests, Rust formatting/lints/tests, and container smoke tests before release.
