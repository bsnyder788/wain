-- Accounts, sign-in, sessions and the credit ledger.

create table users (
    id uuid primary key,
    -- Trimmed and lowercased, so one person has one account.
    email text not null unique,
    created_at timestamptz not null default now()
);

-- One row per sign-in code emailed to someone. Codes are short, so each one
-- is tied to an unguessable challenge id and allowed only a few attempts.
create table login_challenges (
    id uuid primary key,
    email text not null,
    -- SHA-256 over the server secret, the challenge id and the code.
    code_hash bytea not null,
    attempts int not null default 0,
    expires_at timestamptz not null,
    consumed_at timestamptz,
    created_at timestamptz not null default now()
);
create index login_challenges_email_created on login_challenges (email, created_at);

create table sessions (
    -- SHA-256 of the cookie value; the value itself is never stored.
    token_hash bytea primary key,
    user_id uuid not null references users (id) on delete cascade,
    expires_at timestamptz not null,
    created_at timestamptz not null default now()
);
create index sessions_user on sessions (user_id);

-- Append-only. A user's balance is the sum of their deltas: grants are
-- positive, spending is negative, and a refund is a positive entry that
-- points at the spend it undoes.
create table credit_entries (
    id uuid primary key,
    user_id uuid not null references users (id) on delete cascade,
    delta int not null,
    reason text not null,
    refunds uuid references credit_entries (id),
    created_at timestamptz not null default now()
);
create index credit_entries_user on credit_entries (user_id, created_at);
-- A spend can be refunded once.
create unique index credit_entries_one_refund on credit_entries (refunds) where refunds is not null;
