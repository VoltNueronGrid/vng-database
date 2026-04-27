import { create } from "zustand";

export interface Toast {
  id: string;
  message: string;
  kind: "info" | "success" | "error";
}

interface ToastState {
  toasts: Toast[];
  show(message: string, kind?: Toast["kind"]): void;
  dismiss(id: string): void;
}

export const useToastStore = create<ToastState>()((set) => ({
  toasts: [],

  show(message, kind = "info") {
    const id = `toast-${Date.now()}-${Math.random()}`;
    set((s) => ({ toasts: [...s.toasts, { id, message, kind }] }));
    setTimeout(() => {
      set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
    }, 2400);
  },

  dismiss(id) {
    set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
  },
}));
