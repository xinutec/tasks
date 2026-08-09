import { describe, expect, it } from 'vitest';

import { EVERYTHING, focusOn, holderLabel, inBucket, parseWho, whoParam } from './holder';
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
  const cases: [string, Assignee, boolean][] = [
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
      expect(inBucket(assignee, parseWho(who), 'pippijn')).toBe(want);
    });
  }

  it('treats an unknown viewer as any person rather than none', () => {
    // `/api/me` is in flight for the first paint. Matching nobody there would
    // draw an empty list, which reads as "no work" — the one wrong answer this
    // screen must not give.
    expect(inBucket(me, parseWho('mine'), null)).toBe(true);
  });

  it('does not count another person as mine', () => {
    expect(inBucket({ kind: 'person', id: 'someone-else' }, parseWho('mine'), 'pippijn')).toBe(false);
  });
});

describe('one named holder', () => {
  it('shows that session and no other', () => {
    // #657: `with a session` is every session at once. This is the selection
    // the app could not make — the one that answers "what is hardware holding".
    expect(inBucket(named, parseWho('session:sess-1'), 'pippijn')).toBe(true);
    expect(inBucket(unnamed, parseWho('session:sess-1'), 'pippijn')).toBe(false);
    expect(inBucket(nobody, parseWho('session:sess-1'), 'pippijn')).toBe(false);
    expect(inBucket(me, parseWho('session:sess-1'), 'pippijn')).toBe(false);
  });

  it('shows that person and no other', () => {
    expect(inBucket(me, parseWho('person:pippijn'), 'pippijn')).toBe(true);
    expect(inBucket(named, parseWho('person:pippijn'), 'pippijn')).toBe(false);
  });

  it('does not confuse a session id with a person id', () => {
    // The prefix is what stops this: both are bare strings on the wire, and a
    // session could perfectly well be identified as `pippijn`.
    const twin: Assignee = { kind: 'session', id: 'pippijn' };
    expect(inBucket(twin, parseWho('person:pippijn'), 'pippijn')).toBe(false);
    expect(inBucket(twin, parseWho('session:pippijn'), 'pippijn')).toBe(true);
  });

  it('does not read a holder id as a bucket', () => {
    // A session named `all` must not turn the filter into "everything". The
    // prefix means `session:all` and `all` are different strings, which is the
    // reason for prefixing rather than accepting bare ids.
    const awkward: Assignee = { kind: 'session', id: 'all' };
    expect(inBucket(nobody, parseWho('session:all'), 'pippijn')).toBe(false);
    expect(inBucket(awkward, parseWho('session:all'), 'pippijn')).toBe(true);
  });
});

describe('focusOn', () => {
  it('names the holder a row should link to', () => {
    expect(whoParam(focusOn(named))).toBe('session:sess-1');
    expect(whoParam(focusOn(me))).toBe('person:pippijn');
  });

  it('sends the pile to the bucket it already has', () => {
    // Not `nobody:` — two spellings for one selection is how they drift.
    expect(whoParam(focusOn(nobody))).toBe('pile');
  });
});

describe('parseWho', () => {
  it('reads every selection the URL can carry', () => {
    expect(parseWho('pile')).toEqual({ kind: 'bucket', bucket: 'pile' });
    expect(parseWho('session:sess-1')).toEqual({ kind: 'session', id: 'sess-1' });
    expect(parseWho('person:pippijn')).toEqual({ kind: 'person', id: 'pippijn' });
  });

  it('falls back to everything rather than to nothing', () => {
    // ⚠ This is the case the old string union got WRONG by construction: the
    // query parameter was cast to `Who`, so `?who=garbage` was typed as valid,
    // matched no branch, and returned undefined — an empty list, which on this
    // screen reads as "no open work" rather than as a bad URL.
    for (const raw of [null, undefined, '', 'garbage', 'session', 'session:', 'person:']) {
      expect(parseWho(raw), `${raw}`).toEqual(EVERYTHING);
    }
  });

  it('round-trips through the URL', () => {
    for (const raw of ['mine', 'sessions', 'pile', 'session:sess-1', 'person:pippijn']) {
      expect(whoParam(parseWho(raw))).toBe(raw);
    }
    // `all` is the default and is left out of the URL entirely.
    expect(whoParam(parseWho('all'))).toBe(null);
  });
});
