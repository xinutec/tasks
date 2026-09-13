import { TestBed } from '@angular/core/testing';
import { SwUpdate, UnrecoverableStateEvent, VersionEvent } from '@angular/service-worker';
import { Subject } from 'rxjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { SwUpdates } from './sw-updates';

// The rules live in `@xinutec/ui-harness/sw-updates` and are unit-tested there
// against a fake. What these cover is what a fake cannot reach: that Angular's
// `SwUpdate.versionUpdates` really feeds the policy, filtered to VERSION_READY,
// and that a reload really happens. Only the NAVIGATION is stubbed, so
// applyUpdate() runs for real — including its failure path.
function setup(isEnabled: boolean) {
  const versionUpdates = new Subject<VersionEvent>();
  const unrecoverable = new Subject<UnrecoverableStateEvent>();
  const checkForUpdate = vi.fn().mockResolvedValue(false);
  const activateUpdate = vi.fn().mockResolvedValue(true);
  TestBed.configureTestingModule({
    providers: [
      SwUpdates,
      {
        provide: SwUpdate,
        useValue: { isEnabled, versionUpdates, unrecoverable, checkForUpdate, activateUpdate },
      },
    ],
  });
  const svc = TestBed.inject(SwUpdates);
  const reload = vi.spyOn(svc, 'reload').mockImplementation(() => {});
  return { svc, versionUpdates, unrecoverable, checkForUpdate, activateUpdate, reload };
}

// Built whole rather than asserted from a `{ type }` stub: `as VersionEvent`
// silences the compiler about the fields Angular really sends, so the day the
// service reads one of them the test still passes on a shape the browser never
// produces.
const ready: VersionEvent = {
  type: 'VERSION_READY',
  currentVersion: { hash: 'old' },
  latestVersion: { hash: 'new' },
};
const detected: VersionEvent = { type: 'VERSION_DETECTED', version: { hash: 'new' } };
const noUpdate: VersionEvent = { type: 'NO_NEW_VERSION_DETECTED', version: { hash: 'old' } };

function setVisibility(state: 'visible' | 'hidden') {
  Object.defineProperty(document, 'visibilityState', { value: state, configurable: true });
  document.dispatchEvent(new Event('visibilitychange'));
}

describe('SwUpdates', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    sessionStorage.clear(); // the one-shot recovery marker lives here
    Object.defineProperty(document, 'visibilityState', { value: 'visible', configurable: true });
  });
  afterEach(() => vi.useRealTimers());

  it('checks at startup and reloads when a new version is ready right away', () => {
    const { svc, versionUpdates, checkForUpdate, activateUpdate } = setup(true);
    svc.start();
    expect(checkForUpdate).toHaveBeenCalledOnce();
    versionUpdates.next(ready);
    expect(activateUpdate).toHaveBeenCalledOnce();
  });

  it('does nothing when the service worker is disabled (dev build)', () => {
    const { svc, versionUpdates, checkForUpdate, activateUpdate } = setup(false);
    svc.start();
    expect(checkForUpdate).not.toHaveBeenCalled();
    versionUpdates.next(ready);
    expect(activateUpdate).not.toHaveBeenCalled();
  });

  it('ignores version events other than VERSION_READY', () => {
    const { svc, versionUpdates, activateUpdate } = setup(true);
    svc.start();
    versionUpdates.next(detected);
    versionUpdates.next(noUpdate);
    expect(activateUpdate).not.toHaveBeenCalled();
  });

  it('re-checks for updates when the app becomes visible again (stale tab)', () => {
    const { svc, checkForUpdate } = setup(true);
    svc.start();
    expect(checkForUpdate).toHaveBeenCalledTimes(1);
    setVisibility('hidden');
    expect(checkForUpdate).toHaveBeenCalledTimes(1); // hiding does not check
    setVisibility('visible');
    expect(checkForUpdate).toHaveBeenCalledTimes(2);
  });

  it('defers a mid-session update to the next backgrounding, not mid-use', () => {
    const { svc, versionUpdates, activateUpdate } = setup(true);
    svc.start();
    vi.advanceTimersByTime(60_000); // long past the startup window
    versionUpdates.next(ready);
    expect(activateUpdate).not.toHaveBeenCalled(); // user may be mid-edit
    setVisibility('hidden');
    expect(activateUpdate).toHaveBeenCalledOnce(); // reloads invisibly once backgrounded
  });

  it('applies a mid-session update immediately when the app is hidden', () => {
    const { svc, versionUpdates, activateUpdate } = setup(true);
    svc.start();
    vi.advanceTimersByTime(60_000);
    Object.defineProperty(document, 'visibilityState', { value: 'hidden', configurable: true });
    versionUpdates.next(ready);
    expect(activateUpdate).toHaveBeenCalledOnce();
  });

  it('checkNow applies immediately — the user explicitly asked', async () => {
    const { svc, versionUpdates, checkForUpdate, activateUpdate } = setup(true);
    svc.start();
    vi.advanceTimersByTime(60_000);
    checkForUpdate.mockResolvedValueOnce(true);
    await expect(svc.checkNow()).resolves.toBe('updating');
    versionUpdates.next(ready);
    expect(activateUpdate).toHaveBeenCalledOnce(); // no deferral on a manual check
  });

  it('checkNow reports current when no update was found', async () => {
    const { svc } = setup(true);
    svc.start();
    await expect(svc.checkNow()).resolves.toBe('current');
  });
});
