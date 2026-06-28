import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { SchemaRegistry, SchemaDatabase } from "@/api/studio-client";
import { StudioApiClient } from "@/api/studio-client";

export type ConnectionMode = "admin" | "operator" | "tenant";
export type ServerType = "voltnuerongrid" | "postgresql" | "mysql" | "other";
export type RuntimeTarget = "local" | "docker" | "cloud" | "custom";
export type HealthState = "unverified" | "ok" | "degraded" | "error";
export type ConnectionProtocol = "http" | "native";

/**
 * R9: Lifecycle state machine for a connection.
 *
 *  idle              — no active connection attempt
 *  validating        — health + database reachability check in progress
 *  awaiting_db_choice — server is reachable but the target database was not found;
 *                       user must choose an existing DB or create a new one
 *  active            — connection validated and scoped to a known database
 *  error             — connection or authentication failed
 */
export type ConnectionLifecycleState =
  | "idle"
  | "validating"
  | "awaiting_db_choice"
  | "active"
  | "error";

/** Default ports per protocol */
export const PROTOCOL_DEFAULT_PORTS: Record<ConnectionProtocol, number> = {
  http: 8080,
  native: 7542,
};

export interface ConnectionSettings {
  id: string;
  name: string;
  protocol: ConnectionProtocol;
  serverType: ServerType;
  runtimeTarget: RuntimeTarget;
  baseUrl: string;
  host: string;
  port: number;
  database?: string;
  username?: string;
  mode: ConnectionMode;
  adminKey?: string;
  operatorId?: string;
  tenantId?: string;
  userId?: string;
  sslEnabled: boolean;
  createdAt: number;
  lastUsed?: number;
}

export interface ConnectionHealth {
  state: HealthState;
  checkedAt?: number;
  message?: string;
}

export function defaultConnection(
  overrides?: Partial<ConnectionSettings>
): ConnectionSettings {
  const now = Date.now();
  return {
    id: `conn-${now}`,
    name: "New Connection",
    protocol: "http",
    serverType: "voltnuerongrid",
    runtimeTarget: "local",
    baseUrl: "http://127.0.0.1:8080",
    host: "127.0.0.1",
    port: 8080,
    // Default database ensures new connections are isolated from the global namespace.
    // Users can change this to any existing database or leave blank to browse all.
    database: "default",
    mode: "admin",
    sslEnabled: false,
    createdAt: now,
    ...overrides,
  };
}

interface ConnectionState {
  connections: ConnectionSettings[];
  health: Record<string, ConnectionHealth>;
  activeId: string | null;
  schema: SchemaRegistry | null;

  // R9: connection lifecycle state — drives App-level rendering gates.
  lifecycleState: ConnectionLifecycleState;
  lifecycleError: string | null;

  // runtime-only (not persisted): resolved admin keys loaded from keychain
  resolvedKeys: Record<string, string>;

  addConnection(s: ConnectionSettings): void;
  updateConnection(id: string, patch: Partial<ConnectionSettings>): void;
  removeConnection(id: string): void;
  setActive(id: string | null): void;
  setHealth(id: string, h: ConnectionHealth): void;
  setSchema(s: SchemaRegistry | null): void;
  setResolvedKey(id: string, key: string): void;

  // R9: validate an existing connection by id before activating it.
  // Sets lifecycleState through idle → validating → awaiting_db_choice | active | error.
  validateConnection(id: string): Promise<void>;

  // R9: called from DatabaseChoiceModal when user picks / creates a DB.
  confirmDatabase(dbName: string): void;

  getActive(): ConnectionSettings | null;
  getActiveKey(): string | undefined;
  getDatabases(): SchemaDatabase[];
}

export const useConnectionStore = create<ConnectionState>()(
  persist(
    (set, get) => ({
      connections: [],
      health: {},
      activeId: null,
      schema: null,
      resolvedKeys: {},
      lifecycleState: "idle" as ConnectionLifecycleState,
      lifecycleError: null,

      addConnection(s) {
        set((state) => ({ connections: [...state.connections, s] }));
      },

      updateConnection(id, patch) {
        set((state) => ({
          connections: state.connections.map((c) =>
            c.id === id ? { ...c, ...patch } : c
          ),
        }));
      },

      removeConnection(id) {
        set((state) => ({
          connections: state.connections.filter((c) => c.id !== id),
          activeId: state.activeId === id ? null : state.activeId,
        }));
      },

      setActive(id) {
        set({ activeId: id, schema: null, lifecycleState: "idle", lifecycleError: null });
        if (id) {
          set((state) => ({
            connections: state.connections.map((c) =>
              c.id === id ? { ...c, lastUsed: Date.now() } : c
            ),
          }));
        }
      },

      setHealth(id, h) {
        set((state) => ({ health: { ...state.health, [id]: h } }));
      },

      setSchema(s) {
        set({ schema: s });
      },

      setResolvedKey(id, key) {
        set((state) => ({
          resolvedKeys: { ...state.resolvedKeys, [id]: key },
        }));
      },

      async validateConnection(id) {
        const { connections, resolvedKeys } = get();
        const conn = connections.find((c) => c.id === id);
        if (!conn) {
          set({ lifecycleState: "error", lifecycleError: "Connection not found" });
          return;
        }

        set({ lifecycleState: "validating", lifecycleError: null, activeId: id });

        const adminKey = resolvedKeys[id] ?? conn.adminKey;
        const client = new StudioApiClient({
          baseUrl: conn.baseUrl,
          adminApiKey: conn.mode === "admin" ? adminKey : undefined,
          operatorId: conn.operatorId,
          tenantId: conn.tenantId,
          userId: conn.userId,
          database: conn.database,
        });

        try {
          // Step 1: verify server is reachable.
          await client.health();

          // Step 2: if a target database is configured, verify it exists.
          const targetDb = conn.database?.trim();
          if (targetDb && conn.mode === "admin") {
            const listed = await client.listDatabases();
            const exists = listed.databases.some(
              (d) => d.name.toLowerCase() === targetDb.toLowerCase()
            );
            if (!exists) {
              // Server reachable but database not found — prompt user to choose.
              set({ lifecycleState: "awaiting_db_choice", lifecycleError: null });
              return;
            }
          }

          // Validation passed — activate.
          set({
            lifecycleState: "active",
            lifecycleError: null,
            connections: connections.map((c) =>
              c.id === id ? { ...c, lastUsed: Date.now() } : c
            ),
          });
        } catch (err) {
          const msg = err instanceof Error ? err.message : String(err);
          const isAuth = msg.includes("401") || msg.includes("403") || msg.toLowerCase().includes("unauthorized");
          set({
            lifecycleState: "error",
            lifecycleError: isAuth
              ? `Authentication failed (${msg}). Check your credentials in Connection Settings.`
              : `Could not reach server: ${msg}`,
          });
        }
      },

      confirmDatabase(dbName) {
        // User has picked / created a database — patch the active connection and activate.
        const { activeId, connections } = get();
        if (!activeId) return;
        set({
          lifecycleState: "active",
          lifecycleError: null,
          connections: connections.map((c) =>
            c.id === activeId ? { ...c, database: dbName, lastUsed: Date.now() } : c
          ),
        });
      },

      getActive() {
        const { connections, activeId } = get();
        return connections.find((c) => c.id === activeId) ?? null;
      },

      getActiveKey() {
        const { activeId, resolvedKeys } = get();
        if (!activeId) return undefined;
        // Prefer runtime-resolved key (Tauri keychain), fall back to persisted adminKey
        return resolvedKeys[activeId] ?? get().connections.find((c) => c.id === activeId)?.adminKey;
      },

      getDatabases() {
        const { schema } = get();
        return schema?.databases ?? [];
      },
    }),
    {
      name: "vng-studio-connections",
      // Do NOT persist resolvedKeys — those live only in memory
      partialize: (s) => ({
        connections: s.connections.map((c) => ({ ...c })),
        activeId: s.activeId,
      }),
    }
  )
);
