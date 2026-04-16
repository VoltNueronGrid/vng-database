"use strict";
/**
 * Providers module - all tree view providers
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.summarizeQuery = exports.describeQueryHistoryEntry = exports.buildQueryHistoryItems = exports.createQueryHistoryProvider = exports.QueryHistoryProvider = exports.createDatabaseExplorerProvider = exports.DatabaseExplorerProvider = void 0;
var DatabaseExplorerProvider_1 = require("./DatabaseExplorerProvider");
Object.defineProperty(exports, "DatabaseExplorerProvider", { enumerable: true, get: function () { return DatabaseExplorerProvider_1.DatabaseExplorerProvider; } });
Object.defineProperty(exports, "createDatabaseExplorerProvider", { enumerable: true, get: function () { return DatabaseExplorerProvider_1.createDatabaseExplorerProvider; } });
var QueryHistoryProvider_1 = require("./QueryHistoryProvider");
Object.defineProperty(exports, "QueryHistoryProvider", { enumerable: true, get: function () { return QueryHistoryProvider_1.QueryHistoryProvider; } });
Object.defineProperty(exports, "createQueryHistoryProvider", { enumerable: true, get: function () { return QueryHistoryProvider_1.createQueryHistoryProvider; } });
var QueryHistoryTree_1 = require("./QueryHistoryTree");
Object.defineProperty(exports, "buildQueryHistoryItems", { enumerable: true, get: function () { return QueryHistoryTree_1.buildQueryHistoryItems; } });
Object.defineProperty(exports, "describeQueryHistoryEntry", { enumerable: true, get: function () { return QueryHistoryTree_1.describeQueryHistoryEntry; } });
Object.defineProperty(exports, "summarizeQuery", { enumerable: true, get: function () { return QueryHistoryTree_1.summarizeQuery; } });
//# sourceMappingURL=index.js.map