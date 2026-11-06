import { describe, expect, it } from "vitest";
import { locatePath } from "../src/components/strategies/CodeEditor";
import { defaultValue, issuesFor, moveItem, valueText } from "../src/components/strategies/editorUtils";

describe("editor utils", () => {
  it("moves items and ignores out-of-range moves", () => {
    expect(moveItem([1, 2, 3], 0, 2)).toEqual([2, 3, 1]);
    expect(moveItem([1, 2, 3], 0, -1)).toEqual([1, 2, 3]);
  });
  it("renders values of every kind", () => {
    expect(valueText({ fake: "a.bin" })).toBe("a.bin");
    expect(valueText({ list: "l.txt" })).toBe("l.txt");
    expect(valueText("443")).toBe("443");
    expect(defaultValue({ type: "list" }, { fakes: [], lists: ["list-general.txt"] })).toEqual({ list: "list-general.txt" });
  });
  it("filters issues by path", () => {
    const issues = [
      { severity: "error" as const, path: "rules[1].args[0]", message: "x" },
      { severity: "warning" as const, path: "rules[1]", message: "y" },
      { severity: "error" as const, path: "rules[10]", message: "z" },
    ];
    expect(issuesFor(issues, "rules[1]", false).length).toBe(2);
    expect(issuesFor(issues, "rules[1].args[0]").length).toBe(1);
  });
  it("locates validation paths in JSON", () => {
    const text = JSON.stringify({ name: "a", global: [{ opt: "--wf-tcp" }], rules: [{ args: [{ opt: "--a" }] }, { args: [{ opt: "--b" }, { opt: "--c" }] }] }, null, 2);
    expect(text.slice(locatePath(text, "rules[1].args[1]")).startsWith('"opt": "--c"')).toBe(true);
    expect(text.slice(locatePath(text, "global[0]")).startsWith('"opt": "--wf-tcp"')).toBe(true);
  });
});
