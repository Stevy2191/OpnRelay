# Deployment

OpnRelay runs as Docker containers on a single company server. **All processing runs on
that machine**: capture uploads, rendering, exports, video transcoding. No external
services are required. SMTP and an SSO identity provider are optional integrations that
the admin configures.

## 1. Requirements (30–40 users)

| Resource | Minimum | Recommended |
|---|---|---|
| CPU | 2 vCPU | 4 vCPU (video transcoding benefits) |
| RAM | 2 GB | 4 GB |
| Disk | 20 GB | 100 GB+ SSD (≈ 300 KB per screenshot step; video is much larger) |
| OS | Any Linux with Docker Engine 24+ and Compose v2 (Windows Server/macOS with Docker Desktop also works for testing) | |
| Network | Port 443 reachable from user devices (LAN/VPN) | Internal DNS name, e.g. `guides.company.local` |

## 2. One-line install

On a Linux server (Ubuntu, Debian, RHEL, ...), as root:

```bash
curl -fsSL https://raw.githubusercontent.com/stevy2191/opnrelay/main/deploy/install.sh | sudo bash
```

The installer:
1. installs Docker (official `get.docker.com` script) if it is missing;
2. asks how people reach OpnRelay:
   - **HTTPS with Caddy** (recommended): Caddy runs on ports 80/443 and gets
     certificates automatically, either from its own internal CA (internal names; trust
     its root certificate on PCs, `opnrelay ca-cert`) or from Let's Encrypt (public DNS
     name; give an email address);
   - **your own reverse proxy** (nginx, IIS, Traefik, a load balancer...): OpnRelay
     listens on `127.0.0.1:8080` (configurable) and trusts the proxy's
     `X-Forwarded-For`;
   - **plain HTTP for testing**: OpnRelay listens on `0.0.0.0:<port>`;
3. asks for the company name and administrator email;
4. writes `/opt/opnrelay/.env`, generates secrets (`/opt/opnrelay/secrets/`), pulls the
   image `ghcr.io/stevy2191/opnrelay`, starts everything and waits until it is healthy;
5. prints a **one-time link** for the administrator to set a password, and installs the
   `opnrelay` helper command.

Unattended: pass the answers as variables, e.g.
`... | sudo OPNRELAY_MODE=caddy OPNRELAY_HOST=guides.acme.local OPNRELAY_TLS=internal
OPNRELAY_ORG="Acme" OPNRELAY_ADMIN_EMAIL=it@acme.com OPNRELAY_YES=1 bash`
(see the header of [`deploy/install.sh`](../deploy/install.sh) for all variables).

**Back up `/opt/opnrelay/secrets/master_key` somewhere safe and separate.** Every
screenshot and every backup is encrypted with it.

### The `opnrelay` command

| Command | Does |
|---|---|
| `opnrelay status` | Show the containers |
| `opnrelay logs [service]` | Follow logs (default: server) |
| `opnrelay update` | Pull the latest image and restart (migrations run automatically) |
| `opnrelay backup` | Run a backup now |
| `opnrelay admin-link EMAIL` | Create/recover an owner and print a one-time link |
| `opnrelay ca-cert` | Print Caddy's root certificate (internal CA mode) |
| `opnrelay verify-audit` | Check the audit log hash chain |
| `opnrelay start / stop / restart / version` | |

### Manual / from source

```bash
git clone https://github.com/stevy2191/opnrelay.git && cd opnrelay/deploy
cp .env.example .env            # edit: COMPOSE_FILE, OPNRELAY_PUBLIC_URL, ...
sudo ./generate-secrets.sh
docker compose -f compose.yaml -f compose.caddy.yaml -f compose.build.yaml up -d --build
docker compose exec server opnrelay-server admin create-owner --email you@company.com
```

## 3. Compose services

| File | Contents |
|---|---|
| [`compose.yaml`](../deploy/compose.yaml) | `server` (distroless, non-root, read-only filesystem, no capabilities) and `db` (PostgreSQL 16, not exposed) |
| [`compose.caddy.yaml`](../deploy/compose.caddy.yaml) | Adds Caddy on 80/443 (HTTPS mode) |
| [`compose.port.yaml`](../deploy/compose.port.yaml) | Publishes the server port on `OPNRELAY_HTTP_BIND` (own-proxy and HTTP modes) |
| [`compose.build.yaml`](../deploy/compose.build.yaml) | Builds the image from source instead of pulling |

`COMPOSE_FILE` in `.env` selects the combination. Secrets are passed as files
(`*_FILE` variables), never as plain environment values.

### Image
- Multi-stage build: web UI built with Node, server built with Rust, final image based on `gcr.io/distroless/cc-debian12:nonroot` (no shell).
- The web UI is embedded in the server binary.
- **Desktop recorder installers and extension packages are bundled in the image**
  (`/opt/opnrelay/downloads`) by the release workflow and offered to administrators on
  the in-app **Downloads** page. Nothing is downloaded at install time.
- Published by [`.github/workflows/release.yml`](../.github/workflows/release.yml) on
  version tags: `ghcr.io/stevy2191/opnrelay:<version>` and `:latest` (linux/amd64).
  Planned: arm64, Sigstore signatures and an SBOM.

### Server CLI

```
opnrelay-server serve                      # run (applies migrations first)
opnrelay-server migrate                    # apply database migrations only
opnrelay-server admin create-owner ...     # create or recover an owner (prints a one-time link)
opnrelay-server keys generate              # print a new random 256-bit key
opnrelay-server audit verify               # verify the audit log hash chain (exit 2 if broken)
opnrelay-server health                     # container health probe
```

## 4. Configuration (`.env`)

| Variable | Example | Notes |
|---|---|---|
| `OPNRELAY_DOMAIN` | `guides.company.local` | required (Compose); the server's public URL is `https://<domain>` |
| `OPNRELAY_TLS` | `internal` or `you@company.com` | Caddy's internal CA, or Let's Encrypt for public names |
| `OPNRELAY_VERSION` | `0.1.0` | image tag |
| `OPNRELAY_MAX_UPLOAD_MB` | `50` | maximum upload size |

Server variables (set by `compose.yaml`; relevant when running the binary directly):

| Variable | Default | Notes |
|---|---|---|
| `OPNRELAY_PUBLIC_URL` | (required) | external URL; `https://` enables secure cookies and HSTS |
| `OPNRELAY_DATABASE_URL[_FILE]` | (required) | PostgreSQL URL |
| `OPNRELAY_MASTER_KEY[_FILE]` | (required) | 64 hex chars; see `keys generate` |
| `OPNRELAY_STORAGE` | `fs:///data/media` | encrypted object store directory |
| `OPNRELAY_BIND` | `0.0.0.0:8080` | listen address |
| `OPNRELAY_TRUST_PROXY` | `false` | use `X-Forwarded-For` from the reverse proxy for client IPs |
| `OPNRELAY_LOG_FORMAT` | text | `json` for structured logs |
| `OPNRELAY_LOG` | `info,sqlx=warn` | log filter |

Organisation policies (MFA, SSO, sharing, retention, redaction) are configured in the UI,
not via environment variables, so they are audited.

## 5. TLS

Caddy options:
- **Public DNS name:** automatic Let's Encrypt.
- **Internal-only name:** `tls internal` (Caddy's local CA; distribute its root cert via GPO/MDM), or mount the company's own certificate and key.

## 6. Backups & restore

Configured in the app under **Admin → Backups** (see [microsoft.md](microsoft.md) for
OneDrive):
- Daily backup at a chosen hour, kept on the server (`/data/backups` in the `media`
  volume; newest *N* kept) and optionally copied to **OneDrive** (newest *M* kept).
- Each backup is one encrypted file with the database snapshot and all screenshots.
- **Back up now** button, download links, last result shown in the app; every run is
  in the audit log.

Restore on a new server with the installer (`OPNRELAY_RESTORE` + `OPNRELAY_RESTORE_KEY`,
see [microsoft.md §4](microsoft.md#4-restoring)) or manually:
`docker compose cp file server:/data/backups/x.orbk && docker compose exec server
opnrelay-server backup restore /data/backups/x.orbk` (into an empty installation).

Also keep a copy of the **master key** offline: without it backups cannot be decrypted.

## 7. Upgrades

```bash
opnrelay update        # or: set OPNRELAY_VERSION in /opt/opnrelay/.env first to pin a version
```
Migrations run automatically on start (forward-only, transactional). Take a backup first.
Release notes flag any breaking changes.

## 8. Client rollout

The desktop apps are **not signed with paid Apple/Microsoft certificates**
([ADR 0007](adr/0007-no-paid-code-signing.md)). They are safe to install. The
operating system just shows a one-time "unknown developer" warning. Every release file
comes with a SHA-256 checksum and a free Sigstore signature so IT can verify it.

### Windows
- **Manual install:** SmartScreen shows "Windows protected your PC" → **More info → Run anyway** (first install only).
- **No warning (recommended):** push the MSI with Intune, SCCM or Group Policy. Managed deployment doesn't show the SmartScreen prompt.
- **Optional, free:** if the company runs an internal certificate authority (Active Directory Certificate Services), IT can sign the MSI with an internal code-signing certificate and trust it via Group Policy. Open-source projects can also apply to the free **SignPath Foundation** programme for a publicly trusted certificate.

### macOS
- Builds are signed with a free **self-signed** certificate held by the project. This keeps the app's identity stable between updates, so the Accessibility and Screen Recording permissions don't have to be re-granted after every update.
- **First launch:** macOS blocks the app once. Go to **System Settings → Privacy & Security → Open Anyway** (macOS 15+), or IT runs `xattr -dr com.apple.quarantine /Applications/OpnRelay.app`.
- **Managed Macs:** deploy the `.pkg` via the company MDM (e.g. Jamf, Intune), and pre-approve the permissions with a PPPC profile.
- Not available without the paid Apple programme: notarisation and Homebrew cask distribution.

### Linux
- No signing fees. `.deb`/`.rpm` repositories are signed with a free project GPG key; AppImage has a detached signature.

### Updates
- The desktop app updates itself from **your own OpnRelay server** (`/downloads`), verified with the project's ed25519 update key (free, built into Tauri).

### Browser extension
- **Firefox:** signed by Mozilla for free (listed on AMO or unlisted for self-hosting).
- **Edge Add-ons:** free.
- **Chrome Web Store:** a one-time US$5 developer registration (not yearly). Alternatively, skip the store and force-install the self-hosted `.crx` via Chrome/Edge enterprise policy on managed machines.
- The server URL is preconfigured through managed storage in all cases.

### Getting the installers
Administrators download them from the in-app **Downloads** page (admin only). The page
shows the server address to enter in the recorder.

### Server URL preconfiguration
- Windows: `msiexec /i OpnRelay-Recorder-Windows-x64.msi SERVERURL=https://guides.company.com /qn`
  (sets `OPNRELAY_SERVER_URL` machine-wide).
- macOS / Linux: set the `OPNRELAY_SERVER_URL` environment variable (MDM profile, `/etc/environment`).

## 9. Hardening checklist

- [ ] Server reachable only from LAN/VPN (unless public links are needed externally)
- [ ] MFA required for all users (or SSO-only with IdP MFA)
- [ ] Break-glass Owner account with hardware key MFA, credentials stored offline
- [ ] Workspaces created per department (HR, EMS, …) with the right members
- [ ] Nightly backups + master key escrow verified
- [ ] Host OS patched; Docker images updated monthly (subscribe to release notifications)
- [ ] Audit log export scheduled to off-box storage / SIEM
