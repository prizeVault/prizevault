// Proof-of-funds view: turns the contract's `status` record into something a
// sponsor, a team manager or a Discord mod can read at a glance. Pure
// functions; the CLI and the tests feed them either a hand-built record or the
// output of the contract client.

import { DEFAULT_DECIMALS, formatAmount, placementAmounts } from "./placements.ts";

export type TournamentState = "Created" | "Open" | "ResultsSubmitted" | "Confirmed" | "Cancelled" | "Closed";

export const STATES: TournamentState[] = ["Created", "Open", "ResultsSubmitted", "Confirmed", "Cancelled", "Closed"];

export interface TournamentView {
  organizer: string;
  sponsor: string;
  token: string;
  poolRequired: bigint;
  poolFunded: bigint;
  placements: number[];
  registerBy: number;
  resultsBy: number;
  claimBy: number;
  referee: string | null;
  state: TournamentState;
}

/**
 * Normalises whatever the SDK hands back for the `Tournament` struct. Depending
 * on the code path (spec-typed client vs. raw scValToNative) the enum arrives
 * as "Open", ["Open"] or { tag: "Open" }, amounts as bigint/number/string, the
 * Option<Address> as null/undefined/string, and addresses as string or Address.
 */
export function normalizeStatus(raw: unknown): TournamentView {
  if (typeof raw !== "object" || raw === null) {
    throw new Error("status: expected an object");
  }
  const r = raw as Record<string, unknown>;
  const state = normalizeState(r.state);
  return {
    organizer: addr(r.organizer, "organizer"),
    sponsor: addr(r.sponsor, "sponsor"),
    token: addr(r.token, "token"),
    poolRequired: big(r.pool_required, "pool_required"),
    poolFunded: big(r.pool_funded, "pool_funded"),
    placements: list(r.placements).map((b) => Number(b)),
    registerBy: Number(big(r.register_by, "register_by")),
    resultsBy: Number(big(r.results_by, "results_by")),
    claimBy: Number(big(r.claim_by, "claim_by")),
    referee: r.referee === null || r.referee === undefined ? null : addr(r.referee, "referee"),
    state,
  };
}

export function normalizeState(v: unknown): TournamentState {
  let name: unknown = v;
  if (Array.isArray(v)) {
    name = v[0];
  } else if (typeof v === "object" && v !== null && "tag" in v) {
    name = (v as { tag: unknown }).tag;
  }
  if (typeof name !== "string" || !STATES.includes(name as TournamentState)) {
    throw new Error(`status: unknown state ${JSON.stringify(v)}`);
  }
  return name as TournamentState;
}

export type Badge = "FUNDED" | "PARTIALLY FUNDED" | "UNFUNDED" | "CANCELLED" | "CLOSED";

export interface ProofOfFunds {
  badge: Badge;
  /** pool_funded >= pool_required */
  funded: boolean;
  /** funded share in basis points (10_000 = 100%) */
  fundedBps: number;
  phase: string;
  nextDeadline: { label: string; at: number } | null;
  lines: string[];
}

export interface ProofOptions {
  decimals?: number;
  symbol?: string;
  contractId?: string;
  id?: number;
}

export function proofOfFunds(t: TournamentView, nowSec: number, opts: ProofOptions = {}): ProofOfFunds {
  const decimals = opts.decimals ?? DEFAULT_DECIMALS;
  const symbol = opts.symbol ?? "tokens";
  const fmt = (v: bigint) => `${formatAmount(v, decimals)} ${symbol}`;
  const funded = t.poolFunded >= t.poolRequired;
  const fundedBps = t.poolRequired > 0n ? Number((t.poolFunded * 10_000n) / t.poolRequired) : 0;

  let badge: Badge;
  if (t.state === "Cancelled") badge = "CANCELLED";
  else if (t.state === "Closed") badge = "CLOSED";
  else if (funded) badge = "FUNDED";
  else if (t.poolFunded > 0n) badge = "PARTIALLY FUNDED";
  else badge = "UNFUNDED";

  const { phase, nextDeadline } = describePhase(t, nowSec);

  const lines: string[] = [];
  const title = opts.id !== undefined ? `PrizeVault tournament #${opts.id}` : "PrizeVault tournament";
  lines.push(`${title}: ${badge}`);
  if (opts.contractId) lines.push(`contract      ${opts.contractId}`);
  lines.push(`escrowed      ${fmt(t.poolFunded)} of ${fmt(t.poolRequired)} (${(fundedBps / 100).toFixed(2)}%)`);
  lines.push(`state         ${t.state} - ${phase}`);
  lines.push(`sponsor       ${t.sponsor}`);
  lines.push(`organizer     ${t.organizer}`);
  lines.push(`referee       ${t.referee ?? "none (organizer's results are final)"}`);
  lines.push(`token         ${t.token}`);
  lines.push(`register by   ${iso(t.registerBy)} ${relative(t.registerBy, nowSec)}`);
  lines.push(`results by    ${iso(t.resultsBy)} ${relative(t.resultsBy, nowSec)}`);
  lines.push(`claim by      ${iso(t.claimBy)} ${relative(t.claimBy, nowSec)}`);
  lines.push(`placements    ${t.placements.length} places, ${t.placements.reduce((a, b) => a + b, 0)} bps`);
  const amounts = placementAmounts(t.poolRequired, t.placements);
  t.placements.forEach((bps, i) => {
    lines.push(`  ${String(i + 1).padStart(3)}. ${(bps / 100).toFixed(2).padStart(6)}%  ${fmt(amounts[i])}`);
  });
  return { badge, funded, fundedBps, phase, nextDeadline, lines };
}

export function renderProof(p: ProofOfFunds): string {
  return p.lines.join("\n");
}

function describePhase(t: TournamentView, now: number): { phase: string; nextDeadline: ProofOfFunds["nextDeadline"] } {
  switch (t.state) {
    case "Created":
      return {
        phase: t.poolFunded >= t.poolRequired ? "fully funded, waiting for the organizer to open registration" : "collecting the pool; registration cannot open until it is fully escrowed",
        nextDeadline: { label: "register by", at: t.registerBy },
      };
    case "Open":
      if (now < t.registerBy) {
        return { phase: "registration open", nextDeadline: { label: "register by", at: t.registerBy } };
      }
      return { phase: "registration closed, waiting for results", nextDeadline: { label: "results by", at: t.resultsBy } };
    case "ResultsSubmitted":
      return { phase: "results submitted, waiting for the referee", nextDeadline: { label: "results by", at: t.resultsBy } };
    case "Confirmed":
      if (now < t.claimBy) {
        return { phase: "results confirmed, winners can claim", nextDeadline: { label: "claim by", at: t.claimBy } };
      }
      return { phase: "claim window closed, unclaimed prizes can be swept to the sponsor", nextDeadline: null };
    case "Cancelled":
      return { phase: "cancelled, pool refunded to the sponsor", nextDeadline: null };
    case "Closed":
      return { phase: "closed, unclaimed prizes refunded to the sponsor", nextDeadline: null };
  }
}

export function iso(sec: number): string {
  return new Date(sec * 1000).toISOString().replace(".000Z", "Z");
}

function relative(at: number, now: number): string {
  const diff = at - now;
  const abs = Math.abs(diff);
  const unit = abs >= 86_400 ? `${Math.floor(abs / 86_400)}d` : abs >= 3_600 ? `${Math.floor(abs / 3_600)}h` : `${Math.floor(abs / 60)}m`;
  return diff >= 0 ? `(in ${unit})` : `(${unit} ago)`;
}

function addr(v: unknown, field: string): string {
  if (typeof v === "string") return v;
  if (v && typeof v === "object" && typeof (v as { toString?: unknown }).toString === "function") {
    const s = String(v);
    if (s.length > 0 && s !== "[object Object]") return s;
  }
  throw new Error(`status: ${field} is not an address`);
}

function big(v: unknown, field: string): bigint {
  if (typeof v === "bigint") return v;
  if (typeof v === "number" && Number.isInteger(v)) return BigInt(v);
  if (typeof v === "string" && /^-?\d+$/.test(v)) return BigInt(v);
  throw new Error(`status: ${field} is not an integer`);
}

function list(v: unknown): unknown[] {
  if (!Array.isArray(v)) throw new Error("status: placements is not a list");
  return v;
}
