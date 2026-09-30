// Findr query console.
//
// Every calculation here comes from ./mirror.js, which mirrors the Rust
// library. This file owns the controls and the rendering only.
import { SAMPLES, execute, cliCommand, formatNumber, render } from "./mirror.js";

const { el, $, $$, toast } = A;

const state = {
  sample: "access",
  command: "stats",
  expression: "status=500",
  field: "latency",
  by: "country",
  n: 5,
  following: null,
  followIndex: 0,
};

const sample = () => SAMPLES.find((entry) => entry.id === state.sample) || SAMPLES[0];

/* ---------------------------------------------------------------- controls */

function renderSampleTabs() {
  $("#sample-tabs").replaceChildren(...SAMPLES.map((entry) =>
    el("button", {
      class: "chip" + (entry.id === state.sample ? " active" : ""),
      onclick: () => {
        state.sample = entry.id;
        stopFollowing();
        renderSampleTabs();
        renderSource();
        renderQueryFields();
        runQuery();
      },
    }, entry.label)));
}

function renderCommandTabs() {
  const commands = [["stats", "stats"], ["top", "top"], ["filter", "filter"], ["tail", "tail"]];
  $("#command-tabs").replaceChildren(...commands.map(([id, label]) =>
    el("button", {
      class: "tab" + (state.command === id ? " active" : ""),
      "data-cmd": id,
      onclick: () => {
        state.command = id;
        stopFollowing();
        renderCommandTabs();
        renderQueryFields();
        runQuery();
      },
    }, label)));
}

function renderQueryFields() {
  const host = $("#query-fields");
  const field = (id, label, value, hint) => {
    const wrapper = el("label", { class: "field" }, label,
      el("input", { class: "input", id, value, spellcheck: "false" }));
    if (hint) wrapper.append(el("span", { class: "query-hint", text: hint }));
    return wrapper;
  };

  if (state.command === "stats") {
    host.replaceChildren(field("field", "numeric field", state.field, "Comma separate fields. Empty auto-detects every numeric field."));
    $("#field").addEventListener("input", (event) => {
      state.field = event.target.value;
      updateCli();
    });
  } else if (state.command === "top") {
    host.replaceChildren(el("div", { class: "field-row" },
      field("by", "group by", state.by),
      field("n", "rows", String(state.n))));
    $("#by").addEventListener("input", (event) => {
      state.by = event.target.value;
      updateCli();
    });
    $("#n").addEventListener("input", (event) => {
      state.n = Math.max(1, Number(event.target.value) || 1);
      updateCli();
    });
  } else if (state.command === "filter") {
    host.replaceChildren(field("expr", "expression", state.expression, "status=500, latency>500, msg contains timeout. Commas combine with AND."));
    $("#expr").addEventListener("input", (event) => {
      state.expression = event.target.value;
      updateCli();
    });
  } else {
    host.replaceChildren(el("div", { class: "query-hint" },
      "tail prints the last lines of the file. The Rust binary also supports --follow for appended lines; the Follow button below replays that behaviour in the mirror."));
  }
  updateCli();
}

function updateCli() {
  $("#cli-command").textContent = cliCommand(state);
}

function renderSource() {
  const entry = sample();
  $("#format-badge").textContent = entry.format;
  $("#source-meta").textContent = `examples/${entry.label}`;
  $("#source-format").textContent = entry.id === "service" ? "key=value" : "json";
  const lines = entry.text.split("\n").filter((line) => line.trim()).slice(0, 8);
  $("#source-preview").replaceChildren(...lines.map((line, index) =>
    el("div", { class: "src-line fmt-" + (entry.id === "service" ? "keyvalue" : "json") },
      el("span", { class: "ln", text: String(index + 1) }),
      el("span", { class: "lc", text: line }))));
}

/* ----------------------------------------------------------------- results */

function statTile(label, value, small = false) {
  return el("div", { class: "result-stat" },
    el("div", { class: "label", text: label }),
    el("div", { class: "value" + (small ? " small" : ""), text: value }));
}

function renderSummary(result) {
  if (result.kind === "stats") {
    const first = result.fields[0]?.summary;
    $("#result-summary").replaceChildren(
      statTile("records", String(result.records)),
      statTile("matched", String(result.matched)),
      statTile("malformed", String(result.malformed)),
      statTile("p95", first ? formatNumber(first.p95) + " ms" : "n/a", true),
    );
  } else if (result.kind === "top") {
    $("#result-summary").replaceChildren(
      statTile("records", String(result.records)),
      statTile("matched", String(result.matched)),
      statTile("present", String(result.present)),
      statTile("top value", result.entries[0]?.key || "n/a", true),
    );
  } else if (result.kind === "filter") {
    const rate = result.records ? (result.matched / result.records) * 100 : 0;
    $("#result-summary").replaceChildren(
      statTile("records", String(result.records)),
      statTile("matched", String(result.matched)),
      statTile("malformed", String(result.malformed.length)),
      statTile("match rate", formatNumber(Math.round(rate)) + "%", true),
    );
  } else {
    $("#result-summary").replaceChildren(
      statTile("lines shown", String(result.lines.length)),
      statTile("records", String(result.records)),
      statTile("malformed", String(result.malformed.length)),
      statTile("mode", state.following ? "follow" : "static", true),
    );
  }
}

function renderStats(result) {
  if (!result.fields.length) {
    return el("div", { class: "empty-state" }, "No numeric field matched. Try --field latency or a different sample.");
  }
  const table = A.dataTable({
    columns: [
      { key: "field", label: "field", render: (row) => el("span", { class: "mono", text: row.field }) },
      { key: "count", label: "count", align: "right", render: (row) => row.summary.count },
      { key: "min", label: "min", align: "right", render: (row) => formatNumber(row.summary.min) },
      { key: "mean", label: "mean", align: "right", render: (row) => formatNumber(row.summary.mean) },
      { key: "p50", label: "p50", align: "right", render: (row) => formatNumber(row.summary.p50) },
      { key: "p95", label: "p95", align: "right", render: (row) => formatNumber(row.summary.p95) },
      { key: "p99", label: "p99", align: "right", render: (row) => formatNumber(row.summary.p99) },
      { key: "max", label: "max", align: "right", render: (row) => formatNumber(row.summary.max) },
    ],
    rows: result.fields,
  });
  return el("div", {}, table, el("div", { class: "explain" },
    el("div", { class: "explain-row" }, el("span", { class: "k", text: "percentiles" }), el("span", { text: "Sorted values, interpolated between ranks. p50 is the median." })),
    el("div", { class: "explain-row" }, el("span", { class: "k", text: "numeric fields" }), el("span", { text: "A field qualifies only when every present value is a number." })),
    el("div", { class: "explain-row" }, el("span", { class: "k", text: "malformed" }), el("span", { text: "Bad lines are skipped with a warning; --strict turns that into an error." }))));
}

function renderTop(result) {
  if (!result.entries.length) {
    return el("div", { class: "empty-state" }, `No values for "${result.field}" in the matching records.`);
  }
  const top = result.entries[0].count;
  return A.dataTable({
    columns: [
      { key: "rank", label: "rank", width: "50px", render: (row) => String(result.entries.indexOf(row) + 1) },
      { key: "key", label: result.field, render: (row) => el("div", { class: "bar-row" },
        el("span", { class: "mono", text: row.key, style: "min-width:36px" }),
        el("div", { class: "bar-track" }, el("div", { class: "bar-fill", style: `width:${Math.round((row.count / top) * 100)}%` }))) },
      { key: "count", label: "count", align: "right", render: (row) => String(row.count) },
      { key: "share", label: "share", align: "right", render: (row) => formatNumber(Math.round((row.count / result.present) * 1000) / 10) + "%" },
    ],
    rows: result.entries,
  });
}

function renderFilter(result) {
  if (!result.lines.length) {
    return el("div", { class: "empty-state" }, "No records matched. Try status=500 or latency>500.");
  }
  const rows = result.lines.slice(0, 12).map((line) => ({ ...line.record, __line: line.lineNo }));
  const table = A.dataTable({
    columns: [
      { key: "__line", label: "line", render: (row) => el("span", { class: "mono", text: `#${row.__line}` }) },
      { key: "status", label: "status", align: "right", render: (row) => el("span", { class: "mono", text: render(row.status) }) },
      { key: "latency", label: "latency", align: "right", render: (row) => el("span", { class: "mono", text: render(row.latency) }) },
      { key: "level", label: "level", render: (row) => el("span", { class: "mono", text: render(row.level) }) },
      { key: "path", label: "path", render: (row) => el("span", { class: "mono", text: render(row.path || row.route || row.msg || "") }) },
      { key: "country", label: "country", render: (row) => el("span", { class: "mono", text: render(row.country) }) },
    ],
    rows,
  });
  const note = el("p", { class: "small muted", style: "margin:10px 0 0" },
    `Showing ${Math.min(12, result.lines.length)} of ${result.matched} matched records.`);
  const raw = el("div", { class: "terminal raw-output" }, ...result.lines.slice(0, 6).map((line) =>
    el("div", { class: "terminal-line" },
      el("span", { class: "mark", text: ">" }),
      el("span", { class: "mono", text: line.raw }))));
  const wrapper = el("div", {}, table, note,
    el("div", { class: "section-label", text: "stdout" }), raw,
    el("p", { class: "small muted", style: "margin:8px 0 0" }, "The binary prints raw matching lines to stdout and a summary to stderr."));
  if (result.malformed.length) {
    wrapper.append(el("div", { class: "warning-list" }, ...result.malformed.map((problem) =>
      el("div", { class: "warning-row", text: `warning: line ${problem.line}: ${problem.message}` }))));
  }
  return wrapper;
}

function renderTail(result) {
  const terminal = el("div", { class: "terminal", id: "terminal" }, ...result.lines.map((line) =>
    el("div", { class: "terminal-line" },
      el("span", { class: "mark", text: ">" }),
      el("span", { class: "mono", text: line.raw }))));
  if (state.following) {
    terminal.append(el("div", { class: "terminal-line live" },
      el("span", { class: "mark", text: ">" }),
      el("span", { class: "mono", text: "following appended lines..." })));
  }
  return terminal;
}

function renderResult(result) {
  $("#result-kind").textContent = result.kind;
  $("#result-meta").textContent = result.kind === "tail"
    ? `${result.records} records, ${result.lines.length} lines shown`
    : `${result.records} records, ${result.matched} matched`;
  $("#result-summary").replaceChildren();
  renderSummary(result);

  if (result.kind === "stats") $("#result-body").replaceChildren(renderStats(result));
  else if (result.kind === "top") $("#result-body").replaceChildren(renderTop(result));
  else if (result.kind === "filter") $("#result-body").replaceChildren(renderFilter(result));
  else $("#result-body").replaceChildren(renderTail(result));

  $("#follow").hidden = result.kind !== "tail";
  $("#result-note").textContent = {
    stats: "Percentiles use linear interpolation between ranks, the same formula as src/stats.rs.",
    top: "Ordered by count, then by value, so identical runs produce identical output.",
    filter: "Numeric comparisons only match numeric fields. Missing fields never satisfy a comparison.",
    tail: "The mirror replays buffered lines. The Rust binary can follow a file that is still growing.",
  }[result.kind];
}

function runQuery() {
  const result = execute({
    command: state.command,
    text: sample().text,
    expression: state.command === "filter" ? state.expression : "",
    field: state.field,
    by: state.by,
    n: state.n,
  });
  renderResult(result);
}

/* ---------------------------------------------------------------- following */

function stopFollowing() {
  if (state.following) {
    clearInterval(state.following);
    state.following = null;
  }
  $("#follow").textContent = "Follow";
}

function toggleFollowing() {
  if (state.following) {
    stopFollowing();
    runQuery();
    return;
  }
  const lines = sample().text.split("\n").filter((line) => line.trim());
  state.following = setInterval(() => {
    const terminal = $("#terminal");
    if (!terminal) return;
    const line = lines[state.followIndex % lines.length];
    state.followIndex += 1;
    terminal.append(el("div", { class: "terminal-line live" },
      el("span", { class: "mark", text: ">" }),
      el("span", { class: "mono", text: line })));
    if (terminal.children.length > 40) terminal.firstElementChild?.remove();
    terminal.scrollTop = terminal.scrollHeight;
  }, 900);
  $("#follow").textContent = "Following";
  runQuery();
}

/* -------------------------------------------------------------------- boot */

function applyHash() {
  const hash = location.hash.replace("#", "");
  if (["stats", "top", "filter", "tail"].includes(hash)) state.command = hash;
  if (hash === "filter") state.expression = "status=500";
}

function init() {
  applyHash();
  renderSampleTabs();
  renderCommandTabs();
  renderSource();
  renderQueryFields();
  runQuery();

  $("#run").addEventListener("click", runQuery);
  $("#follow").addEventListener("click", toggleFollowing);
  $("#copy-cli").addEventListener("click", async () => {
    const command = $("#cli-command").textContent;
    try {
      await navigator.clipboard.writeText(command);
      toast("Command copied", "success");
    } catch {
      toast("Clipboard is blocked in this browser", "error");
    }
  });
}

init();
