interface TransportLogChannel {
  appendLine(message: string): void;
}

let channel: TransportLogChannel | undefined;

function loadVscodeModule(): typeof import("vscode") | undefined {
  try {
    // Keep runtime compatibility for node --test where the vscode module is not present.
    // eslint-disable-next-line @typescript-eslint/no-require-imports
    return require("vscode") as typeof import("vscode");
  } catch {
    return undefined;
  }
}

function createFallbackChannel(): TransportLogChannel {
  return {
    appendLine: () => {
      // No-op in non-extension runtimes (unit tests).
    },
  };
}

export function getTransportOutputChannel(): TransportLogChannel {
  if (!channel) {
    const vscode = loadVscodeModule();
    channel = vscode?.window.createOutputChannel("VoltNueronGrid Transport") ?? createFallbackChannel();
  }
  return channel;
}

/** NT-S5-002 scaffold: structured lines for transport preference vs actual data-plane (HTTP until TS native execution lands). */
export function appendTransportLogLine(message: string): void {
  const ts = new Date().toISOString();
  getTransportOutputChannel().appendLine(`[${ts}] ${message}`);
}
