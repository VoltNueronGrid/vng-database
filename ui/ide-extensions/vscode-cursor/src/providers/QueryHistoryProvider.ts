import * as vscode from "vscode";
import { QueryHistoryEntry } from "../models";
import { QueryExecutionService } from "../services";

export interface QueryHistoryTreeItem {
  type: "entry" | "empty";
  label: string;
  entry?: QueryHistoryEntry;
}

export class QueryHistoryProvider implements vscode.TreeDataProvider<QueryHistoryTreeItem> {
  private readonly _onDidChangeTreeData = new vscode.EventEmitter<QueryHistoryTreeItem | undefined | void>();
  readonly onDidChangeTreeData = this._onDidChangeTreeData.event;
  private activeConnectionId: string | undefined;

  constructor(private readonly queryExecutionService: QueryExecutionService) {}

  setActiveConnection(connectionId?: string): void {
    this.activeConnectionId = connectionId;
    this.refresh();
  }

  refresh(): void {
    this._onDidChangeTreeData.fire();
  }

  getTreeItem(element: QueryHistoryTreeItem): vscode.TreeItem {
    const treeItem = new vscode.TreeItem(element.label, vscode.TreeItemCollapsibleState.None);

    if (element.type === "empty") {
      treeItem.contextValue = "historyEmpty";
      treeItem.iconPath = new vscode.ThemeIcon("history");
      treeItem.description = "Run queries to populate history";
      return treeItem;
    }

    const entry = element.entry!;
    treeItem.id = entry.id;
    treeItem.contextValue = "historyEntry";
    treeItem.iconPath = new vscode.ThemeIcon(entry.status === "success" ? "pass-filled" : "error");
    treeItem.description = `${entry.status} • ${entry.executionTime ?? 0} ms • ${new Date(entry.timestamp).toLocaleTimeString()}`;
    treeItem.tooltip = `${entry.query}\n\nStatus: ${entry.status}\nExecution: ${entry.executionTime ?? 0} ms\nTimestamp: ${new Date(
      entry.timestamp
    ).toLocaleString()}`;
    treeItem.command = {
      command: "vng.reRunHistoryQuery",
      title: "Re-run Query",
      arguments: [entry.id],
    };
    return treeItem;
  }

  async getChildren(element?: QueryHistoryTreeItem): Promise<QueryHistoryTreeItem[]> {
    if (element) {
      return [];
    }

    const entries = this.queryExecutionService.getHistory(this.activeConnectionId).slice(0, 50);
    if (entries.length === 0) {
      return [{ type: "empty", label: "No query history yet" }];
    }

    return entries.map((entry) => ({
      type: "entry",
      label: summarizeQuery(entry.query),
      entry,
    }));
  }
}

function summarizeQuery(query: string): string {
  const normalized = query.replace(/\s+/g, " ").trim();
  if (normalized.length <= 70) {
    return normalized;
  }
  return `${normalized.slice(0, 67)}...`;
}

export function createQueryHistoryProvider(queryExecutionService: QueryExecutionService): QueryHistoryProvider {
  return new QueryHistoryProvider(queryExecutionService);
}