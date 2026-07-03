import { useUiStore } from "@/store/ui";
import { ConnectionList } from "./ConnectionList";
import { DatabasesPanel } from "./DatabasesPanel";
import { UsersPanel } from "./UsersPanel";

export function Sidebar() {
  const sidebarTab = useUiStore((s) => s.sidebarTab);
  const setSidebarTab = useUiStore((s) => s.setSidebarTab);
  const sidebarPinned = useUiStore((s) => s.sidebarPinned);
  const toggleSidebarPinned = useUiStore((s) => s.toggleSidebarPinned);
  const setSidebarPinned = useUiStore((s) => s.setSidebarPinned);

  function openTab(tab: "connections" | "databases" | "users" | "history" | "saved") {
    setSidebarTab(tab);
    if (!sidebarPinned) {
      setSidebarPinned(true);
    }
  }

  const schemaLabel = sidebarPinned ? "Schema" : "🗂";
  const dbsLabel = sidebarPinned ? "DBs" : "🛢";
  const usersLabel = sidebarPinned ? "Users" : "👥";
  const historyLabel = sidebarPinned ? "History" : "🕘";
  const savedLabel = sidebarPinned ? "Saved" : "💾";
  const compactClass = sidebarPinned ? "" : "icon-only";

  return (
    <div className={`sidebar ${sidebarPinned ? "" : "unpinned"}`}>
      <div className="sidebar-activity">
        {sidebarPinned ? (
          <select
            className="sidebar-tab-select"
            value={sidebarTab}
            onChange={(e) => setSidebarTab(e.target.value as typeof sidebarTab)}
            title="Sidebar section"
          >
            <option value="connections">Schema</option>
            <option value="databases">DBs</option>
            <option value="users">Users</option>
            <option value="history">History</option>
            <option value="saved">Saved</option>
          </select>
        ) : (
          <>
            <button
              className={`activity-btn ${compactClass} ${sidebarTab === "connections" ? "active" : ""}`}
              onClick={() => openTab("connections")}
              title="Connections & Schema"
            >
              {schemaLabel}
            </button>
            <button
              className={`activity-btn ${compactClass} ${sidebarTab === "databases" ? "active" : ""}`}
              onClick={() => openTab("databases")}
              title="Databases (create / drop)"
            >
              {dbsLabel}
            </button>
            <button
              className={`activity-btn ${compactClass} ${sidebarTab === "users" ? "active" : ""}`}
              onClick={() => openTab("users")}
              title="Users & Roles"
            >
              {usersLabel}
            </button>
            <button
              className={`activity-btn ${compactClass} ${sidebarTab === "history" ? "active" : ""}`}
              onClick={() => openTab("history")}
              title="Query History"
            >
              {historyLabel}
            </button>
            <button
              className={`activity-btn ${compactClass} ${sidebarTab === "saved" ? "active" : ""}`}
              onClick={() => openTab("saved")}
              title="Saved Queries"
            >
              {savedLabel}
            </button>
          </>
        )}
        <button
          className="activity-btn pin-btn"
          onClick={toggleSidebarPinned}
          title={sidebarPinned ? "Unpin sidebar" : "Pin sidebar"}
        >
          {sidebarPinned ? "⟨" : "⟩"}
        </button>
      </div>

      {sidebarPinned && <div className="sidebar-scroll">
        {sidebarTab === "connections" && (
          <>
            <ConnectionList />
          </>
        )}
        {sidebarTab === "databases" && <DatabasesPanel />}
        {sidebarTab === "users" && <UsersPanel />}
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
      </div>}
    </div>
  );
}
