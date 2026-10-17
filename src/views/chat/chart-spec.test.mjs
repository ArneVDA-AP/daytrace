// Run: node --experimental-strip-types --test src/views/chat/chart-spec.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  CHART_LIMITS,
  chartBody,
  chartFence,
  chartSummary,
  chartTable,
  chartsAsText,
  contrastRatio,
  jsonDepth,
  luminanceOf,
  markColor,
  markColors,
  niceScale,
  parseChart,
  sanitizeHex,
} from "./chart-spec.ts";

// The six examples of Dayflow's prompt (Core/AI/ChatPromptBuilder.swift), as the model writes them.
const EXAMPLES = {
  bar: '{ "title": "Time by activity (today)", "x": ["Research", "YouTube"], "y": [45, 20], "color": "#F96E00" }',
  line: '{ "title": "Focus time by day", "x": ["Mon", "Tue", "Wed"], "y": [2.5, 3.0, 1.8], "color": "#1F6FEB" }',
  stacked_bar:
    '{ "title": "Work vs Personal by day", "x": ["Mon", "Tue"], "series": [{ "name": "Work", "values": [2.5, 3.1], "color": "#1F6FEB" }, { "name": "Personal", "values": [1.2, 0.8], "color": "#F96E00" }] }',
  donut: '{ "title": "Time split (today)", "labels": ["Work", "Personal"], "values": [3.0, 5.7], "colors": ["#1F6FEB", "#F96E00"] }',
  heatmap:
    '{ "title": "Focus by daypart", "x": ["Mon", "Tue", "Wed"], "y": ["Morning", "Afternoon", "Evening"], "values": [[1.2, 0.8, 1.5], [2.0, 1.6, 1.1], [0.7, 1.0, 0.9]], "color": "#1F6FEB" }',
  gantt:
    '{ "title": "Focus blocks (today)", "items": [{ "label": "Research", "start": 9.0, "end": 10.5, "color": "#1F6FEB" }, { "label": "Break", "start": 10.5, "end": 11.0, "color": "#F96E00" }] }',
};

test("Dayflow's six prompt examples parse", () => {
  for (const [type, body] of Object.entries(EXAMPLES)) {
    const r = parseChart(type, body);
    assert.ok(r.ok, `${type}: ${r.ok ? "" : r.reason}`);
    assert.equal(r.spec.type, type);
    assert.ok(chartSummary(r.spec).length > 10);
    assert.ok(chartTable(r.spec).rows.length > 0);
  }
  const bar = parseChart("bar", EXAMPLES.bar);
  assert.deepEqual(bar.ok && bar.spec, { type: "bar", title: "Time by activity (today)", labels: ["Research", "YouTube"], values: [45, 20], color: "#F96E00" });
});

test("the fence info string", () => {
  assert.deepEqual(chartFence("chart type=bar"), { type: "bar", inline: "" });
  assert.deepEqual(chartFence("chart  type = Stacked_Bar "), { type: "stacked_bar", inline: "" });
  assert.deepEqual(chartFence("CHART type=line {\"x\":[]}"), { type: "line", inline: '{"x":[]}' });
  assert.deepEqual(chartFence("chart"), { type: null, inline: "" });
  assert.deepEqual(chartFence("chart bar"), { type: null, inline: "bar" });
  assert.equal(chartFence("charts"), null);
  assert.equal(chartFence("chartjs"), null);
  assert.equal(chartFence("json"), null);
  assert.equal(chartFence(""), null);
});

test("pie is another name for donut; unknown and missing types are refused", () => {
  const pie = parseChart("pie", EXAMPLES.donut);
  assert.ok(pie.ok && pie.spec.type === "donut");
  const r = parseChart("scatter", EXAMPLES.bar);
  assert.ok(!r.ok && /not a chart type/.test(r.reason));
  const none = parseChart(null, EXAMPLES.bar);
  assert.ok(!none.ok && /no chart type/.test(none.reason));
});

const reason = (type, body) => {
  const r = parseChart(type, body);
  assert.ok(!r.ok, `expected a refusal for ${body.slice(0, 80)}`);
  return r.reason;
};

test("data that does not add up rejects the whole block", () => {
  assert.match(reason("bar", '{"title":"t","x":["a","b"],"y":[1]}'), /"y" has 1 value but "x" has 2/);
  assert.match(reason("bar", '{"title":"t","x":["a"],"y":["1"]}'), /must be a number/);
  assert.match(reason("bar", '{"title":"t","x":[1],"y":[1]}'), /must be text/);
  assert.match(reason("bar", '{"title":"t","x":["  "],"y":[1]}'), /must not be empty/);
  assert.match(reason("bar", '{"title":"t","x":[],"y":[]}'), /"x" is empty/);
  assert.match(reason("bar", '{"title":"t","y":[1]}'), /"x" is missing/);
  assert.match(reason("bar", '[1,2]'), /JSON object/);
  assert.match(reason("bar", "{title: 't'}"), /not valid JSON/);
  assert.match(reason("bar", '{"title":"t","x":["a"],"y":[1],}'), /not valid JSON/);
  assert.match(reason("bar", ""), /no chart data/);
  assert.match(reason("stacked_bar", '{"title":"t","x":["a","b"],"series":[{"name":"W","values":[1]}]}'), /"values" has 1 value but "x" has 2/);
  assert.match(reason("stacked_bar", '{"title":"t","x":["a"],"series":[{"name":"W","values":[-1]}]}'), /can’t be negative/);
  assert.match(reason("stacked_bar", '{"title":"t","x":["a"],"series":[{"values":[1]}]}'), /"name" of every series must be text/);
  assert.match(reason("donut", '{"title":"t","labels":["a"],"values":[0]}'), /All values are 0/);
  assert.match(reason("heatmap", '{"title":"t","x":["a","b"],"y":["r"],"values":[[1]]}'), /row of "values" has 1 values but "x" has 2/);
  assert.match(reason("heatmap", '{"title":"t","x":["a"],"y":["r","s"],"values":[[1]]}'), /one row per y label/);
  assert.match(reason("gantt", '{"title":"t","items":[{"label":"a","start":2,"end":2}]}'), /start before its end/);
  assert.match(reason("gantt", '{"title":"t","items":[{"label":"a","start":1}]}'), /"end" must be a number/);
});

test("colors are optional and a bad one is ignored", () => {
  assert.equal(sanitizeHex("F96E00"), "#F96E00");
  assert.equal(sanitizeHex(" #f96e00 "), "#F96E00");
  assert.equal(sanitizeHex("#80F96E00"), "#F96E00", "AARRGGBB like Dayflow's NSColor(hex:)");
  assert.equal(sanitizeHex("red"), null);
  assert.equal(sanitizeHex("#FFF"), null);
  assert.equal(sanitizeHex("url(javascript:alert(1))"), null);
  assert.equal(sanitizeHex(12), null);
  const r = parseChart("bar", '{"title":"t","x":["a"],"y":[1],"color":"var(--x); fill: red"}');
  assert.ok(r.ok && r.spec.color === null);
  const d = parseChart("donut", '{"title":"t","labels":["a","b"],"values":[1,2],"colors":["#111111"]}');
  assert.deepEqual(d.ok && d.spec.colors, [null, null], "a color list that does not match the labels is dropped (Dayflow)");
});

test("limits: points, series, slices, cells, strings, numbers, nesting, size", () => {
  const xs = (n) => JSON.stringify(Array.from({ length: n }, (_, i) => `d${i}`));
  const ys = (n) => JSON.stringify(Array.from({ length: n }, (_, i) => i));
  assert.ok(parseChart("bar", `{"title":"t","x":${xs(CHART_LIMITS.points)},"y":${ys(CHART_LIMITS.points)}}`).ok);
  assert.match(reason("bar", `{"title":"t","x":${xs(CHART_LIMITS.points + 1)},"y":${ys(CHART_LIMITS.points + 1)}}`), /at most 60 bars/);
  const series = (n) => JSON.stringify(Array.from({ length: n }, (_, i) => ({ name: `s${i}`, values: [1] })));
  assert.match(reason("stacked_bar", `{"title":"t","x":["a"],"series":${series(CHART_LIMITS.series + 1)}}`), /at most 12 series/);
  assert.match(reason("donut", `{"title":"t","labels":${xs(17)},"values":${ys(17)}}`), /at most 16 slices/);
  const row = (n) => JSON.stringify(Array.from({ length: n }, () => 1));
  const rows = (r, c) => `[${Array.from({ length: r }, () => row(c)).join(",")}]`;
  assert.ok(parseChart("heatmap", `{"title":"t","x":${xs(24)},"y":${xs(7)},"values":${rows(7, 24)}}`).ok);
  assert.match(reason("heatmap", `{"title":"t","x":${xs(40)},"y":${xs(40)},"values":${rows(40, 40)}}`), /at most 1,000 cells/);
  assert.match(reason("bar", `{"title":"${"a".repeat(121)}","x":["a"],"y":[1]}`), /title is longer than 120/);
  assert.match(reason("bar", `{"title":"t","x":["${"a".repeat(61)}"],"y":[1]}`), /at most 60 characters/);
  assert.match(reason("bar", '{"title":"t","x":["a"],"y":[1e10]}'), /between −1 billion and 1 billion/);
  assert.match(reason("bar", '{"title":"t","x":["a"],"y":[1e999]}'), /must be a number/, "Infinity from JSON is not a number to draw");
  assert.match(reason("bar", '{"title":"t","x":[["a"]],"y":[[[[1]]]]}'), /nested deeper/);
  assert.match(reason("bar", `{"title":"t","x":["a"],"y":[1],"pad":"${" ".repeat(CHART_LIMITS.body)}"}`), /longer than 16,000 characters/);
  // Whitespace in labels collapses (a newline in an SVG label would not show).
  const r = parseChart("bar", '{"title":"Line\\none","x":["a\\n  b"],"y":[1]}');
  assert.ok(r.ok && r.spec.title === "Line one" && r.spec.labels[0] === "a b");
});

test("jsonDepth skips strings and escapes", () => {
  assert.equal(jsonDepth('{"a":[1,[2]]}'), 3);
  assert.equal(jsonDepth('{"a":"[[[[[[\\"[[[["}'), 1);
  assert.equal(jsonDepth("[".repeat(50000)), 50000);
});

test("hostile blocks are refused quickly", () => {
  const N = 200_000;
  const cases = [
    ["deep arrays", "[".repeat(N)],
    ["deep objects", '{"a":'.repeat(N / 5)],
    ["long string", `{"title":"${"x".repeat(N)}"}`],
    ["many escapes", `{"title":"${"\\\\".repeat(N / 2)}"}`],
    ["huge number list", `{"title":"t","x":["a"],"y":[${"1,".repeat(N / 2)}1]}`],
    ["unterminated", '{"title":"' + "a".repeat(N)],
    ["just under the size limit, many points", `{"title":"t","x":[${'"a",'.repeat(3000)}"a"],"y":[${"1,".repeat(3000)}1]}`],
  ];
  for (const [name, body] of cases) {
    const t0 = performance.now();
    const r = parseChart("bar", body);
    const ms = performance.now() - t0;
    assert.ok(!r.ok, name);
    assert.ok(ms < 50, `${name} took ${ms.toFixed(1)} ms`);
  }
});

test("chart blocks in answer text: body, unclosed, closing on the data line", () => {
  assert.deepEqual(chartBody(["{", '"a":1', "}", "```", "after"], 0, ""), { body: '{\n"a":1\n}', next: 4, closed: true });
  assert.deepEqual(chartBody(['{"a":1}```', "after"], 0, ""), { body: '{"a":1}', next: 1, closed: true });
  assert.deepEqual(chartBody(["{"], 0, ""), { body: "{", next: 1, closed: false });
  assert.deepEqual(chartBody([], 0, '{"a":1}```'), { body: '{"a":1}', next: 0, closed: true });
});

test("Copy answer writes a drawable chart as its table and keeps the rest", () => {
  const text = ["Pulled today's cards.", "", "```chart type=bar", EXAMPLES.bar, "```", "", "```chart type=bar", "{bad", "```", "Done."].join("\n");
  const out = chartsAsText(text);
  assert.ok(out.includes("**Time by activity (today)**\n\n| Label | Value |\n| --- | --- |\n| Research | 45 |\n| YouTube | 20 |"), out);
  assert.ok(out.includes("```chart type=bar\n{bad\n```"), "a block that can't be drawn stays raw");
  assert.ok(out.startsWith("Pulled today's cards.") && out.endsWith("Done."));
  const example = "```markdown\n```chart type=bar\n```";
  assert.equal(chartsAsText(example), example, "a chart inside another code block is left alone");
  assert.equal(chartsAsText("no fences"), "no fences");
});

test("niceScale", () => {
  assert.deepEqual(niceScale(0, 45).ticks, [0, 10, 20, 30, 40, 50]);
  assert.deepEqual(niceScale(0, 3.1).ticks, [0, 1, 2, 3, 4]);
  assert.deepEqual(niceScale(0, 0).ticks, [0, 0.2, 0.4, 0.6, 0.8, 1]);
  assert.deepEqual(niceScale(-20, 45).ticks, [-20, 0, 20, 40, 60]);
  assert.deepEqual(niceScale(0, 70).ticks, [0, 20, 40, 60, 80]);
  assert.deepEqual(niceScale(0, 120).ticks, [0, 20, 40, 60, 80, 100, 120]);
  assert.deepEqual(niceScale(0, 7).ticks, [0, 2, 4, 6, 8]);
  assert.deepEqual(niceScale(9, 11).ticks, [9, 9.5, 10, 10.5, 11]);
  const big = niceScale(0, 1e9);
  assert.ok(big.ticks.length <= 21 && big.hi >= 1e9);
  const same = niceScale(5, 5);
  assert.ok(same.lo < 5 && same.hi > 5);
});

test("model colors get 3:1 on the content layer in both themes", () => {
  const white = 1;
  for (const hex of ["#F96E00", "#1F6FEB", "#FFE0B0", "#FFFFFF", "#000000", "#00FF00", "#101020", "#B0E0FF", "#8E24AA", "#777777"]) {
    const { light, dark, darkSurfaceY } = markColors(hex);
    assert.ok(contrastRatio(luminanceOf(light), white) >= 3, `${hex} light ${light} ${contrastRatio(luminanceOf(light), white).toFixed(2)}`);
    assert.ok(contrastRatio(luminanceOf(dark), darkSurfaceY) >= 3, `${hex} dark ${dark} ${contrastRatio(luminanceOf(dark), darkSurfaceY).toFixed(2)}`);
  }
  // A color that already has the contrast in both themes is used as given.
  const mid = markColors("#1F6FEB");
  assert.equal(mid.light, "#1F6FEB");
  assert.equal(markColor("#000000").startsWith("light-dark(#000000, #"), true);
});
