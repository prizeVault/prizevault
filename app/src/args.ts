// CLI argument parsing: pure functions, no I/O, so `npm test` covers them offline.

export interface ParsedArgs {
  command: string | undefined;
  positional: string[];
  flags: Record<string, string | true>;
}

export interface CommandSpec {
  summary: string;
  required: string[];
  optional: string[];
  /** Read-only commands only simulate; they need no secret key. */
  readonly: boolean;
}

/** Flags accepted by every command. */
export const GLOBAL_FLAGS = ["rpc-url", "network-passphrase", "contract-id", "secret-key", "json", "help"];

/** Flags that never take a value, so `--json extra` leaves `extra` positional. */
export const BOOLEAN_FLAGS = new Set(["json", "help", "dry-run", "expired"]);

export const COMMANDS: Record<string, CommandSpec> = {
  create: {
    summary: "Create a tournament (organizer signs); prints the new id",
    required: ["sponsor", "token", "pool", "placements", "register-by", "results-by", "claim-by"],
    optional: ["organizer", "referee"],
    readonly: false,
  },
  fund: {
    summary: "Move --amount of the token from --from (default: signer) into escrow",
    required: ["id", "amount"],
    optional: ["from"],
    readonly: false,
  },
  open: {
    summary: "Open registration; fails unless the pool is fully funded (proof of funds)",
    required: ["id"],
    optional: [],
    readonly: false,
  },
  register: {
    summary: "Register --player (default: signer) before register-by",
    required: ["id"],
    optional: ["player"],
    readonly: false,
  },
  results: {
    summary: "Validate a results export and submit the ranking (organizer signs)",
    required: ["id", "file"],
    optional: ["roster", "dry-run"],
    readonly: false,
  },
  confirm: {
    summary: "Referee confirms the submitted ranking; prizes become claimable",
    required: ["id"],
    optional: [],
    readonly: false,
  },
  reject: {
    summary: "Referee rejects the submitted ranking; organizer may resubmit",
    required: ["id"],
    optional: [],
    readonly: false,
  },
  claim: {
    summary: "Winner (--player, default: signer) claims their prize",
    required: ["id"],
    optional: ["player"],
    readonly: false,
  },
  refund: {
    summary: "After claim-by: sweep unclaimed prizes back to the sponsor (anyone)",
    required: ["id"],
    optional: [],
    readonly: false,
  },
  cancel: {
    summary: "Organizer cancels before results are confirmed; with --expired anyone can cancel after results-by",
    required: ["id"],
    optional: ["expired"],
    readonly: false,
  },
  status: {
    summary: "Print the tournament record (read-only)",
    required: ["id"],
    optional: [],
    readonly: true,
  },
  "proof-of-funds": {
    summary: "Print a shareable proof-of-funds summary (read-only)",
    required: ["id"],
    optional: [],
    readonly: true,
  },
};

/**
 * Parses argv (without node and script). Supports `--key value`, `--key=value`,
 * boolean `--flag`, `-h`. The first bare token is the command; later bare
 * tokens are positional.
 */
export function parseArgv(argv: string[]): ParsedArgs {
  const flags: Record<string, string | true> = {};
  const positional: string[] = [];
  let command: string | undefined;
  for (let i = 0; i < argv.length; i++) {
    const tok = argv[i];
    if (tok === "-h" || tok === "--help") {
      flags.help = true;
      continue;
    }
    if (tok.startsWith("--")) {
      const body = tok.slice(2);
      const eq = body.indexOf("=");
      if (eq >= 0) {
        flags[body.slice(0, eq)] = body.slice(eq + 1);
        continue;
      }
      const next = argv[i + 1];
      if (!BOOLEAN_FLAGS.has(body) && next !== undefined && !next.startsWith("--")) {
        flags[body] = next;
        i++;
      } else {
        flags[body] = true;
      }
      continue;
    }
    if (command === undefined) {
      command = tok;
    } else {
      positional.push(tok);
    }
  }
  return { command, positional, flags };
}

/** Returns a list of errors; empty means the invocation is well-formed. */
export function validateArgs(parsed: ParsedArgs): string[] {
  const errors: string[] = [];
  if (parsed.command === undefined) {
    errors.push("missing command");
    return errors;
  }
  const spec = COMMANDS[parsed.command];
  if (!spec) {
    errors.push(`unknown command "${parsed.command}"`);
    return errors;
  }
  for (const r of spec.required) {
    if (parsed.flags[r] === undefined || parsed.flags[r] === true) {
      errors.push(`--${r} is required for ${parsed.command}`);
    }
  }
  const known = new Set([...spec.required, ...spec.optional, ...GLOBAL_FLAGS]);
  for (const k of Object.keys(parsed.flags)) {
    if (!known.has(k)) {
      errors.push(`unknown flag --${k} for ${parsed.command}`);
    }
  }
  if (parsed.positional.length > 0) {
    errors.push(`unexpected argument(s): ${parsed.positional.join(" ")}`);
  }
  return errors;
}

export function usage(): string {
  const lines = ["usage: prizevault <command> [--flags]", ""];
  for (const [name, spec] of Object.entries(COMMANDS)) {
    const req = spec.required.map((f) => `--${f} <${f}>`).join(" ");
    const opt = spec.optional.map((f) => `[--${f}]`).join(" ");
    lines.push(`  ${name.padEnd(16)} ${[req, opt].filter(Boolean).join(" ")}`);
    lines.push(`  ${"".padEnd(16)} ${spec.summary}`);
  }
  lines.push("");
  lines.push("global: --rpc-url --network-passphrase --contract-id --secret-key --json --help");
  lines.push("env:    STELLAR_RPC_URL STELLAR_NETWORK_PASSPHRASE PRIZEVAULT_CONTRACT_ID STELLAR_SECRET_KEY");
  lines.push("deadlines accept ISO-8601 (2026-10-05T21:00:00Z), unix seconds, or +Nd / +Nh / +Nm");
  return lines.join("\n");
}

/** ISO-8601, unix seconds, or a relative offset (+7d, +36h, +90m) from `nowSec`. */
export function parseDeadline(text: string, nowSec: number): number {
  const t = text.trim();
  const rel = /^\+(\d+)([dhm])$/.exec(t);
  if (rel) {
    const n = Number(rel[1]);
    const mult = rel[2] === "d" ? 86_400 : rel[2] === "h" ? 3_600 : 60;
    return nowSec + n * mult;
  }
  if (/^\d+$/.test(t)) {
    return Number(t);
  }
  const ms = Date.parse(t);
  if (Number.isNaN(ms)) {
    throw new Error(`invalid deadline "${text}": use ISO-8601, unix seconds, or +Nd/+Nh/+Nm`);
  }
  return Math.floor(ms / 1000);
}

/** Tournament ids are u32. */
export function parseId(text: string): number {
  if (!/^\d+$/.test(text.trim())) {
    throw new Error(`invalid id "${text}": expected a non-negative integer`);
  }
  const n = Number(text.trim());
  if (n > 0xffff_ffff) {
    throw new Error(`invalid id "${text}": exceeds u32`);
  }
  return n;
}

/** Flag value as string, or undefined when absent or boolean. */
export function flagString(parsed: ParsedArgs, name: string): string | undefined {
  const v = parsed.flags[name];
  return typeof v === "string" ? v : undefined;
}
