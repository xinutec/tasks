import { describe, expect, it } from 'vitest';

import { Who, holderLabel, inBucket } from './holder';
import { Assignee } from './models';

const nobody: Assignee = { kind: 'nobody' };
const me: Assignee = { kind: 'person', id: 'pippijn', name: 'pippijn' };
const named: Assignee = { kind: 'session', id: 'sess-1', name: 'memview' };
const unnamed: Assignee = { kind: 'session', id: 'sess-2' };

describe('holderLabel', () => {
  it('prefers the name and falls back to the id', () => {
    expect(holderLabel(named)).toBe('memview');
    expect(holderLabel(unnamed)).toBe('sess-2');
  });

  it('never renders an empty holder', () => {
    // A blank chip reads as unassigned, which is a lie about a task somebody is
    // holding — so an assignee with neither name nor id says so.
    expect(holderLabel(nobody)).toBe('nobody');
    expect(holderLabel({ kind: 'session' })).toBe('nobody');
  });
});

describe('inBucket', () => {
  const cases: [Who, Assignee, boolean][] = [
    ['all', nobody, true],
    ['all', named, true],
    ['mine', me, true],
    ['mine', named, false],
    ['mine', nobody, false],
    ['sessions', named, true],
    ['sessions', unnamed, true],
    ['sessions', me, false],
    ['pile', nobody, true],
    ['pile', me, false],
  ];

  for (const [who, assignee, want] of cases) {
    it(`${who}: ${assignee.kind} → ${want}`, () => {
      expect(inBucket(assignee, who, 'pippijn')).toBe(want);
    });
  }

  it('treats an unknown viewer as any person rather than none', () => {
    // `/api/me` is in flight for the first paint. Matching nobody there would
    // draw an empty list, which reads as "no work" — the one wrong answer this
    // screen must not give.
    expect(inBucket(me, 'mine', null)).toBe(true);
  });

  it('does not count another person as mine', () => {
    expect(inBucket({ kind: 'person', id: 'someone-else' }, 'mine', 'pippijn')).toBe(false);
  });
});
