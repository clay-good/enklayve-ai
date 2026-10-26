# Change: The minimal agent (web + sandboxed folder)

## Why

Phase 4 gives Enklayve exactly two capabilities beyond document retrieval, and
no more: look things up on the web, and read/write one folder the user grants.
No shell, no code execution, no OS control.

## Decision: web is "ask once, then automatic" (not silent-on, not buried-opt-in)

The user asked for automatic web search. Silent-on-by-default was rejected
because it would break the product's core promise ("nothing leaves your device
unless you choose") and make the "100% local" copy false. The chosen model keeps
it effortless without betraying trust: ask once (a single yes/no, changeable
anytime); once enabled, search happens automatically whenever the documents
don't answer. Only the user's question is ever sent — never their documents.

## What changes

### Web access
- `web.rs`: keyless DuckDuckGo HTML search + readable page fetch. The fragile
  HTML parsing is isolated in `parse_duckduckgo_html` and unit-tested.
- `web_access` setting ("unasked" | "enabled" | "disabled") with get/set commands.
- `maybe_web_prompt` hooks into both chat paths: when documents don't answer and
  web is enabled, it searches and builds a prompt that answers from the results,
  citing sources as Markdown links; it emits a `web-search` event so the UI shows
  "Searching the web…".
- Frontend: a one-time consent modal, a 🌐 On/Off toggle, and the web indicator.

### Sandboxed working folder
- `fs_sandbox.rs`: path-safe read/write confined to one granted folder; rejects
  `..`, absolute paths, and symlink escapes. Unit-tested.
- `working_folder` setting + `set/get_working_folder` + `save_to_working_folder`.
- Frontend: a "Save to folder" action on answers (prompts to pick a folder once).

## Impact

- Affected specs: `agent-tools` (web requirement updated to ask-once-then-auto),
  `security-vault` (unchanged — web is still a conscious choice).
- Affected code: new `web.rs`, `fs_sandbox.rs`; `commands.rs`, `lib.rs`;
  frontend `App`, `ChatWindow`, `MessageBubble`, `DocumentPanel` + CSS.

## Notes / follow-ups (important)
- **Web live paths are not verified here.** This environment cannot run the app +
  model + a real network round-trip, so search/fetch are compile-checked and the
  parser is unit-tested, but the live DuckDuckGo integration needs in-app testing.
- **Scraping caveat.** DuckDuckGo HTML scraping is fragile and subject to their
  terms; the provider is isolated behind `web::search` and should be reviewed and
  possibly swapped for a sanctioned API.
- **No autonomous tool-calling loop.** The model does not itself decide to call
  tools mid-answer; web is a deterministic fallback and folder writes are a user
  action. A true function-calling agent loop (and richer file tools) would be a
  later, separately-tested step.
