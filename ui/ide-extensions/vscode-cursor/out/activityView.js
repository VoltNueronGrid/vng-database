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
exports.VngActionsProvider = void 0;
const vscode = __importStar(require("vscode"));
class VngActionItem extends vscode.TreeItem {
    constructor(label, commandId, description) {
        super(label, vscode.TreeItemCollapsibleState.None);
        this.description = description;
        this.command = {
            command: commandId,
            title: label,
        };
    }
}
class VngActionsProvider {
    constructor() {
        this.items = [
            new VngActionItem("Connection Wizard", "vng.connectWizard", "Configure runtime target and auth mode"),
            new VngActionItem("Test Connection", "vng.testConnection", "Run health, SQL, and schema checks"),
            new VngActionItem("Run Query", "vng.runQuery", "Execute SQL statement"),
            new VngActionItem("Cancel Active Query", "vng.cancelActiveQuery", "Cancel running SQL execution"),
            new VngActionItem("Show Query Results", "vng.showQueryResults", "Open paginated query result grid"),
            new VngActionItem("Search Query History", "vng.searchQueryHistory", "Find and re-run recent SQL"),
            new VngActionItem("Analyze Query", "vng.analyzeQuery", "Inspect SQL behavior and diagnostics"),
            new VngActionItem("Show Schema Registry", "vng.showSchemaRegistry", "List available schema metadata"),
        ];
    }
    getTreeItem(element) {
        return element;
    }
    getChildren() {
        return Promise.resolve(this.items);
    }
}
exports.VngActionsProvider = VngActionsProvider;
//# sourceMappingURL=activityView.js.map