import * as vscode from "vscode";
import { QueryHistoryEntry } from "../models";
import { QueryExecutionService } from "../services";
import { buildQueryHistoryItems, describeQueryHistoryEntry, QueryHistoryListItem } from "./QueryHistoryTree";

export interface QueryHistoryTreeItem extends QueryHistoryListItem {
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
    const presentation = describeQueryHistoryEntry(entry);
    treeItem.id = entry.id;
    treeItem.contextValue = "historyEntry";
    treeItem.iconPath = new vscode.ThemeIcon(presentation.iconId);
    treeItem.description = presentation.description;
    treeItem.tooltip = presentation.tooltip;
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
    return buildQueryHistoryItems(entries);
  }
}

export function createQueryHistoryProvider(queryExecutionService: QueryExecutionService): QueryHistoryProvider {
  return new QueryHistoryProvider(queryExecutionService);
}