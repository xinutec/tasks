import { HttpErrorResponse } from '@angular/common/http';

/**
 * What went wrong, in words fit for the screen, at one boundary taking
 * `unknown` (or a phone shows `[object Object]`).
 */
export function reason(err: unknown): string {
  if (err instanceof HttpErrorResponse) {
    // A refusal's `{"error": …}` sentence beats anything composed here.
    const said: unknown = err.error;
    if (typeof said === 'object' && said !== null && 'error' in said) {
      const inner = (said as { error?: unknown }).error;
      if (typeof inner === 'string' && inner.trim()) return inner.trim();
    }
    if (typeof said === 'string' && said.trim()) return said.trim();
    // Status 0 is no answer at all: on the VPN, usually the tunnel.
    if (err.status === 0) return 'no answer — check the VPN';
    return `the service answered ${err.status}`;
  }
  if (err instanceof Error) return err.message;
  return 'something went wrong';
}
