# PrizeVault validation

Proof level today: **0** (idea backed by researched evidence; working software verified locally). Nothing in this file is user validation. **No external users yet.**

## Evidence tiers

### Researched evidence (cited)

From `research/04-problems-consumer-creator-services.md` §P8 and `research/08-final-selection.md` §01:

- Prize money is paid 30–180+ days late or never; the workflow is announce pool -> compete -> collect W-9/KYC -> sponsor invoices settle -> wires trickle out.
- Esports World Cup 2024: allegations of prize money, salaries and vendor invoices unpaid ~6 months after the event.
- YaLLa Esports bankruptcy: more than $700k of prize money unpaid.
- FISSURE acknowledged 15 teams as creditors (Astralis, FURIA, G2, Team Liquid) in June 2026 while planning a new $1M event.
- Fog Esports: 2+ months of salary arrears.
- Payout vendors (i-payout) market against "30-day waits"; Tipalti and manual banking are the Web2 alternatives; Community Gaming (Polygon) is the Web3 one, with current activity unverifiable; a Soroban tournament-escrow repo exists on testnet at 40% completion with 0 stars.
- "A culture of delayed payment" documented across two decades; tier-2/3 tournaments are the long tail; pools of $2k–$200k.
- Stellar-side facts used in the design (`research/01-stellar-ecosystem.md`): native USDC and other dollar stablecoins; persistent storage archives at TTL 0 and is restorable; per-transaction limits in the order of 100M instructions and 128 KiB entries; sub-cent base fees.

### Observed facts (this build environment only)

- `cargo test`: 46 tests pass in the Soroban host, including negative tests for every auth role and every deadline, and the 64-player scenario with exact payout, claim and refund amounts.
- `stellar contract build`: 20 384-byte wasm, 19 exported functions.
- `npm test`: 32 offline tests pass (placement math mirror, results/roster parsing on the seed files, argument parsing, offline transaction building, proof view).
- The testnet RPC, Horizon and Friendbot were unreachable from the build machine; `scripts/deploy-testnet.sh` was written and reviewed but not run.

### Team assumptions

- Tier-2/3 organisers can hold USDC on Stellar (directly or through a custodial partner) and are willing to lock the pool 1–4 weeks before the event.
- Sponsors see "funds escrowed before registration" as a benefit worth demanding, not a liquidity cost to avoid.
- Players and teams at this tier can receive a Stellar USDC payment (existing wallet, exchange deposit address, or an anchor) within a day of results.
- A referee role (a league admin, a co-organiser or a sponsor representative) is available for events where organiser-only results would not be accepted.

### Hypotheses (to test)

- H1: at least 5 of 10 interviewed TOs report a last-three-events payout time above 30 days.
- H2: a TO will run a real community event with the pool escrowed on testnet and at least 80% of winners claim within 7 days.
- H3: a TO or sponsor will pay 1% of the pool or $49 per event for the escrow plus proof-of-funds page.
- H4: sponsors, once shown the page, ask for it as a sponsorship condition for the next event.

### Simulated / demo data

`data/seed/` is synthetic: 64 gamertags with realistic mess (trailing spaces, unicode, a reused tag on two players, an address pasted with whitespace, a payout-redirect request in a note), a 16-place table copied from a percentage sheet (with a first draft that sums to 99.90% and is rejected), a raw bracket export where the champion is pasted again on row 12 (rejected by both the parser and the contract), the corrected export, and a 7 777.7777777 USDC pool chosen because it produces rounding dust. Addresses are valid Stellar public keys generated deterministically for the seed; no keys or funds behind them.

### Actual validation

None yet. No deployment, no tournament run, no interview conducted, no metric measured.

## Baseline and success metric

| Metric | Baseline (Researched) | Target after PrizeVault |
|---|---|---|
| Results-to-payout time for claimed prizes | 30–180+ days, sometimes never | under 24 hours |
| Share of the pool escrowed before registration opens | 0% (announced, not collected) | 100% |
| Unclaimed prize money returned to the sponsor within the claim window | not tracked / not returned | 100% |
| Sponsor ability to verify the pool independently | none | one link, any RPC |

## Experiment plan

**Week 1 - baseline interviews (H1).** Ten organisers recruited on Discord servers and Battlefy/Toornament communities. Structured questions: pool size and funding source of the last three events; date results were final vs. date the last prize was paid; who chased whom; whether a sponsor ever asked for proof of funds; what they would accept as a referee. Deliverable: a table of 30 payout timelines and the median delay. Kill signal: median under 14 days and nobody chasing payments.

**Week 2 - one real community tournament on testnet (H2).** A $500–$2 000 pool run by one of the interviewed TOs, with the pool escrowed through `scripts/deploy-testnet.sh` and the proof-of-funds link posted in the event announcement. Measure: hours from confirmed results to each claim, claim rate within 7 days, number of players who could not claim and why (no wallet, no trustline, wrong address), referee reaction time. Kill signal: claim rate under 50% or the TO unwilling to repeat.

**Willingness to pay (H3, same TO plus two others).** Offer the next event at 1% of the pool or $49 flat; record the answer and the objections.

**Sponsor pull (H4).** Show the proof-of-funds page to two sponsors of the interviewed TOs and ask whether they would require it.

## Killer questions

1. **Would the user care if it disappeared?** Plausible from the evidence (teams already threaten "escrow or we don't play" after the FISSURE case), untested with a real organiser.
2. **Did anyone outside the team use it?** No.
3. **Before/after measured?** No. Baselines above are researched from published cases, not measured with our users.
4. **Value lost without AI?** None by design: the product has no AI component. Results come from bracket tools and referees; an LLM would add nothing verifiable.
5. **Reason to keep using after the demo?** Hypothesis: every event needs a new escrow and sponsors, once they have seen a proof-of-funds link, will ask for it again. Untested.
6. **Would someone pay?** Hypothesis (1% of pool or $49/event). Untested.
7. **Does the chain create value?** Yes, in a specific way: the pool must exist before registration and anyone can verify it without trusting the organiser; the release rule (placement table, deadlines, refunds) is enforced by code; the escrow is not the organiser's bank account. A Web2 escrow service could offer the same promise but not the independent verification.
8. **Why this chain?** Stellar has native USDC and other dollar stablecoins, sub-cent fees for 64–256 payouts, anchor and cash-out coverage in the regions tier-2 teams live in, and cheap accounts for players without wallets. The alternative (Arbitrum/Xai) requires EVM wallets from non-crypto teams and has no cash-out edge for them.
