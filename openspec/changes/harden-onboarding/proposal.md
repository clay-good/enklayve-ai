# Change: Harden zero-setup onboarding

## Why

Most of the onboarding flow already existed and is good: hardware detection,
a single auto-selected best-fit model (`model_selection::get_best_model_for_hardware`),
a one-confirmation `WelcomeFlow` with a live progress bar, and a download path
that already checks GGUF magic + size and self-heals corrupt files. Three gaps
remained against the `onboarding` and `resilience` specs:

1. **No resume.** `download_model` wrote straight to the final file and deleted
   any partial on failure, so a dropped connection meant starting a multi-GB
   download over.
2. **Checksum verification was never wired in.** `verify_checksum` existed but
   was unused.
3. **Selection ignored disk space.** A model was chosen purely by RAM, so a
   machine with enough RAM but little free disk could be told to download a
   model that won't fit.

## What changes

- Resumable downloads: stream into a `{model}.part` file; on interruption keep
  it and resume with an HTTP `Range` request on the next attempt; validate and
  rename to the final name only when complete. Handles servers that ignore the
  range (restart) and stale/garbage `.part` files (discard if not GGUF).
- Wire checksum verification: when a model's `checksum` is non-empty, verify
  SHA-256 after download before promoting the file; fall back to the existing
  integrity check (GGUF magic + size + full-length) when no checksum is known.
- Disk-aware selection: from the RAM-appropriate tier, step down to the best
  model whose size (plus 2 GB headroom) fits available storage.
- Frontend: pass the checksum through, and offer "Resume download" after an
  interrupted download instead of a cold restart.

## Impact

- Affected specs: `onboarding`, `resilience`.
- Affected code: `downloads.rs`, `model_selection.rs`, `commands.rs`
  (`download_model` gains an optional `checksum`), `WelcomeFlow.tsx`.
- No data migration. The model `checksum` fields in `models.rs` are still empty;
  the SHA-256 path activates once they are populated — until then the integrity
  check (which already rejected truncated/HTML files) is the guard.
