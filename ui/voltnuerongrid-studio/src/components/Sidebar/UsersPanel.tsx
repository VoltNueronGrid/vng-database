// Users & Roles panel — client-side mock until server admin endpoints exist.
// Persists to localStorage so user-management UX can be exercised end-to-end.

import { useState, useEffect } from "react";
import { useModalStore } from "@/store/modal";
import { openMenuFor } from "@/store/contextMenu";
import { buildUserMenu } from "@/components/ContextMenu/menus";

interface UserDraft {
  id: string;
  username: string;
  role: string;
  active: boolean;
  createdAt: number;
}

const STORAGE_KEY = "vng-studio-users-mock";
const BUILT_IN_ROLES = ["dba", "operator", "readwrite", "readonly"];

function loadUsers(): UserDraft[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return JSON.parse(raw) as UserDraft[];
  } catch {
    // ignore
  }
  // Seed with plausible users so the UI isn't empty on first load.
  return [
    { id: "u-admin",   username: "admin",   role: "dba",       active: true, createdAt: Date.now() - 1e9 },
    { id: "u-analyst", username: "analyst", role: "readonly",  active: true, createdAt: Date.now() - 1e8 },
    { id: "u-etl",     username: "etl_bot", role: "readwrite", active: true, createdAt: Date.now() - 1e7 },
  ];
}

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

export function UsersPanel() {
  const [users, setUsers] = useState<UserDraft[]>(() => loadUsers());
  const openModal = useModalStore((s) => s.open);

  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(users));
    } catch {
      // ignore
    }
  }, [users]);

  return (
    <div>
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

      {users.map((u) => (
        <div
          key={u.id}
          className="conn-item"
          onContextMenu={openMenuFor(() => buildUserMenu(u.username))}
          title={`Created ${new Date(u.createdAt).toLocaleDateString()}`}
        >
          <span className={`conn-dot ${u.active ? "ok" : "none"}`} />
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

      <div
        style={{
          padding: "12px",
          fontSize: 10.5,
          color: "var(--text-3)",
          lineHeight: 1.5,
        }}
      >
        Right-click a user to manage. User management is local-only until server admin endpoints are wired.
      </div>
    </div>
  );
}
