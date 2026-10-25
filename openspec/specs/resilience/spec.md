# Resilience & Self-Healing

## Purpose

For non-technical users with no terminal to fall back on, the app must assume
things will go wrong — bad files, interrupted downloads, corrupt indexes, out of
memory, a model that won't load — and recover on its own or degrade gracefully
with a plain-language explanation. The app SHALL never show a raw stack trace,
never crash to desktop on a recoverable error, and never leave corrupt state.

## Requirements

### Requirement: No user-facing operation SHALL panic or surface a raw error

Every fallible operation reachable from the UI SHALL return a plain-language,
actionable message. Internal detail goes to the log, not the screen.

#### Scenario: Any operation fails
- **WHEN** an operation fails for any reason
- **THEN** the user sees a short, human explanation and a suggested next step
- **AND** the technical detail is written to the local log for diagnostics

### Requirement: The model runtime SHALL recover from load and memory failures

If a model fails to load or inference hits an out-of-memory condition, the app
SHALL attempt recovery (retry, reduce context/batch, or recommend a smaller
model) rather than becoming unusable.

#### Scenario: Out of memory during inference
- **WHEN** inference OOMs on the current model
- **THEN** the app recovers the session, explains what happened, and offers to
  switch to a smaller model that fits

#### Scenario: Corrupt or incomplete model file
- **WHEN** the model file fails checksum or won't load
- **THEN** the app offers a one-click re-download and does not crash

### Requirement: The index SHALL be verifiable and repairable

The app SHALL detect a corrupt or out-of-sync vector/document index and SHALL
offer to rebuild it from the stored documents without data loss.

#### Scenario: Corrupt index on startup
- **WHEN** the index fails an integrity check at startup
- **THEN** the app offers to rebuild from the already-ingested documents, and
  chat remains available (ungrounded) while it rebuilds

### Requirement: Long operations SHALL be resumable and cancelable

Model downloads and bulk ingestion SHALL resume after interruption and SHALL be
cancelable without corrupting state.

#### Scenario: Quit mid-ingestion
- **WHEN** the user quits while indexing a large folder
- **THEN** on relaunch, already-indexed files are intact and the remainder
  resumes or can be resumed, with no duplicates

### Requirement: A crash SHALL be recoverable on next launch

If the app terminates unexpectedly, the next launch SHALL restore to a consistent
state — no half-written encrypted records, no orphaned partial documents.

#### Scenario: Recover after crash
- **WHEN** the app is relaunched after an unexpected termination
- **THEN** it opens to a consistent state and reports if anything was rolled back
