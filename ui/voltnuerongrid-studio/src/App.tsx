import { useUiStore } from "@/store/ui";
import { TitleBar } from "@/components/TitleBar/TitleBar";
import { Sidebar } from "@/components/Sidebar/Sidebar";
import { Workspace } from "@/components/Workspace/Workspace";
import { RightPanel } from "@/components/RightPanel/RightPanel";
import { StatusBar } from "@/components/StatusBar/StatusBar";
import { ConnectionPanel } from "@/components/ConnectionPanel/ConnectionPanel";
import { Dashboard } from "@/components/Dashboard/Dashboard";
import { Welcome } from "@/components/Welcome/Welcome";

export function App() {
  const screen = useUiStore((s) => s.screen);
  const connectionPanelOpen = useUiStore((s) => s.connectionPanelOpen);
  const rightPanelOpen = useUiStore((s) => s.rightPanelOpen);

  return (
    <div className="app">
      <TitleBar />

      {screen === "welcome" && <Welcome />}

      {(screen === "main" || screen === "dashboard") && (
        <>
          <div className="main-layout">
            <Sidebar />
            {screen === "main" && (
              <>
                <Workspace />
                {rightPanelOpen && <RightPanel />}
              </>
            )}
            {screen === "dashboard" && <Dashboard />}
          </div>
          <StatusBar />
        </>
      )}

      {connectionPanelOpen && <ConnectionPanel />}
    </div>
  );
}
