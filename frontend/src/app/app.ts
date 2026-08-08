import { Component, inject, signal } from '@angular/core';
import { MatButtonModule } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatMenuModule } from '@angular/material/menu';
import { MatProgressBarModule } from '@angular/material/progress-bar';
import { MatToolbarModule } from '@angular/material/toolbar';
import { Router, RouterLink, RouterOutlet } from '@angular/router';

import { AuthStore } from './auth';
import { BUILD_INFO } from './build-info';
import { Me } from './models';
import { TasksApi } from './tasks-api';

@Component({
  selector: 'app-root',
  templateUrl: './app.html',
  styleUrl: './app.scss',
  imports: [
    RouterOutlet,
    RouterLink,
    MatToolbarModule,
    MatButtonModule,
    MatIconModule,
    MatMenuModule,
    MatProgressBarModule,
  ],
})
export class App {
  /** Which build this page is — stamped into the bundle, so a cached page shows
   *  its own age rather than the server's. See scripts/stamp-version.mjs. */
  protected readonly build = BUILD_INFO;
  protected readonly builtAt = new Date(BUILD_INFO.builtAt).toLocaleString();

  private api = inject(TasksApi);
  private router = inject(Router);
  readonly auth = inject(AuthStore);

  readonly me = signal<Me | null>(null);
  readonly loading = signal(true);

  constructor() {
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
