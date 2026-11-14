-- Append-only record of security-relevant actions: who did what, when, from where.
-- Nothing in the application updates or deletes rows here.

CREATE TABLE audit_log (
    id        TEXT PRIMARY KEY NOT NULL,
    at        TEXT NOT NULL,
    kind      TEXT NOT NULL, -- e.g. admin_export, admin_access_denied, ticket_yeeted
    actor     TEXT,
    ticket_id TEXT,
    client_ip TEXT,
    detail    TEXT           -- JSON or free text, never secrets
);

CREATE INDEX audit_log_at ON audit_log (at);
