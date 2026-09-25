import { HttpErrorResponse, HttpInterceptorFn } from '@angular/common/http';
import { Injectable, inject, signal } from '@angular/core';
import { catchError, throwError } from 'rxjs';

/** Session state: whether the sign-in wall is up. */
@Injectable({ providedIn: 'root' })
export class AuthStore {
  readonly needsSignIn = signal(false);
}

/**
 * Flips the sign-in wall on any 401.
 *
 * No share token: this is a working surface for two parties, not a document
 * to publish.
 */
export const authInterceptor: HttpInterceptorFn = (req, next) => {
  const auth = inject(AuthStore);
  return next(req).pipe(
    catchError((err: unknown) => {
      if (err instanceof HttpErrorResponse && err.status === 401) {
        auth.needsSignIn.set(true);
      }
      return throwError(() => err);
    }),
  );
};
