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

        setResult({
          tabId,
          status: res.status,
          routePath: res.route_path,
          elapsedMs:
            res.transaction?.elapsed_ms ?? res.olap?.elapsed_ms ?? Date.now() - start,
          rejectedCount: res.rejected_statement_count,
          transactionId: res.transaction?.transaction_id,
          columns: (res.columns ?? []).map((c) => ({ name: c.name, type: c.data_type })),
          rows: res.rows ?? [],
          rowCount: res.rows?.length ?? 0,
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
