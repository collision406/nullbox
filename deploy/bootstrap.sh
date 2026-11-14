#!/usr/bin/env bash
# One-time preparation of the team box. Run it on the server, not locally:
#
#   scp deploy/bootstrap.sh team-09:/tmp/bootstrap.sh
#   ssh team-09 'sudo bash /tmp/bootstrap.sh'
#
# It is idempotent. It closes the setup debug console on :8888, adds swap for
# the image build, and turns on a host firewall with only SSH and HTTP(S).
set -euo pipefail

# --- 1. the setup debug console must not be public --------------------------
# It is served by `python3 -m http.server 8888 --bind 0.0.0.0`.
pkill -f 'http\.server 8888' || true

for unit in debug-console.service debug-console.socket; do
  if systemctl list-unit-files "$unit" >/dev/null 2>&1; then
    systemctl disable --now "$unit" || true
  fi
done

# Anything still listening on 8888 gets named, so it can be dealt with.
if ss -ltn | grep -q ':8888 '; then
  echo "WARNING: something still listens on :8888" >&2
  ss -ltnp | grep ':8888 ' >&2 || true
fi

# --- 2. swap: the Rust image needs more than 2 GiB to link ------------------
if ! swapon --show | grep -q .; then
  fallocate -l 4G /swapfile
  chmod 600 /swapfile
  mkswap /swapfile
  swapon /swapfile
  grep -q '^/swapfile ' /etc/fstab || echo '/swapfile none swap sw 0 0' >> /etc/fstab
  echo "added 4G swap"
fi

# --- 3. firewall: SSH and HTTP(S) only --------------------------------------
# Docker publishes ports through its own chains, so keep forwarding enabled or
# containers lose DNS and outbound ACME.
if [ -f /etc/default/ufw ]; then
  sed -i 's/^DEFAULT_FORWARD_POLICY=.*/DEFAULT_FORWARD_POLICY="ACCEPT"/' /etc/default/ufw
fi
ufw default deny incoming
ufw default allow outgoing
ufw allow 22/tcp
ufw allow 80/tcp
ufw allow 443/tcp
ufw --force enable

echo
echo "done. listening sockets:"
ss -ltnp
