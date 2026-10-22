# WireHub UI

English network console with a white interface. Uses the existing Rust HTTP API unchanged.

```sh
pnpm --dir frontend install
pnpm --dir frontend dev
pnpm --dir frontend build
```

Initial setup defaults to `10.10.10.0/24` (hub `.1`, peers `.2–.254`). The subnet can be edited before setup; existing networks keep their configured range.

The dev server proxies `/api` to `127.0.0.1:51820`. Production assets are written to `frontend/dist`; rebuild the Rust binary to embed the latest UI.

## Traffic

Overview counts each delivered transfer once using the sum of hub receive counters,
rather than adding both sides of the same internal transfer. Current rate is the
byte delta divided by the actual interval between successful samples (normally
5 seconds); the chart keeps the last 5 minutes of samples while signed in.
The first sample, peer membership changes, counter resets, and recovery after a
failed request or hidden tab establish a new baseline before showing a rate.
Signing out clears the samples. Individual peer rows retain directional counters.

## Groups

Drag any of the four handles to connect groups. One-way access follows the drag start → end; both-way access sets both existing ACLs. Select a link to reverse its direction or disconnect it, including with Delete/Backspace. Self-access has a separate switch. Changes apply only after **Save**; partial saves reload server policy, and uncertain results require a successful reload before further editing.

Group positions are saved only in browser local storage. Auto layout and fit controls adjust the canvas. No backend layout endpoint is required. Interactions follow [the reference canvas](https://github.com/touken928/WireHub/tree/v0/web/src/components/groups).

## Component boundaries

`App.tsx` owns the authenticated session, authoritative resource lists and the
freshness boundary for background reads. Modal state records one kind and, for
forward editing, the rule ID; unrelated dialog states cannot be combined.

`components/forwards/ForwardDialog.tsx` owns forward form drafts, loading and
revision preconditions. Both creation and editing invalidate background reads
at mutation start and settlement. The resource owner merges confirmed forwards
by ID, so a refresh cannot duplicate a newly created rule.

`components/Dialog.tsx` owns modal focus, keyboard containment and dismissal.
It focuses an enabled control even while a form is loading. API response parsing
and mutation error messages live in `src/api/client.ts`.

## UI regression tests

Requires Node.js 22 and an installed Google Chrome. After building:

```sh
pnpm --dir frontend test:ui
```

The test serves the production bundle on a temporary loopback port and intercepts every API request with isolated fixtures. It covers actual handle dragging, direction, layout restoration, keyboard deletion, self-access, partial save recovery, CRUD, session reset, setup, English labels, and mobile layout. It does not access a live hub.

Every run includes the core suite and all conflict, race and recovery scenarios.
Screenshots and failure diagnostics are saved automatically under the repository's
ignored `artifacts/ui/` directory. No test environment variables are required.

## Visual alignment checks

After building, run `pnpm --dir frontend test:visual` with Google Chrome installed.
The isolated fixture suite captures every page at 320–1920 px, including long
names, large counters, group/link details, dialogs, validation, empty states,
policy conflicts and short windows. Geometry checks catch horizontal overflow,
column drift, inconsistent heading positions, metric baselines, form alignment
and dialog centering. Screenshots and the JSON report are saved to the ignored
`artifacts/visual/` directory for visual review.
