# ADR 0007: No paid code-signing certificates

**Status:** Accepted

## Context
Apple notarisation needs the Apple Developer Program (US$99/year) and publicly trusted Windows signing certificates cost US$200–500/year. The project owner does not want recurring fees for the apps.

## Decision
Ship desktop apps without paid OS vendor signing, and use only free mechanisms:

| Platform | Mechanism |
|---|---|
| All | Tauri updater ed25519 signatures (project key); SHA-256 checksums; Sigstore (cosign keyless) signatures for every release file; SBOM |
| Windows | Unsigned MSI by default. Optional: company internal CA (AD CS) signing, or the free SignPath Foundation programme for open-source projects |
| macOS | Signed with a stable **self-signed** code-signing identity (keeps permission grants across updates); not notarised |
| Linux | GPG-signed package repositories; detached AppImage signatures |
| Extensions | Firefox AMO signing (free); Edge Add-ons (free); Chrome via the one-time US$5 store fee or enterprise force-install of a self-hosted `.crx` |

## Consequences
- First install shows an OS warning ("Windows protected your PC" / macOS "Open Anyway"). This doesn't happen with managed deployment (Intune, SCCM, GPO, MDM), which is how most of the 30–40 users will receive the app. Deployment docs give step-by-step instructions.
- macOS: no notarisation and no Homebrew cask. On macOS 15+ users approve the app once in **System Settings → Privacy & Security**.
- Integrity is still verifiable: releases are reproducible-build targets with checksums and Sigstore transparency-log signatures, and updates come only from the company's own server and are verified with the update key.
- This can be revisited later (e.g. if SignPath approves the project, or if the company later decides to pay) without architectural changes.
