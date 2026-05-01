/**
 * Studio-wide Settings panel.
 *
 * Opened from the ⚙ button in the TitleBar. Persists preferences via
 * useSettingsStore (localStorage).  All settings are user-scoped — they apply
 * across every database and connection for this browser / desktop user.
 */
import { useEffect, useRef } from "react";
import {
  useSettingsStore,
  type DdlDoubleClickAction,
} from "@/store/settings";
import { useUiStore } from "@/store/ui";

export function SettingsPanel() {
  const closeSettings = useUiStore((s) => s.closeSettings);
  const {
    ddlDoubleClickAction,
    defaultQueryLimit,
    confirmUnsavedClose,
    update,
    reset,
  } = useSettingsStore();

  const panelRef = useRef<HTMLDivElement>(null);

  // Close on Escape key
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeSettings();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [closeSettings]);

  // Close on backdrop click
  function handleBackdrop(e: React.MouseEvent) {
    if (panelRef.current && !panelRef.current.contains(e.target as Node)) {
      closeSettings();
    }
  }

  return (
    <div className="settings-overlay" onMouseDown={handleBackdrop}>
      <div className="settings-panel" ref={panelRef} role="dialog" aria-label="Studio Settings">
        {/* Header */}
        <div className="settings-header">
          <span className="settings-title">⚙ Studio Settings</span>
          <button className="settings-close" onClick={closeSettings} aria-label="Close settings">
            ✕
          </button>
        </div>

        <div className="settings-body">
          {/* ── Schema Explorer ─────────────────────────── */}
          <section className="settings-section">
            <h3 className="settings-section-title">Schema Explorer</h3>

            <div className="settings-row">
              <label className="settings-label" htmlFor="ddl-dblclick">
                Double-click on object
                <span className="settings-hint">
                  What happens when you double-click a view, trigger, function,
                  event, or other schema object in the sidebar.
                </span>
              </label>
              <select
                id="ddl-dblclick"
                className="settings-select"
                value={ddlDoubleClickAction}
                onChange={(e) =>
                  update({ ddlDoubleClickAction: e.target.value as DdlDoubleClickAction })
                }
              >
                <option value="open_tab">Open DDL in new SQL tab</option>
                <option value="copy_clipboard">Copy DDL to clipboard</option>
              </select>
            </div>
          </section>

          {/* ── Query Editor ────────────────────────────── */}
          <section className="settings-section">
            <h3 className="settings-section-title">Query Editor</h3>

            <div className="settings-row">
              <label className="settings-label" htmlFor="default-limit">
                Default row limit
                <span className="settings-hint">
                  Maximum rows returned when a query has no explicit LIMIT clause.
                </span>
              </label>
              <input
                id="default-limit"
                type="number"
                className="settings-input"
                min={1}
                max={100000}
                step={100}
                value={defaultQueryLimit}
                onChange={(e) => {
                  const v = parseInt(e.target.value, 10);
                  if (!isNaN(v) && v > 0) update({ defaultQueryLimit: v });
                }}
              />
            </div>

            <div className="settings-row">
              <label className="settings-label" htmlFor="confirm-close">
                Confirm before closing unsaved tabs
              </label>
              <input
                id="confirm-close"
                type="checkbox"
                className="settings-checkbox"
                checked={confirmUnsavedClose}
                onChange={(e) => update({ confirmUnsavedClose: e.target.checked })}
              />
            </div>
          </section>
        </div>

        {/* Footer */}
        <div className="settings-footer">
          <button className="btn" onClick={reset} title="Restore all defaults">
            Reset to defaults
          </button>
          <button className="btn primary" onClick={closeSettings}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
}
