#!/usr/bin/env bash
# Ask the server to deploy main. This is the same command CI runs.
#
#   ./deploy/deploy.sh
#   DEPLOY_HOST=team-09 ./deploy/deploy.sh
#
# It deliberately deploys the server's main checkout, never the local working
# tree: production must not contain uncommitted development work.
set -euo pipefail

HOST=${DEPLOY_HOST:-team-09}
ssh "$HOST" /usr/local/bin/nullbox-deploy
