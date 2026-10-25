# Enklayve — Project Context

## What this is

Enklayve is a **local, private, always-free desktop app** for asking your own
documents (and the web, opt-in) anything. Think "NotebookLM, but it runs entirely
on your machine and treats your files like a vault." One install, no account, no
cloud, no configuration. Upload a folder, or just start chatting.

Positioning (validated by market research, 2026-10): the local-LLM space is
crowded (Jan, LM Studio, GPT4All, AnythingLLM, Ollama) but every option still
demands setup and none pairs on-device document chat with an encrypted,
biometric-locked vault. Local models (Qwen3-class) are now good enough for
reliable RAG **and** tool calling, which was not true when the project started.
Enklayve's wedge is the intersection: **dead-simple + secure + capable.**

## Non-goals (protect the scope)

- No mobile apps. Desktop only (macOS, Windows, Linux).
- No coding harness, no terminal, no shell execution, no arbitrary OS control.
- No cloud inference, no telemetry, no accounts. Local-first, always.
- Not a general agent platform. The agent does two things: read your data, and
  (opt-in) fetch from the web. Everything else stays out.

## Product principles

1. **Dead simple.** Zero setup. The app picks and installs the right model for
   the machine. The user uploads files and chats. That is the whole flow.
2. **Secure by default.** Encrypted at rest (AES-256-GCM), biometric unlock,
   nothing leaves the device unless the user explicitly turns on web access.
3. **Self-healing.** Failures are expected (bad PDFs, interrupted downloads,
   corrupt indexes, OOM). The app recovers or degrades gracefully and tells the
   user plainly — it never silently breaks or shows a raw stack trace.
4. **Tasteful and minimal.** Fewer features done impeccably. If a feature can't
   be made effortless, it doesn't ship.

## Tech stack

- **Shell:** Tauri 2 (Rust backend + React 19 / TypeScript / Vite frontend).
  Chosen deliberately and staying — small single binary, native crypto/biometric
  access, low memory, strong security posture. No Electron rewrite.
- **Inference:** llama.cpp (via Rust bindings), GGUF models, Metal/CUDA accel.
- **Embeddings / retrieval:** fastembed + SQLite vector store + reranker.
- **Documents:** pdf-extract / lopdf / hayro (PDF), docx-rs (DOCX), calamine
  (XLSX/CSV), html2text (HTML), ocrs + rten (OCR).
- **Storage:** SQLite (rusqlite, bundled), encrypted database layer.
- **Release:** GitHub Actions builds and publishes signed installers for all
  four targets (macOS arm64 + x86_64, Windows, Linux) on tag push.

## Conventions

- Rust: `anyhow`/`thiserror`, structured logging via `logger.rs`. Every fallible
  user-facing operation returns a plain-language error, never a panic.
- Specs live in `openspec/specs/<capability>/spec.md` as the durable contract.
  New work is proposed as a change under `openspec/changes/` with spec deltas,
  then folded into the specs once shipped. See `openspec/AGENTS.md`.
- Keep changes surgical. Match surrounding style. No speculative abstraction.
