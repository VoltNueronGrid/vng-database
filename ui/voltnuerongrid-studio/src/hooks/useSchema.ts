import { useCallback, useEffect } from "react";
import { StudioApiClient } from "@/api/studio-client";
import { useConnectionStore } from "@/store/connection";

export function useSchema() {
  const activeId = useConnectionStore((s) => s.activeId);

  const refresh = useCallback(async () => {
    const { getActive, getActiveKey, setSchema, setHealth } =
      useConnectionStore.getState();
    const conn = getActive();
    if (!conn) return;

    const client = new StudioApiClient({
      baseUrl: conn.baseUrl,
      adminApiKey: conn.mode === "admin" ? getActiveKey() : undefined,
      operatorId: conn.operatorId,
    });

    try {
      const registry = await client.getSchemaTree();
      setSchema(registry);
      setHealth(conn.id, { state: "ok", checkedAt: Date.now() });
    } catch (err) {
      setHealth(conn.id, {
        state: "error",
        checkedAt: Date.now(),
        message: String(err),
      });
    }
  }, []);

  // Auto-fetch when the active connection changes
  useEffect(() => {
    if (activeId) refresh();
  }, [activeId, refresh]);

  return { refresh };
}
