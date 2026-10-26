# Tasks: Polish & trust

Status: implemented 2026-10-25. Pure logic unit-tested; CI release and a real
cross-machine restore were not executed in this environment.

## 1. Crash recovery
- [x] 1.1 WAL + synchronous=NORMAL + busy_timeout on every connection.
- [x] 1.2 `run_startup_recovery`: quick_check + orphan cleanup, reported to the UI.
- [x] 1.3 Backup checkpoints the WAL before copying the database.
- [x] 1.4 Unit tests: WAL is enabled; recovery removes orphans and reports ok.
- [ ] 1.5 Verify in-app after a hard kill.

## 2. Backup/restore across machines
- [x] 2.1 Existing backup/restore hardened with the pre-copy checkpoint.
- [ ] 2.2 Verify a real machine-to-machine restore with security enabled.

## 3. Signed cross-platform releases
- [x] 3.1 CI reads macOS Developer ID + notarization + updater key from secrets,
      falling back to ad-hoc so the build is green with none set.
- [x] 3.2 Updater set inactive until keyed (placeholder pubkey would break builds);
      endpoint corrected to the real repo.
- [x] 3.3 `RELEASING.md` documents the full signing setup and secrets.
- [x] 3.4 Fix stale `user/enklayve-dev` repo links in the README.
- [ ] 3.5 Run a tagged release and confirm all four installers publish.
- [ ] 3.6 Add the maintainer's signing certificates as repo secrets (manual).
