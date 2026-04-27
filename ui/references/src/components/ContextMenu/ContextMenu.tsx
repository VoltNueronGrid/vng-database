import { useEffect, useRef, useState } from "react";
import { useContextMenuStore, type ContextMenuItem } from "@/store/contextMenu";

const MENU_W = 240;
const MENU_PAD = 6;

export function ContextMenu() {
  const open = useContextMenuStore((s) => s.open);
  const hide = useContextMenuStore((s) => s.hide);
  const ref = useRef<HTMLDivElement>(null);
  const [openSub, setOpenSub] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setOpenSub(null);
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) hide();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") hide();
    };
    const onScroll = () => hide();
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    window.addEventListener("wheel", onScroll, { passive: true });
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("wheel", onScroll);
    };
  }, [open, hide]);

  if (!open) return null;

  // Clamp to viewport.
  const x = Math.min(open.x, window.innerWidth - MENU_W - MENU_PAD);
  const y = Math.min(open.y, window.innerHeight - 100);

  function handleSelect(item: ContextMenuItem) {
    if (item.disabled || item.separator || item.submenu) return;
    item.onSelect?.();
    hide();
  }

  return (
    <div
      ref={ref}
      className="ctx-menu"
      style={{ left: x, top: y }}
      role="menu"
    >
      {open.title && <div className="ctx-menu-title">{open.title}</div>}
      {open.items.map((item, i) =>
        item.separator ? (
          <div key={`sep-${i}`} className="ctx-menu-sep" />
        ) : (
          <MenuRow
            key={item.id}
            item={item}
            isSubOpen={openSub === item.id}
            onHover={() => setOpenSub(item.submenu ? item.id : null)}
            onSelect={() => handleSelect(item)}
          />
        )
      )}
    </div>
  );
}

function MenuRow({
  item,
  isSubOpen,
  onHover,
  onSelect,
}: {
  item: ContextMenuItem;
  isSubOpen: boolean;
  onHover: () => void;
  onSelect: () => void;
}) {
  const rowRef = useRef<HTMLDivElement>(null);
  const hide = useContextMenuStore((s) => s.hide);

  return (
    <div
      ref={rowRef}
      className={`ctx-menu-item${item.disabled ? " disabled" : ""}${item.danger ? " danger" : ""}`}
      onMouseEnter={onHover}
      onClick={onSelect}
      role="menuitem"
    >
      <span className="ctx-menu-icon">{item.icon ?? ""}</span>
      <span className="ctx-menu-label">{item.label}</span>
      {item.shortcut && <span className="ctx-menu-shortcut">{item.shortcut}</span>}
      {item.submenu && <span className="ctx-menu-arrow">▸</span>}
      {item.submenu && isSubOpen && rowRef.current && (
        <div
          className="ctx-menu submenu"
          style={{
            left: rowRef.current.getBoundingClientRect().width - 4,
            top: -4,
          }}
        >
          {item.submenu.map((sub, i) =>
            sub.separator ? (
              <div key={`sep-${i}`} className="ctx-menu-sep" />
            ) : (
              <div
                key={sub.id}
                className={`ctx-menu-item${sub.disabled ? " disabled" : ""}${sub.danger ? " danger" : ""}`}
                onClick={(e) => {
                  e.stopPropagation();
                  if (sub.disabled) return;
                  sub.onSelect?.();
                  hide();
                }}
              >
                <span className="ctx-menu-icon">{sub.icon ?? ""}</span>
                <span className="ctx-menu-label">{sub.label}</span>
                {sub.shortcut && (
                  <span className="ctx-menu-shortcut">{sub.shortcut}</span>
                )}
              </div>
            )
          )}
        </div>
      )}
    </div>
  );
}
