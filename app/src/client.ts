// Contract client built from the spec extracted from the wasm (src/spec.json),
// so no RPC round-trip is needed to construct it. Invocations still need
// STELLAR_RPC_URL: read-only calls simulate, mutating calls sign and send.

import { Keypair, contract } from "@stellar/stellar-sdk";
import specJson from "./spec.json" with { type: "json" };
import type { Config } from "./config.ts";

/** Contract function names, as exported by the wasm. */
export const FUNCTIONS: readonly string[] = specJson.functions;

export function loadSpec(): contract.Spec {
  return new contract.Spec(specJson.spec_base64);
}

export interface MakeClientOptions {
  signer?: Keypair;
  publicKey?: string;
}

export function makeClient(cfg: Config, opts: MakeClientOptions = {}): contract.Client {
  if (!cfg.contractId) {
    throw new Error("PRIZEVAULT_CONTRACT_ID (or --contract-id) is required");
  }
  const base: contract.ClientOptions = {
    contractId: cfg.contractId,
    networkPassphrase: cfg.networkPassphrase,
    rpcUrl: cfg.rpcUrl,
    // plain http is only sensible for a local quickstart node
    allowHttp: cfg.rpcUrl.startsWith("http://"),
    publicKey: opts.signer?.publicKey() ?? opts.publicKey,
  };
  const options = opts.signer ? { ...base, ...contract.basicNodeSigner(opts.signer, cfg.networkPassphrase) } : base;
  return new contract.Client(loadSpec(), options);
}

export interface CallOutcome {
  result: unknown;
  txHash?: string;
}

type Method = (args: Record<string, unknown>) => Promise<contract.AssembledTransaction<unknown>>;

/**
 * Calls a contract function by name. With `send = false` the transaction is
 * only simulated and the simulated return value is returned; otherwise it is
 * signed with the client's signer and submitted.
 */
export async function invoke(
  client: contract.Client,
  method: string,
  args: Record<string, unknown>,
  send: boolean,
): Promise<CallOutcome> {
  const fn = (client as unknown as Record<string, unknown>)[method];
  if (typeof fn !== "function") {
    throw new Error(`contract has no function "${method}" (known: ${FUNCTIONS.join(", ")})`);
  }
  const tx = await (fn as Method).call(client, args);
  if (!send) {
    return { result: unwrap(tx.result) };
  }
  const sent = await tx.signAndSend();
  return { result: unwrap(sent.result), txHash: sent.sendTransactionResponse?.hash };
}

/** Functions returning Result<T, Error> come back as Ok/Err wrappers; unwrap them (Err throws). */
export function unwrap(v: unknown): unknown {
  if (v && typeof v === "object" && typeof (v as { unwrap?: unknown }).unwrap === "function") {
    return (v as { unwrap: () => unknown }).unwrap();
  }
  return v;
}
