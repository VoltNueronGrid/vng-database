"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.QueryHistoryProvider = void 0;
exports.createQueryHistoryProvider = createQueryHistoryProvider;
const vscode = __importStar(require("vscode"));
class QueryHistoryProvider {
    constructor(queryExecutionService) {
        this.queryExecutionService = queryExecutionService;
        this._onDidChangeTreeData = new vscode.EventEmitter();
        this.onDidChangeTreeData = this._onDidChangeTreeData.event;
    }
    setActiveConnection(connectionId) {
        this.activeConnectionId = connectionId;
        this.refresh();
    }
    refresh() {
        this._onDidChangeTreeData.fire();
    }
    getTreeItem(element) {
        const treeItem = new vscode.TreeItem(element.label, vscode.TreeItemCollapsibleState.None);
        if (element.type === "empty") {
            treeItem.contextValue = "historyEmpty";
            treeItem.iconPath = new vscode.ThemeIcon("history");
            treeItem.description = "Run queries to populate history";
            return treeItem;
        }
        const entry = element.entry;
        treeItem.id = entry.id;
        treeItem.contextValue = "historyEntry";
        treeItem.iconPath = new vscode.ThemeIcon(entry.status === "success" ? "pass-filled" : "error");
        treeItem.description = `${entry.status} • ${entry.executionTime ?? 0} ms • ${new Date(entry.timestamp).toLocaleTimeString()}`;
        treeItem.tooltip = `${entry.query}\n\nStatus: ${entry.status}\nExecution: ${entry.executionTime ?? 0} ms\nTimestamp: ${new Date(entry.timestamp).toLocaleString()}`;
        treeItem.command = {
            command: "vng.reRunHistoryQuery",
            title: "Re-run Query",
            arguments: [entry.id],
        };
        return treeItem;
    }
    async getChildren(element) {
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
exports.QueryHistoryProvider = QueryHistoryProvider;
function summarizeQuery(query) {
    const normalized = query.replace(/\s+/g, " ").trim();
    if (normalized.length <= 70) {
        return normalized;
    }
    return `${normalized.slice(0, 67)}...`;
}
function createQueryHistoryProvider(queryExecutionService) {
    return new QueryHistoryProvider(queryExecutionService);
}
//# sourceMappingURL=QueryHistoryProvider.js.map