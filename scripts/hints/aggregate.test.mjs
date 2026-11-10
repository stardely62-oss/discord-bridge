// node --test scripts/hints/aggregate.test.mjs
import assert from "node:assert/strict";
import fs from "node:fs";
import { test } from "node:test";
import yaml from "js-yaml";
import { aggregate, MAX_STRATEGIES, parseIssues, parseReport, render, sections, strategyId, strategyNamesFrom } from "./aggregate.mjs";

const hintsFile = "resources/hints/providers.json";
const current = JSON.parse(fs.readFileSync(hintsFile, "utf8"));
const names = strategyNamesFrom("crates/bridge-core/tests/fixtures/flowseal-1.10.4");
const empty = { ...current, providers: current.providers.map((p) => ({ ...p, strategies: [] })) };

let n = 0;
/** An issue exactly as the form produces it. */
function issue(author, provider, strategy, { works = true, at, extra = {} } = {}) {
  n += 1;
  const body = ["### Версия Bridge", "", "0.2.0", "", "### Windows", "", "Windows 11 x64", "", "### Провайдер", "", provider, "", "### Регион и уточнения", "", "_No response_", "", "### Стратегия", "", strategy, "", "### Режим IP и игровой фильтр (если меняли)", "", "Не менял(а)", "", "### Что работает", "", `- [${works ? "x" : " "}] Discord открывается (текст, картинки)`, "- [x] Голосовые каналы Discord", "- [ ] YouTube (видео грузится)", "", "### Что не работает и подробности", "", "_No response_"].join("\n");
  return { number: n, state: "open", labels: [{ name: "strategy" }], user: { login: author, type: "User" }, created_at: at ?? `2026-11-${String(10 + (n % 15)).padStart(2, "0")}T00:00:00Z`, body, ...extra };
}
const of = (r, id) => r.table.providers.find((p) => p.id === id).strategies;

test("the form, the table and the strategy list name the same things", () => {
  const form = yaml.load(fs.readFileSync(".github/ISSUE_TEMPLATE/strategy.yml", "utf8"));
  const field = (id) => form.body.find((b) => b.id === id);
  assert.deepEqual(field("provider").attributes.options.slice(0, -1), current.providers.map((p) => p.name.ru));
  assert.deepEqual(field("strategy").attributes.options.slice(0, -1), names);
  assert.equal(names.length, 22);
  // The labels the parser reads.
  assert.equal(field("provider").attributes.label, "Провайдер");
  assert.equal(field("strategy").attributes.label, "Стратегия");
  assert.equal(field("works").attributes.label, "Что работает");
  assert.ok(field("works").attributes.options[0].label.startsWith("Discord открывается"));
  assert.ok(form.labels.includes("strategy"));
  assert.equal(strategyId("general (FAKE TLS AUTO ALT2)"), "flowseal-general-fake-tls-auto-alt2");
  assert.equal(strategyId("general"), "flowseal-general");
});

test("a strategy is listed only when several different people report it", () => {
  const one = aggregate(empty, [issue("ann", "Ростелеком", "general (ALT3)")], names, "2026-12-01");
  assert.deepEqual(of(one, "rostelecom"), []);
  assert.equal(one.changed, false);
  assert.equal(one.table.updated, empty.updated);

  const two = aggregate(empty, [issue("ann", "Ростелеком", "general (ALT3)"), issue("bob", "Ростелеком", "general (ALT3)"), issue("bob", "ТТК", "general (ALT3)")], names, "2026-12-01");
  assert.deepEqual(of(two, "rostelecom"), ["flowseal-general-alt3"]);
  assert.deepEqual(of(two, "ttk"), []);
  assert.equal(two.changed, true);
  assert.equal(two.table.updated, "2026-12-01");
});

test("one person has one voice per provider, and the latest report wins", () => {
  const same = Array.from({ length: 5 }, () => issue("ann", "МГТС", "general (ALT)"));
  assert.deepEqual(of(aggregate(empty, same, names, "d"), "mgts"), []);
  const changedMind = [issue("ann", "МГТС", "general (ALT)", { at: "2026-11-01T00:00:00Z" }), issue("bob", "МГТС", "general (ALT)", { at: "2026-11-01T00:00:00Z" }), issue("Ann", "МГТС", "general (ALT2)", { at: "2026-11-05T00:00:00Z" })];
  assert.deepEqual(of(aggregate(empty, changedMind, names, "d"), "mgts"), []);
  // "Does not work any more" withdraws an earlier voice.
  const withdrawn = [issue("ann", "МГТС", "general (ALT)", { at: "2026-11-01T00:00:00Z" }), issue("bob", "МГТС", "general (ALT)", { at: "2026-11-01T00:00:00Z" }), issue("bob", "МГТС", "general (ALT)", { works: false, at: "2026-11-09T00:00:00Z" })];
  assert.deepEqual(of(aggregate(empty, withdrawn, names, "d"), "mgts"), []);
});

test("the most reported come first, the list is capped", () => {
  const issues = [];
  names.slice(0, 7).forEach((s, i) => {
    for (let k = 0; k < 2 + (i === 3 ? 3 : 0); k++) issues.push(issue(`u${i}-${k}`, "Yota", s));
  });
  const got = of(aggregate(empty, issues, names, "d"), "yota");
  assert.equal(got.length, MAX_STRATEGIES);
  assert.equal(got[0], strategyId(names[3]));
  assert.deepEqual(got.slice(1), names.slice(0, 3).concat(names[4]).map(strategyId));
});

test("only the form's own options count; free text and rejected issues do not", () => {
  const ok = issue("ann", "Дом.ру", "general (ALT)");
  const p = (i) => parseReport(i, current.providers, names);
  assert.equal(p(ok).provider, "domru");
  assert.equal(p(issue("a", "Другой (напишу ниже)", "general (ALT)")), null);
  assert.equal(p(issue("a", "Дом.ру", "Своя или изменённая")), null);
  assert.equal(p(issue("a", "Дом.ру\n### Стратегия\n\ngeneral (ALT2)", "general (ALT)")), null);
  assert.equal(p(issue("a", "Дом.ру", "../../etc")), null);
  assert.equal(p(issue("a", "Дом.ру", "general (ALT)", { extra: { labels: [] } })), null);
  assert.equal(p(issue("a", "Дом.ру", "general (ALT)", { extra: { pull_request: {} } })), null);
  assert.equal(p(issue("a", "Дом.ру", "general (ALT)", { extra: { state: "closed", state_reason: "not_planned" } })), null);
  assert.equal(p(issue("a", "Дом.ру", "general (ALT)", { extra: { state: "closed", state_reason: "completed" } })).provider, "domru");
  assert.equal(p(issue("a", "Дом.ру", "general (ALT)", { extra: { user: { login: "x[bot]", type: "Bot" } } })), null);
  assert.equal(p({ ...ok, body: null }), null);
  assert.equal(p(issue("a", "Дом.ру", "general (ALT)", { works: false })).works, false);
  assert.deepEqual(sections("x\n### A\n\n1\n\n### B\r\n\r\n2"), { A: "1", B: "2" });
});

test("the table round-trips byte for byte and keeps every provider", () => {
  assert.equal(render(current), fs.readFileSync(hintsFile, "utf8"));
  const r = aggregate(empty, [issue("ann", "T2 (Tele2)", "general"), issue("bob", "T2 (Tele2)", "general")], names, "2026-12-01");
  assert.deepEqual(r.table.providers.map((p) => p.id), current.providers.map((p) => p.id));
  assert.deepEqual(JSON.parse(render(r.table)), r.table);
  assert.equal(parseIssues('[{"a":1}]\n[{"a":2}]').length, 2);
  assert.equal(parseIssues("[]").length, 0);
  assert.equal(parseIssues('[[{"a":1}],[{"a":2}]]').length, 2);
});
