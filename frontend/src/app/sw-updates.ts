import { Injectable, inject } from '@angular/core';
import { SwUpdate, VersionReadyEvent } from '@angular/service-worker';
import {
  type PagePort,
  type ServiceWorkerPort,
  SwUpdates as SwUpdatePolicy,
} from '@xinutec/ui-harness/sw-updates';
import { filter } from 'rxjs';

/** Marks that we already reloaded out of a broken service worker; session-scoped, a stable key. */
const RECOVERY_KEY = 'tasks.sw-recovery-attempted';

/**
 * Self-update, the Angular adapter for `@xinutec/ui-harness/sw-updates`. The
 * cached shell would otherwise never learn a newer build exists.
 */
@Injectable({ providedIn: 'root' })
export class SwUpdates {
  private readonly sw = inject(SwUpdate);

  private readonly serviceWorker: ServiceWorkerPort = ((sw: SwUpdate) => ({
    // A local, not `this`: a getter in an object literal doesn't capture `this`, and a copy would freeze.
    get isEnabled(): boolean {
      return sw.isEnabled;
    },
    onVersionReady: (handler: () => void): void => {
      sw.versionUpdates
        .pipe(filter((event): event is VersionReadyEvent => event.type === 'VERSION_READY'))
        .subscribe(() => handler());
    },
    onUnrecoverable: (handler: () => void): void => {
      // The cache is broken and the server no longer has the files (a roll-forward
      // after eviction): only a fresh load recovers.
      sw.unrecoverable.subscribe(() => handler());
    },
    checkForUpdate: () => sw.checkForUpdate(),
    activateUpdate: () => sw.activateUpdate(),
  }))(this.sw);

  private readonly page: PagePort = {
    get hidden(): boolean {
      return document.visibilityState === 'hidden';
    },
    onVisibilityChange: (handler: () => void): void => {
      document.addEventListener('visibilitychange', handler);
    },
    recoveryAttempted: () => sessionStorage.getItem(RECOVERY_KEY) !== null,
    markRecoveryAttempted: () => sessionStorage.setItem(RECOVERY_KEY, '1'),
    reload: () => this.reload(),
    now: () => Date.now(),
  };

  private readonly policy = new SwUpdatePolicy(this.serviceWorker, this.page);

  start(): void {
    this.policy.start();
  }

  /** The one place the page is thrown away, a method so tests can assert it. */
  reload(): void {
    document.location.reload();
  }
}
