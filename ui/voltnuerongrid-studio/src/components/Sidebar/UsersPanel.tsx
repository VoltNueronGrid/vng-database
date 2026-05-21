// Users & Roles panel — wired to /api/v1/admin/users server endpoints.
// Falls back to localStorage when no admin connection is active so the panel
// remains usable in read-only / disconnected scenarios.

import { useState, useEffect, useCallback } from "react";
import { useModalStore } from "@/store/modal";
import { openMenuFor } from "@/store/contextMenu";
import { buildUserMenu } from "@/components/ContextMenu/menus";
import { useConnectionStore } from "@/store/connection";
import { StudioApiClient, type AdminUserEntry } from "@/api/studio-client";

const STORAGE_KEY = "vng-studio-users-cache";
const BUILT_IN_ROLES = ["dba", "operator", "readwrite", "readonly"];

// ── Colour helpers ────────────────────────────────────────────────────────────

function roleBg(r: string) {
  if (r === "dba")       return "#ef444411";
  if (r === "operator")  return "#9333ea11";
  if (r === "readwrite") return "#3b82f611";
  return "#22c55e11";
}

function roleFg(r: string) {
  if (r === "dba")       return "var(--red)";
  if (r === "operator")  return "#c084fc";
  if (r === "readwrite") return "var(--blue)";
  return "var(--green)";
}

function roleBd(r: string) {
  if (r === "dba")       return "#ef444433";
  if (r === "operator")  return "#9333ea33";
  if (r === "readwrite") return "#3b82f633";
  return "#22c55e33";
}

// ── Component ─────────────────────────────────────────────────────────────────

export function UsersPanel() {
  const [users, setUsers] = useState<AdminUserEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const openModal = useModalStore((s) => s.open);
  const activeConn = useConnectionStore((s) => s.getActive());
  const activeKey = useConnectionStore((s) => s.getActiveKey());

  const loadUsers = useCallback(async () => {
    // ── Try server first ──────────────────────────────────────────────────
    if (activeConn) {
      setLoading(true);
      setError(null);
      try {
        const client = new StudioApiClient({
          baseUrl: activeConn.baseUrl,
          adminApiKey: activeKey,
          operatorId: activeConn.operatorId,
        });
        const res = await client.listUsers();
        setUsers(res.users);
        // Cache for offline display.
        try {
          localStorage.setItem(STORAGE_KEY, JSON.stringify(res.users));
        } catch { /* ignore quota errors */ }
        return;
      } catch (err) {
        setError(err instanceof Error ? err.message : "Failed to load users");
      } finally {
        setLoading(false);
      }
    }

    // ── Fall back to localStorage cache ────────────────────────────────────
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (raw) {
        setUsers(JSON.parse(raw) as AdminUserEntry[]);
        return;
      }
    } catch { /* ignore */ }

    // ── Seed with built-in admin placeholder ──────────────────────────────
    setUsers([
      {
        user_id: "u-admin",
        username: "admin",
        role: "dba",
        created_ms: Date.now() - 1_000_000_000,
      },
    ]);
  }, [activeConn, activeKey]);

  // Reload whenever the active connection changes.
  useEffect(() => {
    void loadUsers();
  }, [loadUsers]);

  return (
    <div>
      {/* ── Section: Users ───────────────────────────────────────────── */}
      <div className="conn-section-header">
        <span className="label-xs">Users</span>
        <button
          className="conn-add-btn"
          title="Create User"
          onClick={() => openModal({ kind: "create-user" })}
        >
          ＋
        </button>
      </div>

      {loading && (
        <div style={{ padding: "8px 12px", fontSize: 10.5, color: "var(--text-3)" }}>
          Loading…
        </div>
      )}

      {error && !loading && (
        <div style={{ padding: "6px 12px", fontSize: 10.5, color: "var(--red)", lineHeight: 1.4 }}>
          {error}
          <button
            style={{ marginLeft: 8, fontSize: 10, cursor: "pointer", color: "var(--text-2)" }}
            onClick={() => void loadUsers()}
          >
            Retry
          </button>
        </div>
      )}

      {!loading && users.map((u) => (
        <div
          key={u.user_id}
          className="conn-item"
          onContextMenu={openMenuFor(() => buildUserMenu(u.username))}
          title={`${u.user_id} — created ${new Date(u.created_ms).toLocaleDateString()}`}
        >
          <span className="conn-dot ok" />
          <span className="conn-item-name">{u.username}</span>
          <span
            className="conn-type-badge"
            style={{
              background: roleBg(u.role),
              color: roleFg(u.role),
              borderColor: roleBd(u.role),
            }}
          >
            {u.role}
          </span>
        </div>
      ))}

      {/* ── Section: Roles ───────────────────────────────────────────── */}
      <div className="conn-section-header" style={{ marginTop: 14 }}>
        <span className="label-xs">Roles</span>
        <button
          className="conn-add-btn"
          title="Create Role"
          onClick={() => openModal({ kind: "create-role" })}
        >
          ＋
        </button>
      </div>

      {BUILT_IN_ROLES.map((r) => (
        <div key={r} className="conn-item" style={{ cursor: "default" }}>
          <span className="tree-icon">🛡</span>
          <span className="conn-item-name">{r}</span>
          <span className="tree-count">
            {users.filter((u) => u.role === r).length}
          </span>
        </div>
      ))}

      {!activeConn && (
        <div
          style={{
            padding: "12px",
            fontSize: 10.5,
            color: "var(--text-3)",
            lineHeight: 1.5,
          }}
        >
          Connect with an admin key to manage server-side users.
        </div>
      )}
    </div>
  );
}
