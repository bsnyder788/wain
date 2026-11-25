# wain

Design wainscoting and wall trim for a room, then get the woodworking plans:
a dimensioned elevation, a cut list and a shopping list.

## Layout

```
crates/
  wain-core/    Layout engine: units, wall layout, cut list, stock packing. Pure Rust, no I/O.
  wain-wasm/    WebAssembly bindings for the engine (JSON in, JSON out).
  wain-server/  HTTP API (axum). So far: /healthz and POST /api/v1/plan.
apps/
  web/          SvelteKit single-page app. Runs the engine in the browser.
scripts/
  build-engine.sh   Builds the WebAssembly package and TypeScript types for the web app.
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

## Develop

```sh
# Engine and server
cargo test --workspace
cargo run -p wain-server          # http://127.0.0.1:8080, PORT to change

# Web app
cd apps/web
npm install
npm run engine                    # build WASM + TS types; re-run after changing Rust
npm run dev                       # http://localhost:5173
npm run check && npm test && npm run build
```

`npm run engine` writes `apps/web/src/lib/engine/pkg` and
`apps/web/src/lib/engine/bindings`. Both are generated and git-ignored, so a
fresh clone needs it once before the web app will build.

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
