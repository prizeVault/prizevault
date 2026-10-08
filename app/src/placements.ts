// Placement table handling and the bps payout math, mirrored 1:1 from the
// contract (contracts/prizevault/src/lib.rs: validate_placements and
// placement_amounts). Keep the two in sync; the Rust tests and app tests both
// pin the same expected numbers.

export const BPS_DENOMINATOR = 10_000;
export const MAX_PLACEMENTS = 256;
/** USDC on Stellar has 7 decimal places; 1 USDC = 10_000_000 stroops. */
export const DEFAULT_DECIMALS = 7;

export class PlacementsError extends Error {
  readonly issues: string[];
  constructor(issues: string[]) {
    super(issues.join("; "));
    this.name = "PlacementsError";
    this.issues = issues;
  }
}

export interface ReadPlacementsResult {
  /** Basis points per place, first place first. */
  bps: number[];
  labels: (string | undefined)[];
  warnings: string[];
}

/**
 * Reads a placements document in any of the shapes organisers produce:
 * a bare array of bps, an array of `{ place, bps, label }` rows, or an object
 * with a `placements` array. Integer strings are coerced with a warning; rows
 * are used in file order, and a `place` field that disagrees is reported.
 */
export function readPlacements(doc: unknown): ReadPlacementsResult {
  const warnings: string[] = [];
  let rows: unknown[];
  if (Array.isArray(doc)) {
    rows = doc;
  } else if (isRecord(doc) && Array.isArray(doc.placements)) {
    rows = doc.placements;
  } else {
    throw new PlacementsError(["placements document must be an array or an object with a `placements` array"]);
  }

  const bps: number[] = [];
  const labels: (string | undefined)[] = [];
  rows.forEach((row, i) => {
    let raw: unknown = row;
    let label: string | undefined;
    let place: unknown;
    if (isRecord(row)) {
      raw = row.bps;
      label = typeof row.label === "string" ? row.label : undefined;
      place = row.place;
    }
    if (typeof raw === "string" && /^\s*\d+\s*$/.test(raw)) {
      warnings.push(`row ${i + 1}: bps given as the string "${raw}", coerced to ${Number(raw.trim())}`);
      raw = Number(raw.trim());
    }
    if (typeof raw !== "number" || !Number.isInteger(raw)) {
      throw new PlacementsError([`row ${i + 1}: bps must be an integer, got ${JSON.stringify(raw)}`]);
    }
    if (place !== undefined && Number(place) !== i + 1) {
      warnings.push(`row ${i + 1}: place field says ${JSON.stringify(place)}; rows are used in file order`);
    }
    bps.push(raw);
    labels.push(label);
  });
  return { bps, labels, warnings };
}

/** Returns the list of problems with a placement table; empty means valid. */
export function validatePlacements(bps: number[]): string[] {
  const issues: string[] = [];
  if (bps.length === 0) {
    issues.push("placements are empty");
  }
  if (bps.length > MAX_PLACEMENTS) {
    issues.push(`too many placements: ${bps.length} > ${MAX_PLACEMENTS}`);
  }
  bps.forEach((b, i) => {
    if (!Number.isInteger(b) || b <= 0) {
      issues.push(`place ${i + 1}: bps must be a positive integer, got ${String(b)}`);
    }
  });
  const sum = bps.reduce((acc, b) => acc + (Number.isFinite(b) ? b : 0), 0);
  if (sum !== BPS_DENOMINATOR) {
    issues.push(
      `placements sum to ${sum} bps (${(sum / 100).toFixed(2)}% of the pool), expected exactly ${BPS_DENOMINATOR}`,
    );
  }
  return issues;
}

export function assertPlacements(bps: number[]): number[] {
  const issues = validatePlacements(bps);
  if (issues.length > 0) {
    throw new PlacementsError(issues);
  }
  return bps;
}

/**
 * Amount per placement, in token base units. Every placement but the last
 * receives floor(pool * bps / 10_000); the last receives the remainder, so the
 * amounts always sum to exactly `pool` and rounding dust lands on the last place.
 */
export function placementAmounts(pool: bigint, bps: number[]): bigint[] {
  const out: bigint[] = [];
  let distributed = 0n;
  for (let i = 0; i < bps.length; i++) {
    const amount =
      i === bps.length - 1 ? pool - distributed : (pool * BigInt(bps[i])) / BigInt(BPS_DENOMINATOR);
    distributed += amount;
    out.push(amount);
  }
  return out;
}

/** "7777.7777777" -> 77777777777n (with 7 decimals). Commas and underscores are ignored. */
export function parseAmount(text: string, decimals = DEFAULT_DECIMALS): bigint {
  const cleaned = text.replace(/[,_\s]/g, "");
  const m = /^(\d+)(?:\.(\d*))?$/.exec(cleaned);
  if (!m) {
    throw new Error(`invalid amount "${text}": expected digits with an optional fraction`);
  }
  const whole = m[1];
  const frac = m[2] ?? "";
  if (frac.length > decimals) {
    throw new Error(`invalid amount "${text}": more than ${decimals} decimal places`);
  }
  return BigInt(whole) * 10n ** BigInt(decimals) + BigInt(frac.padEnd(decimals, "0") || "0");
}

/** 77777777777n -> "7777.7777777"; trailing zeros trimmed to at least two decimals. */
export function formatAmount(units: bigint, decimals = DEFAULT_DECIMALS): string {
  const negative = units < 0n;
  const abs = negative ? -units : units;
  const base = 10n ** BigInt(decimals);
  const whole = abs / base;
  let frac = (abs % base).toString().padStart(decimals, "0");
  frac = frac.replace(/0+$/, "");
  if (frac.length < 2) {
    frac = frac.padEnd(2, "0");
  }
  return `${negative ? "-" : ""}${whole.toString()}.${frac}`;
}

export function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}
