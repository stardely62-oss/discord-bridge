import { autocompletion, CompletionContext } from "@codemirror/autocomplete";
import { json } from "@codemirror/lang-json";
import { Diagnostic, linter, lintGutter } from "@codemirror/lint";
import { EditorView } from "@codemirror/view";
import CodeMirror from "@uiw/react-codemirror";
import { useMemo } from "react";
import { api } from "../../services/api";
import type { OptionSpec } from "../../services/types";

const theme = EditorView.theme({
  "&": { backgroundColor: "rgb(var(--c-sunken))", color: "rgb(var(--c-ink))" },
  ".cm-content": { caretColor: "rgb(var(--c-accent))" },
  ".cm-gutters": { backgroundColor: "rgb(var(--c-sunken))", color: "rgb(var(--c-ink-faint))", border: "none", borderRight: "1px solid rgb(var(--c-line))" },
  ".cm-activeLine": { backgroundColor: "rgb(var(--c-ink) / 0.03)" },
  ".cm-activeLineGutter": { backgroundColor: "rgb(var(--c-ink) / 0.05)" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": { backgroundColor: "rgb(var(--c-accent) / 0.25) !important" },
  ".cm-tooltip": { backgroundColor: "rgb(var(--c-surface))", border: "1px solid rgb(var(--c-line-strong))", color: "rgb(var(--c-ink))" },
  ".cm-tooltip-autocomplete ul li[aria-selected]": { backgroundColor: "rgb(var(--c-accent) / 0.25)", color: "rgb(var(--c-ink))" },
});

function lineStart(doc: string, line: number): number {
  let pos = 0;
  for (let i = 1; i < line; i++) {
    const n = doc.indexOf("\n", pos);
    if (n < 0) return doc.length;
    pos = n + 1;
  }
  return pos;
}

function nth(text: string, needle: string, from: number, n: number): number {
  let pos = from;
  for (let k = 0; k <= n; k++) {
    pos = text.indexOf(needle, k === 0 ? pos : pos + 1);
    if (pos < 0) return -1;
  }
  return pos;
}

/** Best-effort mapping of a validation path (rules[2].args[3]) to a text offset. */
export function locatePath(text: string, path: string): number {
  const g = /^global\[(\d+)\]/.exec(path);
  if (g) {
    const base = text.indexOf('"global"');
    return base < 0 ? 0 : Math.max(0, nth(text, '"opt"', base, Number(g[1])));
  }
  const r = /^rules\[(\d+)\](?:\.args\[(\d+)\])?/.exec(path);
  if (r) {
    const base = text.indexOf('"rules"');
    if (base < 0) return 0;
    const args = nth(text, '"args"', base, Number(r[1]));
    if (args < 0) return base;
    return r[2] === undefined ? args : Math.max(args, nth(text, '"opt"', args, Number(r[2])));
  }
  const key = text.indexOf(`"${path.split(/[.[]/)[0]}"`);
  return key < 0 ? 0 : key;
}

/**
 * JSON editor for the structured strategy format. Syntax errors and schema
 * errors come from the Rust backend (the same validation the helper runs).
 */
export function CodeEditor({ value, onChange, specs, readOnly }: { value: string; onChange: (v: string) => void; specs: OptionSpec[]; readOnly: boolean }) {
  const extensions = useMemo(() => {
    const complete = (ctx: CompletionContext) => {
      const word = ctx.matchBefore(/--[a-z0-9-]*/);
      if (!word || (word.from === word.to && !ctx.explicit)) return null;
      return {
        from: word.from,
        options: specs.map((s) => ({ label: s.name, detail: s.titleRu, info: s.helpRu, type: s.scope === "global" ? "keyword" : "property" })),
      };
    };
    const lint = linter(
      async (view) => {
        const text = view.state.doc.toString();
        const out: Diagnostic[] = [];
        try {
          const r = await api.parseStrategyJson(text);
          if (!r.ok) {
            const from = r.line ? Math.min(lineStart(text, r.line) + Math.max(0, (r.column ?? 1) - 1), text.length) : 0;
            out.push({ from, to: Math.min(from + 1, text.length), severity: "error", message: r.error ?? "ошибка" });
          } else {
            for (const i of r.details?.issues ?? []) {
              const from = locatePath(text, i.path);
              const to = Math.min(text.length, from + 5);
              out.push({ from, to, severity: i.severity === "error" ? "error" : "warning", message: `${i.path}: ${i.message}` });
            }
          }
        } catch {
          /* backend unavailable in browser preview */
        }
        return out;
      },
      { delay: 500 },
    );
    return [json(), lint, lintGutter(), autocompletion({ override: [complete] }), theme];
  }, [specs]);

  return (
    <CodeMirror
      value={value}
      onChange={onChange}
      readOnly={readOnly}
      theme={document.documentElement.dataset.theme === "light" ? "light" : "dark"}
      height="100%"
      basicSetup={{ foldGutter: true, highlightActiveLine: true, bracketMatching: true, autocompletion: false }}
      extensions={extensions}
      aria-label="JSON"
    />
  );
}
