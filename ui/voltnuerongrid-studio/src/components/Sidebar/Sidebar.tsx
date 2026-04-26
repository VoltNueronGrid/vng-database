import { useUiStore } from "@/store/ui";
import { ConnectionList } from "./ConnectionList";
import { SchemaTree } from "./SchemaTree";

export function Sidebar() {
  const sidebarTab = useUiStore((s) => s.sidebarTab);
  const setSidebarTab = useUiStore((s) => s.setSidebarTab);

  return (
    <div className="sidebar">
      <div className="sidebar-activity">
        <button
          className={`activity-btn ${sidebarTab === "connections" ? "active" : ""}`}
          onClick={() => setSidebarTab("connections")}
        >
          Connections
        </button>
        <button
          className={`activity-btn ${sidebarTab === "history" ? "active" : ""}`}
          onClick={() => setSidebarTab("history")}
        >
          History
        </button>
        <button
          className={`activity-btn ${sidebarTab === "saved" ? "active" : ""}`}
          onClick={() => setSidebarTab("saved")}
        >
          Saved
        </button>
      </div>

      <div className="sidebar-scroll">
        {sidebarTab === "connections" && (
          <>
            <ConnectionList />
            <SchemaTree />
          </>
        )}
        {sidebarTab === "history" && (
          <div style={{ padding: "16px 12px", color: "var(--text-3)", fontSize: 12 }}>
            Query history — coming soon.
          </div>
        )}
        {sidebarTab === "saved" && (
          <div style={{ padding: "16px 12px", color: "var(--text-3)", fontSize: 12 }}>
            Saved queries — coming soon.
          </div>
        )}
      </div>
    </div>
  );
}
