import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { SchemaRegistry, SchemaDatabase } from "@/api/studio-client";

export type ConnectionMode = "admin" | "operator" | "tenant";
export type ServerType = "voltnuerongrid" | "postgresql" | "mysql" | "other";
export type RuntimeTarget = "local" | "docker" | "cloud" | "custom";
export type HealthState = "unverified" | "ok" | "degraded" | "error";
export type ConnectionProtocol = "http" | "native";

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

  // runtime-only (not persisted): resolved admin keys loaded from keychain
  resolvedKeys: Record<string, string>;

  addConnection(s: ConnectionSettings): void;
  updateConnection(id: string, patch: Partial<ConnectionSettings>): void;
  removeConnection(id: string): void;
  setActive(id: string | null): void;
  setHealth(id: string, h: ConnectionHealth): void;
  setSchema(s: SchemaRegistry | null): void;
  setResolvedKey(id: string, key: string): void;

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
        set({ activeId: id, schema: null });
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
        const { schema, activeId, connections } = get();
        const databases = schema?.databases ?? [];
        const active = connections.find((c) => c.id === activeId);
        const selectedDatabase = active?.database?.trim().toLowerCase();
        if (!selectedDatabase) return databases;
        return databases.filter(
          (db) => db.name.trim().toLowerCase() === selectedDatabase
        );
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
