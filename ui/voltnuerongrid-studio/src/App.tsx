import { useEffect, useRef } from "react";
import { useUiStore } from "@/store/ui";
import { useConnectionStore } from "@/store/connection";
import { TitleBar } from "@/components/TitleBar/TitleBar";
import { Sidebar } from "@/components/Sidebar/Sidebar";
import { Workspace } from "@/components/Workspace/Workspace";
import { RightPanel } from "@/components/RightPanel/RightPanel";
import { StatusBar } from "@/components/StatusBar/StatusBar";
import { ConnectionPanel } from "@/components/ConnectionPanel/ConnectionPanel";
import { Dashboard } from "@/components/Dashboard/Dashboard";
import { Welcome } from "@/components/Welcome/Welcome";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { ContextMenu } from "@/components/ContextMenu/ContextMenu";
import { ResourceModal } from "@/components/Modals/ResourceModal";
import { DatabaseChoiceModal } from "@/components/Modals/DatabaseChoiceModal";
import { Toast } from "@/components/Toast/Toast";
import { SettingsPanel } from "@/components/Settings/SettingsPanel";

export function App() {
  const screen = useUiStore((s) => s.screen);
  const sidebarWidth = useUiStore((s) => s.sidebarWidth);
  const setSidebarWidth = useUiStore((s) => s.setSidebarWidth);
  const sidebarPinned = useUiStore((s) => s.sidebarPinned);
  const connectionPanelOpen = useUiStore((s) => s.connectionPanelOpen);
  const rightPanelOpen = useUiStore((s) => s.rightPanelOpen);
  const settingsPanelOpen = useUiStore((s) => s.settingsPanelOpen);

  // R9: Gate workspace and SQL editor on connection lifecycle.
  const lifecycleState = useConnectionStore((s) => s.lifecycleState);
  const lifecycleError = useConnectionStore((s) => s.lifecycleError);
  const connectionActive = lifecycleState === "active";
  const resizingRef = useRef(false);

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!resizingRef.current) return;
      setSidebarWidth(e.clientX);
    };
    const onUp = () => {
      resizingRef.current = false;
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
  }, [setSidebarWidth]);

  function beginSidebarResize() {
    if (!sidebarPinned) return;
    resizingRef.current = true;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }

  return (
    <div className="app">
      <ErrorBoundary label="TitleBar">
        <TitleBar />
      </ErrorBoundary>

      {screen === "welcome" && (
        <ErrorBoundary label="Welcome">
          <Welcome />
        </ErrorBoundary>
      )}

      {(screen === "main" || screen === "dashboard") && (
        <>
          <div className="main-layout">
            <ErrorBoundary label="Sidebar">
              <div
                className={`sidebar-shell ${sidebarPinned ? "" : "unpinned"}`}
                style={{
                  width: sidebarPinned ? sidebarWidth : 56,
                  minWidth: sidebarPinned ? sidebarWidth : 56,
                  maxWidth: sidebarPinned ? sidebarWidth : 56,
                }}
              >
                <Sidebar />
                {sidebarPinned && (
                  <div
                    className="sidebar-resizer"
                    onMouseDown={beginSidebarResize}
                    title="Resize sidebar"
                  />
                )}
              </div>
            </ErrorBoundary>

            {/* R9: Workspace and SQL editor are only rendered when lifecycle is active. */}
            {screen === "main" && connectionActive && (
              <ErrorBoundary label="Workspace">
                <Workspace />
              </ErrorBoundary>
            )}

            {screen === "main" && connectionActive && rightPanelOpen && (
              <ErrorBoundary label="RightPanel">
                <RightPanel />
              </ErrorBoundary>
            )}

            {/* R9: Show a validation-in-progress overlay when connecting. */}
            {screen === "main" && lifecycleState === "validating" && (
              <div
                className="workspace"
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  flex: 1,
                  color: "var(--text-3)",
                  fontSize: 13,
                }}
              >
                Validating connection…
              </div>
            )}

            {/* R9: Show error state when connection validation fails. */}
            {screen === "main" && lifecycleState === "error" && (
              <div
                className="workspace"
                style={{
                  display: "flex",
                  flexDirection: "column",
                  alignItems: "center",
                  justifyContent: "center",
                  flex: 1,
                  gap: 10,
                  color: "var(--red)",
                  fontSize: 13,
                  padding: "0 40px",
                  textAlign: "center",
                }}
              >
                <span>⚠ Connection failed</span>
                {lifecycleError && (
                  <span style={{ color: "var(--text-2)", fontSize: 12 }}>{lifecycleError}</span>
                )}
              </div>
            )}

            {screen === "dashboard" && (
              <ErrorBoundary label="Dashboard">
                <Dashboard />
              </ErrorBoundary>
            )}
          </div>

          <ErrorBoundary label="StatusBar">
            <StatusBar />
          </ErrorBoundary>
        </>
      )}

      {connectionPanelOpen && (
        <ErrorBoundary label="ConnectionPanel">
          <ConnectionPanel />
        </ErrorBoundary>
      )}

      {/* R9: DatabaseChoiceModal shown when target DB is not found on the server. */}
      <ErrorBoundary label="DatabaseChoiceModal">
        <DatabaseChoiceModal />
      </ErrorBoundary>

      <ErrorBoundary label="ResourceModal">
        <ResourceModal />
      </ErrorBoundary>

      <ContextMenu />
      <Toast />

      {settingsPanelOpen && (
        <ErrorBoundary label="SettingsPanel">
          <SettingsPanel />
        </ErrorBoundary>
      )}
    </div>
  );
}
