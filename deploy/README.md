# Deploying Nullbox

One host, two containers:

- `web` — Caddy on ports 80/443. Terminates TLS, serves the built SPA, proxies
  `/api/*` to the API. The only published service.
- `api` — the axum binary on the private Compose network, storing SQLite in the
  `nullbox-data` volume. It is never published to the host.

Everything is same-origin, so the browser needs no CORS and no second hostname.

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
sudo systemctl list-units --all | grep -i debug     # find the unit name
sudo systemctl disable --now <unit>
sudo ss -ltnp | grep 8888                           # must be empty

sudo ufw default deny incoming
sudo ufw default allow outgoing
sudo ufw allow 22/tcp
sudo ufw allow 80/tcp
sudo ufw allow 443/tcp
sudo ufw --force enable
```

Docker publishes 80/443 before ufw's rules, so keep Docker's forward policy at
`ACCEPT` (`/etc/default/ufw`) and verify the site still loads afterwards.

### Swap on a small instance

A 2 GiB box can build the Rust image only with swap. Once:

```sh
sudo fallocate -l 4G /swapfile
sudo chmod 600 /swapfile
sudo mkswap /swapfile
sudo swapon /swapfile
echo '/swapfile none swap sw 0 0' | sudo tee -a /etc/fstab
```

## First deploy

```sh
ssh team-09 'sudo install -d -o deploy -g deploy /srv/nullbox'
scp .env.production.example team-09:/srv/nullbox/.env
ssh team-09 'chmod 600 /srv/nullbox/.env'
# edit /srv/nullbox/.env: set SITE_ADDRESS, PUBLIC_WEB_URL and a fresh ADMIN_TOKEN

./deploy/deploy.sh
```

`deploy.sh` rsyncs the tree (without `.env`, `.git`, `target`, `node_modules`)
and runs `docker compose up -d --build`.

## Updates

```sh
./deploy/deploy.sh
```

Compose rebuilds the changed images and recreates only the affected containers.
The SQLite volume and the Caddy certificates survive.

## Verify

```sh
curl -fsS https://team-09.fasthacks.cloud/api/healthz
curl -fsSI https://team-09.fasthacks.cloud/ | head -5
docker compose -f /srv/nullbox/docker-compose.yml ps
docker compose -f /srv/nullbox/docker-compose.yml logs --tail=50 api web
```

## Rollback

Images are tagged `nullbox-api:local` / `nullbox-web:local`, so a rollback means
checking out the previous commit and running `./deploy/deploy.sh` again. The
database is not rolled back: migrations are forward-only, so roll back the code
only to a commit whose schema the database still satisfies.

## Notes and limits

- Backups are not wired up yet. The SQLite database is on the `nullbox-data`
  volume; copying the file while the API writes is not a backup. Litestream or
  `sqlite3 .backup` plus off-box storage is the next step (see the production
  readiness list).
- `docker compose down -v` deletes the database and the certificates. Do not.
- The API answers SIGINT but not SIGTERM today; a container stop is therefore
  abrupt. Tracked in `docs/PRODUCTION_READINESS.md`.
