#!/usr/bin/env bash
# Run on team-09 as root to authorize the CI deploy key:
#
#   sudo bash deploy/authorize-ci-key.sh 'ssh-ed25519 AAAA... ci@github-actions'
#
# The key is restricted to the deploy command, so it cannot be used for a
# shell, port forwarding or anything else. Give it a bare key line, without
# its own options. The private half belongs in the repository's Actions
# secret DEPLOY_SSH_KEY and must never be copied here.
set -euo pipefail

KEY=${1:-}
case "$KEY" in
  ssh-ed25519\ *|ssh-rsa\ *|ecdsa-sha2-*\ *) ;;
  *) echo "usage: $0 'ssh-ed25519 AAAA... comment'" >&2; exit 2 ;;
esac

HOME_DIR=/home/deploy
AUTH="$HOME_DIR/.ssh/authorized_keys"
CMD=/usr/local/bin/nullbox-deploy

[ -x "$CMD" ] || { echo "$CMD is missing; install deploy/remote-deploy.sh first" >&2; exit 1; }

install -d -m 0700 -o deploy -g deploy "$HOME_DIR/.ssh"
touch "$AUTH"
chown deploy:deploy "$AUTH"
chmod 0600 "$AUTH"

if grep -qF "$KEY" "$AUTH"; then
  echo "key already authorized"
else
  printf 'restrict,command="%s" %s\n' "$CMD" "$KEY" >> "$AUTH"
  echo "key authorized for $CMD only"
fi
