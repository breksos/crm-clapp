-- p2: identity. Every table carries org_id (orgs' own id *is* the org id), because retrofitting
-- tenancy later is a rewrite in which the query that misses a filter is a data leak.
-- Secrets (passwords, session tokens, invite tokens) are stored as hashes only.

CREATE TABLE orgs (
    id          text PRIMARY KEY,
    name        text   NOT NULL,
    created_at  bigint NOT NULL
);

-- One home org per user in p2 (an invite or a sign-up puts them there); memberships record the role.
CREATE TABLE users (
    id             text PRIMARY KEY,
    org_id         text   NOT NULL REFERENCES orgs (id),
    email          text   NOT NULL,
    email_norm     text   NOT NULL UNIQUE,
    name           text   NOT NULL,
    password_hash  text   NOT NULL,          -- argon2id, PHC string
    created_at     bigint NOT NULL
);

CREATE TABLE memberships (
    org_id   text NOT NULL REFERENCES orgs (id),
    user_id  text NOT NULL REFERENCES users (id),
    role     text NOT NULL CHECK (role IN ('owner', 'member')),   -- recorded now, enforced in p4
    PRIMARY KEY (org_id, user_id)
);

CREATE TABLE invites (
    id          text PRIMARY KEY,
    org_id      text   NOT NULL REFERENCES orgs (id),
    token_hash  text   NOT NULL UNIQUE,      -- sha256 of the link's token; the token is shown once
    email       text,
    created_by  text   NOT NULL REFERENCES users (id),
    expires_at  bigint NOT NULL,
    used_at     bigint
);

CREATE TABLE seats (
    id          text PRIMARY KEY,
    org_id      text   NOT NULL REFERENCES orgs (id),
    user_id     text   NOT NULL REFERENCES users (id),
    device      text   NOT NULL,
    created_at  bigint NOT NULL
);

CREATE TABLE sessions (
    id          text PRIMARY KEY,
    org_id      text   NOT NULL REFERENCES orgs (id),
    seat_id     text   NOT NULL REFERENCES seats (id),
    token_hash  text   NOT NULL UNIQUE,      -- sha256 of the bearer token
    expires_at  bigint NOT NULL,
    revoked_at  bigint
);

CREATE TABLE agent_idents (
    workspace_id    text PRIMARY KEY,
    org_id          text NOT NULL REFERENCES orgs (id),
    user_id         text NOT NULL REFERENCES users (id),
    local_agent_id  text NOT NULL,
    name            text NOT NULL,
    UNIQUE (org_id, user_id, local_agent_id)
);

-- Failed credential attempts, for rate limiting. The key is a hash (of an email or an address),
-- never the value. org_id is null: before authentication there is no org to attribute it to.
CREATE TABLE login_attempts (
    id      bigserial PRIMARY KEY,
    org_id  text,
    kind    text   NOT NULL CHECK (kind IN ('email', 'ip')),
    key     text   NOT NULL,
    at      bigint NOT NULL
);
CREATE INDEX login_attempts_key ON login_attempts (kind, key, at);
