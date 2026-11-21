#!/bin/sh
# SPDX-License-Identifier: AGPL-3.0-only
# Creates ./secrets/{master_key,db_password,database_url}. Never overwrites.
set -eu
cd "$(dirname "$0")"
umask 077
mkdir -p secrets
rand() { od -An -N32 -tx1 /dev/urandom | tr -d ' \n'; }
if [ ! -f secrets/master_key ]; then
  rand > secrets/master_key
  echo "Created secrets/master_key  <-- BACK THIS UP SEPARATELY. Without it, screenshots cannot be decrypted."
fi
if [ ! -f secrets/db_password ]; then
  rand > secrets/db_password
fi
if [ ! -f secrets/database_url ]; then
  printf 'postgres://opnrelay:%s@db:5432/opnrelay' "$(cat secrets/db_password)" > secrets/database_url
fi
# The server container runs as uid 65532 and must be able to read its secrets.
if [ "$(id -u)" = "0" ]; then
  chown 65532:65532 secrets/master_key secrets/database_url
else
  chmod 644 secrets/master_key secrets/database_url
  echo "Note: run this script with sudo to restrict the secrets to the container user only."
fi
echo "Secrets ready in $(pwd)/secrets"
