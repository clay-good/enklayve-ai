# Security Vault

## Purpose

Enklayve's wedge is that it treats your data like a vault, not a product.
Documents, embeddings, and conversations are encrypted at rest; access is gated
by biometrics; nothing leaves the device without explicit opt-in. This capability
is already partly built (AES-256-GCM, Argon2, biometric unlock) and is restated
here as the standing contract.

## Requirements

### Requirement: All user content SHALL be encrypted at rest

Documents, extracted text, embeddings, and conversation history SHALL be stored
encrypted with AES-256-GCM, with keys derived via Argon2 from the user's secret
and never written to disk in plaintext.

#### Scenario: Data on disk is unreadable without the key
- **WHEN** an attacker copies the app's data directory
- **THEN** no document text, embedding, or message is readable without the key

### Requirement: Access SHALL be gated by biometric unlock where available

The app SHALL support Touch ID (macOS), Windows Hello, and Linux fingerprint, and
SHALL lock on launch and after inactivity.

#### Scenario: Locked on return
- **WHEN** the app has been idle past the lock timeout
- **THEN** it requires biometric (or fallback secret) re-unlock before showing
  any content

### Requirement: The app SHALL be fully functional offline and SHALL not phone home

Beyond the one-time model download and explicitly enabled web access, the app
SHALL make no network requests — no telemetry, no analytics, no account checks.

#### Scenario: Network monitor is quiet
- **WHEN** the app is used with web access off
- **THEN** a network monitor shows no outbound traffic after the model is present

### Requirement: Encrypted backup and restore SHALL be supported

The user SHALL be able to export an encrypted backup and restore it on the same
or another machine without data loss.

#### Scenario: Restore on a new machine
- **WHEN** the user restores a backup on a new machine with the correct secret
- **THEN** documents, indexes, and conversations are recovered intact
