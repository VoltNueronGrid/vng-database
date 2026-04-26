import { useCallback, useEffect } from "react";
import { StudioApiClient } from "@/api/studio-client";
import { useConnectionStore } from "@/store/connection";

export function useSchema() {
  const getActive = useConnectionStore((s) => s.getActive);
  const getActiveKey = useConnectionStore((s) => s.getActiveKey);
  const activeId = useConnectionStore((s) => s.activeId);
  const setSchema = useConnectionStore((s) => s.setSchema);
  const setHealth = useConnectionStore((s) => s.setHealth);

  const refresh = useCallback(async () => {
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
  }, [getActive, getActiveKey, setSchema, setHealth]);

  // Auto-fetch when the active connection changes
  useEffect(() => {
    if (activeId) refresh();
  }, [activeId, refresh]);

  return { refresh };
}
