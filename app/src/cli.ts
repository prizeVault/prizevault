#!/usr/bin/env node
// prizevault CLI: create|fund|open|register|results|confirm|reject|claim|refund|cancel|status|proof-of-funds
//
// Read-only commands simulate against STELLAR_RPC_URL; the others sign with
// STELLAR_SECRET_KEY and submit. `results --dry-run` validates an export
// without touching the network.

import { readFileSync, realpathSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { Keypair } from "@stellar/stellar-sdk";
import { COMMANDS, flagString, parseArgv, parseDeadline, parseId, usage, validateArgs } from "./args.ts";
import { invoke, makeClient } from "./client.ts";
import { loadConfig, type Config } from "./config.ts";
import { assertPlacements, formatAmount, parseAmount, PlacementsError, placementAmounts, readPlacements } from "./placements.ts";
import { normalizeStatus, proofOfFunds, renderProof, type TournamentView } from "./proof.ts";
import { parseResults, readRoster, ResultsError, type RosterEntry } from "./results.ts";

export type Print = (line: string) => void;

export async function main(argv: string[], out: Print = console.log, err: Print = console.error): Promise<number> {
  const parsed = parseArgv(argv);
  if (parsed.flags.help || parsed.command === undefined) {
    out(usage());
    return parsed.command === undefined && !parsed.flags.help ? 2 : 0;
  }
  const errors = validateArgs(parsed);
  if (errors.length > 0) {
    for (const e of errors) err(`error: ${e}`);
    err("");
    err(usage());
    return 2;
  }
  const command = parsed.command;
  const spec = COMMANDS[command];
  const cfg = loadConfig(process.env, {
    rpcUrl: flagString(parsed, "rpc-url"),
    networkPassphrase: flagString(parsed, "network-passphrase"),
    contractId: flagString(parsed, "contract-id"),
    secretKey: flagString(parsed, "secret-key"),
  });
  const json = parsed.flags.json === true;
  const now = Math.floor(Date.now() / 1000);

  try {
    // ---- offline path: validate a results export without any network
    if (command === "results" && parsed.flags["dry-run"]) {
      const { addresses, warnings } = loadResults(flagString(parsed, "file")!, flagString(parsed, "roster"), undefined);
      for (const w of warnings) err(`warning: ${w}`);
      out(`ranking is valid: ${addresses.length} places`);
      addresses.forEach((a, i) => out(`  ${String(i + 1).padStart(3)}. ${a}`));
      out("dry run: nothing submitted");
      return 0;
    }

    const signer = spec.readonly ? undefined : requireSigner(cfg);
    const client = makeClient(cfg, { signer });
    const send = !spec.readonly;
    const id = parsed.flags.id !== undefined ? parseId(flagString(parsed, "id")!) : undefined;
    const me = signer?.publicKey();

    switch (command) {
      case "create": {
        const placementsDoc = readJson(flagString(parsed, "placements")!);
        const { bps, warnings } = readPlacements(placementsDoc);
        for (const w of warnings) err(`warning: ${w}`);
        assertPlacements(bps);
        const pool = parseAmount(flagString(parsed, "pool")!, cfg.tokenDecimals);
        const token = flagString(parsed, "token") ?? cfg.tokenContractId;
        if (!token) throw new Error("--token or TOKEN_CONTRACT_ID is required");
        const args = {
          organizer: flagString(parsed, "organizer") ?? me,
          sponsor: flagString(parsed, "sponsor"),
          token,
          pool_required: pool,
          placements: bps,
          register_by: BigInt(parseDeadline(flagString(parsed, "register-by")!, now)),
          results_by: BigInt(parseDeadline(flagString(parsed, "results-by")!, now)),
          claim_by: BigInt(parseDeadline(flagString(parsed, "claim-by")!, now)),
          referee: flagString(parsed, "referee") ?? undefined,
        };
        const amounts = placementAmounts(pool, bps);
        out(`pool ${formatAmount(pool, cfg.tokenDecimals)} ${cfg.tokenSymbol} over ${bps.length} places (1st: ${formatAmount(amounts[0], cfg.tokenDecimals)}, last: ${formatAmount(amounts[amounts.length - 1], cfg.tokenDecimals)})`);
        const { result, txHash } = await invoke(client, "create", args, send);
        out(`created tournament #${String(result)}${hash(txHash)}`);
        return 0;
      }
      case "fund": {
        const amount = parseAmount(flagString(parsed, "amount")!, cfg.tokenDecimals);
        const { txHash } = await invoke(client, "fund", { id, from: flagString(parsed, "from") ?? me, amount }, send);
        out(`funded #${id} with ${formatAmount(amount, cfg.tokenDecimals)} ${cfg.tokenSymbol}${hash(txHash)}`);
        return 0;
      }
      case "open": {
        const { txHash } = await invoke(client, "open_registration", { id }, send);
        out(`registration open for #${id}: pool fully escrowed${hash(txHash)}`);
        return 0;
      }
      case "register": {
        const player = flagString(parsed, "player") ?? me;
        const { txHash } = await invoke(client, "register", { id, player }, send);
        out(`registered ${player} in #${id}${hash(txHash)}`);
        return 0;
      }
      case "results": {
        const status = normalizeStatus(unwrapStatus(await invoke(client, "status", { id }, false)));
        const { addresses, warnings } = loadResults(flagString(parsed, "file")!, flagString(parsed, "roster"), status.placements.length);
        for (const w of warnings) err(`warning: ${w}`);
        const { txHash } = await invoke(client, "submit_results", { id, ranking: addresses }, send);
        out(`submitted ${addresses.length} places for #${id}${status.referee ? " (waiting for referee)" : " (confirmed, no referee)"}${hash(txHash)}`);
        return 0;
      }
      case "confirm": {
        const { txHash } = await invoke(client, "confirm_results", { id }, send);
        out(`results confirmed for #${id}: prizes are claimable${hash(txHash)}`);
        return 0;
      }
      case "reject": {
        const { txHash } = await invoke(client, "reject_results", { id }, send);
        out(`results rejected for #${id}: organizer may resubmit${hash(txHash)}`);
        return 0;
      }
      case "claim": {
        const player = flagString(parsed, "player") ?? me;
        const { result, txHash } = await invoke(client, "claim", { id, player }, send);
        out(`claimed ${formatAmount(BigInt(String(result)), cfg.tokenDecimals)} ${cfg.tokenSymbol} for ${player}${hash(txHash)}`);
        return 0;
      }
      case "refund": {
        const { result, txHash } = await invoke(client, "refund_unclaimed", { id }, send);
        out(`refunded ${formatAmount(BigInt(String(result)), cfg.tokenDecimals)} ${cfg.tokenSymbol} of unclaimed prizes to the sponsor; #${id} closed${hash(txHash)}`);
        return 0;
      }
      case "cancel": {
        const method = parsed.flags.expired ? "cancel_expired" : "cancel";
        const { result, txHash } = await invoke(client, method, { id }, send);
        out(`cancelled #${id}; ${formatAmount(BigInt(String(result)), cfg.tokenDecimals)} ${cfg.tokenSymbol} refunded to the sponsor${hash(txHash)}`);
        return 0;
      }
      case "status": {
        const view = normalizeStatus(unwrapStatus(await invoke(client, "status", { id }, false)));
        if (json) {
          out(JSON.stringify(view, replacer, 2));
        } else {
          for (const [k, v] of Object.entries(view)) out(`${k.padEnd(14)}${typeof v === "bigint" ? v.toString() : Array.isArray(v) ? v.join(",") : String(v)}`);
        }
        return 0;
      }
      case "proof-of-funds": {
        const view = normalizeStatus(unwrapStatus(await invoke(client, "status", { id }, false)));
        const proof = proofOfFunds(view, now, { decimals: cfg.tokenDecimals, symbol: cfg.tokenSymbol, contractId: cfg.contractId, id });
        out(json ? JSON.stringify({ ...proof, tournament: view }, replacer, 2) : renderProof(proof));
        return 0;
      }
      default:
        err(`error: unhandled command ${command}`);
        return 2;
    }
  } catch (e) {
    if (e instanceof ResultsError || e instanceof PlacementsError) {
      err(`error: ${e.name}:`);
      for (const issue of e.issues) err(`  - ${issue}`);
      return 1;
    }
    err(`error: ${e instanceof Error ? e.message : String(e)}`);
    return 1;
  }
}

function requireSigner(cfg: Config): Keypair {
  if (!cfg.secretKey) {
    throw new Error("STELLAR_SECRET_KEY (or --secret-key) is required for this command");
  }
  return Keypair.fromSecret(cfg.secretKey);
}

function readJson(path: string): unknown {
  return JSON.parse(readFileSync(path, "utf8"));
}

function loadResults(file: string, rosterFile: string | undefined, maxPlaces: number | undefined) {
  let roster: RosterEntry[] | undefined;
  const warnings: string[] = [];
  if (rosterFile) {
    const r = readRoster(readJson(rosterFile));
    roster = r.players;
    warnings.push(...r.warnings);
  }
  const parsed = parseResults(readJson(file), { roster, maxPlaces });
  warnings.push(...parsed.warnings);
  return { addresses: parsed.addresses, warnings };
}

function unwrapStatus(outcome: { result: unknown }): unknown {
  return outcome.result;
}

function hash(txHash: string | undefined): string {
  return txHash ? ` (tx ${txHash})` : "";
}

function replacer(_key: string, value: unknown): unknown {
  return typeof value === "bigint" ? value.toString() : value;
}

export function describeView(view: TournamentView): string {
  return `${view.state}: ${view.poolFunded}/${view.poolRequired}`;
}

const invokedDirectly = (() => {
  try {
    return process.argv[1] !== undefined && import.meta.url === pathToFileURL(realpathSync(process.argv[1])).href;
  } catch {
    return false;
  }
})();

if (invokedDirectly) {
  main(process.argv.slice(2)).then((code) => process.exit(code));
}

// Entry point for the CLI tool
