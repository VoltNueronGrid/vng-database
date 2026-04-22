/**
 * Connection model representing a database connection configuration
 */

export type ConnectionMode = "admin" | "operator" | "tenant";
export type ServerType = "voltnuerongrid" | "postgresql" | "mysql" | "other";
export type RuntimeTarget = "local" | "docker" | "cloud" | "custom";

export interface SSLConfig {
  enabled: boolean;
  caPath?: string;
  certPath?: string;
  keyPath?: string;
  rejectUnauthorized?: boolean;
}

export interface AdvancedOptions {
  connectionTimeout?: number; // milliseconds
  idleTimeout?: number; // milliseconds
  keepAlive?: boolean;
  maxConnections?: number;
}

export interface ConnectionSettings {
  // Connection identity
  id: string; // unique identifier
  name: string; // display name
  serverType: ServerType;
  runtimeTarget: RuntimeTarget;

  /** Dual-transport preference (workspace default can override via settings). */
  transportMode?: "http" | "native" | "auto";
  /** Optional native endpoint (`vng://...`) when not inferring from baseUrl. */
  nativeEndpoint?: string;

  // Server config
  baseUrl: string;
  host: string;
  port: number;
  database?: string;

  // Credentials
  username?: string;
  // password is NOT stored in this interface - stored in SecretStorage

  // Auth mode
  mode: ConnectionMode;
  adminKey?: string; // for admin mode (stored in SecretStorage)
  operatorId?: string; // for operator mode
  tenantId?: string; // for tenant mode
  userId?: string; // for tenant mode

  // SSL/TLS
  ssl: SSLConfig;

  // Advanced
  advanced: AdvancedOptions;

  // Metadata
  createdAt: number; // timestamp
  lastUsed?: number; // timestamp
}

export type ConnectionHealthState = "unverified" | "verified" | "degraded" | "error";

export interface ConnectionDiagnostic {
  state: ConnectionHealthState;
  lastChecked?: number; // epoch ms
  message?: string;
}

export interface Connection {
  id: string;
  settings: ConnectionSettings;
  isActive: boolean;
  isConnected: boolean;
  diagnostic: ConnectionDiagnostic;
}

export interface StoredConnection {
  id: string;
  settings: Omit<ConnectionSettings, "adminKey">; // adminKey stored separately in SecretStorage
}

/**
 * Validate connection settings
 */
export function validateConnectionSettings(settings: Partial<ConnectionSettings>): string | null {
  if (!settings.name || settings.name.trim().length === 0) {
    return "Connection name is required";
  }
  if (!settings.host || settings.host.trim().length === 0) {
    return "Host is required";
  }
  if (!settings.port || settings.port < 1 || settings.port > 65535) {
    return "Port must be between 1 and 65535";
  }
  if (!settings.baseUrl || settings.baseUrl.trim().length === 0) {
    return "Base URL is required";
  }
  if (settings.mode === "operator" && !settings.operatorId) {
    return "Operator ID required for operator mode";
  }
  if (settings.mode === "tenant" && !settings.tenantId) {
    return "Tenant ID required for tenant mode";
  }
  if (settings.ssl?.enabled) {
    const sslPaths = [settings.ssl.caPath, settings.ssl.certPath, settings.ssl.keyPath];
    if (sslPaths.some((path) => path !== undefined && path.trim().length === 0)) {
      return "SSL certificate paths cannot be empty";
    }
  }
  if (settings.advanced?.connectionTimeout !== undefined && settings.advanced.connectionTimeout <= 0) {
    return "Connection timeout must be greater than 0";
  }
  if (settings.advanced?.idleTimeout !== undefined && settings.advanced.idleTimeout <= 0) {
    return "Idle timeout must be greater than 0";
  }
  if (settings.advanced?.maxConnections !== undefined && settings.advanced.maxConnections <= 0) {
    return "Max connections must be greater than 0";
  }
  return null;
}

/**
 * Create a default connection template
 */
export function createDefaultConnection(overrides?: Partial<ConnectionSettings>): ConnectionSettings {
  const now = Date.now();
  return {
    id: `conn-${now}`,
    name: overrides?.name || "New Connection",
    serverType: overrides?.serverType || "voltnuerongrid",
    runtimeTarget: overrides?.runtimeTarget || "local",
    baseUrl: overrides?.baseUrl || "http://127.0.0.1:8080",
    host: overrides?.host || "127.0.0.1",
    port: overrides?.port || 8080,
    database: overrides?.database,
    username: overrides?.username,
    mode: overrides?.mode || "admin",
    operatorId: overrides?.operatorId,
    tenantId: overrides?.tenantId,
    userId: overrides?.userId,
    ssl: overrides?.ssl || {
      enabled: false,
    },
    advanced: overrides?.advanced || {
      connectionTimeout: 5000,
      idleTimeout: 300000,
      keepAlive: true,
      maxConnections: 10,
    },
    createdAt: now,
    ...overrides,
  };
}
