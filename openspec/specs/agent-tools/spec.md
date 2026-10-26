# Agent Tools (Minimal)

## Purpose

Enklayve is an assistant, not just a search box — but the toolset stays
deliberately tiny and tasteful. The agent does exactly three things: retrieve
from your documents (see `ask-your-data`), fetch from the web (opt-in), and
read/write inside one folder you designate. No shell, no coding, no OS control.
This is the whole agent. Scope creep here is the primary risk and is rejected by
default.

## Requirements

### Requirement: The agent SHALL expose only an allowlisted, minimal set of tools

The only tools SHALL be: document retrieval, web fetch/search, and sandboxed file
read/write. No tool that executes code, runs shell commands, controls the OS, or
automates arbitrary applications SHALL exist.

#### Scenario: No code execution surface
- **WHEN** the model attempts to call anything outside the allowlist
- **THEN** the call is rejected; there is no execution path to add such tools at
  runtime

### Requirement: Web access SHALL be a conscious choice — asked once, then automatic

The app SHALL NOT make web requests until the user has made an explicit choice.
It asks once (a single yes/no the user can change anytime); until they choose to
enable it, the app stays fully offline and makes no outbound requests beyond the
one-time model download. Once enabled, web search happens automatically whenever
the user's documents do not answer a question. Only the user's question is ever
sent out — never their documents.

#### Scenario: Offline until the choice is made
- **WHEN** a fresh install has not yet been asked, or the user chose to keep it offline
- **THEN** no document query or chat triggers any network request
- **AND** the UI makes the on/off state unambiguous (a visible toggle)

#### Scenario: Automatic after enabling
- **WHEN** web access is enabled and a question is not answered by the documents
- **THEN** the agent automatically searches the web to answer it, shows that a
  web search is happening, and cites web sources as URL links distinct from
  document citations
- **AND** only the question — not any document content — leaves the device

### Requirement: File tools SHALL be confined to one user-designated folder

When the user grants a working folder, the agent MAY read and write only within
that folder. It SHALL NOT read or write anywhere else on the machine, and paths
SHALL be canonicalized to prevent traversal outside the sandbox.

#### Scenario: Write a result to the working folder
- **WHEN** the user asks the agent to save a summary and has granted a folder
- **THEN** the agent writes the file inside that folder only
- **AND** an attempt to write outside it (e.g. `../`) is refused

#### Scenario: No folder granted
- **WHEN** the user has not granted a working folder
- **THEN** file write is unavailable and the agent says so, rather than writing
  to a default location

### Requirement: Side-effectful tool use SHALL be transparent

Every web fetch and every file write SHALL be visible to the user as it happens,
with what was fetched or written, so the agent's actions are always auditable.

#### Scenario: Visible actions
- **WHEN** the agent fetches a page or writes a file
- **THEN** the action and its target appear inline in the conversation
