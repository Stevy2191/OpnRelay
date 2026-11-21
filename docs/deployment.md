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

## 2. Quick start

```bash
git clone https://github.com/stevy2191/opnrelay.git
cd opnrelay/deploy
cp .env.example .env          # set OPNRELAY_DOMAIN (and OPNRELAY_TLS)
sudo ./generate-secrets.sh    # creates ./secrets/{master_key,db_password,database_url}
docker compose up -d --build  # builds the image locally (or pulls a published tag)
docker compose exec server opnrelay-server admin create-owner \
    --email you@company.com --name "Your Name" --org-name "Your Company"
```

The last command prints a **one-time link**. Open it, set your password, sign in, turn on
two-factor authentication under *Account*, create a workspace per team, and invite users
from *Admin*. Invitation and password-reset links are shown to the admin to pass on (email
delivery is optional and comes later), and they expire after 7 days (invites) or
24 hours (resets).

**Back up `deploy/secrets/master_key` somewhere safe and separate.** Every screenshot is
encrypted with it.

## 3. Compose services

See [`deploy/compose.yaml`](../deploy/compose.yaml). In short:

| Service | Image | Notes |
|---|---|---|
| `caddy` | `caddy:2` | The only service with published ports (80/443). TLS via its internal CA or Let's Encrypt. |
| `server` | built from [`deploy/Dockerfile`](../deploy/Dockerfile) | Distroless, non-root (uid 65532), read-only root filesystem, all capabilities dropped, `no-new-privileges`. Encrypted media in the `media` volume. Built-in health check (`opnrelay-server health`). |
| `db` | `postgres:16-alpine` | Not exposed outside the Compose network. |

Secrets are passed as files (`*_FILE` variables), never as plain environment values.

### Image
- Multi-stage build: web UI built with Node, server built with Rust, final image based on `gcr.io/distroless/cc-debian12:nonroot` (~49 MB, no shell).
- The web UI is embedded in the server binary.
- Planned for releases: multi-arch (`linux/amd64`, `linux/arm64`), Sigstore `cosign` signatures and an SBOM.

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

- **Database:** `docker compose exec db pg_dump -Fc opnrelay > backup.dump` (nightly, via cron or the included `backup` profile service).
- **Media:** back up the `media` volume (already encrypted at rest).
- **Master key:** back up **separately** (password manager / offline). Without it, media cannot be decrypted. With it plus a backup, everything can be.
- Restore is documented and tested in CI (`deploy/tests/restore.sh`).

## 7. Upgrades

```bash
# set OPNRELAY_VERSION in .env
docker compose pull && docker compose up -d
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

### Server URL preconfiguration
- MSI property / config file `OPNRELAY_SERVER_URL`; macOS configuration profile; Linux `/etc/opnrelay/config.toml`.

## 9. Hardening checklist

- [ ] Server reachable only from LAN/VPN (unless public links are needed externally)
- [ ] MFA required for all users (or SSO-only with IdP MFA)
- [ ] Break-glass Owner account with hardware key MFA, credentials stored offline
- [ ] Workspaces created per department (HR, EMS, …) with the right members
- [ ] Nightly backups + master key escrow verified
- [ ] Host OS patched; Docker images updated monthly (subscribe to release notifications)
- [ ] Audit log export scheduled to off-box storage / SIEM
