// Tauri IPC wrappers — thin layer over Rust commands.
// Falls back gracefully when running in a plain browser (e.g. design prototyping).

const isTauri = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function invokeIfTauri<T>(
  cmd: string,
  args?: Record<string, unknown>
): Promise<T> {
  if (!isTauri()) {
    throw new Error(`Tauri not available — cannot call '${cmd}' in browser`);
  }
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

export const tauriCredentials = {
  store(connId: string, key: string, value: string): Promise<void> {
    return invokeIfTauri("store_credential", { connId, key, value });
  },
  get(connId: string, key: string): Promise<string | null> {
    return invokeIfTauri("get_credential", { connId, key });
  },
  delete(connId: string, key: string): Promise<void> {
    return invokeIfTauri("delete_credential", { connId, key });
  },
};

export const tauriFiles = {
  readSql(path: string): Promise<string> {
    return invokeIfTauri("read_sql_file", { path });
  },
  writeSql(path: string, content: string): Promise<void> {
    return invokeIfTauri("write_sql_file", { path, content });
  },
};

export const tauriWindow = {
  minimize(): Promise<void> {
    return invokeIfTauri("window_minimize");
  },
  toggleMaximize(): Promise<void> {
    return invokeIfTauri("window_toggle_maximize");
  },
  close(): Promise<void> {
    return invokeIfTauri("window_close");
  },
};
