import { type Page } from '@playwright/test';

/**
 * The data both render checks run against.
 *
 * One set, shared by the assertions (`ui-pages.spec.ts`) and the screenshots
 * (`shots.spec.ts`), so what a person looks at is exactly what the harness
 * measured. Two fixture sets would mean the picture and the check could disagree
 * and neither would say so.
 *
 * It is deliberately BUSY, and every item in it is a hazard carried on purpose.
 * The three that matter are specific to this app:
 *   1. **A session id is 36 unbreakable characters** and is what a session with
 *      no name is shown as — in a chip, in a menu, and in every history line.
 *   2. **A subject may be 200 characters** and is prose, so it wraps beside a
 *      meta column that must not be pushed off the edge.
 *   3. **A holder chip may be a raw session id**, which is the widest thing
 *      the meta column ever has to carry.
 */
export const ME = { kind: 'person', id: 'pippijn', name: 'Pippijn' };

/** A named session and an unnamed one: the second is the hard case, because it
 *  is drawn as its raw id everywhere a name would go. */
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
];

/** Who holds what. Carries the two hazards this screen has: an unnamed session,
 *  drawn as 36 unbreakable characters beside a count that must not be pushed
 *  off the edge; and a session that has cleared its plate (`0/56`), which is
 *  the row the second number exists for. */
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
  { kind: 'person', id: 'pippijn', name: 'Pippijn', open: 4, total: 19 },
  { kind: 'nobody', name: 'nobody', open: 7, total: 61 },
];

/** Real subjects from the corpus, plus one at the full 200-character cap. */
export const TASKS = [
  {
    id: 80,
    subject: 'Stop walking every transcript on the request path',
    status: 'open',
    // Ranked, and in the pile: the widest a list row's meta gets, since a pile
    // row also carries a "from" chip. If a rank is going to collide with
    // anything at phone width it is here.
    priority: 'P3',
    escalated_to: 'P0',
    // A deadline that has passed: the one state worth colouring, and the widest
    // this row gets — rank, date, id and a "from" chip at phone width.
    due: '2026-08-01',
    overdue: true,
    assignee: { kind: 'nobody' },
    // In the pile and says where it came from — the row `filed_by` exists for.
    filed_by: 'health',
    detailed: true,
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
    // Held AND filed by somebody else. The holder wins: this row must not draw
    // two names, and who is carrying it is the more useful one.
    filed_by: 'dev-lint',
    detailed: true,
    created_at: '2026-08-05T09:00:00Z',
    updated_at: '2026-08-08T09:00:00Z',
  },
  {
    id: 106,
    subject: 'The console task reader points at a store we deliberately emptied',
    status: 'open',
    // Blocked, and by TWO tasks — the case a single column could not have
    // carried. Also the widest this meta row gets: rank, block icon, id and an
    // unnamed session's uuid on one line at phone width.
    priority: 'P3',
    blocked_on: [80, 92],
    blocked: true,
    // No name: drawn as the raw uuid, which is wider than the phone.
    assignee: { kind: 'session', id: SESSIONS[1].id },
    detailed: false,
    created_at: '2026-08-08T09:00:00Z',
    updated_at: '2026-08-08T09:00:00Z',
  },
  {
    id: 110,
    subject: 'Nothing watches the boot disk, and nix deletes store paths mid-build before it fills',
    status: 'open',
    assignee: { kind: 'person', id: 'pippijn', name: 'pippijn' },
    detailed: true,
    created_at: '2026-08-06T09:00:00Z',
    updated_at: '2026-08-06T09:00:00Z',
  },
  {
    id: 128,
    // The other pile row, deliberately beside the one that speaks: Pippijn and
    // unnamed sessions leave nothing to say, and "not said" has to look like
    // silence rather than like a missing value.
    subject: 'Left for whoever picks it up, by somebody with no name to give',
    status: 'open',
    assignee: { kind: 'nobody' },
    detailed: false,
    created_at: '2026-08-07T09:00:00Z',
    updated_at: '2026-08-07T09:00:00Z',
  },
  {
    id: 131,
    subject: 'Something of mine that belongs to no checkout',
    status: 'open',
    assignee: { kind: 'person', id: 'pippijn', name: 'pippijn' },
    detailed: false,
    created_at: '2026-08-08T09:00:00Z',
    updated_at: '2026-08-08T09:00:00Z',
  },
];

/** The version an edit replaced, as the undo panel shows it.
 *
 *  Long, and deliberately: a real task body runs to thousands of characters, and
 *  the question this fixture answers is whether the panel's two buttons are
 *  still reachable at phone height once a body that size is above them. */
export const PREVIOUS = {
  at: '2026-08-13T09:06:12Z',
  actor: 'dev-lint',
  // Somebody else's edit, matching `actor` — the case the CLI refuses and this
  // page shows instead. A fixture saying `mine: true` beside a foreign actor
  // would be a shape no server can produce.
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
};

/** A task page with everything that can crowd the column: a long subject, a
 *  body with a fenced block and a table, and a history whose actor is a raw
 *  session id. */
export const DETAIL = {
  ...TASKS[1],
  // Something has overwritten this task, which is what puts the undo control on
  // the screen at all. The task page is where a clobbered body is noticed, so
  // the shot has to include it.
  restorable: true,
  // Blocked here too, so the TASK screen's chip is drawn in the shot rather
  // than only the list's icon — they are different markup and only one of them
  // spells the ids out.
  blocked_on: [80, 106],
  blocked: true,
  // Raised by its deadline, so the task screen draws BOTH the level it sorts
  // as and the one somebody set — the pair only this screen shows.
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
      actor: 'pippijn',
    },
    {
      at: '2026-08-08T09:00:00Z',
      kind: 'assigned',
      detail: 'nobody → memview',
      actor: 'pippijn',
    },
    {
      at: '2026-08-08T10:00:00Z',
      kind: 'status',
      detail: 'open → doing',
      // The raw id, which is what an unnamed session's history line carries.
      actor: SESSIONS[1].id,
    },
  ],
};

/** The same task, closed without being done.
 *
 *  A separate fixture because a dropped task is only ever *read*: it is gone
 *  from every list the API answers, so the one screen that can show one is this
 *  one, and the strike-through on the chip has nowhere else to be looked at. */
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
      actor: 'pippijn',
    },
  ],
};

/** Mock every backend call. Catch-all FIRST — Playwright runs handlers
 *  last-registered-first. */
export async function mockApi(page: Page): Promise<void> {
  await page.route('**/api/**', (r) =>
    r.request().method() === 'GET' ? r.fulfill({ json: [] }) : r.fulfill({ status: 204, body: '' }),
  );
  await page.route('**/api/me', (r) => r.fulfill({ json: ME }));
  await page.route('**/api/tasks**', (r) => r.fulfill({ json: TASKS }));
  await page.route('**/api/tasks/*', (r) => r.fulfill({ json: DETAIL }));
  await page.route('**/api/sessions', (r) => r.fulfill({ json: SESSIONS }));
  await page.route('**/api/holders', (r) => r.fulfill({ json: HOLDERS }));
  // Registered after the task route: Playwright matches handlers newest-first,
  // and this path is the one with an extra segment.
  await page.route('**/api/tasks/*/previous', (r) => r.fulfill({ json: PREVIOUS }));
}
