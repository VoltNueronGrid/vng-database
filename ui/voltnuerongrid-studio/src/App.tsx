import { useUiStore } from "@/store/ui";
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
import { Toast } from "@/components/Toast/Toast";
import { SettingsPanel } from "@/components/Settings/SettingsPanel";

export function App() {
  const screen = useUiStore((s) => s.screen);
  const connectionPanelOpen = useUiStore((s) => s.connectionPanelOpen);
  const rightPanelOpen = useUiStore((s) => s.rightPanelOpen);
  const settingsPanelOpen = useUiStore((s) => s.settingsPanelOpen);

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
              <Sidebar />
            </ErrorBoundary>

            {screen === "main" && (
              <ErrorBoundary label="Workspace">
                <Workspace />
              </ErrorBoundary>
            )}

            {screen === "main" && rightPanelOpen && (
              <ErrorBoundary label="RightPanel">
                <RightPanel />
              </ErrorBoundary>
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
