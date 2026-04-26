import { useEditorStore } from "@/store/editor";
import type { Tab } from "@/store/editor";

function tabIcon(tab: Tab): string {
  if (tab.type === "sql") return "📄";
  if (tab.type === "table") return "📋";
  return "📊";
}

export function TabBar() {
  const tabs = useEditorStore((s) => s.tabs);
  const activeTabId = useEditorStore((s) => s.activeTabId);
  const setActiveTab = useEditorStore((s) => s.setActiveTab);
  const closeTab = useEditorStore((s) => s.closeTab);
  const openSqlTab = useEditorStore((s) => s.openSqlTab);

  return (
    <div className="tabbar">
      {tabs.map((tab) => (
        <div
          key={tab.id}
          className={`tab ${tab.id === activeTabId ? "active" : ""}`}
          onClick={() => setActiveTab(tab.id)}
        >
          <span className="tab-icon">{tabIcon(tab)}</span>
          <span className="tab-label">{tab.title}</span>
          {tab.isDirty && <span className="tab-dirty">●</span>}
          <button
            className="tab-close"
            onClick={(e) => { e.stopPropagation(); closeTab(tab.id); }}
            title="Close tab"
          >
            ✕
          </button>
        </div>
      ))}
      <button
        className="tab-new-btn"
        onClick={() => openSqlTab()}
        title="New query"
      >
        +
      </button>
    </div>
  );
}
