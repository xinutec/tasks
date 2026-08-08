// The app-specific half of the shared phone-width harness (@xinutec/ui-harness).
// Read by BOTH playwright.config.ts and the harness's static server, so there is
// one place to say what this app is and no port to keep in step — the port is
// allocated from `app`.

/** @type {import('@xinutec/ui-harness/config').HarnessSpec} */
export default {
  app: 'tasks',
  dist: 'dist/tasks-web/browser',
  // Fallback stub only — the specs page.route everything. Signed-in person with
  // an empty list, so an un-mocked run still renders.
  api: {
    '/api/me': { kind: 'person', id: 'pippijn', name: 'Pippijn' },
    '/api/tasks': [],
    '/api/repos': [],
    '/api/sessions': [],
  },
};
