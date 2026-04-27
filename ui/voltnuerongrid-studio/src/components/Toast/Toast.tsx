import { useToastStore } from "@/store/toast";

export function Toast() {
  const toasts = useToastStore((s) => s.toasts);
  const dismiss = useToastStore((s) => s.dismiss);

  if (toasts.length === 0) return null;

  // Stack last 3 toasts; most recent on top.
  const visible = toasts.slice(-3);
  const latest = visible[visible.length - 1];

  const dotColor =
    latest.kind === "success"
      ? "var(--green)"
      : latest.kind === "error"
      ? "var(--red)"
      : "var(--brand-cyan)";

  return (
    <div
      className="toast"
      onClick={() => dismiss(latest.id)}
      title="Click to dismiss"
    >
      <span style={{ color: dotColor, fontSize: 10 }}>●</span>
      {latest.message}
    </div>
  );
}
