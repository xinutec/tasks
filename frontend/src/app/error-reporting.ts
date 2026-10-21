/**
 * Uncaught errors and failed requests, into the trace that reaches the server log: a phone's console is unread.
 */
import { ErrorHandler, inject, Injectable, Injector } from '@angular/core';
import { HttpErrorResponse, HttpInterceptorFn } from '@angular/common/http';
import { catchError, throwError } from 'rxjs';

import { Telemetry } from './telemetry';

/** The message of anything thrown, unwrapping rejections; `String()` of an object says nothing. */
export function messageOf(error: unknown): string {
  const inner =
    typeof error === 'object' && error !== null && 'rejection' in error ? error.rejection : error;
  if (inner instanceof Error) return inner.message;
  if (typeof inner === 'string') return inner;
  return 'non-Error thrown';
}

@Injectable()
export class TelemetryErrorHandler implements ErrorHandler {
  // On first use: the trace needs the Router, which can't exist before the ErrorHandler.
  private readonly injector = inject(Injector);

  handleError(error: unknown): void {
    console.error(error);
    try {
      this.injector.get(Telemetry).failure('error', messageOf(error));
    } catch {
      // Reporting must never throw from the one place that catches everything.
    }
  }
}

/** A failed API call. The trace's own POST is skipped: its failure would feed itself. */
export const failedRequestInterceptor: HttpInterceptorFn = (req, next) => {
  const telemetry = inject(Telemetry);
  return next(req).pipe(
    catchError((err: unknown) => {
      if (!req.url.includes('/api/telemetry')) {
        const status = err instanceof HttpErrorResponse ? String(err.status) : '?';
        telemetry.failure('http', `${req.method} ${req.url} -> ${status}`);
      }
      return throwError(() => err);
    }),
  );
};
