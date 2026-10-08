// Parsing of the roster and results files exported from bracket tools.
// Exports are messy: whitespace around addresses, places as strings, gamertag
// case drift, and the occasional row pasted twice. Everything that would make
// the contract reject the ranking (duplicate, unregistered, too long, gaps) is
// reported here first, with the row that caused it.

import { StrKey } from "@stellar/stellar-sdk";
import { isRecord } from "./placements.ts";

export interface RosterEntry {
  seat: number;
  gamertag: string;
  address: string;
  region?: string;
  team?: string;
}

export interface RankingEntry {
  place: number;
  gamertag: string;
  address: string;
  seat?: number;
}

export interface ParsedResults {
  /** Sorted by place, 1 first. */
  ranking: RankingEntry[];
  /** Addresses in place order: the argument for `submit_results`. */
  addresses: string[];
  warnings: string[];
}

export class ResultsError extends Error {
  readonly issues: string[];
  /** Non-fatal observations collected before the error was raised. */
  readonly warnings: string[];
  constructor(issues: string[], warnings: string[] = []) {
    super(issues.join("\n"));
    this.name = "ResultsError";
    this.issues = issues;
    this.warnings = warnings;
  }
}

/** Ed25519 account (G...) or contract (C...) address. Muxed accounts are not accepted. */
export function isValidAddress(address: string): boolean {
  return StrKey.isValidEd25519PublicKey(address) || StrKey.isValidContract(address);
}

export interface ReadRosterResult {
  players: RosterEntry[];
  warnings: string[];
}

/** Reads `{ players: [...] }` or a bare array. Throws on invalid or duplicate addresses. */
export function readRoster(doc: unknown): ReadRosterResult {
  const rows = Array.isArray(doc) ? doc : isRecord(doc) && Array.isArray(doc.players) ? doc.players : null;
  if (!rows) {
    throw new ResultsError(["roster document must be an array or an object with a `players` array"]);
  }
  const warnings: string[] = [];
  const issues: string[] = [];
  const players: RosterEntry[] = [];
  const seenAddress = new Map<string, number>();
  const seenTag = new Map<string, number>();

  rows.forEach((row, i) => {
    const label = `roster row ${i + 1}`;
    if (!isRecord(row)) {
      issues.push(`${label}: not an object`);
      return;
    }
    const gamertag = cleanString(row.gamertag, `${label} gamertag`, warnings);
    const address = cleanString(row.address, `${label} address`, warnings);
    if (gamertag === null || address === null) {
      issues.push(`${label}: gamertag and address are required`);
      return;
    }
    if (!isValidAddress(address)) {
      issues.push(`${label} (${gamertag}): "${address}" is not a valid Stellar address`);
      return;
    }
    const seat = typeof row.seat === "number" && Number.isInteger(row.seat) ? row.seat : i;
    const dupSeat = seenAddress.get(address);
    if (dupSeat !== undefined) {
      issues.push(`${label} (${gamertag}): address ${address} already used by seat ${dupSeat}`);
      return;
    }
    seenAddress.set(address, seat);
    const tagKey = gamertag.toLowerCase();
    const dupTag = seenTag.get(tagKey);
    if (dupTag !== undefined) {
      warnings.push(`${label}: gamertag "${gamertag}" also used by seat ${dupTag} (different address, kept both)`);
    } else {
      seenTag.set(tagKey, seat);
    }
    const entry: RosterEntry = { seat, gamertag, address };
    if (typeof row.region === "string") entry.region = row.region;
    if (typeof row.team === "string") entry.team = row.team;
    players.push(entry);
  });

  if (issues.length > 0) {
    throw new ResultsError(issues, warnings);
  }
  return { players, warnings };
}

export interface ParseResultsOptions {
  /** When given, every ranked address must be on the roster. */
  roster?: RosterEntry[];
  /** When given, the ranking may not be longer than this (the placement table length). */
  maxPlaces?: number;
}

/**
 * Parses a results export (`{ ranking: [...] }` or a bare array) into an
 * ordered list of addresses ready for `submit_results`. Throws a ResultsError
 * listing every problem the contract would reject, plus structural ones.
 */
export function parseResults(doc: unknown, opts: ParseResultsOptions = {}): ParsedResults {
  const rows = Array.isArray(doc) ? doc : isRecord(doc) && Array.isArray(doc.ranking) ? doc.ranking : null;
  if (!rows) {
    throw new ResultsError(["results document must be an array or an object with a `ranking` array"]);
  }
  const warnings: string[] = [];
  const issues: string[] = [];
  const entries: RankingEntry[] = [];

  rows.forEach((row, i) => {
    const label = `row ${i + 1}`;
    if (!isRecord(row)) {
      issues.push(`${label}: not an object`);
      return;
    }
    let place: unknown = row.place;
    if (typeof place === "string" && /^\s*\d+\s*$/.test(place)) {
      warnings.push(`${label}: place given as the string "${place}", coerced`);
      place = Number(place.trim());
    }
    if (typeof place !== "number" || !Number.isInteger(place) || place < 1) {
      issues.push(`${label}: place must be a positive integer, got ${JSON.stringify(row.place)}`);
      return;
    }
    const gamertag = cleanString(row.gamertag, `${label} gamertag`, warnings) ?? "";
    const address = cleanString(row.address, `${label} address`, warnings);
    if (address === null) {
      issues.push(`${label} (place ${place}): address is required`);
      return;
    }
    if (!isValidAddress(address)) {
      issues.push(`${label} (place ${place}, ${gamertag}): "${address}" is not a valid Stellar address`);
      return;
    }
    const entry: RankingEntry = { place, gamertag, address };
    if (typeof row.seat === "number") entry.seat = row.seat;
    entries.push(entry);
  });

  entries.sort((a, b) => a.place - b.place);

  // places must be exactly 1..n
  const byPlace = new Map<number, RankingEntry[]>();
  for (const e of entries) {
    byPlace.set(e.place, [...(byPlace.get(e.place) ?? []), e]);
  }
  for (const [place, list] of byPlace) {
    if (list.length > 1) {
      issues.push(`place ${place} appears ${list.length} times (${list.map((e) => e.gamertag).join(", ")})`);
    }
  }
  for (let p = 1; p <= entries.length; p++) {
    if (!byPlace.has(p)) {
      issues.push(`places are not consecutive: place ${p} is missing`);
    }
  }

  // the contract rejects duplicate addresses
  const byAddress = new Map<string, RankingEntry[]>();
  for (const e of entries) {
    byAddress.set(e.address, [...(byAddress.get(e.address) ?? []), e]);
  }
  for (const [address, list] of byAddress) {
    if (list.length > 1) {
      issues.push(
        `duplicate address ${address} (${list[0].gamertag}) at places ${list.map((e) => e.place).join(" and ")}: the contract rejects this ranking (DuplicateInRanking)`,
      );
    }
  }

  // the contract rejects unregistered addresses
  if (opts.roster) {
    const rosterByAddress = new Map(opts.roster.map((r) => [r.address, r]));
    for (const e of entries) {
      const r = rosterByAddress.get(e.address);
      if (!r) {
        issues.push(`place ${e.place} (${e.gamertag}): ${e.address} is not on the roster (NotRegistered)`);
      } else if (r.gamertag !== e.gamertag) {
        if (r.gamertag.toLowerCase() === e.gamertag.toLowerCase()) {
          warnings.push(`place ${e.place}: gamertag "${e.gamertag}" differs in case from roster "${r.gamertag}"`);
        } else {
          warnings.push(`place ${e.place}: gamertag "${e.gamertag}" does not match roster "${r.gamertag}" for ${e.address}`);
        }
      }
    }
  }

  if (entries.length === 0) {
    issues.push("ranking is empty (InvalidRanking)");
  }
  if (opts.maxPlaces !== undefined && entries.length > opts.maxPlaces) {
    issues.push(`ranking has ${entries.length} places but the placement table has ${opts.maxPlaces} (InvalidRanking)`);
  }

  if (issues.length > 0) {
    throw new ResultsError(issues, warnings);
  }
  return { ranking: entries, addresses: entries.map((e) => e.address), warnings };
}

function cleanString(v: unknown, label: string, warnings: string[]): string | null {
  if (typeof v !== "string") {
    return null;
  }
  const trimmed = v.trim();
  if (trimmed !== v) {
    warnings.push(`${label}: surrounding whitespace removed`);
  }
  return trimmed.length > 0 ? trimmed : null;
}
