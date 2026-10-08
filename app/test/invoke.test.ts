import { test } from "node:test";
import assert from "node:assert/strict";
import { Keypair, StrKey, TransactionBuilder, contract, scValToNative, type Transaction } from "@stellar/stellar-sdk";
import { FUNCTIONS, loadSpec, makeClient, unwrap } from "../src/client.ts";
import { TESTNET_PASSPHRASE, loadConfig } from "../src/config.ts";
import { buildUnsignedTx, describeInvocation, encodeArgs } from "../src/invoke.ts";

const CONTRACT = StrKey.encodeContract(Buffer.alloc(32, 1));
const TOKEN = StrKey.encodeContract(Buffer.alloc(32, 2));
const SOURCE = Keypair.random().publicKey();

const EXPECTED_FUNCTIONS = [
  "cancel", "cancel_expired", "claim", "claimable", "claimed", "confirm_results", "count", "create", "fund",
  "is_registered", "open_registration", "paid_out", "ranking", "refund_unclaimed", "register", "reject_results",
  "roster", "status", "submit_results",
];

test("the extracted spec exposes every contract function", () => {
  assert.deepEqual([...FUNCTIONS].sort(), EXPECTED_FUNCTIONS);
  const spec = loadSpec();
  assert.deepEqual(spec.funcs().map((f) => String(f.name)).sort(), EXPECTED_FUNCTIONS);
  const errors = spec.errorCases();
  assert.equal(errors.length, 20, "20 contract error codes");
});

test("status transaction is built offline with a single u32 argument", () => {
  const tx = buildUnsignedTx({
    contractId: CONTRACT,
    method: "status",
    args: { id: 7 },
    spec: loadSpec(),
    sourcePublicKey: SOURCE,
    sequence: "0",
    networkPassphrase: TESTNET_PASSPHRASE,
  });
  assert.equal(tx.operations.length, 1);
  assert.equal(tx.source, SOURCE);
  assert.equal(tx.fee, "100");
  const d = describeInvocation(tx);
  assert.deepEqual(d, { contractId: CONTRACT, method: "status", argCount: 1 });
  // and it round-trips through transaction XDR
  const b64 = tx.toXDR();
  assert.ok(b64.length > 100);
  const back = TransactionBuilder.fromXDR(b64, TESTNET_PASSPHRASE) as Transaction;
  assert.deepEqual(describeInvocation(back), d);
  const [arg] = encodeArgs(loadSpec(), "status", { id: 7 });
  assert.equal(scValToNative(arg), 7);
});

test("create and submit_results arguments are encoded from the spec (Option<Address>, Vec<u32>, Vec<Address>, i128)", () => {
  const spec = loadSpec();
  const organizer = Keypair.random().publicKey();
  const base = {
    organizer,
    sponsor: Keypair.random().publicKey(),
    token: TOKEN,
    pool_required: 77_777_777_777n,
    placements: [3000, 7000],
    register_by: 1_790_604_800n,
    results_by: 1_791_209_600n,
    claim_by: 1_793_801_600n,
  };
  assert.equal(encodeArgs(spec, "create", { ...base, referee: undefined }).length, 9);
  assert.equal(encodeArgs(spec, "create", { ...base, referee: organizer }).length, 9);
  const ranking = [Keypair.random().publicKey(), Keypair.random().publicKey(), Keypair.random().publicKey()];
  const tx = buildUnsignedTx({
    contractId: CONTRACT,
    method: "submit_results",
    args: { id: 0, ranking },
    spec,
    sourcePublicKey: organizer,
    sequence: "123",
    networkPassphrase: TESTNET_PASSPHRASE,
  });
  assert.deepEqual(describeInvocation(tx), { contractId: CONTRACT, method: "submit_results", argCount: 2 });
  assert.equal(tx.sequence, "124");
  // a wrong argument name is caught before anything is sent
  assert.throws(() => encodeArgs(spec, "fund", { id: 0, amount: 5n }), /from/);
  assert.throws(() => encodeArgs(spec, "nope", {}), /nope/);
});

test("makeClient builds a spec-backed contract.Client without RPC access", () => {
  const cfg = loadConfig({ PRIZEVAULT_CONTRACT_ID: CONTRACT, STELLAR_RPC_URL: "http://127.0.0.1:9", TOKEN_DECIMALS: "7" });
  const client = makeClient(cfg);
  assert.ok(client instanceof contract.Client);
  for (const name of EXPECTED_FUNCTIONS) {
    assert.equal(typeof (client as unknown as Record<string, unknown>)[name], "function", `client.${name}`);
  }
  const signed = makeClient(cfg, { signer: Keypair.random() });
  assert.equal(typeof signed.options.signTransaction, "function");
  assert.throws(() => makeClient(loadConfig({})), /PRIZEVAULT_CONTRACT_ID/);
});

test("loadConfig defaults and validation", () => {
  const cfg = loadConfig({});
  assert.equal(cfg.networkPassphrase, TESTNET_PASSPHRASE);
  assert.match(cfg.rpcUrl, /soroban-testnet/);
  assert.equal(cfg.tokenDecimals, 7);
  assert.equal(cfg.tokenSymbol, "USDC");
  assert.throws(() => loadConfig({ TOKEN_DECIMALS: "abc" }), /TOKEN_DECIMALS/);
  assert.equal(loadConfig({ TOKEN_SYMBOL: "TUSD" }, { tokenSymbol: "X" }).tokenSymbol, "X");
});

test("unwrap passes plain values through and unwraps Ok/Err results", () => {
  assert.equal(unwrap(5n), 5n);
  assert.equal(unwrap(new contract.Ok(3)), 3);
  assert.throws(() => unwrap(new contract.Err({ message: "boom" } as never)));
});
