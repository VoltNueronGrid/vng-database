import { useConnectionStore } from "@/store/connection";
import { useUiStore } from "@/store/ui";
import { tauriWindow } from "@/api/tauri";

export function TitleBar() {
  const openConnectionPanel = useUiStore((s) => s.openConnectionPanel);
  const setScreen = useUiStore((s) => s.setScreen);
  const activeId = useConnectionStore((s) => s.activeId);
  const connections = useConnectionStore((s) => s.connections);
  const health = useConnectionStore((s) => s.health);

  const active = connections.find((c) => c.id === activeId);
  const healthState = activeId ? health[activeId]?.state ?? "unverified" : null;

  const dotClass =
    healthState === "ok" ? "ok" : healthState === "error" ? "error" : "none";

  const badgeLabel = active
    ? `${active.name} · ${active.host}:${active.port}`
    : "No connection";

  return (
    <div className="titlebar">
      {/* macOS traffic-lights placeholder (native controls render here when decorations:false) */}
      <div className="titlebar-traffic">
        <button
          className="traffic traffic-close"
          onClick={() => tauriWindow.close().catch(() => {})}
          aria-label="Close"
        />
        <button
          className="traffic traffic-min"
          onClick={() => tauriWindow.minimize().catch(() => {})}
          aria-label="Minimize"
        />
        <button
          className="traffic traffic-max"
          onClick={() => tauriWindow.toggleMaximize().catch(() => {})}
          aria-label="Maximize"
        />
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
      >
        <span className={`conn-badge-dot ${dotClass}`} />
        <span>{badgeLabel}</span>
        <span style={{ color: "var(--text-3)", fontSize: 10 }}>▾</span>
      </button>

      <div className="titlebar-spacer" />

      <div className="titlebar-actions">
        <button
          className="titlebar-btn"
          title="Dashboard"
          onClick={() => setScreen("dashboard")}
        >
          📊
        </button>
        <button
          className="titlebar-btn"
          title="New Connection"
          onClick={() => openConnectionPanel(null)}
        >
          ＋
        </button>
        <button className="titlebar-btn" title="Settings">⚙</button>
      </div>
    </div>
  );
}
