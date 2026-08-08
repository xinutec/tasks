import { HttpErrorResponse } from '@angular/common/http';

/**
 * What went wrong, in words fit to put on screen.
 *
 * One boundary rather than a shape declared at each callsite. The pattern, and
 * the `[object Object]` that shipped to a phone without it, are recorded in the
 * agent console's `errors.ts`; this is that function with this API's error
 * envelope. It takes `unknown` and narrows, which is also what keeps it out of
 * `DL-ANGULAR-HTTP-ERROR-CLASSIFIED`'s sights by construction.
 */
export function reason(err: unknown): string {
  if (err instanceof HttpErrorResponse) {
    // The service answers a refusal with `{"error": "…"}`, and that sentence —
    // "a subject is one line and at most 200 characters — this is 412" — is
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
