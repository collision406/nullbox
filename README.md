# Nullbox

English | [日本語](README.ja.md)

> ヌルボックス: support, but make it void.

A deliberately small, deliberately silly ticketing system. Customers report problems, workers
answer them, and everyone works toward **Inbox Zero** while the **Backlog Beast** watches.

This is the hackathon's starting point. It is a working product with **no** enterprise features:
no auth, no CI/CD, no observability, no backups. Adding those is your job. The code leaves a clear
seam for each one (see [Extension seams](#extension-seams)).

## Quickstart

Requirements: Rust (pinned in `rust-toolchain.toml`), Node 22 (`.nvmrc`), pnpm via Corepack
(`corepack enable`).

```sh
# API on http://localhost:3000 (creates nullbox.db and runs migrations on start)
cp .env.example .env
cargo run -p nullbox-api

# Web app on http://localhost:5173 (talks to the API cross-origin, no dev proxy)
pnpm install
cp apps/web/.env.example apps/web/.env
pnpm dev
```

Open <http://localhost:5173> to file a ticket, and <http://localhost:5173/inbox> for the worker inbox.

### Dev container

Instead of installing the requirements, open the repo in a [dev container](https://containers.dev/)
(VS Code: **Dev Containers: Reopen in Container**; needs Docker). It brings Rust, Node, pnpm and the
GitHub CLI, installs dependencies and creates both `.env` files. Then start the API and the web app
with `cargo run -p nullbox-api` and `pnpm dev`; ports 3000 and 5173 are forwarded to your machine.

`target/` and `node_modules/` live in Docker volumes, so they're not visible on your machine. A
Rust version change needs `rust-toolchain.toml` only; the version in `devcontainer.json` just avoids
a download. Docker itself isn't included: add the `docker-outside-of-docker` feature if you need it.

GitHub Codespaces works when opened in desktop VS Code. In the browser the app can't reach the
API, because `localhost` means your machine there.

## Checks

```sh
cargo test                 # domain + HTTP tests; also regenerates apps/web/src/api/generated
cargo clippy --all-targets
cargo fmt --check
pnpm test                  # Vitest + Testing Library
pnpm lint                  # Biome
pnpm build                 # typecheck + production bundle in apps/web/dist
```

Nothing enforces these yet. That's one of your upgrades.

## Layout

```
apps/web                     React + Vite SPA (customer portal at /, worker inbox at /inbox)
  src/api/client.ts          the only module that calls fetch
  src/api/generated/         TS types generated from Rust by ts-rs (committed)
  src/copy/                  en.ts, ja.ts and the CopyProvider
  src/router.tsx             every route (switch to createHashRouter for Tauri)
crates/domain                pure types + rules: statuses, transitions, Beast mood. No IO.
crates/api                   axum server, services, sqlx repositories
migrations/                  SQL migrations, embedded and run at startup
.sqlx/                       sqlx offline query cache, so a fresh clone builds without a DB
```

## API

All JSON, under `/api`. Errors are always `{ "code": ErrorCode, "message": string }`: `code` is
stable and shared with TypeScript; `message` is English and for developers only.

| Method | Path | Notes |
|---|---|---|
| POST | `/tickets` | create, returns `{ ticket, access_token, access_link }` |
| GET | `/public/tickets/{id}` | customer view, header `X-Ticket-Token` |
| POST | `/public/tickets/{id}/messages` | customer reply; reopens a resolved ticket |
| GET | `/tickets?status=` | worker inbox; no status = all active tickets |
| GET | `/tickets/{id}` | worker view, includes internal notes |
| POST | `/tickets/{id}/messages` | `{ body, internal? }` |
| PATCH | `/tickets/{id}` | `{ status?, severity?, assignee_name? }` (`""` unassigns) |
| POST | `/tickets/{id}/yeet` | close: sets status `resolved` ("Yeeted") |
| GET | `/stats/backlog` | `{ open_count, beast_mood, oldest_open_age_secs }` |
| GET | `/admin/export` | every ticket, for the CEO's spreadsheet; header `X-Admin-Token` |
| GET | `/admin/audit?limit=` | the audit log, newest first; header `X-Admin-Token` |
| GET | `/auth/config` | `{ invite_required, sso }`, for the login screen |
| POST | `/auth/signup` | `{ username, password, invite_code? }`, returns a session `{ token, name, expires_at }` |
| POST | `/auth/login` | `{ username, password }`, same answer |
| POST | `/auth/logout` | ends the session on the server |
| GET | `/me` | `{ name }` of the logged-in worker |
| POST | `/checkout` | open a Stripe Checkout Session (test mode); returns `{ url }` of the Stripe-hosted page |
| GET | `/healthz` | liveness |
| GET | `/teapot` | 418, as specified by RFC 2324 |

Worker endpoints need `Authorization: Bearer <session token>` (see [Accounts](#accounts)); the old `X-Actor-Name` header is ignored.

## Configuration

API, from env vars or `.env` (see `.env.example` and `crates/api/src/config.rs`):
`BIND_ADDR`, `DATABASE_URL`, `CORS_ALLOWED_ORIGINS`, `PUBLIC_WEB_URL`, `ADMIN_TOKEN`, `RATE_LIMIT_PER_MINUTE`,
`RATE_LIMIT_CREATE_PER_MINUTE`, `TRUST_PROXY`, `REQUEST_TIMEOUT_SECS`, `RUST_LOG`.

## Security

- **Admin token**: no default. Without `ADMIN_TOKEN` (32+ characters) the admin endpoints refuse everyone.
  It is compared in constant time, and a token that was once leaked is refused at startup.
- **Customer links**: a secret link opens only the ticket it was issued for.
- **Rate limits** per client IP: 300 requests a minute, 10 new tickets a minute. Over the limit: `429`
  with `Retry-After`. Behind a reverse proxy set `TRUST_PROXY=true`, or every client shares the proxy's IP.
- **Limits**: request bodies up to 128 kB (`413` above), requests up to 30 seconds (`503` above).
- **Headers** on every response: `Content-Security-Policy`, `Strict-Transport-Security`,
  `X-Content-Type-Options`, `X-Frame-Options`, `Referrer-Policy`, `Permissions-Policy`, `Cache-Control: no-store`.
  These cover the API; the web server that serves `apps/web/dist` has to set its own.
- **Audit log**: every ticket change, every export and every refused admin request is stored in
  `audit_log` and readable at `/api/admin/audit`.

## Payments

`/buy` lets a visitor feed the Backlog Beast a snack for ¥500 through Stripe Checkout. The browser asks
`POST /api/checkout`, the API opens the session with `STRIPE_API_KEY` and returns the URL of a page hosted
by Stripe; after paying, the buyer lands on `/thanks`. The card never touches this server, and the price is
fixed in `crates/api/src/payments.rs`: the request has no body to tamper with.

- **Test mode only**: a key that isn't `sk_test_…` or `rk_test_…` stops the API from starting.
- **Without a key** `POST /api/checkout` answers 503 and nothing is sent anywhere.
- **Rate limited** with the ticket creation limit (each call costs a request to Stripe), and audited as
  `checkout_started` (client address and Stripe session id; never the key).
- Test card: `4242 4242 4242 4242`, any future expiry, any CVC.

Tests: `crates/api/tests/payments.rs`, against a fake Stripe on a local port.

One test per item lives in `crates/api/tests/security.rs`.

## Accounts

Workers sign up and log in with a username and a password; every reply carries the account's name.

- **Closed by default**: every worker endpoint answers `401` without a valid session. A session token
  (256 random bits, valid 12 hours) comes from sign-up or log-in and is stored hashed; logging out
  deletes the session on the server, so the token stops working at once.
- **Passwords** are hashed with Argon2id and never stored, logged or returned. They need at least
  10 characters, must not be a common password or contain the username, and must not appear in a known
  breach (checked with Have I Been Pwned's range API: only 5 characters of the SHA-1 leave the server).
  If that service is down the local rules still apply and sign-up goes on.
- **No account enumeration**: a wrong password and an unknown user get the same answer.
- **Guessing is cut off**: 10 sign-ups and log-ins a minute per client IP (`RATE_LIMIT_AUTH_PER_MINUTE`).
- **Sign-up is open by default**, which means anyone can read the inbox. Set `SIGNUP_INVITE_CODE`
  (8+ characters) to require a code.
- **Audit**: `account_created`, `login_succeeded`, `login_failed`, `logout` and every refused worker
  request are in the audit log, without passwords or tokens.
- The SPA keeps the session token in `sessionStorage` (it ends with the tab).

Tests: `crates/api/tests/auth.rs` and `crates/api/src/accounts.rs`.

Web, at build time: `VITE_API_BASE_URL`, the origin of the API without `/api`. Unset, the bundle calls
`http://localhost:3000`, which is only right on your laptop. For a deployment, set it to your site's
address, or to an empty value when one web server serves both the web app and `/api`.

## Extension seams

| Want to add | Start here |
|---|---|
| Login: passwords, SSO, sessions | `Actor` extractor in `crates/api/src/http/extract.rs` (it gets the `AppState`); `authHeaders()` in `apps/web/src/api/client.ts`; for cookies across origins, see `cors()` in `crates/api/src/http/mod.rs` |
| Roles / permissions | `policy::can` in `crates/api/src/policy.rs` (always `true` today) |
| Webhooks, email, metrics | `EventSink` in `crates/api/src/events.rs`; the audit log in `crates/api/src/audit.rs` is one implementation |
| Tracing exporters, more limits | `middleware()` in `crates/api/src/http/mod.rs`; the guards live in `crates/api/src/http/guard.rs` |
| Postgres, multi-tenancy, backups | `TicketRepository` in `crates/api/src/repo/mod.rs` |
| OpenTelemetry | everything logs through `tracing`; add a layer in `main.rs` |
| Payments | a new route next to the others in `crates/api/src/http/mod.rs`; a page in `apps/web/src/router.tsx`; keys in `Config` |
| MCP server for AI agents | a service at `/mcp`, next to the `/api` nest in `router()`; let its tools call `TicketService`, like the worker handlers do |
| More languages, Serious Mode | add a dictionary of type `Copy` next to `apps/web/src/copy/en.ts` |
| Tauri desktop/mobile | `VITE_API_BASE_URL` + `createHashRouter` in `apps/web/src/router.tsx` |

## Decisions

- **Inbox Zero** means no ticket in `open`, `in_progress` or `waiting_on_customer`. Resolved and
  closed tickets are still reachable through the inbox status filter.
- **Yeet** = close from the worker's point of view: the ticket becomes `resolved` ("Yeeted"). A
  customer reply brings it back to `open` ("IT'S ALIVE!"). `closed` ("Sent to the Shadow Realm")
  is final.
- **Lost secret link**: a known MVP limitation. There is no way to recover it (email would fix that).
- **Name**: "Nullbox" in both languages, with ヌルボックス in the Japanese subtitle.
- **IDs and timestamps** are stored as text (uuid v7, RFC 3339 UTC) and generated in Rust, so the
  schema ports to Postgres with type changes only. Ticket numbers are allocated in the repository.
- `X-Actor-Name` is percent-encoded so non-ASCII names (山田) survive HTTP headers.

## Working on the database

Queries are checked at compile time against `.sqlx/`. After changing a query or migration:

```sh
cargo install sqlx-cli --no-default-features --features sqlite   # once
export DATABASE_URL=sqlite://nullbox.db
sqlx database create && sqlx migrate run
cargo sqlx prepare --workspace     # refresh .sqlx, then commit it
```

## Lore

In the beginning there was the Inbox, and the Inbox was without form, and full.

Every unanswered ticket feeds the **Backlog Beast**. At zero it sleeps, curled up somewhere in
`crates/domain/src/beast.rs`. With a handful of tickets it gets peckish and starts watching you.
Past six it is grumpy and starts to steam. Past twenty it becomes a kaiju, and nobody is safe.
The only known cure is to answer the tickets.

Workers do not close tickets: they **yeet** them. Most yeeted tickets stay gone. Some come back:
a customer replies, and the ticket rises from the dead. Truly finished tickets are sent to the
**Shadow Realm**, from which nothing returns (see `TicketStatus::can_transition_to`).

Ticket #404 exists. Nobody has ever found it.

Some say that if you type ↑ ↑ ↓ ↓ ← → ← → B A, the app turns into Enterprise Software. Those who
have filled in its forms are still waiting for approval.

When the server panics, it logs under the target `EXISTENTIAL`. It is not DNS. It is never DNS.
(It was DNS: see `ApiError::ProbablyDns`.)

### Easter eggs

- Konami code: Enterprise Mode™, with absurd mandatory fields and an eternal "Synergizing…"
- Ticket #404 is shown as "Not Found"
- Typing `sudo` in the inbox search: "This incident will be reported"
- Caps Lock on while replying: "Priority: Loud" (cosmetic, honest)
- Random loading messages, e.g. "Bribing the database…"
- `GET /api/teapot`

## License

[The Unlicense](LICENSE): public domain. Do whatever you want with it.
