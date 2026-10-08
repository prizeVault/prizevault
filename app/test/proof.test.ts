import { test } from "node:test";
import assert from "node:assert/strict";
import { Keypair, StrKey } from "@stellar/stellar-sdk";
import { normalizeState, normalizeStatus, proofOfFunds, renderProof, type TournamentView } from "../src/proof.ts";

const T0 = 1_790_000_000;
const DAY = 86_400;
const G = Keypair.random().publicKey();
const C = StrKey.encodeContract(Buffer.alloc(32, 3));
const TABLE = [3000, 1800, 1200, 900, 600, 600, 400, 400, 200, 200, 200, 200, 75, 75, 75, 75];

function view(over: Partial<TournamentView> = {}): TournamentView {
  return {
    organizer: G,
    sponsor: G,
    token: C,
    poolRequired: 77_777_777_777n,
    poolFunded: 77_777_777_777n,
    placements: TABLE,
    registerBy: T0 + 7 * DAY,
    resultsBy: T0 + 14 * DAY,
    claimBy: T0 + 44 * DAY,
    referee: null,
    state: "Open",
    ...over,
  };
}

test("normalizeStatus accepts every shape the SDK produces", () => {
  const raw = {
    organizer: G,
    sponsor: { toString: () => G },
    token: C,
    pool_required: "77777777777",
    pool_funded: 50_000_000_000n,
    placements: [3000n, 7000n],
    register_by: 1790604800n,
    results_by: 1791209600,
    claim_by: "1793801600",
    referee: undefined,
    state: { tag: "ResultsSubmitted", values: undefined },
  };
  const v = normalizeStatus(raw);
  assert.equal(v.sponsor, G);
  assert.equal(v.poolRequired, 77_777_777_777n);
  assert.equal(v.poolFunded, 50_000_000_000n);
  assert.deepEqual(v.placements, [3000, 7000]);
  assert.equal(v.registerBy, 1_790_604_800);
  assert.equal(v.claimBy, 1_793_801_600);
  assert.equal(v.referee, null);
  assert.equal(v.state, "ResultsSubmitted");
  assert.equal(normalizeStatus({ ...raw, referee: G, state: ["Confirmed"] }).referee, G);
  assert.equal(normalizeState("Closed"), "Closed");
  assert.throws(() => normalizeState(["Paused"]), /unknown state/);
  assert.throws(() => normalizeStatus({ ...raw, pool_funded: "lots" }), /pool_funded is not an integer/);
  assert.throws(() => normalizeStatus(null), /expected an object/);
});

test("proof badge and phase follow funding and state", () => {
  const now = T0 + DAY;
  const opts = { decimals: 7, symbol: "USDC", id: 3 };
  const full = proofOfFunds(view(), now, opts);
  assert.equal(full.badge, "FUNDED");
  assert.equal(full.fundedBps, 10_000);
  assert.equal(full.phase, "registration open");
  assert.deepEqual(full.nextDeadline, { label: "register by", at: T0 + 7 * DAY });
  assert.match(renderProof(full), /PrizeVault tournament #3: FUNDED/);
  assert.match(renderProof(full), /escrowed {6}7777\.7777777 USDC of 7777\.7777777 USDC \(100\.00%\)/);
  assert.match(renderProof(full), /16\. {3}0\.75% {2}58\.333334 USDC/, "last place shows its share including dust");

  const partial = proofOfFunds(view({ state: "Created", poolFunded: 50_000_000_000n }), now, opts);
  assert.equal(partial.badge, "PARTIALLY FUNDED");
  assert.equal(partial.fundedBps, 6428);
  assert.match(partial.phase, /cannot open until it is fully escrowed/);

  const empty = proofOfFunds(view({ state: "Created", poolFunded: 0n }), now, opts);
  assert.equal(empty.badge, "UNFUNDED");
  assert.equal(empty.fundedBps, 0);

  const closedReg = proofOfFunds(view(), T0 + 8 * DAY, opts);
  assert.equal(closedReg.phase, "registration closed, waiting for results");
  assert.equal(closedReg.nextDeadline?.label, "results by");

  const submitted = proofOfFunds(view({ state: "ResultsSubmitted", referee: G }), T0 + 8 * DAY, opts);
  assert.match(submitted.phase, /waiting for the referee/);

  const confirmed = proofOfFunds(view({ state: "Confirmed" }), T0 + 10 * DAY, opts);
  assert.match(confirmed.phase, /winners can claim/);
  assert.equal(confirmed.nextDeadline?.label, "claim by");
  const sweepable = proofOfFunds(view({ state: "Confirmed" }), T0 + 45 * DAY, opts);
  assert.match(sweepable.phase, /claim window closed/);
  assert.equal(sweepable.nextDeadline, null);

  assert.equal(proofOfFunds(view({ state: "Cancelled", poolFunded: 0n }), now, opts).badge, "CANCELLED");
  assert.equal(proofOfFunds(view({ state: "Closed" }), now, opts).badge, "CLOSED");
});

test("proof lines show deadlines as ISO with relative hints", () => {
  const p = proofOfFunds(view(), T0, { symbol: "USDC" });
  const text = renderProof(p);
  // T0 = 1_790_000_000 = 2026-09-21T14:13:20Z
  assert.match(text, /register by {3}2026-09-28T14:13:20Z \(in 7d\)/);
  assert.match(text, /claim by {6}2026-11-04T14:13:20Z \(in 44d\)/);
  assert.match(text, /referee {7}none \(organizer's results are final\)/);
  const late = renderProof(proofOfFunds(view(), T0 + 8 * DAY, { symbol: "USDC" }));
  assert.match(late, /register by .* \(1d ago\)/);
});
