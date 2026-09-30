# WireHub UI

English network console with a white interface. Uses the existing Rust HTTP API unchanged.

```sh
pnpm --dir frontend install
pnpm --dir frontend dev
pnpm --dir frontend build
```

Initial setup defaults to `10.10.10.0/24` (hub `.1`, peers `.2–.254`). The subnet can be edited before setup; existing networks keep their configured range.

The dev server proxies `/api` to `127.0.0.1:51820`. Production assets are written to `frontend/dist`; rebuild the Rust binary to embed the latest UI.

## Groups

Drag any of the four handles to connect groups. One-way access follows the drag start → end; both-way access sets both existing ACLs. Select a link to reverse its direction or disconnect it, including with Delete/Backspace. Self-access has a separate switch. Changes apply only after **Save**; partial saves reload server policy, and uncertain results require a successful reload before further editing.

Group positions are saved only in browser local storage. Auto layout and fit controls adjust the canvas. No backend layout endpoint is required. Interactions follow [the reference canvas](https://github.com/touken928/WireHub/tree/v0/web/src/components/groups).

## UI regression tests

Requires Node 20.11+ and an installed Google Chrome. After building:

```sh
pnpm --dir frontend test:ui
```

The test serves the production bundle on a temporary loopback port and intercepts every API request with isolated fixtures. It covers actual handle dragging, direction, layout restoration, keyboard deletion, self-access, partial save recovery, CRUD, session reset, setup, English labels, and mobile layout. It does not access a live hub.

Use `PLAYWRIGHT_CHANNEL` to select another installed browser channel. Set `WIREHUB_UI_SCREENSHOTS` to an output directory to save desktop/mobile screenshots.
