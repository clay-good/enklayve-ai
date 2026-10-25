# Enklayve Roadmap

**Vision:** the local, private, always-free way to *ask your data anything* — a
NotebookLM that runs entirely on your machine and treats your files like a vault,
plus a tiny, tasteful agent that can read your documents and (opt-in) the web.

One install. No account. No setup. No cloud. Desktop only (macOS, Windows, Linux).

## Why now

The local-LLM space is crowded — Jan, LM Studio, GPT4All, AnythingLLM, Ollama —
but every option still demands setup, and none pairs on-device document chat with
an encrypted, biometric-locked vault. Two things changed since this project
started: local models (Qwen3-class) are now genuinely good enough for reliable
retrieval **and** tool calling (Qwen3 8B scores ~0.93 and 14B ~0.97 tool-selection
F1 in Docker's 3,570-test eval, vs ~0.75 for the Qwen2.5 7B this was built on),
and the market has proven steady demand for *private* document AI. Enklayve's
wedge is the intersection the field keeps missing: **dead-simple + secure + capable.**

## Principles (what keeps this from exploding in scope)

1. Dead simple — zero setup, the app picks and installs the right model.
2. Secure by default — encrypted at rest, biometric unlock, offline unless you opt in.
3. Self-healing — it recovers or degrades gracefully, never shows a stack trace.
4. Tasteful and minimal — fewer features, done impeccably. If it can't be
   effortless, it doesn't ship.

**Hard non-goals:** no mobile app, no coding/terminal/shell, no OS automation, no
cloud inference, no telemetry. The agent does exactly three things: read your
documents, fetch the web (opt-in), and read/write one folder you designate.

## Phases

Each phase is a shippable release. Specs live in [`openspec/specs/`](openspec/specs/);
in-flight work lives in [`openspec/changes/`](openspec/changes/).

### Phase 1 — Make the core flawless (v0.2)

Fix what's broken today so the fundamental loop is trustworthy.

| Item | Spec |
|------|------|
| Chat uses the model's chat template + a formatting system prompt (fixes the "weird, non-uniform" output) | `chat-experience` |
| Stable streaming markdown render | `chat-experience` |
| DOCX fallback + partial import; one bad file never aborts a batch (fixes v0.1.6 breakage) | `document-ingestion` |
| Baseline resilience: no user-facing panics, plain-language errors | `resilience` |

First change proposal: [`fix-generation-and-ingestion`](openspec/changes/fix-generation-and-ingestion/).

### Phase 2 — Zero-setup onboarding (v0.3)

| Item | Spec |
|------|------|
| Hardware detection → auto-select best-fit model, no pickers | `onboarding` |
| One-confirmation download with progress, checksum verify, resume | `onboarding` |
| Download/model self-healing (resume, re-download on corrupt) | `resilience` |

### Phase 3 — Ask your data, done well (v0.4)

| Item | Spec |
|------|------|
| Folder drag-and-drop ingestion, per-file status | `document-ingestion` |
| Retrieval + rerank, grounded answers with openable citations | `ask-your-data` |
| Honest "not in your data" behavior with labeled fallbacks | `ask-your-data` |
| Index integrity check + one-click rebuild | `resilience` |

### Phase 4 — The minimal agent (v0.5)

| Item | Spec |
|------|------|
| Opt-in web fetch/search (off by default), URL-cited answers | `agent-tools` |
| Sandboxed read/write in one user-granted folder | `agent-tools` |
| Transparent, inline, auditable tool actions | `agent-tools` |

### Phase 5 — Polish & trust (v1.0)

| Item | Spec |
|------|------|
| Crash recovery to consistent state | `resilience` |
| Encrypted backup/restore across machines | `security-vault` |
| Signed installers, all four targets, CI on tag push | `cross-platform-release` |
| First-run experience polish; "it just works" bar met end-to-end | all |

## Not on the roadmap

Image generation, voice, plugin systems, mobile apps, a general agent platform.
These were in the old roadmap and are intentionally dropped. Enklayve does one
thing — ask your data — impeccably.
