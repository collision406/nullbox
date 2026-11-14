-- Worker accounts and their sessions. Passwords are stored as Argon2id hashes, session
-- tokens as SHA-256 hashes: a copy of this database can't be used to sign in.

CREATE TABLE users (
    id            TEXT PRIMARY KEY NOT NULL,
    username      TEXT NOT NULL,          -- as typed; shown next to every reply
    username_key  TEXT NOT NULL UNIQUE,   -- lowercased; what lookups and uniqueness use
    password_hash TEXT,                   -- Argon2id PHC string; NULL for accounts that only use SSO
    sso_issuer    TEXT,
    sso_subject   TEXT,
    created_at    TEXT NOT NULL
);

CREATE UNIQUE INDEX users_sso ON users (sso_issuer, sso_subject) WHERE sso_issuer IS NOT NULL;

CREATE TABLE sessions (
    token_hash TEXT PRIMARY KEY NOT NULL,
    user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
);

CREATE INDEX sessions_user ON sessions (user_id);

-- One row per sign-in with an identity provider that has been started but not finished.
CREATE TABLE sso_states (
    state_hash TEXT PRIMARY KEY NOT NULL,
    verifier   TEXT NOT NULL,             -- PKCE code verifier
    nonce      TEXT NOT NULL,
    expires_at TEXT NOT NULL
);
