import { createSignal } from "solid-js";
import type { ApiError } from "../api/contract.ts";
import { apiError, toApiError } from "../api/errors.ts";

/** Throw this from a submit handler to show a validation message without calling the core. */
export const invalid = (message: string): ApiError => apiError("invalid", message);

export interface Submit {
  busy: () => boolean;
  error: () => ApiError | null;
  clearError(): void;
  /** Form `onSubmit` handler: prevents the page reload, runs `run`, and keeps the failure for display. */
  submit(event?: Event): Promise<void>;
}

/** Busy flag and error for one form action. A cancelled action is not an error. */
export function createSubmit(run: () => Promise<void>): Submit {
  const [busy, setBusy] = createSignal(false);
  const [error, setError] = createSignal<ApiError | null>(null);
  return {
    busy,
    error,
    clearError: () => setError(null),
    async submit(event) {
      event?.preventDefault();
      if (busy()) return;
      setBusy(true);
      setError(null);
      try {
        await run();
      } catch (failure) {
        const failed = toApiError(failure);
        if (failed.code !== "cancelled") setError(failed);
      } finally {
        setBusy(false);
      }
    },
  };
}
