// Builds resources/hints/providers.json from the users' own reports — the
// issues created with the form .github/ISSUE_TEMPLATE/strategy.yml. Nobody
// picks strategies by hand: a strategy is listed for a provider when at
// least MIN_REPORTERS different people reported that Discord opens with it.
// Dependency-free (Node only); it reads a file and never touches the network.
//
//   gh api --paginate "repos/<owner>/<repo>/issues?labels=strategy&state=all&per_page=100" > issues.json
//   node scripts/hints/aggregate.mjs --issues issues.json            # show what would change
//   node scripts/hints/aggregate.mjs --issues issues.json --write    # update the table
//
// Rules:
// - one person = one voice per provider: only the latest report of an author
//   for a provider counts, so a report can be corrected by a new one;
// - a report counts only if the provider and the strategy are the form's own
//   options and "Discord открывается" is ticked; free text is never used;
// - an issue closed as "not planned" does not count (the owner's way to
//   reject spam or a mistaken report);
// - at most MAX_STRATEGIES per provider, the most reported first.
// The table only orders the candidates of Auto; every attempt is still
// verified on the user's own connection, so a wrong hint costs one attempt.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const MIN_REPORTERS = 2;
export const MAX_STRATEGIES = 5;
const WORKS = "Discord открывается";

/** `general (FAKE TLS AUTO ALT)` → `flowseal-general-fake-tls-auto-alt` (the ids of bridge_core::catalog). */
export const strategyId = (name) => "flowseal-" + name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");

/** Sections of an issue-form body: `### Label` followed by the answer. A label that occurs twice is dropped: the form never produces that. */
export function sections(body) {
  const out = {};
  const twice = new Set();
  const parts = String(body ?? "").replace(/\r\n/g, "\n").split(/^### +/m).slice(1);
  for (const p of parts) {
    const nl = p.indexOf("\n");
    if (nl < 0) continue;
    const label = p.slice(0, nl).trim();
    if (label in out) twice.add(label);
    out[label] = p.slice(nl + 1).trim();
  }
  for (const label of twice) delete out[label];
  return out;
}

/** A report, or null when the issue is not a countable one. */
export function parseReport(issue, providers, strategyNames) {
  if (!issue || issue.pull_request) return null;
  if (!(issue.labels ?? []).some((l) => (typeof l === "string" ? l : l?.name) === "strategy")) return null;
  if (issue.state === "closed" && issue.state_reason === "not_planned") return null;
  const author = issue.user?.login;
  if (typeof author !== "string" || !author || issue.user?.type === "Bot") return null;
  const s = sections(issue.body);
  const provider = providers.find((p) => p.name.ru === s["Провайдер"]);
  const name = s["Стратегия"];
  if (!provider || !strategyNames.includes(name)) return null;
  const works = (s["Что работает"] ?? "").split("\n").some((l) => /^- \[[xX]\] /.test(l) && l.includes(WORKS));
  return { author, provider: provider.id, strategy: strategyId(name), works, at: String(issue.created_at ?? ""), number: issue.number };
}

/** New table from the current one (names and order of providers are kept) and the issues. */
export function aggregate(current, issues, strategyNames, today) {
  const order = strategyNames.map(strategyId);
  const latest = new Map(); // author + provider → their latest report
  for (const issue of issues) {
    const r = parseReport(issue, current.providers, strategyNames);
    if (!r) continue;
    const key = `${r.author.toLowerCase()}\n${r.provider}`;
    const prev = latest.get(key);
    if (!prev || r.at > prev.at || (r.at === prev.at && r.number > prev.number)) latest.set(key, r);
  }
  const counts = new Map(); // provider → strategy → reporters
  for (const r of latest.values()) {
    if (!r.works) continue;
    const m = counts.get(r.provider) ?? new Map();
    m.set(r.strategy, (m.get(r.strategy) ?? 0) + 1);
    counts.set(r.provider, m);
  }
  const providers = current.providers.map((p) => {
    const m = counts.get(p.id) ?? new Map();
    const strategies = [...m.entries()]
      .filter(([, n]) => n >= MIN_REPORTERS)
      .sort((a, b) => b[1] - a[1] || order.indexOf(a[0]) - order.indexOf(b[0]))
      .slice(0, MAX_STRATEGIES)
      .map(([id]) => id);
    return { id: p.id, name: p.name, strategies };
  });
  const changed = JSON.stringify(providers.map((p) => p.strategies)) !== JSON.stringify(current.providers.map((p) => p.strategies ?? []));
  const report = providers.map((p) => ({ id: p.id, reports: [...(counts.get(p.id) ?? new Map()).values()].reduce((a, b) => a + b, 0), strategies: p.strategies })).filter((p) => p.reports);
  return { table: { schemaVersion: current.schemaVersion, updated: changed ? today : current.updated, providers }, changed, counted: latest.size, report };
}

/** The table file exactly as it is stored: one provider per line. */
export function render(table) {
  const q = JSON.stringify;
  const lines = table.providers.map((p) => `    { "id": ${q(p.id)}, "name": { "ru": ${q(p.name.ru)}, "en": ${q(p.name.en)} }, "strategies": [${p.strategies.map(q).join(", ")}] }`);
  return `{\n  "schemaVersion": ${table.schemaVersion},\n  "updated": ${JSON.stringify(table.updated)},\n  "providers": [\n${lines.join(",\n")}\n  ]\n}\n`;
}

/** `gh api --paginate` prints one JSON array per page, one after another. */
export function parseIssues(text) {
  const t = text.trim();
  if (!t) return [];
  try {
    return [JSON.parse(t)].flat(2);
  } catch {
    return JSON.parse(`[${t.replace(/\]\s*\[/g, "],[")}]`).flat(2);
  }
}

/** Names of the built-in strategies, in the order of the Flowseal release. */
export function strategyNamesFrom(dir) {
  const key = (n) => n.split(/(\d+)/).map((x) => (/^\d+$/.test(x) ? Number(x) : x));
  const cmp = (a, b) => {
    const [x, y] = [key(a), key(b)];
    for (let i = 0; i < Math.max(x.length, y.length); i++) {
      if (x[i] === y[i]) continue;
      if (x[i] === undefined) return -1;
      if (y[i] === undefined) return 1;
      return typeof x[i] === typeof y[i] ? (x[i] < y[i] ? -1 : 1) : typeof x[i] === "number" ? -1 : 1;
    }
    return 0;
  };
  return fs.readdirSync(dir).filter((f) => f.endsWith(".bat")).map((f) => f.slice(0, -4)).sort(cmp);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const opt = (k, d) => (args.includes(k) ? args[args.indexOf(k) + 1] : d);
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
  const hintsPath = opt("--hints", path.join(root, "resources/hints/providers.json"));
  const issuesPath = opt("--issues");
  if (!issuesPath) {
    console.error("usage: node scripts/hints/aggregate.mjs --issues <issues.json> [--hints <providers.json>] [--out <file>] [--write]");
    process.exit(2);
  }
  const names = strategyNamesFrom(path.join(root, "crates/bridge-core/tests/fixtures/flowseal-1.10.4"));
  const current = JSON.parse(fs.readFileSync(hintsPath, "utf8"));
  const r = aggregate(current, parseIssues(fs.readFileSync(issuesPath, "utf8")), names, new Date().toISOString().slice(0, 10));
  console.log(`reports counted: ${r.counted} (one per person and provider)`);
  for (const p of r.report) console.log(`  ${p.id}: ${p.reports} working report(s) → ${p.strategies.length ? p.strategies.join(", ") : `none yet (needs ${MIN_REPORTERS} people per strategy)`}`);
  console.log(r.changed ? "the table changes" : "the table does not change");
  const out = opt("--out", args.includes("--write") ? hintsPath : undefined);
  if (out && (r.changed || out !== hintsPath)) fs.writeFileSync(out, render(r.table));
  if (args.includes("--github-output") && process.env.GITHUB_OUTPUT) fs.appendFileSync(process.env.GITHUB_OUTPUT, `changed=${r.changed}\n`);
}
