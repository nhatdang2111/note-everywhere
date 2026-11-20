import { createSignal } from "solid-js";
import { toApiError } from "../api/errors.ts";

export type ToastKind = "info" | "success" | "error";

export interface Toast {
  id: number;
  kind: ToastKind;
  message: string;
}

export interface Toasts {
  items: () => Toast[];
  push(kind: ToastKind, message: string, timeoutMs?: number): number;
  /** Show a failed command's `ApiError.message`. */
  error(error: unknown): void;
  dismiss(id: number): void;
}

export function createToasts(): Toasts {
  const [items, setItems] = createSignal<Toast[]>([]);
  let next = 1;
  const dismiss = (id: number) => setItems((list) => list.filter((toast) => toast.id !== id));
  const push = (kind: ToastKind, message: string, timeoutMs = kind === "error" ? 8000 : 4000): number => {
    const id = next++;
    setItems((list) => [...list.slice(-3), { id, kind, message }]);
    if (timeoutMs > 0) setTimeout(() => dismiss(id), timeoutMs);
    return id;
  };
  return { items, push, dismiss, error: (error) => void push("error", toApiError(error).message) };
}
