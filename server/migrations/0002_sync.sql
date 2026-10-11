-- p3: record data on the server. See docs/sync-contract.md §2.
-- Every table carries org_id, and every query that touches one starts `WHERE org_id = <the session's>`.

-- The org's op counter (the watermark), and the oldest seq `ops` still holds (0 = from the start).
ALTER TABLE orgs ADD COLUMN watermark     bigint NOT NULL DEFAULT 0;
ALTER TABLE orgs ADD COLUMN retained_from bigint NOT NULL DEFAULT 0;

-- One pipeline per org, seeded, read-only in p3.
CREATE TABLE pipelines (
    org_id  text PRIMARY KEY REFERENCES orgs (id),
    id      text  NOT NULL,
    name    text  NOT NULL,
    stages  jsonb NOT NULL
);
INSERT INTO pipelines (org_id, id, name, stages)
SELECT id, 'sales', 'Sales', '["lead","qualified","proposal","negotiation"]'::jsonb FROM orgs;

-- The one handle namespace (company, contact, deal, task): a constraint, not a check.
CREATE TABLE handles (
    org_id  text NOT NULL REFERENCES orgs (id),
    handle  text NOT NULL,
    kind    text NOT NULL CHECK (kind IN ('company', 'contact', 'deal', 'task')),
    id      text NOT NULL,
    PRIMARY KEY (org_id, handle),
    UNIQUE (org_id, id)
);

-- `version` is the seq of the op that last wrote the row. `fields` holds the stored fields that
-- nothing filters or joins on, exactly as the split shape's `data` rows carry them.
CREATE TABLE companies (
    org_id       text   NOT NULL REFERENCES orgs (id),
    id           text   NOT NULL,
    handle       text   NOT NULL,
    version      bigint NOT NULL,
    owner_id     text   NOT NULL,
    archived_at  bigint,
    updated_at   bigint NOT NULL,
    fields       jsonb  NOT NULL,
    PRIMARY KEY (org_id, id)
);

CREATE TABLE contacts (
    org_id       text   NOT NULL REFERENCES orgs (id),
    id           text   NOT NULL,
    handle       text   NOT NULL,
    version      bigint NOT NULL,
    owner_id     text   NOT NULL,
    archived_at  bigint,
    updated_at   bigint NOT NULL,
    company_id   text,
    fields       jsonb  NOT NULL,
    PRIMARY KEY (org_id, id)
);

CREATE TABLE deals (
    org_id       text   NOT NULL REFERENCES orgs (id),
    id           text   NOT NULL,
    handle       text   NOT NULL,
    version      bigint NOT NULL,
    owner_id     text   NOT NULL,
    archived_at  bigint,
    updated_at   bigint NOT NULL,
    company_id   text,
    stage        text   NOT NULL,
    status       text   NOT NULL,
    moved_by     jsonb  NOT NULL,
    moved_at     bigint NOT NULL,
    opened_at    bigint NOT NULL,
    closed_at    bigint,
    pipeline_id  text   NOT NULL,
    fields       jsonb  NOT NULL,
    PRIMARY KEY (org_id, id)
);

-- `due` is an ISO local date (YYYY-MM-DD), as the app stores it.
CREATE TABLE tasks (
    org_id      text   NOT NULL REFERENCES orgs (id),
    id          text   NOT NULL,
    handle      text   NOT NULL,
    version     bigint NOT NULL,
    owner_id    text   NOT NULL,
    updated_at  bigint NOT NULL,
    due         text   NOT NULL,
    done_at     bigint,
    by          jsonb  NOT NULL,
    fields      jsonb  NOT NULL,
    PRIMARY KEY (org_id, id)
);

-- Append-only: an activity or a move is never updated.
CREATE TABLE activities (
    org_id    text   NOT NULL REFERENCES orgs (id),
    id        text   NOT NULL,
    owner_id  text   NOT NULL,
    at        bigint NOT NULL,
    by        jsonb  NOT NULL,
    kind      text   NOT NULL,
    body      text   NOT NULL,
    links     jsonb  NOT NULL,
    PRIMARY KEY (org_id, id)
);

CREATE TABLE moves (
    org_id      text   NOT NULL REFERENCES orgs (id),
    id          text   NOT NULL,
    deal_id     text   NOT NULL,
    at          bigint NOT NULL,
    by          jsonb  NOT NULL,
    from_stage  text   NOT NULL,
    to_stage    text   NOT NULL,
    PRIMARY KEY (org_id, id)
);

-- The ordered log. `op` is 'upsert' (a row, replacing the client's copy) or 'append' (a feed item).
CREATE TABLE ops (
    org_id    text   NOT NULL REFERENCES orgs (id),
    seq       bigint NOT NULL,
    kind      text   NOT NULL,
    id        text   NOT NULL,
    op        text   NOT NULL CHECK (op IN ('upsert', 'append')),
    record    jsonb  NOT NULL,
    by        jsonb  NOT NULL,
    at        bigint NOT NULL,
    write_id  text   NOT NULL,
    PRIMARY KEY (org_id, seq)
);

-- Idempotency: a retried write returns this answer and changes nothing. Kept 24 h.
CREATE TABLE writes (
    org_id    text   NOT NULL REFERENCES orgs (id),
    write_id  text   NOT NULL,
    seq_from  bigint NOT NULL,
    seq_to    bigint NOT NULL,
    answer    jsonb  NOT NULL,
    at        bigint NOT NULL,
    PRIMARY KEY (org_id, write_id)
);
