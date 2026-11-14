#!/usr/bin/env bash
# Runs on team-09 as the deploy user. Installed as /usr/local/bin/nullbox-deploy
# and used as the forced command for the CI key, so a leaked deploy key can
# trigger a deployment but cannot open a shell.
#
#   sudo install -o root -g root -m 0755 deploy/remote-deploy.sh /usr/local/bin/nullbox-deploy
#
# It only ever deploys main, and it keeps the checkout on a local main that
# tracks origin/main. The working tree is a deployment checkout; nobody edits
# it and nobody develops on it.
set -euo pipefail

DIR=${NULLBOX_DIR:-/srv/nullbox}
BRANCH=${NULLBOX_DEPLOY_BRANCH:-main}

cd "$DIR"

# One deploy at a time. The lock lives in the checkout so deploy can create it.
exec 9>"$DIR/.deploy.lock"
if ! flock -n 9; then
  echo "another deploy is already running" >&2
  exit 1
fi

# Track exactly one remote branch, so a later fetch cannot drift elsewhere.
git remote set-branches origin "$BRANCH"
git fetch --depth 1 origin
# Refuse to check out a branch that does not contain the deployment config.
git show "origin/$BRANCH:docker-compose.yml" >/dev/null
# Deployment checkout only: discard local changes and put the branch on main.
git checkout -f -B "$BRANCH" "origin/$BRANCH"
git branch --set-upstream-to="origin/$BRANCH" "$BRANCH" >/dev/null
git remote set-head origin --auto >/dev/null 2>&1 || true
echo "deploying $(git rev-parse --short HEAD) from $BRANCH"


# Build first: if the build fails, the running containers are left alone.
docker compose up -d --build --remove-orphans

# Wait for the API healthcheck so a bad deploy fails the job, not the site.
status=missing
for _ in $(seq 1 30); do
  status=$(docker inspect -f '{{.State.Health.Status}}' nullbox-api-1 2>/dev/null || echo missing)
  [ "$status" = healthy ] && break
  sleep 2
done
docker compose ps
if [ "$status" != healthy ]; then
  echo "api did not become healthy (last status: $status)" >&2
  docker compose logs --tail=50 api >&2 || true
  exit 1
fi
