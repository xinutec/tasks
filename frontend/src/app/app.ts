import { Component, inject, signal } from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatMenuModule } from '@angular/material/menu';
import { MatProgressBarModule } from '@angular/material/progress-bar';
import { NavigationEnd, Router, RouterLink, RouterOutlet } from '@angular/router';
import { Scaffold } from '@xinutec/ui-scaffold';
import { filter, map } from 'rxjs';

import { AuthStore } from './auth';
import { BUILD_INFO } from './build-info';
import { Me } from './models';
import { TasksApi } from './tasks-api';
import { SwUpdates } from './sw-updates';
import { Telemetry } from './telemetry';

@Component({
  selector: 'app-root',
  templateUrl: './app.html',
  styleUrl: './app.scss',
  imports: [
    RouterOutlet,
    RouterLink,
    Scaffold,
    MatButtonModule,
    MatIconModule,
    MatMenuModule,
    MatProgressBarModule,
  ],
})
export class App {
  /**
   * This page's build, stamped in, so a cached page shows its own age (scripts/stamp-version.mjs).
   */
  protected readonly build = BUILD_INFO;
  protected readonly builtAt = new Date(BUILD_INFO.builtAt).toLocaleString();

  private api = inject(TasksApi);
  private router = inject(Router);
  readonly auth = inject(AuthStore);
  private telemetry = inject(Telemetry);
  private readonly swUpdates = inject(SwUpdates);

  readonly me = signal<Me | null>(null);
  /** Whether this is the filing screen, where `+` is hidden: it would only reopen itself. */
  readonly filing = toSignal(
    this.router.events.pipe(
      filter((event) => event instanceof NavigationEnd),
      map((event) => event.urlAfterRedirects.startsWith('/new')),
    ),
    { initialValue: false },
  );
  readonly loading = signal(true);

  constructor() {
    // Wired once in the shell (router events, one capture-phase click), so no view needs to know.
    this.telemetry.init();
    this.swUpdates.start();
    this.api.me().subscribe({
      next: (me) => {
        this.me.set(me);
        this.loading.set(false);
      },
      // The interceptor already raised the sign-in wall on 401.
      error: () => this.loading.set(false),
    });
  }

  /** Post-login return target: wherever the user was heading.
   *
   *  dev-lint: allow-template-method — `computed()` would be wrong, not merely
   *  unnecessary: `router.url` is a plain getter and not a signal, so a computed
   *  would cache the first route the toolbar ever saw. */
  loginHref(): string {
    return `/login?return_to=${encodeURIComponent(this.router.url)}`;
  }

  signOut(): void {
    this.api.logout().subscribe(() => (window.location.href = '/'));
  }
}
