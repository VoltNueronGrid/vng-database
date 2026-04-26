import { useState, useEffect, useCallback } from "react";
import { useConnectionStore } from "@/store/connection";
import { StudioApiClient } from "@/api/studio-client";
import type { ClusterTopologyResponse, AuditEventsResponse } from "@/api/studio-client";

function timeAgo(epochMs: number): string {
  const diff = Date.now() - epochMs;
  if (diff < 60_000) return `${Math.round(diff / 1000)}s ago`;
  if (diff < 3_600_000) return `${Math.round(diff / 60_000)}m ago`;
  return `${Math.round(diff / 3_600_000)}h ago`;
}

export function Dashboard() {
  const getActive = useConnectionStore((s) => s.getActive);
  const getActiveKey = useConnectionStore((s) => s.getActiveKey);

  const [topology, setTopology] = useState<ClusterTopologyResponse | null>(null);
  const [audit, setAudit] = useState<AuditEventsResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [lastRefresh, setLastRefresh] = useState<number | null>(null);
  const [err, setErr] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const conn = getActive();
    if (!conn) return;
    const client = new StudioApiClient({
      baseUrl: conn.baseUrl,
      adminApiKey: conn.mode === "admin" ? getActiveKey() : undefined,
    });
    setLoading(true);
    setErr(null);
    try {
      const [topo, auditRes] = await Promise.allSettled([
        client.getClusterTopology(),
        client.getAuditEvents(20),
      ]);
      if (topo.status === "fulfilled") setTopology(topo.value);
      if (auditRes.status === "fulfilled") setAudit(auditRes.value);
      setLastRefresh(Date.now());
    } catch (e) {
      setErr(String(e));
    } finally {
      setLoading(false);
    }
  }, [getActive, getActiveKey]);

  useEffect(() => { refresh(); }, [refresh]);

  const conn = getActive();

  return (
    <div className="dashboard">
      {/* Header */}
      <div className="dash-title-row">
        <div>
          <div className="dash-title">Cluster Overview</div>
          <div className="dash-sub">
            {conn?.name ?? "—"}
            {lastRefresh && ` · Refreshed ${timeAgo(lastRefresh)}`}
          </div>
        </div>
        <button className="btn primary" onClick={refresh} disabled={loading}>
          {loading ? "⟳ Refreshing…" : "⟳ Refresh"}
        </button>
      </div>

      {err && (
        <div style={{ color: "var(--red)", fontSize: 12, padding: "8px 0" }}>
          ⚠ {err}
        </div>
      )}

      {/* KPI Cards */}
      {topology && (
        <div>
          <div className="section-label">Live Metrics</div>
          <div className="kpi-grid">
            <div className="kpi-card">
              <div className="kpi-val" style={{ color: "var(--brand-cyan)" }}>
                {topology.active_nodes}
              </div>
              <div className="kpi-label">Active Nodes</div>
              <div className="kpi-trend trend-flat">
                {topology.total_nodes} total
              </div>
            </div>
            <div className="kpi-card">
              <div className="kpi-val" style={{ color: "var(--green)" }}>
                {topology.active_sessions + topology.passive_sessions}
              </div>
              <div className="kpi-label">Sessions</div>
              <div className="kpi-trend trend-flat">
                {topology.active_sessions} active
              </div>
            </div>
            <div className="kpi-card">
              <div className="kpi-val" style={{ color: "var(--yellow)" }}>
                {topology.live_transactions}
              </div>
              <div className="kpi-label">Live Transactions</div>
              <div className="kpi-trend trend-flat">
                {topology.total_transactions} total
              </div>
            </div>
            <div className="kpi-card">
              <div className="kpi-val" style={{ color: "var(--orange)" }}>
                {topology.live_locks}
              </div>
              <div className="kpi-label">Active Locks</div>
              <div className="kpi-trend trend-flat">
                {topology.dead_nodes > 0
                  ? `⚠ ${topology.dead_nodes} dead nodes`
                  : "All nodes healthy"}
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Cluster Nodes */}
      {topology && topology.nodes.length > 0 && (
        <div>
          <div className="section-label">Cluster Nodes</div>
          <div className="node-grid">
            {topology.nodes.map((node) => {
              const cpuPct = Math.round(node.used_cpu_pct);
              const ramPct = Math.round((node.used_ram_mb / node.total_ram_mb) * 100);
              return (
                <div key={node.node_id} className="node-card">
                  <div className={`node-status-dot ${node.status}`} />
                  <div className="node-info">
                    <div className="node-name">{node.node_id}</div>
                    <div className={`node-role ${node.role}`}>{node.role}</div>
                    <div className="node-stats">
                      <div className="node-stat">
                        CPU <span className="nv">{cpuPct}%</span>
                      </div>
                      <div className="node-stat">
                        RAM <span className="nv">{Math.round(node.used_ram_mb / 1024)}/ {Math.round(node.total_ram_mb / 1024)} GB</span>
                      </div>
                      <div className="node-stat">
                        Sessions <span className="nv">{node.active_sessions}</span>
                      </div>
                      <div className="node-stat">
                        Txns <span className="nv">{node.live_transactions}</span>
                      </div>
                    </div>
                    <div style={{ marginTop: 6 }}>
                      <div style={{ fontSize: 9, color: "var(--text-3)", marginBottom: 2 }}>CPU</div>
                      <div className="bar-track">
                        <div className="bar-fill bar-cpu" style={{ width: `${cpuPct}%` }} />
                      </div>
                      <div style={{ fontSize: 9, color: "var(--text-3)", marginTop: 5, marginBottom: 2 }}>RAM</div>
                      <div className="bar-track">
                        <div className="bar-fill bar-ram" style={{ width: `${ramPct}%` }} />
                      </div>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* Audit Log */}
      {audit && (
        <div>
          <div className="section-label">
            Recent Audit Events
            <button className="btn btn-sm">View All</button>
          </div>
          <div className="audit-list">
            {audit.events.slice(0, 10).map((ev) => (
              <div key={ev.event_id} className="audit-row">
                <span className={`audit-kind ak-${ev.kind.toLowerCase()}`}>
                  {ev.kind.toUpperCase()}
                </span>
                <span className="audit-actor">{ev.actor}</span>
                <span className="audit-action">{ev.action}</span>
                <span className={`audit-outcome ${ev.outcome === "ok" ? "ao-ok" : "ao-fail"}`}>
                  {ev.outcome.toUpperCase()}
                </span>
                <span className="audit-time">{timeAgo(ev.occurred_epoch_ms)}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {!topology && !loading && !err && (
        <div className="results-empty">
          <div className="re-icon">📊</div>
          <div className="text-muted">Connect to a server to view dashboard.</div>
        </div>
      )}
    </div>
  );
}
