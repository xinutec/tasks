import { describe, expect, it } from 'vitest';

import { Task } from './models';
import { matches } from './search';

const task: Task = {
  id: 92,
  subject: 'Abstractly evaluate what the agents run',
  status: 'open',
  assignee: { kind: 'session', id: 'sess-1', name: 'memview' },
  filed_by: 'dev-lint',
  detailed: false,
  body_lines: 0,
  created_at: '2026-08-05T09:00:00Z',
  updated_at: '2026-08-05T09:00:00Z',
};

describe('matches', () => {
  it('matches everything when nothing is typed', () => {
    expect(matches(task, '')).toBe(true);
    expect(matches(task, '   ')).toBe(true);
  });

  it('finds words in the subject, in any case and any order', () => {
    expect(matches(task, 'AGENTS abstractly')).toBe(true);
  });

  it('needs every word, not any', () => {
    expect(matches(task, 'agents transcript')).toBe(false);
  });

  it('finds the holder and the filer by name', () => {
    expect(matches(task, 'memview')).toBe(true);
    expect(matches(task, 'dev-lint')).toBe(true);
  });

  it('takes a number as the id, with or without its #', () => {
    expect(matches(task, '#92')).toBe(true);
    expect(matches(task, '92')).toBe(true);
    // Not a prefix: #9 is another task.
    expect(matches(task, '#9')).toBe(false);
  });
});
