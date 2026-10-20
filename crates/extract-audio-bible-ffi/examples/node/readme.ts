// Runs every TypeScript example in ../../README.md, in order, as one program, and checks what each
// prints against its `// → ` comments (one for each console.log, in order; `…` stands for any
// text). Run with `npm install && node readme.ts [--show]` after `npm run pack:wasm`.
//
// The examples run in a new folder holding the files in ../fixtures.
import { cpSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { format } from "node:util";

const here = dirname(fileURLToPath(import.meta.url));
const fixtures = join(here, "../fixtures");
const readme = readFileSync(join(here, "../../README.md"), "utf8");
const show = process.argv.includes("--show");

// The examples, their imports gathered into one statement for each module
const blocks = [...readme.matchAll(/^```ts\n([\s\S]*?)^```$/gm)].map((m) => m[1]);
const imports = new Map<string, Set<string>>();
const bodies = blocks.map((block) =>
  block.replace(/^import \{([^}]*)\} from "([^"]+)";\n/gm, (_, names: string, from: string) => {
    const set = imports.get(from) ?? new Set();
    for (const name of names.split(",").map((n) => n.trim()).filter(Boolean)) set.add(name);
    imports.set(from, set);
    return "";
  }),
);
const resolved = (from: string) => (from === "extract-audio-bible" ? import.meta.resolve("extract-audio-bible") : from);
const program = [
  ...[...imports].map(([from, names]) => `import { ${[...names].join(", ")} } from "${resolved(from)}";`),
  ...bodies.map((body, i) => `globalThis.__block(${i});\n${body}`),
].join("\n");

// What each example prints
const printed: string[][] = blocks.map(() => []);
let current = 0;
(globalThis as any).__block = (i: number) => (current = i);
const log = console.log;
// Text is compared a line at a time (blank lines left out), anything else as one line
console.log = (...args: unknown[]) => {
  const text = format(...args);
  if (args.every((a) => typeof a === "string")) printed[current].push(...text.split("\n").filter((l) => l.trim()));
  else printed[current].push(text);
};

const dir = mkdtempSync(join(tmpdir(), "extract-audio-bible-readme-"));
for (const name of readdirSync(fixtures)) cpSync(join(fixtures, name), join(dir, name));
writeFileSync(join(dir, "readme.mts"), program);
process.chdir(dir);
try {
  await import(pathToFileURL(join(dir, "readme.mts")).href);
} finally {
  console.log = log;
}

const flat = (s: string) => s.replace(/\s+/g, " ").trim();
function matches(expected: string, actual: string) {
  if (expected === actual) return true;
  const parts = expected.split("…");
  if (parts.length === 1) return false;
  if (!actual.startsWith(parts[0]) || !actual.endsWith(parts[parts.length - 1])) return false;
  let at = parts[0].length;
  for (const part of parts.slice(1, -1)) {
    const found = actual.indexOf(part, at);
    if (found < 0) return false;
    at = found + part.length;
  }
  return at <= actual.length - parts[parts.length - 1].length;
}

let failed = 0;
blocks.forEach((block, i) => {
  const expected = [...block.matchAll(/\/\/ → (.*)$/gm)].map((m) => flat(m[1]));
  const actual = printed[i].map(flat);
  const line = readme.slice(0, readme.indexOf(block)).split("\n").length;
  const ok = expected.length === actual.length && expected.every((e, j) => matches(e, actual[j]));
  if (!ok) failed++;
  if (!ok || show) {
    log(`${ok ? "ok" : "FAILED"}: the example at README.md:${line}`);
    for (let j = 0; j < Math.max(expected.length, actual.length); j++)
      log(`  ${expected[j] === undefined ? "(nothing)" : expected[j]}\n  ${actual[j] === undefined ? "(nothing printed)" : actual[j]}`);
  }
});
process.chdir(here);
if (failed) log(`${failed} of ${blocks.length} examples print otherwise than README.md says (run in ${dir})`);
else {
  rmSync(dir, { recursive: true, force: true });
  log(`all ${blocks.length} examples print what README.md says`);
}
process.exit(failed ? 1 : 0);
