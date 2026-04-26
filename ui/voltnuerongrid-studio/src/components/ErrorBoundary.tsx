import { Component, type ReactNode, type ErrorInfo } from "react";

interface Props {
  children: ReactNode;
  label?: string;
}

interface State {
  error: Error | null;
  info: ErrorInfo | null;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null, info: null };

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    this.setState({ info });
    console.error(`[ErrorBoundary:${this.props.label ?? "root"}]`, error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div
          style={{
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            height: "100%",
            width: "100%",
            padding: 32,
            background: "#08080f",
            color: "#ef4444",
            fontFamily: "monospace",
            fontSize: 12,
            gap: 12,
            textAlign: "center",
            boxSizing: "border-box",
          }}
        >
          <div style={{ fontSize: 28 }}>⚠</div>
          <div style={{ fontSize: 14, fontWeight: 700, color: "#e4e4f0" }}>
            Render Error{this.props.label ? ` — ${this.props.label}` : ""}
          </div>
          <div style={{ color: "#ef4444", maxWidth: 520, lineHeight: 1.5 }}>
            {this.state.error.message}
          </div>
          {this.state.error.stack && (
            <pre
              style={{
                color: "#6a6a88",
                fontSize: 10,
                maxWidth: 600,
                maxHeight: 200,
                overflow: "auto",
                textAlign: "left",
                background: "#0f0f1a",
                padding: "8px 12px",
                borderRadius: 4,
                border: "1px solid #21212e",
                whiteSpace: "pre-wrap",
                wordBreak: "break-word",
              }}
            >
              {this.state.error.stack}
            </pre>
          )}
          <button
            onClick={() => this.setState({ error: null, info: null })}
            style={{
              marginTop: 8,
              padding: "6px 18px",
              background: "#1c1c2a",
              border: "1px solid #333348",
              borderRadius: 4,
              color: "#e4e4f0",
              cursor: "pointer",
              fontSize: 12,
            }}
          >
            Try again
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
