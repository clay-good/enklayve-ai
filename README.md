<div align="center">

# Enklayve

**A private AI assistant that runs on your own computer.**

No account. No subscription. No API keys.

### [Download the latest release](https://github.com/clay-good/enklayve-ai/releases/latest)

</div>

## Why Enklayve

The AI model runs on your machine, so your files and chats stay there too. It works offline, it costs nothing to use, and you can still look things up on the web when you want to.

- **Ask about your files.** Add PDFs, Word and Excel files, text, web pages, or photos of documents to a chat. Answers point to the file they came from.
- **Search the web when you choose.** Turn it on for one chat with the globe button. You get current answers with sources, and only your question leaves your computer.
- **Keep chats separate.** Each chat has its own files and its own web setting.
- **Lock it.** Add a password if others use your computer.

## Install

### Mac

1. Download the `.dmg` for your Mac: `aarch64` for Apple Silicon (M1 and newer) or `x64` for Intel.
2. Open it and drag **enklayve** into **Applications**.
3. Open **Terminal** and run this once:

   ```bash
   xattr -cr /Applications/enklayve.app
   ```

   Enklayve is not notarized by Apple yet, so macOS blocks it the first time. This command clears the download flag that triggers the block.
4. Open enklayve from Applications.

### Windows

Download the `.exe` and run it. If SmartScreen appears, click **More info**, then **Run anyway**.

### Linux

Download the `.deb`, `.rpm`, or `.AppImage`. To run the AppImage:

```bash
chmod +x enklayve_*_amd64.AppImage
./enklayve_*_amd64.AppImage
```

## Setup

The first time you open Enklayve, it checks your hardware and recommends a model that fits. You can set an optional password, then the model downloads once (about 1 to 9 GB depending on your computer). After that, start a chat.

Enklayve asks once whether to allow web search. You can change it for any chat with the globe button next to the paperclip.

## Privacy

- Your chats and files are stored only on your computer.
- Web search stays off unless you allow it, and it sends only your question.
- No accounts and no tracking.

## Build from source

Requires [Rust](https://rustup.rs) and [Node.js 20+](https://nodejs.org).

```bash
git clone https://github.com/clay-good/enklayve-ai.git
cd enklayve-ai/enklayve-app
npm install
npm run tauri dev
```

Run `npm run tauri build` to package an installer. See [RELEASING.md](RELEASING.md) to publish one.

## License

[MIT](LICENSE)
