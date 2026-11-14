# Production readiness

The event's team page scores the deployment in upgrade groups. Later groups stay
locked until the earlier ones pass, so this is also the suggested order of work.
Points are the event's; the acceptance criteria are ours.

Positions: **done** / **in progress** / **next** / **backlog**.

## 1. Deploy — 1 pt — next

- [x] One-command containers for the API and the SPA (`Dockerfile`, `docker-compose.yml`).
- [x] Caddy terminates TLS and serves SPA + `/api` on one origin (`deploy/Caddyfile`).
- [x] Non-root API, read-only root filesystem, healthcheck, restart policy.
- [x] Runbook for first deploy, updates and rollback (`deploy/README.md`).
- [x] Site reachable at `https://team-09.fasthacks.cloud/` with a valid certificate.
- [x] `curl -fsS https://team-09.fasthacks.cloud/api/healthz` returns `{"status":"ok"}`.
- [x] The event's Deploy check has run and gone green (2026-11-14T05:51Z).
- **Accept when:** the event's Deploy check goes green and the page loads a ticket
  end to end.
- **Verified on 2026-11-14:** Let's Encrypt certificate, HTTP->HTTPS redirect,
  ticket create/read/stats round trip, leaked admin token 403, fresh token 200,
  only 22/80/443 reachable, Caddy logs redact token headers.
- **Model:** `main` is the deployment branch. The server only checks out `main`;
  develop elsewhere, never on the server.

## 2. CI/CD — 4 pts — in progress

- [x] Workflow templates for CI and deploy (`deploy/github-actions/`).
- [x] Server-side deploy command with a build + health gate (`deploy/remote-deploy.sh`).
- [x] Restricted CI key installer (`deploy/authorize-ci-key.sh`).
- [ ] Human copies the templates into `.github/workflows/`.
- [ ] `DEPLOY_SSH_KEY` secret added and Actions enabled on the fork.
- [ ] First automatic deploy on a push to `main`.
- **Model:** `main` is the only deployed branch; `development` is integration;
  the server checkout at `/srv/nullbox` is a deployment target and is never
  developed on.
- **Where:** `.github/workflows/` is outside the agent's permission; a human adds
  the two files from `deploy/github-actions/`.
- **Accept when:** a PR runs fmt, clippy `-D warnings`, `cargo test`, `pnpm lint`,
  `pnpm test`, `pnpm build`; a push to `main` updates the site; a warm run
  restores the caches; a deliberately failing test blocks the merge.
- **Cache:** Rust (`Swatinem/rust-cache`) and pnpm (`setup-node` cache) in CI;
  Docker BuildKit cache mounts on the server for the image build.

## 3. Login — 5 pts — backlog

- [ ] Members-only page stays closed (1 pt).
- [ ] Username + password sign-up and sign-in (2 pts).
- [ ] Single sign-on (2 pts).
- **Where:** `Actor` extractor (`crates/api/src/http/extract.rs`), `policy::can`
  (`crates/api/src/policy.rs`), `authHeaders()` (`apps/web/src/api/client.ts`),
  `cors()` (`crates/api/src/http/mod.rs`).
- **Proposal:** users/sessions tables in SQLite, argon2id hashes, HttpOnly +
  Secure + SameSite=Lax cookies, origin/CSRF check on writes, logout that deletes
  the session. SSO via an OIDC provider (GitHub or a generic issuer) with the same
  session afterwards. Roles: `admin` (export, user management), `agent` (inbox).
- **Accept when:** `/inbox` without a session redirects to login; API worker
  routes return 401; `X-Actor-Name` no longer grants anything; a session can be
  revoked from the server; admin export requires the `admin` role; login is
  rate-limited.

## 4. Security — 6 pts — backlog

- [ ] Rotate the leaked admin secret (1 pt).
- [ ] Get the secret out of git history (1 pt).
- [ ] Customer links stay secret (1 pt).
- [ ] Firewall (1 pt).
- [ ] Audit log (2 pts).
- **Rotate:** set a fresh `ADMIN_TOKEN` in the server `.env`; the code no longer
  falls back to a default, and an empty value disables the export endpoint.
- **History:** the old value is in a public commit. Rotating makes it useless;
  then purge it from history (or recreate the fork) and add secret scanning +
  push protection. Decide which the team wants before rewriting history.
- **Customer links:** keep the token in the URL fragment (already done), never log
  it (Caddy redacts `X-Ticket-Token`, `X-Admin-Token`, `Authorization` and
  `Cookie`), add `Referrer-Policy: no-referrer` on ticket pages, consider expiry
  and a "revoke link" action.
- **Firewall:** only 22/80/443 inbound; remove the public debug console; verify
  from outside the host, not just with `ss`.
- **Audit log:** implement an append-only sink behind `EventSink` recording
  actor, action, resource, outcome, timestamp, source address; include refused
  requests from the `Actor` extractor; expose to admins only.
- **Accept when:** the old token is rejected, a fresh one works, history has no
  live secret, the port scan is clean, and every worker/admin write produces an
  audit record.

## 5. Observability — 2 pts — done

- [x] Structured JSON logs with request IDs and latency (`crates/api/src/observability.rs`, `crates/api/src/http/mod.rs`, `deploy/Caddyfile`).
- [x] `/readyz` that checks the database, separate from `/healthz` (`crates/api/src/http/utility.rs`).
- [x] Metrics (request count/latency/errors, DB size, process) scraped or pushed (`/api/metrics`, `/api/observability/summary`, `/observability`).
- [x] External uptime check and an alert route (`/api/alerts`, `/api/alerts/webhook`, `deploy/uptime-check.sh`).
- **Where:** `middleware()` in `crates/api/src/http/mod.rs`, `main.rs`,
  `crates/api/src/audit.rs`, `crates/api/src/observability.rs`,
  `apps/web/src/routes/worker/ObservabilityPage.tsx`; Caddy logs JSON and propagates `X-Request-Id`.
- **Accept when:** one request can be followed from Caddy through the API; a
  stopped database flips readiness without killing liveness; a dashboard shows
  traffic and errors.

## 6. SEO — 2 pts — backlog

- [ ] Lighthouse SEO 100 (1 pt).
- [ ] Agentic browsing 100 (1 pt).
- **Where:** `apps/web/index.html`, `apps/web/public/`, route metadata.
- **Accept when:** title/description/canonical per route, `robots.txt`, sitemap,
  semantic landmarks, Open Graph, JSON-LD, and the agentic checklist pass.

## 7. Payments — 3 pts — backlog

- [ ] Provider integration, webhook signature verification, idempotent fulfilment,
  a customer-facing page, keys in config only.
- **Where:** a route next to the others in `crates/api/src/http/mod.rs`, a page in
  `apps/web/src/router.tsx`, keys in `Config`.
- **Accept when:** test-mode purchase upgrades a ticket/account, a replayed
  webhook is a no-op, and no key is in the bundle.

## 8. AI — 3 pts — backlog

- [ ] MCP server at `/mcp` exposing ticket tools (2 pts).
- [ ] An agent works a ticket end to end (1 pt).
- **Where:** a service next to the `/api` nest in `router()`; tools call
  `TicketService` like the worker handlers do.
- **Accept when:** an MCP client lists tools, reads a ticket, posts a reply, and
  the action appears in the audit log under the agent's identity.

## Cross-cutting work not scored directly

- [ ] **Backups + restore drill.** SQLite WAL on the `nullbox-data` volume;
  Litestream or `sqlite3 .backup` to off-box storage, retention, and one
  successful restore into a scratch container. *Production-critical.*
- [ ] **Graceful SIGTERM.** `main.rs` waits for Ctrl-C only; add SIGTERM so
  container stops drain in-flight requests and close SQLite cleanly.
- [ ] **Abuse limits.** Rate-limit ticket creation and replies, cap request body
  size, add IP allow/deny for admin.
- [ ] **Dependency and secret scanning.** `cargo audit`, `pnpm audit`, Dependabot,
  secret scanning, SBOM on release.
- [ ] **Migration safety.** Backup before migrate, forward-only, tested on a copy
  of production data.
- [ ] **Load test.** k6 or similar against create/read/reply; record the single-
  node ceiling.
- [ ] **Runbook + on-call.** Deploy, rollback, restore, rotate secrets, incident
  checklist, who is paged.

## Decisions the team needs to make

1. SSO provider (GitHub OAuth vs generic OIDC) and whether passwords stay.
2. Whether to rewrite git history or rotate-and-move-on.
3. Backup destination (S3-compatible, another host, or the deSEC box).
4. Payments provider and whether it is in scope for the event.
5. Whether the SQLite single-node design is enough, or Postgres is expected.
