# Releasing Enklayve

Pushing a `v*` tag triggers [`.github/workflows/release.yml`](.github/workflows/release.yml),
which builds installers for macOS (Apple Silicon + Intel), Windows, and Linux and
attaches them to a draft GitHub Release. The matrix does not fail-fast, so one
platform failing still publishes the others.

```bash
git tag v0.2.0
git push origin v0.2.0
```

Out of the box the builds are **ad-hoc signed** (no Apple/Windows certificates),
so users run the documented `xattr -cr` step on macOS. Everything below is
optional and makes releases properly signed; the build succeeds whether or not
these secrets are set.

## macOS: Developer ID signing + notarization

Add these repo secrets (Settings → Secrets and variables → Actions):

| Secret | What it is |
|--------|------------|
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_CERTIFICATE` | base64 of your `.p12` Developer ID cert |
| `APPLE_CERTIFICATE_PASSWORD` | the `.p12` password |
| `APPLE_ID` | your Apple ID email (for notarization) |
| `APPLE_PASSWORD` | an app-specific password for that Apple ID |
| `APPLE_TEAM_ID` | your 10-char team id |

With these set, `tauri-action` imports the cert, signs, and notarizes. When they
are absent, `APPLE_SIGNING_IDENTITY` falls back to `-` (ad-hoc).

## Windows: Authenticode signing

Set `bundle.windows.certificateThumbprint` in
[`tauri.conf.json`](enklayve-app/src-tauri/tauri.conf.json) to your installed
code-signing certificate's thumbprint (or add a cert-import step to the workflow).
The `timestampUrl` is already configured.

## Auto-updater (optional, off by default)

The updater is **disabled** (`plugins.updater.active: false`) because it needs a
signing key. To enable it:

1. Generate a keypair: `npm run tauri signer generate -- -w ~/.tauri/enklayve.key`
2. Put the **public** key in `tauri.conf.json` → `plugins.updater.pubkey`.
3. Add the **private** key and its password as the repo secrets
   `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
   (already wired in the workflow).
4. Set `plugins.updater.active: true`.

The update endpoint points at this repo's latest release
(`clay-good/enklayve-ai`); change it if the repo moves.
