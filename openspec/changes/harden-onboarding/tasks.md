# Tasks: Harden zero-setup onboarding

Status: implemented and unit-tested (2026-10-25). Resume/verify are network
paths verified by construction + review; full in-app download still pending.

## 1. Resumable download
- [x] 1.1 Stream into a `{model}.part` file; promote to final name only when complete.
- [x] 1.2 On interruption, keep the `.part` and resume via HTTP `Range`.
- [x] 1.3 Handle 206 (append) vs 200 (server ignored range → restart).
- [x] 1.4 Discard a stale `.part` that is not a GGUF before resuming.
- [x] 1.5 Report an interrupted download as resumable, not a hard failure.
- [ ] 1.6 Manual check: kill the app mid-download; relaunch resumes, no restart.

## 2. Checksum / integrity
- [x] 2.1 Verify SHA-256 after download when a checksum is known.
- [x] 2.2 Keep the GGUF-magic + size + full-length integrity check as the guard
      when no checksum is set; delete corrupt files and surface a plain error.
- [ ] 2.3 Populate real SHA-256 checksums in `models.rs` (needs the files).

## 3. Disk-aware selection
- [x] 3.1 Step down from the RAM tier to the best model that fits free disk
      (size + 2 GB headroom).
- [x] 3.2 Unit tests: steps down under disk pressure; never exceeds free disk.

## 4. Frontend
- [x] 4.1 Pass the model checksum through to `download_model`.
- [x] 4.2 Offer "Resume download" after an interrupted download.
