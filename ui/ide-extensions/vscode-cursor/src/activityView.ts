import * as vscode from "vscode";

class VngActionItem extends vscode.TreeItem {
  constructor(label: string, commandId: string, description?: string) {
    super(label, vscode.TreeItemCollapsibleState.None);
    this.description = description;
    this.command = {
      command: commandId,
      title: label,
    };
  }
}

export class VngActionsProvider implements vscode.TreeDataProvider<VngActionItem> {
  private readonly items: VngActionItem[] = [
    new VngActionItem("Connection Wizard", "vng.connectWizard", "Configure runtime target and auth mode"),
    new VngActionItem("Test Connection", "vng.testConnection", "Run health, SQL, and schema checks"),
    new VngActionItem("Run Query", "vng.runQuery", "Execute SQL statement"),
    new VngActionItem("Analyze Query", "vng.analyzeQuery", "Inspect SQL behavior and diagnostics"),
    new VngActionItem("Show Schema Registry", "vng.showSchemaRegistry", "List available schema metadata"),
  ];

  getTreeItem(element: VngActionItem): vscode.TreeItem {
    return element;
  }

  getChildren(): Thenable<VngActionItem[]> {
    return Promise.resolve(this.items);
  }
}
