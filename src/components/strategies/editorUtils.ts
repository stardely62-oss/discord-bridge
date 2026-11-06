import type { Arg, ArgValue, Issue, OptionKind, OptionSpec, Strategy } from "../../services/types";

export const CATEGORY_RU: Record<OptionSpec["category"], string> = {
  capture: "Перехват",
  filter: "Фильтры",
  lists: "Списки",
  desync: "Десинхронизация",
  fake: "Фейки",
  split: "Нарезка",
  misc: "Прочее",
};
export const CATEGORY_EN: Record<OptionSpec["category"], string> = {
  capture: "Capture",
  filter: "Filters",
  lists: "Lists",
  desync: "Desync",
  fake: "Fakes",
  split: "Split",
  misc: "Misc",
};

export function valueText(v?: ArgValue): string {
  if (v === undefined) return "";
  if (typeof v === "string") return v;
  if ("fake" in v) return v.fake;
  return v.list;
}

export function defaultValue(kind: OptionKind, res: { fakes: string[]; lists: string[] }): ArgValue | undefined {
  switch (kind.type) {
    case "flag":
      return undefined;
    case "ports":
      return "443";
    case "int":
      return String(Math.max(kind.min, Math.min(kind.max, 1)));
    case "enum":
      return kind.values[0];
    case "fake":
      return res.fakes[0] ? { fake: res.fakes[0] } : "0x00";
    case "list":
      return { list: res.lists.find((l) => l === "list-general.txt") ?? res.lists[0] ?? "list-general.txt" };
    case "domains":
      return "discord.com";
    case "cidrs":
      return "203.0.113.113/32";
    case "mods":
      return kind.keys[0];
    case "markers":
      return "1";
    case "cutoff":
      return "n3";
    case "autoTtl":
      return "2";
  }
}

export function issuesFor(issues: Issue[], path: string, exact = true): Issue[] {
  return issues.filter((i) => (exact ? i.path === path : i.path === path || i.path.startsWith(path + ".") || i.path.startsWith(path + "[")));
}

export function clone<T>(v: T): T {
  return JSON.parse(JSON.stringify(v)) as T;
}

export function moveItem<T>(arr: T[], from: number, to: number): T[] {
  if (to < 0 || to >= arr.length) return arr;
  const out = arr.slice();
  const [x] = out.splice(from, 1);
  out.splice(to, 0, x);
  return out;
}

export function sameStrategy(a: Strategy, b: Strategy): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export function argLabel(a: Arg, specs: Map<string, OptionSpec>, lang: "ru" | "en"): string {
  const s = specs.get(a.opt);
  if (!s) return a.opt;
  return lang === "en" ? s.titleEn : s.titleRu;
}
