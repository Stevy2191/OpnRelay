#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
#
# OpnRelay one-line installer:
#
#   curl -fsSL https://raw.githubusercontent.com/stevy2191/opnrelay/main/deploy/install.sh | sudo bash
#
# Installs Docker if needed, asks a few questions, generates secrets, starts
# OpnRelay and prints a one-time link to set the administrator's password.
#
# Unattended install: set the answers as environment variables, e.g.
#   curl -fsSL .../install.sh | sudo OPNRELAY_MODE=caddy OPNRELAY_HOST=guides.acme.local \
#        OPNRELAY_ADMIN_EMAIL=it@acme.com OPNRELAY_ORG="Acme" OPNRELAY_YES=1 bash
#
# Variables: OPNRELAY_MODE (caddy|proxy|http), OPNRELAY_HOST, OPNRELAY_TLS
# (internal|<email>), OPNRELAY_PUBLIC_URL (proxy mode), OPNRELAY_HTTP_BIND,
# OPNRELAY_HTTP_PORT, OPNRELAY_ADMIN_EMAIL, OPNRELAY_ADMIN_NAME, OPNRELAY_ORG,
# OPNRELAY_DIR (default /opt/opnrelay), OPNRELAY_VERSION (default latest),
# OPNRELAY_IMAGE, OPNRELAY_SOURCE (where to fetch deploy files), OPNRELAY_YES=1.
#
# Disaster recovery (new server, restore a backup instead of a fresh start):
#   OPNRELAY_RESTORE=/path/opnrelay-backup-....orbk OPNRELAY_RESTORE_KEY=/path/master_key
set -euo pipefail

REPO_RAW="https://raw.githubusercontent.com/stevy2191/opnrelay/main/deploy"
SOURCE="${OPNRELAY_SOURCE:-$REPO_RAW}"
DIR="${OPNRELAY_DIR:-/opt/opnrelay}"
VERSION="${OPNRELAY_VERSION:-latest}"
IMAGE="${OPNRELAY_IMAGE:-ghcr.io/stevy2191/opnrelay}"

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
info() { printf '  %s\n' "$*"; }
warn() { printf '\033[33m! %s\033[0m\n' "$*"; }
die() { printf '\033[31mError: %s\033[0m\n' "$*" >&2; exit 1; }

# Questions are read from the terminal even when this script is piped from curl.
TTY=/dev/tty
ask() { # ask VAR "Question" "default"
  local var="$1" q="$2" def="${3:-}" ans=""
  if [ -n "${!var:-}" ]; then return; fi
  if [ "${OPNRELAY_YES:-}" = "1" ] || [ ! -r "$TTY" ]; then
    [ -n "$def" ] || die "$var is required for an unattended install"
    printf -v "$var" '%s' "$def"; return
  fi
  if [ -n "$def" ]; then printf '%s [%s]: ' "$q" "$def" > "$TTY"; else printf '%s: ' "$q" > "$TTY"; fi
  IFS= read -r ans < "$TTY" || true
  printf -v "$var" '%s' "${ans:-$def}"
}

fetch() { # fetch NAME -> $DIR/NAME
  case "$SOURCE" in
    http*) curl -fsSL "$SOURCE/$1" -o "$DIR/$1" || die "could not download $1 from $SOURCE" ;;
    *) cp "$SOURCE/$1" "$DIR/$1" || die "could not copy $1 from $SOURCE" ;;
  esac
}

# --------------------------------------------------------------- checks --
[ "$(id -u)" -eq 0 ] || die "run as root (put sudo before bash)"
[ "$(uname -s)" = "Linux" ] || die "the installer supports Linux servers (use Docker Desktop and deploy/compose.yaml elsewhere)"
command -v curl >/dev/null || die "curl is required"

bold "OpnRelay installer"
if [ -f "$DIR/.env" ]; then
  warn "OpnRelay is already installed in $DIR. Use 'opnrelay update' to upgrade."
  exit 0
fi

if ! command -v docker >/dev/null 2>&1; then
  bold "Installing Docker…"
  curl -fsSL https://get.docker.com | sh
  systemctl enable --now docker >/dev/null 2>&1 || true
fi
docker compose version >/dev/null 2>&1 || die "Docker Compose v2 is missing (install the docker-compose-plugin package)"

# ------------------------------------------------------------ questions --
echo
bold "How should people reach OpnRelay?"
info "1) HTTPS with Caddy (recommended) - certificates handled automatically on ports 80/443"
info "2) My own reverse proxy (nginx, IIS, Traefik, load balancer...) terminates HTTPS"
info "3) Plain HTTP - for testing only, passwords travel unencrypted"
case "${OPNRELAY_MODE:-}" in
  caddy) CHOICE=1 ;; proxy) CHOICE=2 ;; http) CHOICE=3 ;; "") ask CHOICE "Choose 1, 2 or 3" "1" ;;
  *) die "OPNRELAY_MODE must be caddy, proxy or http" ;;
esac

DEFAULT_HOST="$(hostname -f 2>/dev/null || hostname)"
DEFAULT_IP="$(hostname -I 2>/dev/null | awk '{print $1}')"
HTTP_BIND=""; DOMAIN=""; TLS="internal"; TRUST="true"
case "$CHOICE" in
  1)
    MODE=caddy
    ask OPNRELAY_HOST "Hostname people will use (DNS must point to this server)" "$DEFAULT_HOST"
    DOMAIN="$OPNRELAY_HOST"
    info "Certificates: 'internal' = Caddy's own CA (internal names; trust its root certificate on PCs),"
    info "              or an email address = free Let's Encrypt certificates (public DNS name, port 80 open)."
    ask OPNRELAY_TLS "Certificates (internal or your email)" "internal"
    TLS="$OPNRELAY_TLS"
    PUBLIC_URL="https://$DOMAIN"
    COMPOSE="compose.yaml:compose.caddy.yaml"
    ;;
  2)
    MODE=proxy
    ask OPNRELAY_PUBLIC_URL "Public address of your proxy (https://...)" "https://$DEFAULT_HOST"
    PUBLIC_URL="${OPNRELAY_PUBLIC_URL%/}"
    case "$PUBLIC_URL" in https://*) ;; *) warn "the address does not start with https:// (secure cookies will be off)";; esac
    ask OPNRELAY_HTTP_BIND "Where OpnRelay listens for the proxy (127.0.0.1:PORT if the proxy is on this machine)" "127.0.0.1:8080"
    HTTP_BIND="$OPNRELAY_HTTP_BIND"
    COMPOSE="compose.yaml:compose.port.yaml"
    ;;
  3)
    MODE=http
    ask OPNRELAY_HTTP_PORT "HTTP port" "8080"
    HTTP_BIND="0.0.0.0:$OPNRELAY_HTTP_PORT"
    PUBLIC_URL="http://${OPNRELAY_HOST:-${DEFAULT_IP:-localhost}}:$OPNRELAY_HTTP_PORT"
    TRUST="false"
    COMPOSE="compose.yaml:compose.port.yaml"
    warn "Plain HTTP is for testing. Switch to option 1 or 2 before real use."
    ;;
  *) die "please choose 1, 2 or 3" ;;
esac

RESTORE="${OPNRELAY_RESTORE:-}"
if [ -n "$RESTORE" ]; then
  [ -f "$RESTORE" ] || die "backup file not found: $RESTORE"
  [ -f "${OPNRELAY_RESTORE_KEY:-}" ] || die "set OPNRELAY_RESTORE_KEY to the master_key file saved from the old server"
  info "Restoring from $(basename "$RESTORE"): users, guides and settings come from the backup."
else
  ask OPNRELAY_ORG "Company name" "My Company"
  ask OPNRELAY_ADMIN_EMAIL "Administrator email" ""
  ask OPNRELAY_ADMIN_NAME "Administrator name" "Administrator"
fi

# ---------------------------------------------------------------- files --
bold "Setting up $DIR…"
mkdir -p "$DIR"
chmod 755 "$DIR"
for f in compose.yaml compose.caddy.yaml compose.port.yaml Caddyfile generate-secrets.sh; do fetch "$f"; done
chmod +x "$DIR/generate-secrets.sh"
umask 077
cat > "$DIR/.env" <<EOF
# Written by the OpnRelay installer ($(date -u +%Y-%m-%d)). Mode: $MODE
COMPOSE_FILE=$COMPOSE
OPNRELAY_PUBLIC_URL=$PUBLIC_URL
OPNRELAY_DOMAIN=$DOMAIN
OPNRELAY_TLS=$TLS
OPNRELAY_HTTP_BIND=${HTTP_BIND:-127.0.0.1:8080}
OPNRELAY_TRUST_PROXY=$TRUST
OPNRELAY_IMAGE=$IMAGE
OPNRELAY_VERSION=$VERSION
OPNRELAY_MAX_UPLOAD_MB=50
EOF
umask 022
if [ -n "$RESTORE" ]; then
  mkdir -p "$DIR/secrets"
  install -m 600 "$OPNRELAY_RESTORE_KEY" "$DIR/secrets/master_key"
fi
"$DIR/generate-secrets.sh" >/dev/null

# --------------------------------------------------------------- start --
cd "$DIR"
bold "Downloading and starting OpnRelay (this can take a minute)…"
if [ "${OPNRELAY_SKIP_PULL:-}" != "1" ]; then
  docker compose pull --quiet || die "could not download the OpnRelay image $IMAGE:$VERSION"
fi
docker compose up -d --quiet-pull >/dev/null 2>&1
for _ in $(seq 1 90); do
  if [ "$(docker compose ps server --format '{{.Health}}' 2>/dev/null)" = "healthy" ]; then break; fi
  sleep 2
done
[ "$(docker compose ps server --format '{{.Health}}')" = "healthy" ] || die "OpnRelay did not start; see 'docker compose -f $DIR/compose.yaml logs server'"

LINK=""
if [ -n "$RESTORE" ]; then
  bold "Restoring the backup…"
  TMP="$(mktemp)"; cp "$RESTORE" "$TMP"; chmod 644 "$TMP"
  docker compose cp "$TMP" server:/data/restore.orbk >/dev/null 2>&1 || { rm -f "$TMP"; die "could not copy the backup into the container"; }
  rm -f "$TMP"
  docker compose exec -T server opnrelay-server backup restore /data/restore.orbk || die "restore failed (is OPNRELAY_RESTORE_KEY the right master key?)"
else
  LINK="$(docker compose exec -T server opnrelay-server admin create-owner \
    --email "$OPNRELAY_ADMIN_EMAIL" --name "$OPNRELAY_ADMIN_NAME" --org-name "$OPNRELAY_ORG" | grep -o 'http[^ ]*#/invite/[A-Za-z0-9_-]*' || true)"
fi

# ------------------------------------------------------- helper command --
cat > /usr/local/bin/opnrelay <<EOF
#!/usr/bin/env bash
# OpnRelay maintenance helper (installed by install.sh).
set -euo pipefail
cd "$DIR"
case "\${1:-help}" in
  status)  docker compose ps ;;
  logs)    docker compose logs --tail=200 -f "\${2:-server}" ;;
  start)   docker compose up -d ;;
  stop)    docker compose stop ;;
  restart) docker compose restart ;;
  update)  docker compose pull && docker compose up -d && docker image prune -f >/dev/null && docker compose ps ;;
  admin-link) docker compose exec -T server opnrelay-server admin create-owner --email "\${2:?usage: opnrelay admin-link EMAIL}" ;;
  backup)  docker compose exec -T server opnrelay-server backup run ;;
  verify-audit) docker compose exec -T server opnrelay-server audit verify ;;
  ca-cert) docker compose exec -T caddy cat /data/caddy/pki/authorities/local/root.crt ;;
  version) docker compose exec -T server opnrelay-server --version ;;
  *) echo "usage: opnrelay status|logs [service]|start|stop|restart|update|admin-link EMAIL|backup|verify-audit|ca-cert|version" ;;
esac
EOF
chmod 755 /usr/local/bin/opnrelay

# -------------------------------------------------------------- summary --
echo
bold "OpnRelay is running at $PUBLIC_URL"
echo
if [ -n "$RESTORE" ]; then
  bold "Backup restored. Everyone signs in with their existing accounts."
elif [ -n "$LINK" ]; then
  bold "Next step: open this one-time link to set the administrator password (valid 7 days):"
  echo "  $LINK"
else
  warn "Could not create the administrator link. Run: opnrelay admin-link $OPNRELAY_ADMIN_EMAIL"
fi
echo
bold "Important: back up this file somewhere safe and separate from the server:"
echo "  $DIR/secrets/master_key"
info "Every screenshot and backup is encrypted with it. Without it they cannot be restored."
echo
if [ "$MODE" = "caddy" ] && [ "$TLS" = "internal" ]; then
  info "Browsers will warn about the certificate until PCs trust Caddy's root certificate."
  info "Get it with 'opnrelay ca-cert > opnrelay-root.crt' and deploy it via Group Policy / Intune."
fi
if [ "$MODE" = "proxy" ]; then
  info "Point your reverse proxy at http://$HTTP_BIND and pass the X-Forwarded-For header."
fi
info "The desktop recorder installers are in the app: sign in as an admin and open Downloads."
info "Manage the install with: opnrelay status | logs | update | backup"
