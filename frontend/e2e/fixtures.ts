import { type Page } from '@playwright/test';

import type { Holder, Me, Revision, Session, Task, TaskDetail } from '../src/app/models';

/**
 * The data both render checks use, one set, so the screenshots show exactly
 * what the harness measured. Deliberately busy; the hazards specific to this app:
 *   1. An unnamed session is drawn as its 36-character unbreakable id, in chips,
 *      menus and history.
 *   2. A subject may be 200 characters of prose wrapping beside the meta column.
 *   3. A holder chip may be a raw id, the widest thing in the meta column.
 */
export const ME = { kind: 'person', id: 'user', name: 'User' } satisfies Me;

/** A named and an unnamed session; the second is drawn as its raw id. */
export const SESSIONS = [
  {
    id: '7c0202eb-080b-40a5-a654-8758b4ca723e',
    name: 'memview',
    first_seen: '2026-08-01T09:00:00Z',
    last_seen: '2026-08-08T09:00:00Z',
    open: 12,
  },
  {
    id: '2be586d6-c868-4717-8364-7b5b8610abe5',
    first_seen: '2026-08-08T09:00:00Z',
    last_seen: '2026-08-08T12:00:00Z',
    open: 3,
  },
] satisfies Session[];

/** An unnamed session (36 characters beside a count) and a cleared plate (`0/56`). */
export const HOLDERS = [
  { kind: 'session', id: SESSIONS[0].id, name: 'memview', open: 12, total: 34 },
  { kind: 'session', id: SESSIONS[1].id, open: 3, total: 3 },
  {
    kind: 'session',
    id: '296dae53-3f84-4bd1-afbb-9ddcddedbdbb',
    name: 'health',
    open: 0,
    total: 56,
  },
  { kind: 'person', id: 'user', name: 'User', open: 4, total: 19 },
  { kind: 'nobody', name: 'nobody', open: 7, total: 61 },
] satisfies Holder[];

export const TASKS = [
  {
    id: 80,
    subject: 'Stop walking every transcript on the request path',
    status: 'open',
    // Ranked, in the pile, with a "from" chip: the widest meta row.
    priority: 'P3',
    escalated_to: 'P0',
    // Overdue: rank, date, id and a "from" chip at phone width.
    due: '2026-08-01',
    overdue: true,
    assignee: { kind: 'nobody' },
    // In the pile, with its filer.
    filed_by: 'health',
    detailed: true,
    body_lines: 37,
    created_at: '2026-08-05T09:00:00Z',
    updated_at: '2026-08-05T09:00:00Z',
  },
  {
    id: 92,
    subject:
      'Abstractly evaluate what the agents run, across languages, into one effect language, keeping the undetermined subjects counted rather than dropped, so a gap is a number and never a silence',
    status: 'doing',
    priority: 'P4',
    due: '2026-09-01',
    assignee: { kind: 'session', id: SESSIONS[0].id, name: 'memview' },
    // Held and filed by others: only the holder is drawn.
    filed_by: 'dev-lint',
    detailed: true,
    body_lines: 4,
    created_at: '2026-08-05T09:00:00Z',
    updated_at: '2026-08-08T09:00:00Z',
  },
  {
    id: 106,
    subject: 'The console task reader points at a store we deliberately emptied',
    status: 'open',
    // Blocked by two tasks: rank, block icon, id and a uuid on one line.
    priority: 'P3',
    blocked_on: [80, 92],
    blocked: true,
    // And flagged, adding the sprawl mark.
    sprawl_chars: 18162,
    // No name: the raw uuid, wider than the phone.
    assignee: { kind: 'session', id: SESSIONS[1].id },
    detailed: false,
    body_lines: 0,
    created_at: '2026-08-08T09:00:00Z',
    updated_at: '2026-08-08T09:00:00Z',
  },
  {
    id: 110,
    subject: 'Nothing watches the boot disk, and nix deletes store paths mid-build before it fills',
    status: 'open',
    assignee: { kind: 'person', id: 'user', name: 'user' },
    detailed: true,
    body_lines: 182,
    created_at: '2026-08-06T09:00:00Z',
    updated_at: '2026-08-06T09:00:00Z',
  },
  {
    id: 128,
    // A pile row with no filer: "not said" must look like silence.
    subject: 'Left for whoever picks it up, by somebody with no name to give',
    status: 'open',
    assignee: { kind: 'nobody' },
    detailed: false,
    body_lines: 0,
    created_at: '2026-08-07T09:00:00Z',
    updated_at: '2026-08-07T09:00:00Z',
  },
  {
    id: 131,
    subject: 'Something of mine that belongs to no checkout',
    status: 'open',
    assignee: { kind: 'person', id: 'user', name: 'user' },
    detailed: false,
    body_lines: 0,
    created_at: '2026-08-08T09:00:00Z',
    updated_at: '2026-08-08T09:00:00Z',
  },
] satisfies Task[];

/** Long, like real bodies: are the panel's two buttons still reachable at phone height? */
export const PREVIOUS = {
  at: '2026-08-13T09:06:12Z',
  actor: 'dev-lint',
  // Somebody else's edit; `mine: true` beside a foreign actor is a shape no server produces.
  mine: false,
  subject: 'Abstractly evaluate a shell command, as far as the text determines and no further',
  body: [
    '## Where it stands, 2026-08-13',
    '',
    'The rule is COMPLETE, both layers wired, measured, and still switched off',
    'behind `--recursion`. All three "what is left" items below are closed.',
    '',
    '## The one thing blocking adoption',
    '',
    '`main.rs:1604` probes for `cargo` and `opt` once, before any crate is',
    'examined — deliberately, so a missing tool is not announced once per',
    'manifest. But it happens before anything knows whether a crate opted in,',
    'so a workspace that opted nothing in still pays for the probe and still',
    'reports the tools as missing.',
    '',
    '| layer | file | tests |',
    '| --- | --- | --- |',
    '| 1 | totality-checks/src/ir.rs | 10 |',
    '| 2 | totality-checks/src/descent.rs | 24 |',
  ].join('\n'),
} satisfies Revision;

/**
 * A task page with everything that crowds the column: long subject, fenced block, table,
 * raw-id actor.
 */
export const DETAIL = {
  ...TASKS[1],
  // Overwritten, so the undo control is in the shot.
  restorable: true,
  // Blocked, so the task screen's chip (different markup from the list icon) is drawn.
  blocked_on: [80, 106],
  blocked: true,
  // Flagged, so the critique card is drawn, the only place its words appear.
  sprawl_chars: 18162,
  sprawl_said:
    "Move 'Method note' bullets to a single checklist AFTER 'What to do next', not scattered through dated entries.\n'Still unmapped' is loose inventory not tied to the three actionable items; delete unless it earns its place.",
  // Raised by its deadline, so both ranks are drawn.
  escalated_to: 'P0',
  body: '…',
  body_html: `<p>Read as a language problem, the reader is an <strong>abstract
interpreter</strong>: it evaluates as far as the text determines and stops.</p>
<pre><code>cd health &amp;&amp; nix develop -c bash -c "sed -i 's/a/b/' src/geo/velocity.ts"</code></pre>
<table><thead><tr><th>module</th><th>question it answers</th></tr></thead>
<tbody><tr><td>reader/src/shell_ops.rs</td><td>what does one command do — to which paths?</td></tr></tbody></table>`,
  events: [
    {
      at: '2026-08-05T09:00:00Z',
      kind: 'created',
      detail: 'Abstractly evaluate',
      actor: 'user',
    },
    {
      at: '2026-08-08T09:00:00Z',
      kind: 'assigned',
      detail: 'nobody → memview',
      actor: 'user',
    },
    {
      at: '2026-08-08T10:00:00Z',
      kind: 'status',
      detail: 'open → doing',
      actor: SESSIONS[1].id,
    },
  ],
} satisfies TaskDetail;

/** Dropped: only this screen ever shows one, so its strike-through is checked here. */
export const DROPPED = {
  ...DETAIL,
  status: 'dropped',
  closed_at: '2026-08-08T12:00:00Z',
  events: [
    ...DETAIL.events,
    {
      at: '2026-08-08T12:00:00Z',
      kind: 'status',
      detail: 'doing → dropped',
      actor: 'user',
    },
  ],
} satisfies TaskDetail;

/** Mock every backend call; catch-all first, since Playwright runs the newest handler first. */
export async function mockApi(page: Page): Promise<void> {
  await page.route('**/api/**', (r) => r.fulfill({ status: 204, body: '' }));
  await page.route('**/api/me', (r) => r.fulfill({ json: ME }));
  await page.route('**/api/tasks**', (r) => r.fulfill({ json: TASKS }));
  await page.route('**/api/tasks/*', (r) => r.fulfill({ json: DETAIL }));
  await page.route('**/api/sessions', (r) => r.fulfill({ json: SESSIONS }));
  await page.route('**/api/holders', (r) => r.fulfill({ json: HOLDERS }));
  // After the task route: newest-first, and this path is longer.
  await page.route('**/api/tasks/*/previous', (r) => r.fulfill({ json: PREVIOUS }));
}
