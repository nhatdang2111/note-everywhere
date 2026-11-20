import type { ApiError } from "./contract.ts";

export function isApiError(value: unknown): value is ApiError {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as Record<string, unknown>).code === "string" &&
    typeof (value as Record<string, unknown>).message === "string"
  );
}

/** Normalise whatever a rejected command carried into the contract's `ApiError`. */
export function toApiError(error: unknown): ApiError {
  if (isApiError(error)) return { code: error.code, message: error.message };
  if (typeof error === "string") return { code: "unknown", message: error };
  if (error instanceof Error) return { code: "unknown", message: error.message };
  return { code: "unknown", message: "Something went wrong." };
}

/** Build an `ApiError` that behaves like a rejection from the core. */
export function apiError(code: string, message: string): ApiError {
  return { code, message };
}
