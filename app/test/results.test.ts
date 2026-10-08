import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { Keypair, StrKey } from "@stellar/stellar-sdk";
import { ResultsError, isValidAddress, parseResults, readRoster } from "../src/results.ts";

const seed = (name: string): unknown => JSON.parse(readFileSync(new URL(`../../data/seed/${name}`, import.meta.url), "utf8"));
const roster = () => readRoster(seed("roster.json"));
const G = () => Keypair.random().publicKey();

test("seed roster.json: 64 valid players, whitespace and tag collisions reported as warnings", () => {
  const { players, warnings } = roster();
  assert.equal(players.length, 64);
  assert.ok(players.every((p) => StrKey.isValidEd25519PublicKey(p.address)));
  assert.equal(new Set(players.map((p) => p.address)).size, 64, "addresses are unique");
  assert.ok(warnings.some((w) => /roster row 18 address: surrounding whitespace removed/.test(w)), warnings.join("\n"));
  assert.ok(warnings.some((w) => /gamertag "ghost" also used by seat 39/.test(w)), warnings.join("\n"));
  assert.equal(players[17].address, players[17].address.trim());
  assert.equal(players[8].gamertag, "AceOfSpades", "leading space trimmed");
});

test("readRoster rejects invalid and duplicate addresses", () => {
  const a = G();
  assert.throws(
    () => readRoster([{ gamertag: "a", address: a }, { gamertag: "b", address: a }]),
    (e: unknown) => e instanceof ResultsError && /already used by seat 0/.test(e.issues[0]),
  );
  assert.throws(
    () => readRoster({ players: [{ gamertag: "a", address: a.toLowerCase() }] }),
    (e: unknown) => e instanceof ResultsError && /not a valid Stellar address/.test(e.issues[0]),
  );
  assert.throws(() => readRoster({ nope: true }), ResultsError);
});

test("seed results.json (raw export) is rejected because seat 0 appears at places 1 and 12", () => {
  const { players } = roster();
  let caught: ResultsError | undefined;
  try {
    parseResults(seed("results.json"), { roster: players, maxPlaces: 16 });
  } catch (e) {
    caught = e as ResultsError;
  }
  assert.ok(caught instanceof ResultsError, "expected a ResultsError");
  assert.equal(caught.issues.length, 1, caught.issues.join("\n"));
  assert.match(caught.issues[0], /duplicate address G[A-Z2-7]{55} \(xXShadowStrikeXx\) at places 1 and 12/);
  assert.match(caught.issues[0], /DuplicateInRanking/);
  // the messy-but-tolerated rows are surfaced as warnings, not errors
  assert.ok(caught.warnings.some((w) => /row 7: place given as the string "7"/.test(w)), caught.warnings.join("\n"));
  assert.ok(caught.warnings.some((w) => /row 5 address: surrounding whitespace removed/.test(w)), caught.warnings.join("\n"));
  assert.ok(caught.warnings.some((w) => /place 9: gamertag "MRROBOT" differs in case from roster "MrRobot"/.test(w)), caught.warnings.join("\n"));
});

test("seed results-fixed.json parses to 16 addresses in place order, all on the roster", () => {
  const { players } = roster();
  const { ranking, addresses, warnings } = parseResults(seed("results-fixed.json"), { roster: players, maxPlaces: 16 });
  assert.equal(addresses.length, 16);
  assert.deepEqual(warnings, []);
  assert.equal(addresses[0], players[0].address, "champion is seat 0");
  assert.equal(addresses[15], players[45].address, "16th place is seat 45");
  assert.deepEqual(ranking.map((r) => r.place), Array.from({ length: 16 }, (_, i) => i + 1));
  assert.equal(new Set(addresses).size, 16);
});

test("parseResults enforces the contract's ranking rules before any transaction is built", () => {
  const { players } = roster();
  const fixed = seed("results-fixed.json");
  // longer than the placement table
  assert.throws(
    () => parseResults(fixed, { roster: players, maxPlaces: 8 }),
    (e: unknown) => e instanceof ResultsError && /16 places but the placement table has 8 \(InvalidRanking\)/.test(e.issues.join()),
  );
  // unregistered address
  assert.throws(
    () => parseResults(fixed, { roster: players.slice(1), maxPlaces: 16 }),
    (e: unknown) => e instanceof ResultsError && /place 1 \(xXShadowStrikeXx\): .* is not on the roster \(NotRegistered\)/.test(e.issues.join()),
  );
  // gaps and repeated places
  assert.throws(
    () => parseResults([{ place: 1, gamertag: "a", address: G() }, { place: 3, gamertag: "b", address: G() }]),
    (e: unknown) => e instanceof ResultsError && /place 2 is missing/.test(e.issues.join()),
  );
  assert.throws(
    () => parseResults([{ place: 1, gamertag: "a", address: G() }, { place: 1, gamertag: "b", address: G() }]),
    (e: unknown) => e instanceof ResultsError && /place 1 appears 2 times/.test(e.issues.join()),
  );
  // empty, malformed, bad address
  assert.throws(() => parseResults({ ranking: [] }), (e: unknown) => e instanceof ResultsError && /empty/.test(e.issues[0]));
  assert.throws(() => parseResults({ nope: [] }), ResultsError);
  assert.throws(
    () => parseResults([{ place: 1, gamertag: "a", address: "not-an-address" }]),
    (e: unknown) => e instanceof ResultsError && /not a valid Stellar address/.test(e.issues[0]),
  );
  assert.throws(
    () => parseResults([{ place: "first", gamertag: "a", address: G() }]),
    (e: unknown) => e instanceof ResultsError && /place must be a positive integer/.test(e.issues[0]),
  );
});

test("isValidAddress accepts G and C addresses only", () => {
  assert.ok(isValidAddress(G()));
  assert.ok(isValidAddress(StrKey.encodeContract(Buffer.alloc(32, 7))));
  assert.ok(!isValidAddress(G().slice(0, -1)));
  assert.ok(!isValidAddress(StrKey.encodeMed25519PublicKey(Buffer.alloc(40, 1))));
  assert.ok(!isValidAddress(""));
});
