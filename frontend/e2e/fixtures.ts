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
 *   3. **The filter rows grow with the number of repositories**, without limit.
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

export const REPOS = [
  { repo: 'memview', open: 21 },
  { repo: 'xinutec-infra', open: 8 },
  { repo: 'health', open: 34 },
  { repo: 'tasks', open: 5 },
  { repo: null, open: 2 },
];

/** Who holds what. Carries the two hazards this screen has: an unnamed session,
 *  drawn as 36 unbreakable characters beside a count that must not be pushed
 *  off the edge; and a session that has cleared its plate (`0/56`), which is
 *  the row the second number exists for. */
export const HOLDERS = [
  { kind: 'session', id: SESSIONS[0].id, name: 'memview', open: 12, total: 34 },
  { kind: 'session', id: SESSIONS[1].id, open: 3, total: 3 },
  { kind: 'session', id: '296dae53-3f84-4bd1-afbb-9ddcddedbdbb', name: 'health', open: 0, total: 56 },
  { kind: 'person', id: 'pippijn', name: 'Pippijn', open: 4, total: 19 },
  { kind: 'nobody', name: 'nobody', open: 7, total: 61 },
];

/** Real subjects from the corpus, plus one at the full 200-character cap. */
export const TASKS = [
  {
    id: 80,
    repo: 'memview',
    subject: 'Stop walking every transcript on the request path',
    status: 'open',
    assignee: { kind: 'nobody' },
    detailed: true,
    created_at: '2026-08-05T09:00:00Z',
    updated_at: '2026-08-05T09:00:00Z',
  },
  {
    id: 92,
    repo: 'memview',
    subject:
      'Abstractly evaluate what the agents run, across languages, into one effect language, keeping the undetermined subjects counted rather than dropped, so a gap is a number and never a silence',
    status: 'doing',
    assignee: { kind: 'session', id: SESSIONS[0].id, name: 'memview' },
    detailed: true,
    created_at: '2026-08-05T09:00:00Z',
    updated_at: '2026-08-08T09:00:00Z',
  },
  {
    id: 106,
    repo: 'memview',
    subject: 'The console task reader points at a store we deliberately emptied',
    status: 'open',
    // No name: drawn as the raw uuid, which is wider than the phone.
    assignee: { kind: 'session', id: SESSIONS[1].id },
    detailed: false,
    created_at: '2026-08-08T09:00:00Z',
    updated_at: '2026-08-08T09:00:00Z',
  },
  {
    id: 110,
    repo: 'xinutec-infra',
    subject: 'Nothing watches the boot disk, and nix deletes store paths mid-build before it fills',
    status: 'open',
    assignee: { kind: 'person', id: 'pippijn', name: 'pippijn' },
    detailed: true,
    created_at: '2026-08-06T09:00:00Z',
    updated_at: '2026-08-06T09:00:00Z',
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

/** A task page with everything that can crowd the column: a long subject, a
 *  body with a fenced block and a table, and a history whose actor is a raw
 *  session id. */
export const DETAIL = {
  ...TASKS[1],
  body: '…',
  body_html: `<p>Read as a language problem, the reader is an <strong>abstract
interpreter</strong>: it evaluates as far as the text determines and stops.</p>
<pre><code>cd health &amp;&amp; nix develop -c bash -c "sed -i 's/a/b/' src/geo/velocity.ts"</code></pre>
<table><thead><tr><th>module</th><th>question it answers</th></tr></thead>
<tbody><tr><td>reader/src/shell_ops.rs</td><td>what does one command do — to which paths?</td></tr></tbody></table>`,
  events: [
    { at: '2026-08-05T09:00:00Z', kind: 'created', detail: 'Abstractly evaluate', actor: 'pippijn' },
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

/** Mock every backend call. Catch-all FIRST — Playwright runs handlers
 *  last-registered-first. */
export async function mockApi(page: Page): Promise<void> {
  await page.route('**/api/**', (r) =>
    r.request().method() === 'GET' ? r.fulfill({ json: [] }) : r.fulfill({ status: 204, body: '' }),
  );
  await page.route('**/api/me', (r) => r.fulfill({ json: ME }));
  await page.route('**/api/tasks**', (r) => r.fulfill({ json: TASKS }));
  await page.route('**/api/tasks/*', (r) => r.fulfill({ json: DETAIL }));
  await page.route('**/api/repos', (r) => r.fulfill({ json: REPOS }));
  await page.route('**/api/sessions', (r) => r.fulfill({ json: SESSIONS }));
  await page.route('**/api/holders', (r) => r.fulfill({ json: HOLDERS }));
}
