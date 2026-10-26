# Change: Polish & trust (crash recovery, backup, signed releases)

## Why

Phase 5 is the reliability and distribution layer: survive crashes cleanly, let
people move their encrypted data between machines, and ship signable installers
for every desktop OS.

## What changes

### Crash recovery
- Every SQLite connection now uses WAL journaling (`journal_mode=WAL`,
  `synchronous=NORMAL`, `busy_timeout=5000`), so an interrupted commit is
  auto-recovered atomically on the next open instead of leaving a torn write.
- `run_startup_recovery` runs on launch: a `quick_check` integrity pass plus
  removal of any orphan chunks, returning a report. The UI only speaks up if the
  database is actually damaged (pointing the user at Repair).
- Backups now `wal_checkpoint(TRUNCATE)` before copying `database.db`, so the
  archived file is complete and self-contained.

### Encrypted backup/restore across machines
- Already implemented (`backup.rs`): a zip of the database, documents, settings,
  and a versioned manifest, restored with manifest validation. With security
  enabled the database content is encrypted at rest, and the salt travels inside
  it, so restoring on another machine + the password recovers everything.
  This change hardens it with the pre-copy checkpoint above.

### Signed cross-platform releases via CI
- The release workflow is now signing-ready: macOS Developer ID + notarization
  and the Tauri updater key are read from repo secrets, falling back to ad-hoc
  macOS signing when unset, so the build stays green with no secrets configured.
- The auto-updater is set `active: false` until a signing key is configured (its
  pubkey was a placeholder that would break release builds), and its endpoint is
  corrected to this repo. `RELEASING.md` documents the whole signing setup.
- Stale `user/enklayve-dev` links in the README are corrected to the real repo.

## Impact

- Affected specs: `resilience`, `security-vault`, `cross-platform-release`.
- Affected code: `database.rs`, `index_health.rs`, `backup.rs`, `commands.rs`,
  `lib.rs`, `App.tsx`; `tauri.conf.json`, `release.yml`; new `RELEASING.md`.

## Notes / follow-ups
- Real OS code-signing and notarization need the maintainer's certificates
  (Apple Developer ID, Windows Authenticode) added as secrets — these cannot be
  provisioned in code; `RELEASING.md` lists exactly what to add.
- CI release runs and the end-to-end backup/restore round-trip were not executed
  in this environment; verify on a tagged build and a real machine-to-machine restore.
