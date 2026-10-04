// A JavaScript mirror of the Rust CLI in src/.
//
// This file exists so the static demo can behave like the binary without a
// backend. It mirrors the parsing, filtering, aggregation and formatting rules
// in src/parser.rs, src/filter.rs, src/stats.rs and src/aggregate.rs. When the
// two disagree, the Rust implementation is the source of truth.

/* --------------------------------------------------------------- samples */

const accessRecords = [
  { ts: "2026-01-01T10:00:01Z", level: "info", status: 200, latency: 43, method: "GET", path: "/api/users", country: "NP", userId: "u_101" },
  { ts: "2026-01-01T10:00:02Z", level: "info", status: 200, latency: 88, method: "GET", path: "/api/orders", country: "US", userId: "u_102" },
  { ts: "2026-01-01T10:00:03Z", level: "error", status: 500, latency: 612, method: "POST", path: "/api/orders", country: "NP", userId: "u_103" },
  { ts: "2026-01-01T10:00:04Z", level: "info", status: 201, latency: 121, method: "POST", path: "/api/users", country: "IN", userId: "u_104" },
  { ts: "2026-01-01T10:00:05Z", level: "warn", status: 404, latency: 17, method: "GET", path: "/api/missing", country: "NP", userId: "u_105" },
  { ts: "2026-01-01T10:00:06Z", level: "info", status: 200, latency: 54, method: "GET", path: "/api/users", country: "NP", userId: "u_106" },
  { ts: "2026-01-01T10:00:07Z", level: "info", status: 200, latency: 72, method: "GET", path: "/api/orders", country: "DE", userId: "u_107" },
  { ts: "2026-01-01T10:00:08Z", level: "error", status: 500, latency: 845, method: "POST", path: "/api/checkout", country: "US", userId: "u_108" },
  { ts: "2026-01-01T10:00:09Z", level: "info", status: 200, latency: 39, method: "GET", path: "/api/users", country: "NP", userId: "u_109" },
  { ts: "2026-01-01T10:00:10Z", level: "warn", status: 404, latency: 22, method: "GET", path: "/api/admin", country: "IN", userId: "u_110" },
  { ts: "2026-01-01T10:00:11Z", level: "info", status: 200, latency: 110, method: "GET", path: "/api/orders", country: "NP", userId: "u_111" },
  { ts: "2026-01-01T10:00:12Z", level: "info", status: 201, latency: 260, method: "POST", path: "/api/orders", country: "US", userId: "u_112" },
  { ts: "2026-01-01T10:00:13Z", level: "info", status: 200, latency: 63, method: "GET", path: "/api/users", country: "IN", userId: "u_113" },
  { ts: "2026-01-01T10:00:14Z", level: "error", status: 500, latency: 902, method: "POST", path: "/api/checkout", country: "NP", userId: "u_114" },
  { ts: "2026-01-01T10:00:15Z", level: "info", status: 200, latency: 47, method: "GET", path: "/api/orders", country: "DE", userId: "u_115" },
  { ts: "2026-01-01T10:00:16Z", level: "info", status: 200, latency: 128, method: "GET", path: "/api/users", country: "NP", userId: "u_116" },
  { ts: "2026-01-01T10:00:17Z", level: "warn", status: 404, latency: 19, method: "GET", path: "/api/missing", country: "US", userId: "u_117" },
  { ts: "2026-01-01T10:00:18Z", level: "info", status: 200, latency: 76, method: "GET", path: "/api/orders", country: "NP", userId: "u_118" },
  { ts: "2026-01-01T10:00:19Z", level: "error", status: 500, latency: 731, method: "POST", path: "/api/checkout", country: "IN", userId: "u_119" },
  { ts: "2026-01-01T10:00:20Z", level: "info", status: 200, latency: 58, method: "GET", path: "/api/users", country: "NP", userId: "u_120" },
];

export const ACCESS_LOG = accessRecords.map((record) => JSON.stringify(record)).join("\n") + "\n";

export const SERVICE_LOG = `ts=2026-01-01T10:01:01Z level=info status=200 latency=23 msg="request complete" route=/health
ts=2026-01-01T10:01:02Z level=info status=200 latency=41 msg="request complete" route=/api/orders
ts=2026-01-01T10:01:03Z level=warn status=503 latency=410 msg="upstream timeout" route=/api/orders
ts=2026-01-01T10:01:04Z level=info status=200 latency=38 msg="request complete" route=/health
ts=2026-01-01T10:01:05Z level=error status=500 latency=780 msg="db pool exhausted" route=/api/checkout
ts=2026-01-01T10:01:06Z level=info status=201 latency=156 msg="order created" route=/api/orders
ts=2026-01-01T10:01:07Z level=info status=200 latency=29 msg="request complete" route=/health
ts=2026-01-01T10:01:08Z level=warn status=429 latency=12 msg="rate limited" route=/api/users
ts=2026-01-01T10:01:09Z level=info status=200 latency=62 msg="request complete" route=/api/users
ts=2026-01-01T10:01:10Z level=error status=500 latency=640 msg="db pool exhausted" route=/api/checkout
ts=2026-01-01T10:01:11Z level=info status=200 latency=35 msg="request complete" route=/health
ts=2026-01-01T10:01:12Z level=info status=200 latency=44 msg="request complete" route=/api/orders
`;

export const SAMPLES = [
  { id: "access", label: "access.log", format: "JSON lines", text: ACCESS_LOG },
  { id: "service", label: "service.log", format: "key=value", text: SERVICE_LOG },
];

/* --------------------------------------------------------------- parsing */

export function parseRecord(line, lineNo) {
  const trimmed = line.trim();
  if (!trimmed) return null;
  if (trimmed.startsWith("{")) {
    let value;
    try {
      value = JSON.parse(trimmed);
    } catch (error) {
      throw new Error(`line ${lineNo}: invalid JSON (${error.message})`);
    }
    if (!value || typeof value !== "object" || Array.isArray(value)) {
      throw new Error(`line ${lineNo}: expected a JSON object`);
    }
    const record = {};
    flatten("", value, record);
    return { raw: line, record, lineNo, format: "json" };
  }
  const parsed = parseLogfmt(trimmed);
  return { raw: line, record: parsed.record, lineNo, format: parsed.format };
}

function flatten(prefix, value, record) {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    for (const [key, child] of Object.entries(value)) {
      flatten(prefix ? `${prefix}.${key}` : key, child, record);
    }
  } else {
    record[prefix] = value;
  }
}

function parseLogfmt(line) {
  const record = {};
  const chars = [...line];
  const loose = [];
  let index = 0;
  let pairs = 0;
  while (index < chars.length) {
    while (index < chars.length && /\s/.test(chars[index])) index++;
    if (index >= chars.length) break;
    if (chars[index] === '"') {
      const quoted = readQuoted(chars, index);
      loose.push(quoted.text);
      index = quoted.next;
      continue;
    }
    const keyStart = index;
    while (index < chars.length && !/\s/.test(chars[index]) && chars[index] !== "=") index++;
    const key = chars.slice(keyStart, index).join("");
    if (chars[index] === "=") {
      index++;
      let raw;
      if (chars[index] === '"') {
        const quoted = readQuoted(chars, index);
        raw = quoted.text;
        index = quoted.next;
      } else {
        const valueStart = index;
        while (index < chars.length && !/\s/.test(chars[index])) index++;
        raw = chars.slice(valueStart, index).join("");
      }
      if (key) {
        record[key] = typed(raw);
        pairs++;
      } else {
        loose.push(raw);
      }
    } else if (key) {
      loose.push(key);
    }
  }
  if (loose.length) record.message = loose.join(" ");
  else if (!pairs) record.message = line;
  return { record, format: pairs ? "keyvalue" : "plain" };
}

function readQuoted(chars, start) {
  let text = "";
  let index = start + 1;
  while (index < chars.length) {
    if (chars[index] === "\\" && index + 1 < chars.length) {
      const escaped = chars[index + 1];
      text += escaped === "n" ? "\n" : escaped === "t" ? "\t" : escaped === "r" ? "\r" : escaped;
      index += 2;
      continue;
    }
    if (chars[index] === '"') return { text, next: index + 1 };
    text += chars[index];
    index++;
  }
  return { text, next: index };
}

function typed(raw) {
  if (raw === "") return null;
  if (raw === "true") return true;
  if (raw === "false") return false;
  if (/^-?\d+(\.\d+)?$/.test(raw) && String(Number(raw)) === raw) return Number(raw);
  return raw;
}

export function parseLog(text) {
  const lines = [];
  const malformed = [];
  text.split("\n").forEach((line, index) => {
    if (!line.trim()) return;
    try {
      const parsed = parseRecord(line, index + 1);
      if (parsed) lines.push(parsed);
    } catch (error) {
      malformed.push({ line: index + 1, message: error.message });
    }
  });
  return { lines, malformed };
}

/* -------------------------------------------------------------- filtering */

const NUMERIC = new Set([">", ">=", "<", "<="]);

export function parseFilter(expression) {
  const conditions = [];
  for (const clause of String(expression || "").split(",")) {
    const text = clause.trim();
    if (!text) continue;
    const lower = text.toLowerCase();
    const containsAt = lower.indexOf(" contains ");
    if (containsAt !== -1) {
      conditions.push({
        field: text.slice(0, containsAt).trim(),
        op: "contains",
        value: unquote(text.slice(containsAt + 10).trim()).toLowerCase(),
      });
      continue;
    }
    const operators = [">=", "<=", "!=", ">", "<", "="];
    let matched = false;
    for (const op of operators) {
      const at = text.indexOf(op);
      if (at <= 0) continue;
      const field = text.slice(0, at).trim();
      const raw = unquote(text.slice(at + op.length).trim());
      if (!field) throw new Error(`invalid field in '${text}'`);
      if (NUMERIC.has(op) && Number.isNaN(Number(raw))) {
        throw new Error(`'${raw}' is not a number, required after '${op}'`);
      }
      conditions.push({ field, op, value: NUMERIC.has(op) ? Number(raw) : raw });
      matched = true;
      break;
    }
    if (!matched) throw new Error(`cannot parse condition '${text}'`);
  }
  if (!conditions.length) throw new Error("empty filter expression");
  return { conditions };
}

function unquote(text) {
  if (text.length >= 2 && ((text.startsWith('"') && text.endsWith('"')) || (text.startsWith("'") && text.endsWith("'")))) {
    return text.slice(1, -1);
  }
  return text;
}

export function matches(filter, record) {
  return filter.conditions.every((condition) => {
    const actual = record[condition.field];
    switch (condition.op) {
      case "=":
        if (typeof actual === "number" && !Number.isNaN(Number(condition.value)) && condition.value !== "" && typeof condition.value !== "number") {
          return Math.abs(actual - Number(condition.value)) < Number.EPSILON;
        }
        return render(actual) === String(condition.value);
      case "!=":
        return actual === undefined || render(actual) !== String(condition.value);
      case "contains":
        return actual !== undefined && render(actual).toLowerCase().includes(condition.value);
      case ">":
      case ">=":
      case "<":
      case "<=": {
        if (typeof actual !== "number") return false;
        if (condition.op === ">") return actual > condition.value;
        if (condition.op === ">=") return actual >= condition.value;
        if (condition.op === "<") return actual < condition.value;
        return actual <= condition.value;
      }
      default:
        return false;
    }
  });
}

/* -------------------------------------------------------------- aggregates */

export function percentile(sorted, p) {
  if (!sorted.length) return null;
  const rank = ((sorted.length - 1) * Math.max(0, Math.min(100, p))) / 100;
  const lower = Math.floor(rank);
  const upper = Math.ceil(rank);
  if (lower === upper) return sorted[lower];
  return sorted[lower] * (1 - (rank - lower)) + sorted[upper] * (rank - lower);
}

export function summarize(values) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const sum = sorted.reduce((total, value) => total + value, 0);
  return {
    count: sorted.length,
    min: sorted[0],
    max: sorted[sorted.length - 1],
    sum,
    mean: sum / sorted.length,
    p50: percentile(sorted, 50),
    p95: percentile(sorted, 95),
    p99: percentile(sorted, 99),
  };
}

export function numericFields(parsedLines) {
  const stats = new Map();
  for (const line of parsedLines) {
    for (const [field, value] of Object.entries(line.record)) {
      const entry = stats.get(field) || { numeric: 0, other: 0 };
      if (typeof value === "number") entry.numeric++;
      else entry.other++;
      stats.set(field, entry);
    }
  }
  return [...stats.entries()]
    .filter(([, entry]) => entry.numeric > 0 && entry.other === 0)
    .map(([field]) => field)
    .sort();
}

export function topN(parsedLines, field, n) {
  const counts = new Map();
  for (const line of parsedLines) {
    const value = line.record[field];
    if (value === undefined) continue;
    const key = render(value);
    counts.set(key, (counts.get(key) || 0) + 1);
  }
  return [...counts.entries()]
    .map(([key, count]) => ({ key, count }))
    .sort((left, right) => right.count - left.count || left.key.localeCompare(right.key))
    .slice(0, n);
}

export function presentCount(parsedLines, field) {
  return parsedLines.filter((line) => line.record[field] !== undefined).length;
}

/* ---------------------------------------------------------------- output */

export function render(value) {
  if (value === null || value === undefined) return "";
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

export function formatNumber(value) {
  if (!Number.isFinite(value)) return String(value);
  return Number.isInteger(value) ? String(value) : String(Math.round(value * 1000) / 1000);
}

export function cliCommand({ command, sample, expression, field, by, n }) {
  const file = sample === "service" ? "examples/service.log" : "examples/access.log";
  if (command === "stats") {
    return `findr stats --field ${field || "latency"} ${file}`;
  }
  if (command === "top") {
    return `findr top --by ${by || "country"} -n ${n || 5} ${file}`;
  }
  if (command === "filter") {
    return `findr filter --expr '${expression || "status=500"}' ${file}`;
  }
  return `findr tail --file ${file} --lines 5`;
}

/* ------------------------------------------------------------- execution */

export function execute({ command, text, expression = "", field = "latency", by = "country", n = 5 }) {
  const parsed = parseLog(text);
  const filter = expression.trim() ? parseFilter(expression) : null;
  const filtered = filter ? parsed.lines.filter((line) => matches(filter, line.record)) : parsed.lines;

  if (command === "top") {
    return {
      kind: "top",
      records: parsed.lines.length,
      matched: filtered.length,
      present: presentCount(filtered, by),
      field: by,
      entries: topN(filtered, by, Number(n) || 5),
    };
  }
  if (command === "filter") {
    return {
      kind: "filter",
      records: parsed.lines.length,
      matched: filtered.length,
      malformed: parsed.malformed,
      lines: filtered,
    };
  }
  if (command === "tail") {
    return {
      kind: "tail",
      records: parsed.lines.length,
      malformed: parsed.malformed,
      lines: parsed.lines.slice(-5),
    };
  }

  const fields = field
    ? field.split(",").map((entry) => entry.trim()).filter(Boolean)
    : numericFields(filtered);
  const summaries = [];
  for (const name of fields) {
    const values = filtered.map((line) => line.record[name]).filter((value) => typeof value === "number");
    const summary = summarize(values);
    if (summary) summaries.push({ field: name, summary });
  }
  return {
    kind: "stats",
    records: parsed.lines.length,
    matched: filtered.length,
    malformed: parsed.malformed.length,
    fields: summaries,
  };
}
