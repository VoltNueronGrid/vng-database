import { create } from "zustand";
import type React from "react";

export interface ContextMenuItem {
  id: string;
  label?: string;
  icon?: string;
  /** Render as a separator — other fields ignored. */
  separator?: boolean;
  danger?: boolean;
  disabled?: boolean;
  shortcut?: string;
  /** Nested submenu items. */
  submenu?: ContextMenuItem[];
  onSelect?: () => void;
}

export interface OpenContextMenu {
  x: number;
  y: number;
  items: ContextMenuItem[];
  /** Optional title shown at the top of the menu. */
  title?: string;
}

interface ContextMenuState {
  open: OpenContextMenu | null;
  show(menu: OpenContextMenu): void;
  hide(): void;
}

export const useContextMenuStore = create<ContextMenuState>()((set) => ({
  open: null,
  show(menu) {
    set({ open: menu });
  },
  hide() {
    set({ open: null });
  },
}));

/**
 * Helper: build an onContextMenu handler that opens the given menu.
 * Usage:
 *   <div onContextMenu={openMenuFor(() => buildTableMenu(...))}>
 */
export function openMenuFor(
  build: () => { items: ContextMenuItem[]; title?: string }
) {
  return (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const { items, title } = build();
    useContextMenuStore.getState().show({
      x: e.clientX,
      y: e.clientY,
      items,
      title,
    });
  };
}
