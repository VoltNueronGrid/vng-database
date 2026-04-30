import { useCallback } from "react";
import { StudioApiClient } from "@/api/studio-client";
import { useConnectionStore } from "@/store/connection";
import { useQueryStore } from "@/store/query";

export function useQuery(tabId: string) {
  const getActive = useConnectionStore((s) => s.getActive);
  const getActiveKey = useConnectionStore((s) => s.getActiveKey);
  const { setResult, setExecuting } = useQueryStore();

  const execute = useCallback(
    async (sql: string) => {
      const conn = getActive();
      if (!conn) return;

      const client = new StudioApiClient({
        baseUrl: conn.baseUrl,
        adminApiKey: conn.mode === "admin" ? getActiveKey() : undefined,
        operatorId: conn.operatorId,
        tenantId: conn.tenantId,
        userId: conn.userId,
      });

      setExecuting(tabId, true);
      const start = Date.now();

      try {
        const res = await client.executeSql({
          sql_batch: sql,
        });

        // ── Build columns + rows ──────────────────────────────────────────
        // Primary source: res.columns / res.rows (populated by server SELECT builder).
        // Fallback: res.oltp_rows — point-query OLTP results that carry a data map.
        //   Each oltp_row is { key: "tbl:id", data: { __table, col_name: val, ... } }.
        //   Strip the __table meta key and infer column order from the first row.

        let columns = (res.columns ?? []).map((c) => ({ name: c.name, type: c.data_type }));
        let rows: Array<Record<string, unknown>> = res.rows ?? [];

        if (columns.length === 0 && (res.oltp_rows ?? []).length > 0) {
          const oltpRows = res.oltp_rows!;
          // Build a stable column list from the union of all data keys (excluding __table).
          const colSet = new Set<string>();
          for (const r of oltpRows) {
            for (const k of Object.keys(r.data)) {
              if (!k.startsWith("__")) colSet.add(k);
            }
          }
          // Sort: numeric col_N keys in numeric order; then alphabetical for named keys.
          const colList = [...colSet].sort((a, b) => {
            const mA = /^col_(\d+)$/.exec(a);
            const mB = /^col_(\d+)$/.exec(b);
            if (mA && mB) return parseInt(mA[1]) - parseInt(mB[1]);
            if (mA) return 1;
            if (mB) return -1;
            return a.localeCompare(b);
          });
          columns = colList.map((c) => ({ name: c, type: "text" }));
          rows = oltpRows
            // De-duplicate by key (same logical row can appear for multiple WHERE matches)
            .filter((r, idx, arr) => arr.findIndex((x) => x.key === r.key) === idx)
            .map((r) => {
              const out: Record<string, unknown> = {};
              for (const c of colList) out[c] = r.data[c] ?? null;
              return out;
            });
        }

        setResult({
          tabId,
          status: res.status,
          routePath: res.route_path,
          elapsedMs:
            res.transaction?.elapsed_ms ?? res.olap?.elapsed_ms ?? Date.now() - start,
          rejectedCount: res.rejected_statement_count,
          transactionId: res.transaction?.transaction_id,
          columns,
          rows,
          rowCount: rows.length,
          error: null,
          executedAt: Date.now(),
        });
      } catch (err) {
        setResult({
          tabId,
          status: "error",
          routePath: "unknown",
          elapsedMs: Date.now() - start,
          rejectedCount: 0,
          columns: [],
          rows: [],
          rowCount: 0,
          error: String(err),
          executedAt: Date.now(),
        });
      } finally {
        setExecuting(tabId, false);
      }
    },
    [tabId, getActive, getActiveKey, setResult, setExecuting]
  );

  return { execute };
}
