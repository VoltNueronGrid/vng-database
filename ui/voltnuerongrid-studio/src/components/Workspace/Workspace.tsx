import { TabBar } from "./TabBar";
import { Toolbar } from "./Toolbar";
import { SqlEditorPane } from "./SqlEditorPane";
import { ResultsPane } from "@/components/ResultsPane/ResultsPane";
import { useEditorStore } from "@/store/editor";

export function Workspace() {
  const getActiveTab = useEditorStore((s) => s.getActiveTab);
  const tab = getActiveTab();

  return (
    <div className="workspace">
      <TabBar />
      {tab?.type === "sql" && (
        <>
          <Toolbar />
          <div className="editor-area">
            <SqlEditorPane />
            <div className="resize-handle" />
            <ResultsPane />
          </div>
        </>
      )}
      {tab?.type === "table" && (
        <>
          <Toolbar />
          <div className="editor-area">
            <SqlEditorPane />
            <div className="resize-handle" />
            <ResultsPane />
          </div>
        </>
      )}
      {tab?.type === "dashboard" && (
        <div
          className="results-empty"
          style={{ flex: 1, color: "var(--text-3)" }}
        >
          <div className="re-icon">📊</div>
          <div>Switch to Dashboard view for cluster monitoring.</div>
        </div>
      )}
      {!tab && (
        <div className="results-empty" style={{ flex: 1 }}>
          <div className="re-icon">📝</div>
          <div className="text-muted">Open a query tab to get started.</div>
        </div>
      )}
    </div>
  );
}
