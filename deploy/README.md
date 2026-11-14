# Deploying Nullbox

One host, two containers:

- `web` — Caddy on ports 80/443. Terminates TLS, serves the built SPA, proxies
  `/api/*` to the API. The only published service.
- `api` — the axum binary on the private Compose network, storing SQLite in the
  `nullbox-data` volume. It is never published to the host.

Everything is same-origin, so the browser needs no CORS and no second hostname.

## Deployment model: `main` only

- **`main` is the deployment branch.** The checkout at `/srv/nullbox` is on a
  local `main` that tracks `origin/main`, and nothing else. The deploy command
  enforces this; `git branch` there should show `* main`.
- **The server is a deployment target, not a workspace.** No feature work, no
  hand edits, no debugging by changing files there. Development happens in
  branches and `development`; only merged `main` reaches the server.
- A push to `main` runs the deploy workflow, which SSHes to the server and runs
  `/usr/local/bin/nullbox-deploy`.
- The deploy key is restricted to that command on the server. A leaked key can
  trigger a deploy, but it cannot open a shell.
- The SQLite volume and the Caddy certificates are not part of a deploy and
  survive it. A failed build leaves the running containers untouched.

Flow:

```
agent/*  --PR-->  development  --release PR-->  main  --CI-->  /usr/local/bin/nullbox-deploy
```

## Server prerequisites

- Docker Engine with Compose v2 (`docker compose version`).
- `team-09.fasthacks.cloud` (or the chosen name) resolving to the host, and
  inbound 80/443 open. Caddy obtains a Let's Encrypt certificate automatically.
- Port 22 and nothing else exposed. Remove the setup debug console on `:8888`
  and enable a host firewall (see below).

### First-time hardening

The image ships with a public `python3 -m http.server 8888` serving
`/opt/debug-console`, and its own page says it must never be reachable. Stop and
disable it, then close everything but SSH and HTTP(S):

```sh
sudo bash deploy/bootstrap.sh
```

It also adds 4G swap (the Rust image needs it on a 2 GiB box) and makes the
firewall idempotent. Docker publishes 80/443 before ufw's rules, so the script
keeps Docker's forward policy at `ACCEPT`; verify the site still loads.

## First deploy on a fresh host

```sh
# 1. Deployment checkout, owned by deploy.
ssh team-09 'sudo install -d -o deploy -g deploy /srv/nullbox'
ssh team-09 'git clone --depth 1 --branch main https://github.com/collision406/nullbox.git /srv/nullbox'

# 2. Server-only settings (never commit this file).
scp .env.production.example team-09:/srv/nullbox/.env
ssh team-09 'chmod 600 /srv/nullbox/.env'
# edit SITE_ADDRESS, PUBLIC_WEB_URL and ADMIN_TOKEN on the server

# 3. The one command CI is allowed to run.
scp deploy/remote-deploy.sh team-09:/tmp/
ssh team-09 'sudo install -o root -g root -m 0755 /tmp/remote-deploy.sh /usr/local/bin/nullbox-deploy'

# 4. First deploy by hand; CI takes over afterwards.
./deploy/deploy.sh
```

## CI/CD

The templates live in `deploy/github-actions/`. A human must copy them into
`.github/workflows/`: the agent token may not edit workflow files.

1. Install `/usr/local/bin/nullbox-deploy` (step 3 above).
2. Authorize the organiser's public key. Pass the bare key line, without its own
   options:
   ```sh
   sudo bash deploy/authorize-ci-key.sh 'ssh-ed25519 AAAA... ci@github-actions'
   ```
   It is written as `restrict,command="/usr/local/bin/nullbox-deploy" ...`.
3. Add the private half as the repository secret **`DEPLOY_SSH_KEY`**
   (Settings → Secrets and variables → Actions). It must never be copied to the
   server or into this repository.
4. Copy the workflows:
   ```sh
   mkdir -p .github/workflows
   cp deploy/github-actions/ci.yml     .github/workflows/ci.yml
   cp deploy/github-actions/deploy.yml .github/workflows/deploy.yml
   ```
5. Enable Actions on the fork if it is disabled.
6. Merge the deployment config into `main`. The first deploy builds on the
   server (roughly 15–20 minutes on 2 vCPU); later builds reuse the BuildKit
   cache mounts in the Dockerfile.

`ci.yml` runs format, clippy, the Rust tests, lint, the web tests and the web
build on every PR and on `development`/`main`. `deploy.yml` runs only for
`main` and after a manual dispatch. It also probes `/api/healthz` from the
runner after the deploy.

### Rotating the deploy key

Remove the old line from `/home/deploy/.ssh/authorized_keys`, run
`authorize-ci-key.sh` with the new public key, then replace the `DEPLOY_SSH_KEY`
secret. Old key first or new key first both work; do not leave both indefinitely.

## Updates

Push to `main`. CI does the rest.

Manual deploy (for example if CI is down):

```sh
./deploy/deploy.sh
# or: ssh team-09 /usr/local/bin/nullbox-deploy
```

The command fetches `main`, refuses a tree without `docker-compose.yml`, builds,
starts, and waits for the API healthcheck. A failed build leaves the previous
containers serving.

## Verify

```sh
curl -fsS https://team-09.fasthacks.cloud/api/healthz
curl -fsSI https://team-09.fasthacks.cloud/ | head -5
ssh team-09 'cd /srv/nullbox && docker compose ps'
ssh team-09 'cd /srv/nullbox && docker compose logs --tail=50 api web'
```

## Rollback

Code rolls back through `main`: revert the offending commit and let CI deploy.
The database is not rolled back; migrations are forward-only, so only revert to
a commit whose schema the database still satisfies. For an urgent code-only
rollback while CI is down, check out the previous commit in `/srv/nullbox` and
run `docker compose up -d --build` by hand, then restore `main` afterwards.

## Notes and limits

- Backups are not wired up yet. The SQLite database is on the `nullbox-data`
  volume; copying the file while the API writes is not a backup. Litestream or
  `sqlite3 .backup` plus off-box storage is the next step.
- `docker compose down -v` deletes the database and the certificates. Do not.
- The API answers SIGINT but not SIGTERM today; a container stop is therefore
  abrupt. Tracked in `docs/PRODUCTION_READINESS.md`.
- The first build on a cold server is slow. Do not add `--no-cache` to the
  deploy command; it throws away the only build cache the small host has.
