#!/usr/bin/env bash
# Copy this working tree to the team box and rebuild/restart there.
#
#   ./deploy/deploy.sh
#   DEPLOY_HOST=team-09 DEPLOY_DIR=/srv/nullbox ./deploy/deploy.sh
#
# The server keeps its own .env; it is excluded from the copy and never leaves
# the box. The build runs on the server because that is where Docker is; on a
# small instance, give it swap first (see deploy/README.md).
set -euo pipefail

HOST=${DEPLOY_HOST:-team-09}
DIR=${DEPLOY_DIR:-/srv/nullbox}

cd "$(dirname "$0")/.."

ssh "$HOST" "install -d -m 0755 '$DIR'"

rsync -az --delete \
  --exclude '.git/' \
  --exclude 'target/' \
  --exclude 'node_modules/' \
  --exclude 'apps/web/node_modules/' \
  --exclude 'apps/web/dist/' \
  --exclude '*.db' \
  --exclude '*.db-shm' \
  --exclude '*.db-wal' \
  --exclude '.env' \
  ./ "$HOST:$DIR/"

ssh "$HOST" "cd '$DIR' && docker compose up -d --build && docker compose ps"
