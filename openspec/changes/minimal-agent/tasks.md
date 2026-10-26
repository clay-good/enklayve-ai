# Tasks: The minimal agent (web + sandboxed folder)

Status: implemented 2026-10-25. Pure logic unit-tested; live web paths need
in-app verification (see proposal).

## 1. Web access (ask once, then auto)
- [x] 1.1 `web.rs`: search + fetch; unit-tested parser, percent-decode, strip-html.
- [x] 1.2 `web_access` setting + get/set commands.
- [x] 1.3 `maybe_web_prompt` fallback in both chat paths; URL citations; event.
- [x] 1.4 Frontend: one-time consent modal, 🌐 toggle, "Searching the web…" indicator.
- [ ] 1.5 Verify live search/fetch in-app with a real model.
- [ ] 1.6 Review the DuckDuckGo scraping provider for ToS / swap for an API.

## 2. Sandboxed working folder
- [x] 2.1 `fs_sandbox.rs`: path-safe resolve/read/write; unit-tested traversal guards.
- [x] 2.2 `working_folder` setting + set/get + `save_to_working_folder` commands.
- [x] 2.3 Frontend: "Save to folder" action (prompts to pick a folder once).
- [ ] 2.4 Verify save/read round-trip in-app.

## 3. Docs
- [x] 3.1 Update `agent-tools` spec to ask-once-then-auto.
- [x] 3.2 Update README and chat empty-state copy to match (no false "100% offline").

## 4. Deferred
- [ ] Autonomous tool-calling loop (model decides to search/read/write mid-answer)
      and richer file tools — needs live-model testing.
