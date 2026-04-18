import * as vscode from "vscode";

let channel: vscode.OutputChannel | undefined;

export function getTransportOutputChannel(): vscode.OutputChannel {
  if (!channel) {
    channel = vscode.window.createOutputChannel("VoltNueronGrid Transport");
  }
  return channel;
}

/** NT-S5-002 scaffold: structured lines for transport preference vs actual data-plane (HTTP until TS native execution lands). */
export function appendTransportLogLine(message: string): void {
  const ts = new Date().toISOString();
  getTransportOutputChannel().appendLine(`[${ts}] ${message}`);
}
