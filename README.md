<div align="center">

# Enklayve

**A private, local AI assistant for chatting with your documents.**

100% on-device · encrypted at rest · no accounts, no telemetry, no API keys.

### [⬇︎ Download the latest release](https://github.com/clay-good/enklayve-ai/releases/latest)

</div>

---

## What it is

Enklayve runs a capable AI model entirely on your own machine. Ask questions,
drop in documents (PDF, DOCX, TXT, MD) to ask about them, and optionally let it
search the web — your files never leave your device. Everything you do is saved
and encrypted in the app's own folder. There is nothing to configure.

## Install

1. **[Download the latest release](https://github.com/clay-good/enklayve-ai/releases/latest)** and pick the file for your OS:
   - **macOS** — `.dmg` (Apple Silicon & Intel)
   - **Windows** — `.msi` or `.exe`
   - **Linux** — `.AppImage` or `.deb`
2. Open it and follow the short setup.
3. On first run, Enklayve automatically picks and downloads the right model for
   your hardware. That's it — start chatting.

> Distributed via GitHub Releases for now. App Store listings may come later.

## Build from source

Requires [Rust](https://rustup.rs) and [Node.js 20+](https://nodejs.org).

```bash
git clone https://github.com/clay-good/enklayve-ai.git
cd enklayve-ai/enklayve-app
npm install
npm run tauri dev      # run locally
npm run tauri build    # package an installer for your OS
```

## Privacy

Your documents and chats stay on your device, encrypted at rest. Web search,
when you turn it on, sends only your question — never your documents. No
telemetry, no tracking, no accounts.

## More

- [Roadmap](ROADMAP.md) · [Releasing](RELEASING.md)
- Built with [Tauri](https://tauri.app), Rust, and React.

## License

[MIT](LICENSE)
