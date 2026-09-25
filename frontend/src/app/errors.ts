import { HttpErrorResponse } from '@angular/common/http';

/**
 * What went wrong, in words fit to put on screen.
 *
 * One boundary rather than a shape declared at each callsite, or a phone shows
 * `[object Object]`. It takes `unknown` and narrows, which also satisfies
 * `DL-ANGULAR-HTTP-ERROR-CLASSIFIED` by construction.
 */
export function reason(err: unknown): string {
  if (err instanceof HttpErrorResponse) {
    // The service answers a refusal with `{"error": "…"}`, and that sentence is
    // better than anything this function could compose.
    const said: unknown = err.error;
    if (typeof said === 'object' && said !== null && 'error' in said) {
      const inner = (said as { error?: unknown }).error;
      if (typeof inner === 'string' && inner.trim()) return inner.trim();
    }
    if (typeof said === 'string' && said.trim()) return said.trim();
    // Status 0 is not a server saying no — it is no answer at all. On the VPN
    // that is nearly always the tunnel rather than the service.
    if (err.status === 0) return 'no answer — check the VPN';
    return `the service answered ${err.status}`;
  }
  if (err instanceof Error) return err.message;
  return 'something went wrong';
}
