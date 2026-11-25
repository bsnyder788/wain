# wain

Design wainscoting and wall trim for a room, then get the woodworking plans:
a dimensioned elevation, a cut list and a shopping list.

## Layout

```
crates/
  wain-core/    Layout engine: units, wall layout, cut list, stock packing. Pure Rust, no I/O.
  wain-wasm/    WebAssembly bindings for the engine (JSON in, JSON out).
  wain-server/  HTTP API (axum + Postgres): accounts, credits, AI suggestions, plan.
apps/
  web/          SvelteKit single-page app. Runs the engine in the browser.
scripts/
  build-engine.sh   Builds the WebAssembly package and TypeScript types for the web app.
  dev-db.sh         Starts a local Postgres in Docker for the server and its tests.
```

The engine is the source of truth for every dimension. The same `wain-core`
code runs natively in the server and as WebAssembly in the browser, so a
drawing and its cut list can never disagree.

All lengths are integer nanometres (`Length`). That unit divides both 1/64"
and 0.1 mm exactly, so imperial and metric projects are both stored without
rounding. Display units are a preference; see `crates/wain-core/src/units.rs`.

## Prerequisites

- Rust via [rustup](https://rustup.rs), with the WebAssembly target:
  `rustup target add wasm32-unknown-unknown`. A Homebrew Rust on its own
  cannot build the WebAssembly package.
- Node 22.17 or newer.
- Docker, for the local Postgres that the server and its tests use.

## Develop

```sh
# Database, then engine and server
scripts/dev-db.sh                 # prints DATABASE_URL; `stop` and `reset` also work
cp .env.example .env              # once; the server reads it
export DATABASE_URL=postgres://wain:wain@127.0.0.1:54320/wain
cargo test --workspace            # server tests create a throwaway database each
cargo run -p wain-server          # http://127.0.0.1:8787

# Web app
cd apps/web
npm install
npm run engine                    # build WASM + TS types; re-run after changing Rust
npm run dev                       # http://localhost:5173
npm run check && npm test && npm run build
```

`npm run engine` writes `apps/web/src/lib/engine/pkg` and
`apps/web/src/lib/engine/bindings` (types for both the engine and the server's
API). Both are generated and git-ignored, so a fresh clone needs it once
before the web app will build.

The web app runs without the server: designing, drawings and cut lists all
happen in the browser. Accounts and AI features need it, and Vite proxies
`/api` to it on port 8787.

## Engine notes

- A wall is split into *runs* (between corners and door casings) and each run
  into *zones* of constant height (full height, or under a window). Each style
  fills a zone with equally spaced members.
- Styles so far: picture frame, board and batten, shaker panel.
- Spacing is agreed across the whole room: every zone picks the bay count
  nearest a shared spacing that starts at the user's target and settles
  halfway toward what the walls can actually achieve
  (`layout/align.rs`).
- Spacing then shifts by a bay or two to keep trim off outlets and switches;
  when nothing clears, the plan carries a warning instead.
- Walls are listed in order around the room, and each wall says how its right
  end meets the next (`Corner`). Where two trimmed walls meet, the earlier
  wall's trim runs through and the next one butts against it
  (`layout/corners.rs`):
  - Flat boards overlap by the board thickness. Members are spaced on the
    *visible* face, so every wall shows full-width end members and equal
    bays; the hidden overlap only changes cut sizes (a wider corner batten
    on the through wall at an inside corner, a narrower one behind the lap at
    an outside corner). Rails butt inside and are mitered outside.
  - Profiled mouldings (chair rail, new baseboard) are coped inside and
    mitered outside.
  - A door in the corner cancels the joint, so the other wall runs to the
    wall.
- Stock packing picks a length for each board, so one long rail does not force
  short pieces onto long stock. Parts longer than any stock are split and
  flagged as needing a joint.

Not handled yet: choosing which wall runs through at a corner (it follows
wall order), corners that are out of square, stairs, and sheet goods.

## Photo preview

- The user marks a wall's four corners on a photo. `wain-core`'s
  `perspective` module turns those into a homography between the wall's
  elevation and the photo (closed form, no solver), which the web app uses to
  draw the plan's parts over the photo and to turn a drag on the photo into
  an obstruction's position on the wall.
- `Wall.photo` holds only the image id, its pixel size and the corners (as
  fractions of the image). Layout never reads it, and half-placed corners do
  not block the plan.
- Images live in the browser's IndexedDB for now (`apps/web/src/lib/photos.ts`).
  On import they are scaled to at most 2048 px and re-encoded, which also
  strips metadata such as GPS location. Cloud storage arrives with accounts.
- Lens distortion is ignored, and the overlay is a flat sketch rather than a
  rendering.

## Server

Everything is under `/api/v1`. Settings come from the environment
(`.env.example` lists them).

- **Sign-in** is by a six-digit code sent to the user's email, exchanged for a
  session cookie (`HttpOnly`, `SameSite=Lax`). Codes and session tokens are
  stored only as hashes, each code allows five attempts and ten minutes, and
  an address can request five codes per quarter hour. There is no email
  provider yet, so the server only starts with `WAIN_DEV=1`, which shows the
  code in the browser and the log instead.
- **Credits** meter features that cost money. The ledger
  (`credit_entries`) is append-only and a balance is the sum of its rows. An
  action is charged up front under a row lock and refunded if it fails, so a
  failed AI call costs nothing and two requests cannot spend the same credit.
  New accounts get 10.
- **AI suggestions** (`POST /suggestions`, 1 credit) send the wall's photo and
  measurements to Claude and get back a few treatments chosen from the
  engine's styles, as four numbers each. Those become ordinary `Design`s that
  the engine validates; any it cannot build are dropped. The model never
  decides geometry. Needs `ANTHROPIC_API_KEY`; `WAIN_FAKE_AI=1` answers with
  fixed samples instead, for working on the flow at no cost.
- State-changing requests from another site are refused (the `Origin` host
  must match `Host`), on top of the cookie's `SameSite` setting.

Not built yet: email delivery, Google and Apple sign-in, saving projects and
photos to the server, photoreal renders, and billing.

