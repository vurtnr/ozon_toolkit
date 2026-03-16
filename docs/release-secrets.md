# Release Secrets Reference

This project uses `tauri-apps/tauri-action` in `.github/workflows/release.yml`.

## Mandatory

- `GITHUB_TOKEN`
  - Provided by GitHub Actions automatically.
  - Required for creating/updating draft releases.

## macOS Signing / Notarization (Recommended for distribution)

- `APPLE_CERTIFICATE`
  - Base64-encoded `.p12` certificate content.
- `APPLE_CERTIFICATE_PASSWORD`
  - Password for the `.p12` certificate.
- `APPLE_SIGNING_IDENTITY`
  - Signing identity name visible in Keychain.
- `APPLE_ID`
  - Apple Developer account ID/email.
- `APPLE_PASSWORD`
  - App-specific password for notarization.
- `APPLE_TEAM_ID`
  - Apple Developer Team ID.

## Tauri Updater Signature (Optional, required if updater is enabled)

- `TAURI_SIGNING_PRIVATE_KEY`
  - Private key generated for updater signatures.
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
  - Password for the updater private key.

## Windows Signing (Optional but recommended)

- `WINDOWS_CERTIFICATE`
  - Base64-encoded signing certificate.
- `WINDOWS_CERTIFICATE_PASSWORD`
  - Password for Windows signing certificate.

## Setup Notes

1. Add secrets in GitHub: `Settings -> Secrets and variables -> Actions`.
2. Keep `releaseDraft: true` until signing/notarization flow is verified.
3. After validation, switch to non-draft release if desired.
