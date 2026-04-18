import * as vscode from "vscode";

/** Injected dual-transport preference (NT-S3-004); data-plane still HTTP until TS native client lands. */
export type InjectedTransportMode = "http" | "native" | "auto";

export interface TransportInjection {
  transportMode: InjectedTransportMode;
  /** Optional `vng://host:port` when using native or auto with an explicit native endpoint. */
  nativeEndpoint?: string;
}

export function readTransportInjectionFromConfig(): TransportInjection {
  const cfg = vscode.workspace.getConfiguration("voltnuerongrid");
  const raw = (cfg.get<string>("transportMode") ?? "http").toLowerCase();
  const transportMode: InjectedTransportMode =
    raw === "native" || raw === "auto" ? raw : "http";
  const nativeEndpoint = cfg.get<string>("nativeEndpoint")?.trim();
  return {
    transportMode,
    nativeEndpoint: nativeEndpoint && nativeEndpoint.length > 0 ? nativeEndpoint : undefined,
  };
}
