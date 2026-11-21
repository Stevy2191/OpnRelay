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
cp .env.example .env          # set OPNRELAY_PUBLIC_URL, etc.
./generate-secrets.sh         # creates ./secrets/{master_key,db_password,session_key}
docker compose up -d
docker compose exec server opnrelay-server admin create-owner --email you@company.com
```

Then browse to `https://<your-host>`, log in, enrol MFA, set branding, and invite users.

## 3. Compose services

```yaml
# deploy/compose.yaml (planned)
services:
  caddy:
    image: caddy:2
    ports: ["443:443", "80:80"]
    volumes: [./Caddyfile:/etc/caddy/Caddyfile:ro, caddy_data:/data]
    depends_on: [server]

  server:
    image: ghcr.io/stevy2191/opnrelay:${OPNRELAY_VERSION}
    command: ["opnrelay-server", "serve", "--with-worker"]
    env_file: .env
    environment:
      OPNRELAY_DATABASE_URL_FILE: /run/secrets/database_url
      OPNRELAY_MASTER_KEY_FILE: /run/secrets/master_key
      OPNRELAY_SESSION_KEY_FILE: /run/secrets/session_key
      OPNRELAY_STORAGE: fs:///data/media
    secrets: [database_url, master_key, session_key]
    volumes: [media:/data/media]
    read_only: true
    tmpfs: [/tmp]
    cap_drop: [ALL]
    security_opt: ["no-new-privileges:true"]
    user: "10001:10001"
    depends_on: { db: { condition: service_healthy } }

  db:
    image: postgres:16-alpine
    environment:
      POSTGRES_DB: opnrelay
      POSTGRES_USER: opnrelay
      POSTGRES_PASSWORD_FILE: /run/secrets/db_password
    secrets: [db_password]
    volumes: [pgdata:/var/lib/postgresql/data]
    healthcheck: { test: ["CMD", "pg_isready", "-U", "opnrelay"], interval: 5s }

secrets:
  master_key:   { file: ./secrets/master_key }
  session_key:  { file: ./secrets/session_key }
  db_password:  { file: ./secrets/db_password }
  database_url: { file: ./secrets/database_url }

volumes: { pgdata: {}, media: {}, caddy_data: {} }
```

The database is **not** exposed on any host port. Only Caddy publishes ports.

### Image
- Multi-stage build: Rust (musl, static) + web assets built in Node → final image based on `gcr.io/distroless/static` (or `cgr.dev/chainguard/static`), plus a separate `-worker` variant that includes `ffmpeg`. Video features are disabled when using the slim image.
- Multi-arch: `linux/amd64`, `linux/arm64`.
- Signed with Sigstore `cosign`; SBOM attached.

## 4. Configuration (`.env`)

| Variable | Example | Notes |
|---|---|---|
| `OPNRELAY_PUBLIC_URL` | `https://guides.company.local` | required |
| `OPNRELAY_VERSION` | `0.1.0` | image tag |
| `OPNRELAY_STORAGE` | `fs:///data/media` or `s3://bucket?endpoint=https://minio.local` | |
| `OPNRELAY_SMTP_URL` | `smtps://user@mail.company.com:465` | optional; password via secret file |
| `OPNRELAY_TRUSTED_PROXIES` | `172.16.0.0/12` | for client IP in audit log |
| `OPNRELAY_LOG_FORMAT` | `json` | |
| `OPNRELAY_METRICS` | `false` | enable `/metrics` on an internal port |
| `OPNRELAY_MAX_UPLOAD_MB` | `200` | |

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
