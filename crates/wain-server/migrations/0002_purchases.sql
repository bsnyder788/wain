-- Credit packs bought through Stripe Checkout.

create table purchases (
    id uuid primary key,
    user_id uuid not null references users (id) on delete cascade,
    -- Stripe delivers webhooks at least once. This makes a repeat a no-op.
    stripe_session_id text not null unique,
    credits int not null,
    -- What was charged, in the currency's smallest unit, as Stripe reports it.
    amount_total bigint,
    currency text,
    created_at timestamptz not null default now()
);
create index purchases_user on purchases (user_id, created_at);
