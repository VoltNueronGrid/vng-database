import { useConnectionStore } from "@/store/connection";
import { useUiStore } from "@/store/ui";
import { useSchema } from "@/hooks/useSchema";

export function ConnectionList() {
  const connections = useConnectionStore((s) => s.connections);
  const activeId = useConnectionStore((s) => s.activeId);
  const health = useConnectionStore((s) => s.health);
  const setActive = useConnectionStore((s) => s.setActive);
  const openConnectionPanel = useUiStore((s) => s.openConnectionPanel);
  const setScreen = useUiStore((s) => s.setScreen);
  const { refresh } = useSchema();

  function connect(id: string) {
    setActive(id);
    setScreen("main");
    refresh();
  }

  return (
    <div>
      <div className="conn-section-header">
        <span className="label-xs">Connections</span>
        <button
          className="conn-add-btn"
          title="Add Connection"
          onClick={() => openConnectionPanel(null)}
        >
          +
        </button>
      </div>

      {connections.length === 0 && (
        <div style={{ padding: "8px 12px", color: "var(--text-3)", fontSize: 11.5 }}>
          No connections yet.{" "}
          <span
            style={{ color: "var(--brand-cyan)", cursor: "pointer" }}
            onClick={() => openConnectionPanel(null)}
          >
            Add one
          </span>
        </div>
      )}

      {connections.map((c) => {
        const h = health[c.id];
        const dotClass =
          h?.state === "ok" ? "ok" : h?.state === "error" ? "error" : "none";
        const isVng = c.serverType === "voltnuerongrid";
        return (
          <div
            key={c.id}
            className={`conn-item ${c.id === activeId ? "active" : ""}`}
            onClick={() => connect(c.id)}
            onDoubleClick={() => openConnectionPanel(c.id)}
            title={`${c.host}:${c.port} — double-click to edit`}
          >
            <span className={`conn-dot ${dotClass}`} />
            <span className="conn-item-name">{c.name}</span>
            <span className={`conn-type-badge ${isVng ? "" : "pg"}`}>
              {isVng ? "VNG" : c.serverType.toUpperCase().slice(0, 2)}
            </span>
          </div>
        );
      })}
    </div>
  );
}
