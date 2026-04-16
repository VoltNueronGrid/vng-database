/**
 * ConnectionManager: manage multiple database connections with secure storage
 */

import * as vscode from "vscode";
import { Connection, ConnectionSettings, StoredConnection } from "../models/Connection";

export class ConnectionManager {
  private connections: Map<string, Connection> = new Map();
  private activeConnectionId: string | null = null;
  private context: vscode.ExtensionContext;

  constructor(context: vscode.ExtensionContext) {
    this.context = context;
  }

  /**
   * Initialize: load connections from storage
   */
  async initialize(): Promise<void> {
    try {
      const stored = this.context.globalState.get<StoredConnection[]>("vng.connections", []);
      for (const conn of stored) {
        const settings: ConnectionSettings = {
          ...(conn.settings as ConnectionSettings),
        };
        const adminKey = await this.context.secrets.get(`vng.adminKey.${conn.id}`);

        if (adminKey) settings.adminKey = adminKey;

        this.connections.set(conn.id, {
          id: conn.id,
          settings: settings as ConnectionSettings,
          isActive: false,
          isConnected: false,
        });
      }

      // Restore active connection
      const activeId = this.context.globalState.get<string>("vng.activeConnection");
      if (activeId && this.connections.has(activeId)) {
        this.activeConnectionId = activeId;
        this.connections.get(activeId)!.isActive = true;
      }
    } catch (error) {
      console.error("Failed to initialize ConnectionManager:", error);
    }
  }

  /**
   * Add a new connection
   */
  async addConnection(settings: ConnectionSettings): Promise<Connection> {
    const id = settings.id || `conn-${Date.now()}`;
    settings.id = id;

    // Keep non-secret connection settings in globalState; secret key stays in SecretStorage.
    if (settings.adminKey) {
      await this.context.secrets.store(`vng.adminKey.${id}`, settings.adminKey);
    }

    const connection: Connection = {
      id,
      settings: { ...settings },
      isActive: false,
      isConnected: false,
    };

    this.connections.set(id, connection);
    await this.persist();

    return connection;
  }

  /**
   * Update an existing connection
   */
  async updateConnection(id: string, settings: Partial<ConnectionSettings>): Promise<Connection | null> {
    const conn = this.connections.get(id);
    if (!conn) return null;

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
  async deleteConnection(id: string): Promise<boolean> {
    if (!this.connections.has(id)) return false;

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
  getConnection(id: string): Connection | null {
    return this.connections.get(id) || null;
  }

  /**
   * Get active connection
   */
  getActiveConnection(): Connection | null {
    if (!this.activeConnectionId) return null;
    return this.connections.get(this.activeConnectionId) || null;
  }

  /**
   * Set active connection
   */
  async setActiveConnection(id: string): Promise<Connection | null> {
    if (!this.connections.has(id)) return null;

    // Clear previous active
    if (this.activeConnectionId) {
      const prev = this.connections.get(this.activeConnectionId);
      if (prev) prev.isActive = false;
    }

    // Set new active
    this.activeConnectionId = id;
    const conn = this.connections.get(id)!;
    conn.isActive = true;

    await this.persist();
    return conn;
  }

  /**
   * Clear the active connection without deleting any saved profiles.
   */
  async clearActiveConnection(): Promise<void> {
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
  listConnections(): Connection[] {
    return Array.from(this.connections.values());
  }

  /**
   * Search connections by name
   */
  searchConnections(query: string): Connection[] {
    const lowerQuery = query.toLowerCase();
    return Array.from(this.connections.values()).filter(
      (conn) =>
        conn.settings.name.toLowerCase().includes(lowerQuery) ||
        conn.settings.host.toLowerCase().includes(lowerQuery)
    );
  }

  /**
   * Update connection status
   */
  setConnectionStatus(id: string, isConnected: boolean): void {
    const conn = this.connections.get(id);
    if (conn) {
      conn.isConnected = isConnected;
    }
  }

  /**
   * Clear all connections
   */
  async clearAll(): Promise<void> {
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
  private async persist(): Promise<void> {
    const toStore: StoredConnection[] = Array.from(this.connections.values()).map((conn) => ({
      id: conn.id,
      settings: {
        ...conn.settings,
        adminKey: undefined, // Don't store admin key in globalState
      } as any,
    }));

    await this.context.globalState.update("vng.connections", toStore);
    await this.context.globalState.update("vng.activeConnection", this.activeConnectionId);
  }
}

export function createConnectionManager(context: vscode.ExtensionContext): ConnectionManager {
  return new ConnectionManager(context);
}
