import { useEffect, useRef, useState } from "react";
import { useConnectionStore } from "@/store/connection";
import { useUiStore } from "@/store/ui";
import { useThemeStore, type ThemeMode } from "@/store/theme";
import { tauriWindow } from "@/api/tauri";
import { useSchema } from "@/hooks/useSchema";

const THEME_LABELS: Record<ThemeMode, { icon: string; label: string }> = {
  light:  { icon: "☀", label: "Light" },
  dark:   { icon: "☾", label: "Dark" },
  system: { icon: "◐", label: "System" },
};

export function TitleBar() {
  const openConnectionPanel = useUiStore((s) => s.openConnectionPanel);
  const setScreen = useUiStore((s) => s.setScreen);
  const activeId = useConnectionStore((s) => s.activeId);
  const setActive = useConnectionStore((s) => s.setActive);
  const setSchema = useConnectionStore((s) => s.setSchema);
  const connections = useConnectionStore((s) => s.connections);
  const health = useConnectionStore((s) => s.health);
  const themeMode = useThemeStore((s) => s.mode);
  const setThemeMode = useThemeStore((s) => s.setMode);
  const { refresh } = useSchema();

  const [themeMenuOpen, setThemeMenuOpen] = useState(false);
  const themeAnchorRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!themeMenuOpen) return;
    const onDown = (e: MouseEvent) => {
      if (themeAnchorRef.current && !themeAnchorRef.current.contains(e.target as Node)) {
        setThemeMenuOpen(false);
      }
    };
    window.addEventListener("mousedown", onDown);
    return () => window.removeEventListener("mousedown", onDown);
  }, [themeMenuOpen]);

  const active = connections.find((c) => c.id === activeId);
  const healthState = activeId ? health[activeId]?.state ?? "unverified" : null;
  const dotClass =
    healthState === "ok" ? "ok" : healthState === "error" ? "error" : "none";
  const badgeLabel = active
    ? `${active.name} · ${active.host}:${active.port}`
    : "No connection";

  const themeInfo = THEME_LABELS[themeMode];

  function disconnect() {
    setActive(null);
    setSchema(null);
  }

  return (
    <div className="titlebar">
      <div className="titlebar-traffic">
        <button className="traffic traffic-close" onClick={() => tauriWindow.close().catch(() => {})} aria-label="Close" />
        <button className="traffic traffic-min" onClick={() => tauriWindow.minimize().catch(() => {})} aria-label="Minimize" />
        <button className="traffic traffic-max" onClick={() => tauriWindow.toggleMaximize().catch(() => {})} aria-label="Maximize" />
      </div>

      <div className="titlebar-logo">
        <div className="logo-icon">V</div>
        <span className="titlebar-name">
          <span>Volt</span>NueronGrid Studio
        </span>
      </div>

      <div className="titlebar-spacer" />

      <button
        className="titlebar-conn-badge"
        onClick={() => openConnectionPanel(activeId)}
        style={{ WebkitAppRegion: "no-drag" } as React.CSSProperties}
        title="Click to manage connection"
      >
        <span className={`conn-badge-dot ${dotClass}`} />
        <span>{badgeLabel}</span>
        <span style={{ color: "var(--text-3)", fontSize: 10 }}>▾</span>
      </button>

      <div className="titlebar-spacer" />

      <div className="titlebar-actions">
        {active && (
          <button
            className="titlebar-btn"
            title="Refresh schema"
            onClick={() => refresh()}
          >↻</button>
        )}
        {active && (
          <button
            className="titlebar-btn"
            title="Disconnect"
            onClick={disconnect}
          >⏻</button>
        )}
        <button
          className="titlebar-btn"
          title="Dashboard"
          onClick={() => setScreen("dashboard")}
        >📊</button>
        <button
          className="titlebar-btn"
          title="New Connection"
          onClick={() => openConnectionPanel(null)}
        >＋</button>

        <div className="theme-menu-anchor" ref={themeAnchorRef}>
          <button
            className={`titlebar-btn${themeMenuOpen ? " active" : ""}`}
            title={`Theme: ${themeInfo.label}`}
            onClick={() => setThemeMenuOpen((o) => !o)}
          >{themeInfo.icon}</button>
          {themeMenuOpen && (
            <div className="theme-menu">
              {(["light", "dark", "system"] as ThemeMode[]).map((m) => (
                <button
                  key={m}
                  className={themeMode === m ? "active" : ""}
                  onClick={() => { setThemeMode(m); setThemeMenuOpen(false); }}
                >
                  <span>{THEME_LABELS[m].icon}</span>
                  <span>{THEME_LABELS[m].label}</span>
                  <span className="check">✓</span>
                </button>
              ))}
            </div>
          )}
        </div>

        <button className="titlebar-btn" title="Settings">⚙</button>
      </div>
    </div>
  );
}
