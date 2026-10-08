import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  BPS_DENOMINATOR,
  MAX_PLACEMENTS,
  PlacementsError,
  assertPlacements,
  formatAmount,
  parseAmount,
  placementAmounts,
  readPlacements,
  validatePlacements,
} from "../src/placements.ts";

const seed = (name: string): unknown => JSON.parse(readFileSync(new URL(`../../data/seed/${name}`, import.meta.url), "utf8"));

/** Same pool and table as the Rust scenario test; the expected numbers are pinned in both. */
const POOL = 77_777_777_777n;
const TABLE = [3000, 1800, 1200, 900, 600, 600, 400, 400, 200, 200, 200, 200, 75, 75, 75, 75];

test("seed placements.json reads to a valid 16-place table", () => {
  const { bps, labels, warnings } = readPlacements(seed("placements.json"));
  assert.deepEqual(bps, TABLE);
  assert.equal(labels[0], "Champion");
  assert.deepEqual(warnings, []);
  assert.deepEqual(validatePlacements(bps), []);
  assert.equal(bps.reduce((a, b) => a + b, 0), BPS_DENOMINATOR);
});

test("seed placements-invalid.json is coerced with a warning and then rejected for summing to 9,990", () => {
  const { bps, warnings } = readPlacements(seed("placements-invalid.json"));
  assert.equal(warnings.length, 1);
  assert.match(warnings[0], /row 13: bps given as the string "70"/);
  const issues = validatePlacements(bps);
  assert.equal(issues.length, 1);
  assert.match(issues[0], /sum to 9990 bps \(99\.90%/);
  assert.throws(() => assertPlacements(bps), PlacementsError);
});

test("readPlacements accepts bare arrays and objects, rejects garbage", () => {
  assert.deepEqual(readPlacements([10_000]).bps, [10_000]);
  assert.deepEqual(readPlacements({ placements: [{ bps: 6000 }, { bps: 4000 }] }).bps, [6000, 4000]);
  const { warnings } = readPlacements([{ place: 2, bps: 6000 }, { place: 1, bps: 4000 }]);
  assert.equal(warnings.length, 2, "place fields disagree with file order");
  assert.throws(() => readPlacements({ nope: [] }), PlacementsError);
  assert.throws(() => readPlacements([{ bps: 12.5 }]), /must be an integer/);
  assert.throws(() => readPlacements([{ bps: "30%" }]), /must be an integer/);
});

test("validatePlacements mirrors the contract rules", () => {
  assert.deepEqual(validatePlacements([10_000]), []);
  assert.match(validatePlacements([])[0], /empty/);
  assert.match(validatePlacements([5000, 4999]).join(), /sum to 9999/);
  assert.match(validatePlacements([5000, 5001]).join(), /sum to 10001/);
  assert.match(validatePlacements([10_000, 0]).join(), /place 2: bps must be a positive integer/);
  assert.match(validatePlacements([11_000, -1000]).join(), /place 2: bps must be a positive integer/);
  const tooMany = new Array(MAX_PLACEMENTS + 1).fill(1);
  tooMany[0] = BPS_DENOMINATOR - MAX_PLACEMENTS;
  assert.match(validatePlacements(tooMany).join(), /too many placements: 257 > 256/);
});

test("placementAmounts matches the contract: floor shares, dust on the last place, exact sum", () => {
  const amounts = placementAmounts(POOL, TABLE);
  assert.equal(amounts.length, 16);
  assert.equal(amounts[0], 23_333_333_333n);
  assert.equal(amounts[1], 13_999_999_999n);
  assert.equal(amounts[3], 6_999_999_999n);
  assert.equal(amounts[12], 583_333_333n);
  assert.equal(amounts[15], 583_333_340n, "last place gets its floor share plus 7 stroops of dust");
  assert.equal(amounts.reduce((a, b) => a + b, 0n), POOL);
  // the exact unclaimed remainder the Rust scenario refunds to the sponsor
  const unclaimed = amounts[3] + amounts[5] + amounts[6] + amounts[10] + amounts[11];
  assert.equal(unclaimed, 17_888_888_886n);
  assert.equal(POOL - unclaimed, 59_888_888_891n);
});

test("placementAmounts edge cases", () => {
  assert.deepEqual(placementAmounts(12_345n, [10_000]), [12_345n]);
  assert.deepEqual(placementAmounts(3n, [5000, 3000, 2000]), [1n, 0n, 2n]);
  assert.deepEqual(placementAmounts(0n, [5000, 5000]), [0n, 0n]);
  assert.deepEqual(placementAmounts(10n, []), []);
});

test("parseAmount and formatAmount round-trip 7-decimal USDC", () => {
  assert.equal(parseAmount("7777.7777777"), 77_777_777_777n);
  assert.equal(parseAmount("5,000"), 50_000_000_000n);
  assert.equal(parseAmount("5_000.5"), 50_005_000_000n);
  assert.equal(parseAmount(" 0.0000001 "), 1n);
  assert.equal(parseAmount("12", 0), 12n);
  assert.throws(() => parseAmount("1.12345678"), /more than 7 decimal places/);
  assert.throws(() => parseAmount("abc"), /invalid amount/);
  assert.throws(() => parseAmount("-5"), /invalid amount/);
  assert.equal(formatAmount(77_777_777_777n), "7777.7777777");
  assert.equal(formatAmount(50_000_000_000n), "5000.00");
  assert.equal(formatAmount(583_333_340n), "58.333334");
  assert.equal(formatAmount(-1n), "-0.0000001");
  assert.equal(formatAmount(parseAmount("123.4500000")), "123.45");
});
