"use strict";
/**
 * ConnectionManager: manage multiple database connections with secure storage
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.ConnectionManager = void 0;
exports.createConnectionManager = createConnectionManager;
class ConnectionManager {
    constructor(context) {
        this.connections = new Map();
        this.activeConnectionId = null;
        this.context = context;
    }
    /**
     * Initialize: load connections from storage
     */
    async initialize() {
        try {
            const stored = this.context.globalState.get("vng.connections", []);
            for (const conn of stored) {
                const settings = {
                    ...conn.settings,
                };
                const adminKey = await this.context.secrets.get(`vng.adminKey.${conn.id}`);
                if (adminKey)
                    settings.adminKey = adminKey;
                this.connections.set(conn.id, {
                    id: conn.id,
                    settings: settings,
                    isActive: false,
                    isConnected: false,
                    diagnostic: { state: "unverified" },
                });
            }
            // Restore active connection
            const activeId = this.context.globalState.get("vng.activeConnection");
            if (activeId && this.connections.has(activeId)) {
                this.activeConnectionId = activeId;
                this.connections.get(activeId).isActive = true;
            }
        }
        catch (error) {
            console.error("Failed to initialize ConnectionManager:", error);
        }
    }
    /**
     * Add a new connection
     */
    async addConnection(settings) {
        const id = settings.id || `conn-${Date.now()}`;
        settings.id = id;
        // Keep non-secret connection settings in globalState; secret key stays in SecretStorage.
        if (settings.adminKey) {
            await this.context.secrets.store(`vng.adminKey.${id}`, settings.adminKey);
        }
        const connection = {
            id,
            settings: { ...settings },
            isActive: false,
            isConnected: false,
            diagnostic: { state: "unverified" },
        };
        this.connections.set(id, connection);
        await this.persist();
        return connection;
    }
    /**
     * Update an existing connection
     */
    async updateConnection(id, settings) {
        const conn = this.connections.get(id);
        if (!conn)
            return null;
        conn.settings = { ...conn.settings, ...settings };
        // Update SecretStorage if admin key changed
        if (settings.adminKey) {
            await this.context.secrets.store(`vng.adminKey.${id}`, settings.adminKey);
        }
        await this.persist();
        return conn;
    }
    /**
     * Delete a connection
     */
    async deleteConnection(id) {
        if (!this.connections.has(id))
            return false;
        this.connections.delete(id);
        // Clean up SecretStorage
        await this.context.secrets.delete(`vng.adminKey.${id}`);
        // If it was active, clear active
        if (this.activeConnectionId === id) {
            this.activeConnectionId = null;
        }
        await this.persist();
        return true;
    }
    /**
     * Get a connection by ID
     */
    getConnection(id) {
        return this.connections.get(id) || null;
    }
    /**
     * Get active connection
     */
    getActiveConnection() {
        if (!this.activeConnectionId)
            return null;
        return this.connections.get(this.activeConnectionId) || null;
    }
    /**
     * Set active connection
     */
    async setActiveConnection(id) {
        if (!this.connections.has(id))
            return null;
        // Clear previous active
        if (this.activeConnectionId) {
            const prev = this.connections.get(this.activeConnectionId);
            if (prev)
                prev.isActive = false;
        }
        // Set new active
        this.activeConnectionId = id;
        const conn = this.connections.get(id);
        conn.isActive = true;
        await this.persist();
        return conn;
    }
    /**
     * Clear the active connection without deleting any saved profiles.
     */
    async clearActiveConnection() {
        if (this.activeConnectionId) {
            const prev = this.connections.get(this.activeConnectionId);
            if (prev) {
                prev.isActive = false;
            }
        }
        this.activeConnectionId = null;
        await this.persist();
    }
    /**
     * Get all connections
     */
    listConnections() {
        return Array.from(this.connections.values());
    }
    /**
     * Search connections by name
     */
    searchConnections(query) {
        const lowerQuery = query.toLowerCase();
        return Array.from(this.connections.values()).filter((conn) => (conn.settings?.name ?? "").toLowerCase().includes(lowerQuery) ||
            (conn.settings?.host ?? "").toLowerCase().includes(lowerQuery));
    }
    /**
     * Update connection status and derive health state.
     *
     * - true  → state becomes "verified"
     * - false → "degraded" if was previously "verified", otherwise "error"
     */
    setConnectionStatus(id, isConnected, message) {
        const conn = this.connections.get(id);
        if (!conn) {
            return;
        }
        conn.isConnected = isConnected;
        const now = Date.now();
        if (isConnected) {
            conn.diagnostic = {
                state: "verified",
                lastChecked: now,
                message: message ?? `HTTP 200`,
            };
        }
        else {
            const wasVerified = conn.diagnostic.state === "verified";
            conn.diagnostic = {
                state: wasVerified ? "degraded" : "error",
                lastChecked: now,
                message: message,
            };
        }
    }
    /**
     * Directly set health state and diagnostic message for a connection.
     */
    setConnectionDiagnostic(id, state, message) {
        const conn = this.connections.get(id);
        if (!conn) {
            return;
        }
        conn.diagnostic = {
            state,
            lastChecked: Date.now(),
            message,
        };
    }
    /**
     * Clear all connections
     */
    async clearAll() {
        // Clear SecretStorage
        for (const id of this.connections.keys()) {
            await this.context.secrets.delete(`vng.adminKey.${id}`);
        }
        this.connections.clear();
        this.activeConnectionId = null;
        await this.persist();
    }
    /**
     * Persist connections to storage
     */
    async persist() {
        const toStore = Array.from(this.connections.values()).map((conn) => ({
            id: conn.id,
            settings: {
                ...conn.settings,
                adminKey: undefined, // Don't store admin key in globalState
            },
        }));
        await this.context.globalState.update("vng.connections", toStore);
        await this.context.globalState.update("vng.activeConnection", this.activeConnectionId);
    }
}
exports.ConnectionManager = ConnectionManager;
function createConnectionManager(context) {
    return new ConnectionManager(context);
}
//# sourceMappingURL=ConnectionManager.js.map