import Editor, { type OnMount } from "@monaco-editor/react";
import { useEditorStore } from "@/store/editor";
import { useQuery } from "@/hooks/useQuery";
import { useRef, useEffect } from "react";
import type { editor as MonacoEditor } from "monaco-editor";

const VNG_DARK_THEME = "vng-dark";

function defineTheme(monaco: typeof import("monaco-editor")) {
  monaco.editor.defineTheme(VNG_DARK_THEME, {
    base: "vs-dark",
    inherit: true,
    rules: [
      { token: "keyword.sql",    foreground: "c084fc", fontStyle: "bold" },
      { token: "string.sql",     foreground: "34d399" },
      { token: "number.sql",     foreground: "fb923c" },
      { token: "comment.sql",    foreground: "4b5563", fontStyle: "italic" },
      { token: "operator.sql",   foreground: "94a3b8" },
      { token: "identifier.sql", foreground: "e4e4f0" },
    ],
    colors: {
      "editor.background":              "#08080f",
      "editor.foreground":              "#e4e4f0",
      "editorLineNumber.foreground":    "#5a5a78",
      "editorLineNumber.activeForeground": "#e4e4f0",
      "editor.lineHighlightBackground": "#ffffff08",
      "editorCursor.foreground":        "#00d4ff",
      "editor.selectionBackground":     "#00d4ff22",
      "editorIndentGuide.background1":  "#21212e",
      "editorIndentGuide.activeBackground1": "#2e2e3e",
      "editorWidget.background":        "#15151f",
      "editorWidget.border":            "#2e2e3e",
      "input.background":               "#1c1c2a",
      "list.hoverBackground":           "#25253a",
      "list.activeSelectionBackground": "#2a2a40",
    },
  });
}

export function SqlEditorPane() {
  const activeTabId = useEditorStore((s) => s.activeTabId);
  const getActiveTab = useEditorStore((s) => s.getActiveTab);
  const updateSql = useEditorStore((s) => s.updateSql);
  const setEditorInstance = useEditorStore((s) => s.setEditorInstance);
  const getSelectedSql = useEditorStore((s) => s.getSelectedSql);
  const editorRef = useRef<MonacoEditor.IStandaloneCodeEditor | null>(null);
  const { execute } = useQuery(activeTabId ?? "");

  // Clear the editor instance ref when this pane unmounts
  useEffect(() => {
    return () => setEditorInstance(null);
  }, [setEditorInstance]);

  const tab = getActiveTab();

  const handleMount: OnMount = (editor, monaco) => {
    editorRef.current = editor;
    setEditorInstance(editor);
    defineTheme(monaco);
    monaco.editor.setTheme(VNG_DARK_THEME);

    // ⌘Enter / Ctrl+Enter → run selected text if any, otherwise full content
    editor.addCommand(
      monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter,
      () => {
        const selected = getSelectedSql();
        const sql = selected ?? editor.getValue();
        if (sql.trim()) execute(sql);
      }
    );
  };

  // Show for both sql and table tab types (table tabs have pre-filled SELECT SQL)
  if (!tab || (tab.type !== "sql" && tab.type !== "table")) return null;

  return (
    <div className="editor-pane">
      <Editor
        height="100%"
        defaultLanguage="sql"
        theme={VNG_DARK_THEME}
        value={tab.sql}
        onChange={(value) => {
          if (activeTabId) updateSql(activeTabId, value ?? "");
        }}
        onMount={handleMount}
        loading={
          <div
            style={{
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              height: "100%",
              color: "#5a5a78",
              fontSize: 12,
              fontFamily: "monospace",
            }}
          >
            Loading editor…
          </div>
        }
        options={{
          minimap: { enabled: false },
          fontSize: 13,
          lineHeight: 20,
          fontFamily: "'SF Mono', 'Fira Code', 'Cascadia Code', monospace",
          fontLigatures: true,
          lineNumbers: "on",
          scrollBeyondLastLine: false,
          wordWrap: "off",
          tabSize: 2,
          insertSpaces: true,
          renderLineHighlight: "line",
          cursorBlinking: "smooth",
          cursorSmoothCaretAnimation: "on",
          smoothScrolling: true,
          padding: { top: 12, bottom: 12 },
          suggest: { showFields: true, showFunctions: true, showKeywords: true },
          quickSuggestions: { other: true, comments: false, strings: false },
          parameterHints: { enabled: true },
          formatOnPaste: false,
          formatOnType: false,
          renderWhitespace: "selection",
          folding: true,
          bracketPairColorization: { enabled: true },
          overviewRulerLanes: 0,
          hideCursorInOverviewRuler: true,
          scrollbar: {
            vertical: "auto",
            horizontal: "auto",
            verticalScrollbarSize: 5,
            horizontalScrollbarSize: 5,
          },
        }}
      />
    </div>
  );
}
