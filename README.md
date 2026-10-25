# Enklayve

**Local-First, Always Free — Ask Your Data Anything**

Enklayve is a secure, privacy-first desktop app that lets you ask your own documents anything, answered by AI models running entirely on your computer. No account, no setup, no cloud, no subscriptions. Install it, drop in a folder, and start chatting — or just chat, and (opt-in) let it browse the web. Think NotebookLM, but it runs on your machine and treats your files like a vault.

[![Platform](https://img.shields.io/badge/Platform-macOS%20%7C%20Windows%20%7C%20Linux-blue)](https://github.com/user/enklayve-dev)
[![License](https://img.shields.io/badge/License-MIT-green)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.70%2B-orange)](https://www.rust-lang.org/)

## Why Enklayve?

- **Zero Setup**: The app detects your hardware, picks the best-fit model, and installs it. No model pickers, no config, no terminal.
- **Local-First & Private**: Everything runs on your device and is encrypted at rest. Fully offline by default — web access is strictly opt-in.
- **Always Free**: No subscriptions, no hidden costs, no premium tiers.
- **Capable Models**: Runs modern local models (Qwen3-class) that are genuinely good at both answering from your documents and using tools.
- **GPU Accelerated**: Automatic optimization for Apple Silicon (Metal) and NVIDIA (CUDA).

## Key Features

### Privacy & Security
- End-to-end encryption with AES-256-GCM
- Biometric authentication (Touch ID / Windows Hello / Linux Fingerprint)
- Zero-knowledge architecture
- All data stays on your device

### Ask Your Data
- Drop in a folder or individual files (PDF, DOCX, TXT, Markdown, Excel, CSV, images via OCR)
- Ask questions and get answers grounded in your documents
- Source citations you can click to open the exact passage
- Vector search + reranking for relevant context
- Honest when the answer isn't in your data — no invented citations

### A Tiny, Tasteful Agent
- Retrieve from your documents
- Fetch from the web — **opt-in and off by default**, with URL citations
- Read/write inside **one folder you designate** (sandboxed)
- No coding, no terminal, no OS automation — that's the whole toolset

### Built to Not Break
- Self-healing: resumable downloads, index rebuild, graceful out-of-memory recovery
- Plain-language errors, never a raw stack trace
- Robust ingestion with fallbacks and partial import, so one bad file never stops a batch

### Performance
- Hardware-aware, automatic model selection
- Automatic GPU acceleration (Metal, CUDA)
- Intelligent context window optimization
- Fast embedding generation

## System Requirements

### Minimum
- **RAM**: 8GB (can run 3B models)
- **Storage**: 10GB available space
- **OS**: macOS 10.15+, Windows 10+, Ubuntu 20.04+

### Recommended
- **RAM**: 16GB (can run 7B models smoothly)
- **GPU**: Apple Silicon M1+ or NVIDIA GTX 1060+
- **Storage**: 50GB available space

### For 14B+ Models
- **RAM**: 32GB+
- **GPU**: Apple M2 Pro/Max or NVIDIA RTX 3060+
- **Storage**: 100GB available space

## Installation

### macOS (Apple Silicon & Intel)

1. Download the DMG installer from [Releases](https://github.com/user/enklayve-dev/releases)
2. Open the DMG file
3. Drag Enklayve to Applications folder
4. Run the following command to allow unsigned app:
   ```bash
   xattr -cr /Applications/enklayve.app
   ```
5. Launch Enklayve from Applications

### Windows

1. Download the EXE installer from [Releases](https://github.com/user/enklayve-dev/releases)
2. Run the installer
3. Follow the installation wizard
4. Launch Enklayve from Start Menu

### Linux

1. Download the AppImage from [Releases](https://github.com/user/enklayve-dev/releases)
2. Make it executable:
   ```bash
   chmod +x enklayve-*.AppImage
   ```
3. Run the AppImage:
   ```bash
   ./enklayve-*.AppImage
   ```

## Quick Start

1. **First Launch**: Enklayve will detect your hardware and recommend the best model
2. **Download Model**: Click "Download Recommended Model" (one-time download, 1-20GB depending on model)
3. **Upload Document**: Drag and drop a PDF or document file
4. **Ask Questions**: Start chatting about your document
5. **Get Answers**: Receive intelligent responses with source citations

## Supported Document Formats

- PDF (with OCR support for scanned documents)
- Microsoft Word (.docx)
- Plain Text (.txt)
- Markdown (.md)
- Excel (.xlsx, .xls)
- CSV (.csv)

## Available Models

Enklayve automatically selects and installs the best-fit model for your machine — you don't choose one. Sizes shown are the current Qwen3-class defaults per RAM tier:

| RAM | Model class | Approx. size | Performance |
|-----|-------------|--------------|-------------|
| 8GB | ~3-4B | ~2-3GB | Fast, efficient |
| 16GB | ~7-9B | ~5GB | Balanced, recommended |
| 32GB | ~14B | ~9GB | Very smart |
| 64GB+ | ~30B+ | ~19GB+ | Maximum intelligence |

Models run with GPU acceleration (Metal/CUDA) at interactive speeds on modern hardware. Exact model choices are maintained in the app and may update as better local models ship.

## Building from Source

### Prerequisites
- Node.js 20+
- Rust 1.70+
- Platform-specific tools:
  - **macOS**: Xcode Command Line Tools
  - **Windows**: Visual Studio Build Tools
  - **Linux**: GCC, libwebkit2gtk-4.1-dev

### Build Steps

```bash
# Clone repository
git clone https://github.com/user/enklayve-dev.git
cd enklayve-dev/enklayve-app

# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

### Build Outputs

- **macOS**: `src-tauri/target/release/bundle/dmg/`
- **Windows**: `src-tauri/target/release/bundle/nsis/`
- **Linux**: `src-tauri/target/release/bundle/appimage/`

## Project Structure

```
enklayve-dev/
├── enklayve-app/          # Main Tauri application
│   ├── src/               # React frontend
│   │   ├── components/    # UI components
│   │   ├── App.tsx       # Main app component
│   │   └── main.tsx      # Entry point
│   ├── src-tauri/        # Rust backend
│   │   ├── src/
│   │   │   ├── commands.rs        # Tauri commands
│   │   │   ├── inference.rs       # LLM inference
│   │   │   ├── documents.rs       # Document processing
│   │   │   ├── embeddings.rs      # Vector embeddings
│   │   │   ├── hardware.rs        # GPU detection
│   │   │   ├── encryption.rs      # AES-256-GCM encryption
│   │   │   ├── biometric.rs       # Touch ID / Windows Hello / fprintd
│   │   │   ├── conversations.rs   # Chat history
│   │   │   ├── backup.rs          # Backup/restore
│   │   │   ├── export.rs          # Export functionality
│   │   │   └── model_cache.rs     # Model caching
│   │   └── Cargo.toml    # Rust dependencies
│   └── package.json      # Node dependencies
├── .github/
│   └── workflows/        # CI/CD pipelines
└── README.md            # This file
```

## Architecture

### Technology Stack
- **Frontend**: React 19 + TypeScript + Vite
- **Backend**: Rust + Tauri 2.0
- **LLM Engine**: llama.cpp (via llama-cpp-2)
- **Embeddings**: BGE-Small-EN-v1.5 (fastembed)
- **Database**: SQLite with encryption
- **Vector Search**: Custom cosine similarity
- **Document Processing**: pdf-extract, docx-rs, calamine (Excel), ocrs (OCR)

### GPU Acceleration

#### Apple Silicon (Automatic)
- Metal acceleration enabled by default
- Unified memory architecture for efficient GPU offloading
- Optimized layer distribution based on available RAM

#### NVIDIA GPUs (Windows/Linux)
- Automatic CUDA detection
- Dynamic GPU layer allocation
- Build with: `cargo build --release --features cuda`

## Troubleshooting

### macOS: "App can't be opened because the developer cannot be verified"
Run this command to remove quarantine attribute:
```bash
xattr -r -d com.apple.quarantine /Applications/enklayve.app
```

### Model download is slow
Models are large (1-20GB). Download times depend on your internet speed. Downloads can be resumed if interrupted.

### Out of memory during inference
Switch to a smaller model — the ~3-4B tier works well on 8GB RAM systems.

### App crashes on startup
Check system requirements. Ensure you have at least 8GB RAM and 10GB free disk space.

### GPU not detected
- **NVIDIA**: Install latest drivers and CUDA toolkit
- **Apple**: Metal is automatic on M1/M2/M3/M4 chips

### Linux: Fingerprint authentication not working
Ensure fprintd is installed and fingerprints are enrolled:
```bash
# Install fprintd
sudo apt install fprintd

# Enroll fingerprint
fprintd-enroll
```

## Privacy & Security

Enklayve is built with privacy as the foundation:

- **No telemetry**: We don't collect any usage data
- **No analytics**: No tracking, no crash reports (unless you opt-in)
- **No network calls**: Models run 100% offline
- **Encrypted storage**: All data encrypted with AES-256-GCM
- **Secure credentials**: Platform keychain integration (macOS Keychain, Windows Credential Manager, Linux Secret Service)
- **Biometric unlock**: Touch ID, Windows Hello, and Linux fingerprint (fprintd) support
- **Open source**: Full transparency, audit the code yourself

## Performance Tips

1. **Use Simple Mode**: Advanced mode loads all features, Simple mode is faster
2. **Enable Auto-tuning**: Let Enklayve optimize settings for your hardware
3. **Smaller Context Window**: Reduce context window if experiencing slowdowns
4. **Close Other Apps**: Free up RAM for better performance
5. **Use SSD**: Store models on SSD for faster loading

## Contributing

We welcome contributions! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

MIT License - see [LICENSE](LICENSE) for details.

## Support

- **Issues**: [GitHub Issues](https://github.com/user/enklayve-dev/issues)
- **Discussions**: [GitHub Discussions](https://github.com/user/enklayve-dev/discussions)
- **Documentation**: [docs/](docs/)

## Roadmap

Enklayve is becoming the dead-simple, secure way to ask your data anything —
a fully local NotebookLM plus a tiny, tasteful agent. The plan is sequenced in
[ROADMAP.md](ROADMAP.md), with specs in [openspec/](openspec/):

1. **Make the core flawless** — fix chat formatting (chat templates) and document ingestion (DOCX fallbacks, partial import).
2. **Zero-setup onboarding** — auto-detect hardware, auto-install the best-fit model.
3. **Ask your data, done well** — folder ingestion, grounded answers with clickable citations.
4. **The minimal agent** — opt-in web browsing and a sandboxed working folder. No coding, no terminal, no OS automation.
5. **Polish & trust** — self-healing, encrypted backup/restore, signed cross-platform releases.

Desktop only (macOS, Windows, Linux). No mobile apps.

## Acknowledgments

Built with:
- [Tauri](https://tauri.app/) - Desktop app framework
- [llama.cpp](https://github.com/ggerganov/llama.cpp) - LLM inference engine
- [Qwen](https://huggingface.co/Qwen) - State-of-the-art language models
- [fastembed](https://github.com/Anush008/fastembed-rs) - Fast embeddings

---

Made with care for privacy and local-first computing.
