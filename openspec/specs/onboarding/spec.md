# Onboarding (Zero-Setup)

## Purpose

A new user should go from download to first answer without configuring anything.
The app inspects the machine, picks the best model it can run, installs it, and
hands the user a chat box. No model pickers, no quantization choices, no backends.

## Requirements

### Requirement: The app SHALL detect hardware and select a model with no user input

On first launch the app SHALL detect available RAM, CPU, GPU/accelerator (Metal,
CUDA), and free disk space, and SHALL select a single recommended GGUF model
sized to run comfortably on that machine.

#### Scenario: 16GB Apple Silicon machine
- **WHEN** the app first launches on an M-series Mac with 16GB RAM
- **THEN** it selects a ~7-9B Qwen3-class model with Metal acceleration
- **AND** it presents one "Get started" action, not a model list

#### Scenario: Low-spec machine
- **WHEN** the machine has 8GB RAM
- **THEN** it selects a ~3-4B model that fits in memory
- **AND** it never recommends a model that would exceed available RAM or disk

### Requirement: The app SHALL install the model with a single confirmation and a clear progress state

The user SHALL confirm the one-time download once; the app SHALL show size,
progress, and estimated time, and SHALL verify the download before use.

#### Scenario: One-time model download
- **WHEN** the user confirms "Download recommended model"
- **THEN** the app downloads the GGUF, shows live progress, and verifies its
  SHA-256 checksum before marking it ready
- **AND** on success the user lands directly in a usable chat

#### Scenario: Interrupted download
- **WHEN** the download is interrupted (network drop, quit)
- **THEN** the app resumes from where it left off on next attempt rather than
  restarting, and never leaves a corrupt model in place (see `resilience`)

### Requirement: The app SHALL require zero configuration to reach first answer

No backend selection, no API keys, no vector-DB setup, no terminal steps SHALL
be required at any point in the default flow.

#### Scenario: First answer
- **WHEN** the model is ready
- **THEN** the user can immediately type a question or drop a file, with no
  further setup screens between them and a response
