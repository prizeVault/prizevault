import { test } from "node:test";
import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { COMMANDS, flagString, parseArgv, parseDeadline, parseId, usage, validateArgs } from "../src/args.ts";
import { main } from "../src/cli.ts";

const seedPath = (name: string) => fileURLToPath(new URL(`../../data/seed/${name}`, import.meta.url));

test("parseArgv handles --key value, --key=value, booleans, -h and positionals", () => {
  const p = parseArgv(["create", "--pool", "5000", "--referee=GABC", "--json", "extra", "--claim-by", "+44d"]);
  assert.equal(p.command, "create");
  assert.deepEqual(p.flags, { pool: "5000", referee: "GABC", json: true, "claim-by": "+44d" });
  assert.deepEqual(p.positional, ["extra"]);
  assert.equal(parseArgv(["-h"]).flags.help, true);
  assert.equal(parseArgv([]).command, undefined);
  assert.equal(flagString(p, "pool"), "5000");
  assert.equal(flagString(p, "json"), undefined, "boolean flags are not strings");
});

test("validateArgs: required flags, unknown flags, unknown commands", () => {
  assert.deepEqual(validateArgs(parseArgv(["status", "--id", "0"])), []);
  assert.deepEqual(validateArgs(parseArgv(["fund", "--id", "0", "--amount", "5000", "--rpc-url", "http://x"])), []);
  assert.deepEqual(validateArgs(parseArgv(["fund", "--id", "0"])), ["--amount is required for fund"]);
  assert.deepEqual(validateArgs(parseArgv(["fund", "--id", "0", "--amount"])), ["--amount is required for fund"]);
  assert.deepEqual(validateArgs(parseArgv(["open", "--id", "0", "--player", "G"])), ["unknown flag --player for open"]);
  assert.deepEqual(validateArgs(parseArgv(["explode"])), ['unknown command "explode"']);
  assert.deepEqual(validateArgs(parseArgv([])), ["missing command"]);
  assert.deepEqual(validateArgs(parseArgv(["status", "--id", "0", "stray"])), ["unexpected argument(s): stray"]);
  const create = validateArgs(parseArgv(["create"]));
  assert.equal(create.length, COMMANDS.create.required.length);
});

test("every documented command appears in usage()", () => {
  const text = usage();
  for (const name of Object.keys(COMMANDS)) {
    assert.ok(text.includes(`  ${name}`), `usage lists ${name}`);
  }
  for (const name of ["create", "fund", "open", "register", "results", "confirm", "claim", "refund", "cancel", "status", "proof-of-funds"]) {
    assert.ok(name in COMMANDS, `${name} is a command`);
  }
  assert.ok(COMMANDS.status.readonly && COMMANDS["proof-of-funds"].readonly);
  assert.ok(!COMMANDS.create.readonly);
});

test("parseDeadline: relative, unix and ISO forms", () => {
  const now = 1_790_000_000;
  assert.equal(parseDeadline("+7d", now), now + 7 * 86_400);
  assert.equal(parseDeadline("+36h", now), now + 36 * 3_600);
  assert.equal(parseDeadline("+90m", now), now + 90 * 60);
  assert.equal(parseDeadline("1790604800", now), 1_790_604_800);
  assert.equal(parseDeadline("2026-10-05T21:00:00Z", now), Date.UTC(2026, 9, 5, 21) / 1000);
  assert.throws(() => parseDeadline("next tuesday", now), /invalid deadline/);
  assert.throws(() => parseDeadline("+7w", now), /invalid deadline/);
});

test("parseId accepts u32 only", () => {
  assert.equal(parseId("0"), 0);
  assert.equal(parseId(" 42 "), 42);
  assert.equal(parseId("4294967295"), 4_294_967_295);
  assert.throws(() => parseId("4294967296"), /exceeds u32/);
  assert.throws(() => parseId("-1"), /invalid id/);
  assert.throws(() => parseId("abc"), /invalid id/);
});

async function run(argv: string[]) {
  const out: string[] = [];
  const err: string[] = [];
  const code = await main(argv, (l) => out.push(l), (l) => err.push(l));
  return { code, out: out.join("\n"), err: err.join("\n") };
}

test("cli: help and argument errors never touch the network", async () => {
  const help = await run(["--help"]);
  assert.equal(help.code, 0);
  assert.match(help.out, /usage: prizevault/);
  const none = await run([]);
  assert.equal(none.code, 2);
  const bad = await run(["fund", "--id", "0"]);
  assert.equal(bad.code, 2);
  assert.match(bad.err, /--amount is required for fund/);
  const unknown = await run(["yolo"]);
  assert.equal(unknown.code, 2);
  assert.match(unknown.err, /unknown command "yolo"/);
});

test("cli: results --dry-run validates the corrected seed export offline", async () => {
  const r = await run(["results", "--id", "0", "--file", seedPath("results-fixed.json"), "--roster", seedPath("roster.json"), "--dry-run"]);
  assert.equal(r.code, 0, r.err);
  assert.match(r.out, /ranking is valid: 16 places/);
  assert.match(r.out, /dry run: nothing submitted/);
  assert.match(r.err, /warning: roster row 18 address: surrounding whitespace removed/);
});

test("cli: results --dry-run rejects the raw seed export with the duplicate row", async () => {
  const r = await run(["results", "--id", "0", "--file", seedPath("results.json"), "--roster", seedPath("roster.json"), "--dry-run"]);
  assert.equal(r.code, 1);
  assert.match(r.err, /ResultsError/);
  assert.match(r.err, /duplicate address .* at places 1 and 12/);
  assert.doesNotMatch(r.out, /ranking is valid/);
});

test("cli: mutating commands refuse to run without a signer, before any RPC call", async () => {
  const saved = process.env.STELLAR_SECRET_KEY;
  delete process.env.STELLAR_SECRET_KEY;
  try {
    const r = await run(["open", "--id", "0", "--contract-id", "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAB"]);
    assert.equal(r.code, 1);
    assert.match(r.err, /STELLAR_SECRET_KEY/);
  } finally {
    if (saved !== undefined) process.env.STELLAR_SECRET_KEY = saved;
  }
});

test("cli: read-only commands need a contract id", async () => {
  const saved = process.env.PRIZEVAULT_CONTRACT_ID;
  delete process.env.PRIZEVAULT_CONTRACT_ID;
  try {
    const r = await run(["status", "--id", "0"]);
    assert.equal(r.code, 1);
    assert.match(r.err, /PRIZEVAULT_CONTRACT_ID/);
  } finally {
    if (saved !== undefined) process.env.PRIZEVAULT_CONTRACT_ID = saved;
  }
});
