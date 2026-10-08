//! PrizeVault: esports prize-pool escrow.
//!
//! One contract instance hosts many tournaments. For each tournament the sponsor
//! deposits the full prize pool *before* registration can open (the on-chain proof
//! of funds), the organizer submits a ranking after the event, an optional referee
//! confirms or rejects it, winners claim their placement share, and whatever is
//! still unclaimed after the claim window returns to the sponsor.
//!
//! Deadline semantics used throughout: an action that must happen "before X"
//! requires `now < X`; an action that becomes possible "after X" requires
//! `now >= X`. `now` is the ledger close timestamp in seconds.
#![no_std]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, token, Address, Env, Map,
    Vec,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Placement weights are expressed in basis points and must sum to this value.
pub const BPS_DENOMINATOR: u32 = 10_000;
/// Upper bound on the placement table length (keeps the ranking entry small).
pub const MAX_PLACEMENTS: u32 = 256;
/// Upper bound on registered players per tournament (keeps the roster entry
/// well under the ledger-entry size limit).
pub const MAX_ROSTER: u32 = 1_024;
/// Largest pool accepted, so `pool * bps` can never overflow an i128.
pub const MAX_POOL: i128 = i128::MAX / (BPS_DENOMINATOR as i128);

/// Ledgers per day assuming ~5 second ledger close times.
const DAY_LEDGERS: u32 = 17_280;
/// When an entry's remaining TTL drops below this many ledgers, extend it.
const TTL_THRESHOLD: u32 = 30 * DAY_LEDGERS;
/// Extend entries so they live this many ledgers from the current one (~120 days).
const TTL_EXTEND_TO: u32 = 120 * DAY_LEDGERS;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// No tournament with this id.
    NotFound = 1,
    /// The action is not allowed in the tournament's current state.
    InvalidState = 2,
    /// Placements are empty, too long, contain a zero, or do not sum to 10_000 bps.
    InvalidPlacements = 3,
    /// Deadlines must satisfy now < register_by < results_by < claim_by.
    InvalidDeadlines = 4,
    /// Amount must be positive and the pool must fit within MAX_POOL.
    InvalidAmount = 5,
    /// Funding would exceed pool_required.
    Overfunded = 6,
    /// Registration cannot open until pool_funded == pool_required.
    Underfunded = 7,
    /// Registration deadline has passed.
    RegistrationClosed = 8,
    /// Results cannot be submitted while registration is still open.
    RegistrationStillOpen = 9,
    /// Player already on the roster.
    AlreadyRegistered = 10,
    /// Roster reached MAX_ROSTER.
    RosterFull = 11,
    /// results_by has passed; results can no longer be submitted or confirmed.
    ResultsDeadlinePassed = 12,
    /// results_by has not passed yet; cancel_expired is not available.
    ResultsDeadlineNotReached = 13,
    /// Ranking is empty or longer than the placement table.
    InvalidRanking = 14,
    /// An address in the ranking is not on the roster.
    NotRegistered = 15,
    /// The same address appears twice in the ranking.
    DuplicateInRanking = 16,
    /// Tournament has no referee; confirm/reject do not apply.
    NoReferee = 17,
    /// claim_by has passed; claims are closed.
    ClaimWindowClosed = 18,
    /// claim_by has not passed yet; refund_unclaimed is not available.
    ClaimWindowOpen = 19,
    /// Player has no claimable prize.
    NothingToClaim = 20,
}

#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    /// Created; sponsor is funding the pool.
    Created,
    /// Fully funded; players can register until register_by. After register_by
    /// the tournament stays Open (registration closed) until results arrive.
    Open,
    /// Organizer submitted a ranking; waiting for the referee.
    ResultsSubmitted,
    /// Ranking confirmed; winners can claim until claim_by.
    Confirmed,
    /// Cancelled by the organizer or expired; pool refunded to the sponsor.
    Cancelled,
    /// Claim window over; unclaimed remainder refunded to the sponsor.
    Closed,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tournament {
    pub organizer: Address,
    pub sponsor: Address,
    pub token: Address,
    pub pool_required: i128,
    pub pool_funded: i128,
    /// Placement shares in basis points, first place first; sums to 10_000.
    pub placements: Vec<u32>,
    pub register_by: u64,
    pub results_by: u64,
    pub claim_by: u64,
    pub referee: Option<Address>,
    pub state: State,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Next tournament id (instance storage).
    Counter,
    Tournament(u32),
    /// Map<Address, bool>: registered players.
    Roster(u32),
    /// Vec<Address>: submitted ranking, first place first.
    Ranking(u32),
    /// Map<Address, i128>: prizes not yet claimed.
    Claimable(u32),
    /// Map<Address, i128>: prizes already paid out, per player.
    Claimed(u32),
    /// i128: total paid out so far.
    PaidOut(u32),
}

// ---------------------------------------------------------------------------
// Events (one per state change)
// ---------------------------------------------------------------------------

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TournamentCreated {
    #[topic]
    pub id: u32,
    pub organizer: Address,
    pub sponsor: Address,
    pub token: Address,
    pub pool_required: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolFunded {
    #[topic]
    pub id: u32,
    pub from: Address,
    pub amount: i128,
    pub pool_funded: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistrationOpened {
    #[topic]
    pub id: u32,
    pub pool_funded: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerRegistered {
    #[topic]
    pub id: u32,
    pub player: Address,
    pub roster_size: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResultsSubmitted {
    #[topic]
    pub id: u32,
    pub ranking_len: u32,
    pub needs_referee: bool,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResultsConfirmed {
    #[topic]
    pub id: u32,
    pub ranking_len: u32,
    pub payable_total: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResultsRejected {
    #[topic]
    pub id: u32,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrizeClaimed {
    #[topic]
    pub id: u32,
    #[topic]
    pub player: Address,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnclaimedRefunded {
    #[topic]
    pub id: u32,
    pub sponsor: Address,
    pub amount: i128,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TournamentCancelled {
    #[topic]
    pub id: u32,
    pub refunded: i128,
    pub expired: bool,
}

// ---------------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------------

#[contract]
pub struct PrizeVault;

#[contractimpl]
impl PrizeVault {
    /// Create a tournament. Returns its id. No funds move here.
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        env: Env,
        organizer: Address,
        sponsor: Address,
        token: Address,
        pool_required: i128,
        placements: Vec<u32>,
        register_by: u64,
        results_by: u64,
        claim_by: u64,
        referee: Option<Address>,
    ) -> Result<u32, Error> {
        organizer.require_auth();
        if pool_required <= 0 || pool_required > MAX_POOL {
            return Err(Error::InvalidAmount);
        }
        validate_placements(&placements)?;
        let now = env.ledger().timestamp();
        if !(now < register_by && register_by < results_by && results_by < claim_by) {
            return Err(Error::InvalidDeadlines);
        }

        let id = next_id(&env);
        let t = Tournament {
            organizer: organizer.clone(),
            sponsor: sponsor.clone(),
            token: token.clone(),
            pool_required,
            pool_funded: 0,
            placements,
            register_by,
            results_by,
            claim_by,
            referee,
            state: State::Created,
        };
        save_tournament(&env, id, &t);
        save_roster(&env, id, &Map::new(&env));
        save_paid_out(&env, id, 0);
        TournamentCreated {
            id,
            organizer,
            sponsor,
            token,
            pool_required,
        }
        .publish(&env);
        Ok(id)
    }

    /// Move `amount` of the tournament token from `from` into escrow. Anyone may
    /// fund; refunds always go to the tournament's sponsor. Funding above
    /// `pool_required` is rejected so the vault always holds exactly the
    /// announced pool once registration opens.
    pub fn fund(env: Env, id: u32, from: Address, amount: i128) -> Result<(), Error> {
        from.require_auth();
        let mut t = load_tournament(&env, id)?;
        if t.state != State::Created {
            return Err(Error::InvalidState);
        }
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if t.pool_funded + amount > t.pool_required {
            return Err(Error::Overfunded);
        }
        token::Client::new(&env, &t.token).transfer(&from, &env.current_contract_address(), &amount);
        t.pool_funded += amount;
        save_tournament(&env, id, &t);
        PoolFunded {
            id,
            from,
            amount,
            pool_funded: t.pool_funded,
        }
        .publish(&env);
        Ok(())
    }

    /// Open registration. Requires organizer auth and the pool to be fully
    /// funded: this transition is the proof of funds.
    pub fn open_registration(env: Env, id: u32) -> Result<(), Error> {
        let mut t = load_tournament(&env, id)?;
        t.organizer.require_auth();
        if t.state != State::Created {
            return Err(Error::InvalidState);
        }
        if t.pool_funded < t.pool_required {
            return Err(Error::Underfunded);
        }
        if env.ledger().timestamp() >= t.register_by {
            return Err(Error::RegistrationClosed);
        }
        t.state = State::Open;
        save_tournament(&env, id, &t);
        RegistrationOpened {
            id,
            pool_funded: t.pool_funded,
        }
        .publish(&env);
        Ok(())
    }

    /// Register a player. Player auth, only while Open and before register_by.
    pub fn register(env: Env, id: u32, player: Address) -> Result<(), Error> {
        player.require_auth();
        let t = load_tournament(&env, id)?;
        if t.state != State::Open {
            return Err(Error::InvalidState);
        }
        if env.ledger().timestamp() >= t.register_by {
            return Err(Error::RegistrationClosed);
        }
        let mut roster = load_roster(&env, id);
        if roster.contains_key(player.clone()) {
            return Err(Error::AlreadyRegistered);
        }
        if roster.len() >= MAX_ROSTER {
            return Err(Error::RosterFull);
        }
        roster.set(player.clone(), true);
        let roster_size = roster.len();
        save_roster(&env, id, &roster);
        PlayerRegistered {
            id,
            player,
            roster_size,
        }
        .publish(&env);
        Ok(())
    }

    /// Submit the final ranking (first place first). Organizer auth, after
    /// registration closed and before results_by. The ranking may be shorter
    /// than the placement table (unfilled placements stay in the vault and are
    /// refunded to the sponsor at close) but never longer. Every address must be
    /// registered and unique. Without a referee the results are confirmed
    /// immediately; with one, the tournament waits for confirm/reject. The
    /// organizer may resubmit while the referee has not decided.
    pub fn submit_results(env: Env, id: u32, ranking: Vec<Address>) -> Result<(), Error> {
        let mut t = load_tournament(&env, id)?;
        t.organizer.require_auth();
        if t.state != State::Open && t.state != State::ResultsSubmitted {
            return Err(Error::InvalidState);
        }
        let now = env.ledger().timestamp();
        if now < t.register_by {
            return Err(Error::RegistrationStillOpen);
        }
        if now >= t.results_by {
            return Err(Error::ResultsDeadlinePassed);
        }
        if ranking.is_empty() || ranking.len() > t.placements.len() {
            return Err(Error::InvalidRanking);
        }
        let roster = load_roster(&env, id);
        let mut seen: Map<Address, bool> = Map::new(&env);
        for player in ranking.iter() {
            if !roster.contains_key(player.clone()) {
                return Err(Error::NotRegistered);
            }
            if seen.contains_key(player.clone()) {
                return Err(Error::DuplicateInRanking);
            }
            seen.set(player, true);
        }
        save_ranking(&env, id, &ranking);
        let needs_referee = t.referee.is_some();
        ResultsSubmitted {
            id,
            ranking_len: ranking.len(),
            needs_referee,
        }
        .publish(&env);
        if needs_referee {
            t.state = State::ResultsSubmitted;
            save_tournament(&env, id, &t);
        } else {
            confirm(&env, id, &mut t, &ranking);
        }
        Ok(())
    }

    /// Referee confirms the submitted ranking; prizes become claimable.
    pub fn confirm_results(env: Env, id: u32) -> Result<(), Error> {
        let mut t = load_tournament(&env, id)?;
        let referee = t.referee.clone().ok_or(Error::NoReferee)?;
        referee.require_auth();
        if t.state != State::ResultsSubmitted {
            return Err(Error::InvalidState);
        }
        if env.ledger().timestamp() >= t.results_by {
            return Err(Error::ResultsDeadlinePassed);
        }
        let ranking = load_ranking(&env, id);
        confirm(&env, id, &mut t, &ranking);
        Ok(())
    }

    /// Referee rejects the submitted ranking; the tournament returns to Open
    /// (registration is already closed) so the organizer can resubmit.
    pub fn reject_results(env: Env, id: u32) -> Result<(), Error> {
        let mut t = load_tournament(&env, id)?;
        let referee = t.referee.clone().ok_or(Error::NoReferee)?;
        referee.require_auth();
        if t.state != State::ResultsSubmitted {
            return Err(Error::InvalidState);
        }
        env.storage().persistent().remove(&DataKey::Ranking(id));
        t.state = State::Open;
        save_tournament(&env, id, &t);
        ResultsRejected { id }.publish(&env);
        Ok(())
    }

    /// Winner claims their prize. Player auth, Confirmed state, before claim_by.
    pub fn claim(env: Env, id: u32, player: Address) -> Result<i128, Error> {
        player.require_auth();
        let t = load_tournament(&env, id)?;
        if t.state != State::Confirmed {
            return Err(Error::InvalidState);
        }
        if env.ledger().timestamp() >= t.claim_by {
            return Err(Error::ClaimWindowClosed);
        }
        let mut claimable = load_claimable(&env, id);
        let amount = claimable.get(player.clone()).unwrap_or(0);
        if amount <= 0 {
            return Err(Error::NothingToClaim);
        }
        claimable.remove(player.clone());
        save_claimable(&env, id, &claimable);

        let mut claimed = load_claimed(&env, id);
        claimed.set(player.clone(), amount);
        save_claimed(&env, id, &claimed);
        save_paid_out(&env, id, load_paid_out(&env, id) + amount);

        token::Client::new(&env, &t.token).transfer(&env.current_contract_address(), &player, &amount);
        PrizeClaimed { id, player, amount }.publish(&env);
        Ok(amount)
    }

    /// After claim_by, anyone can sweep whatever was not claimed back to the
    /// sponsor and close the tournament. Returns the refunded amount.
    pub fn refund_unclaimed(env: Env, id: u32) -> Result<i128, Error> {
        let mut t = load_tournament(&env, id)?;
        if t.state != State::Confirmed {
            return Err(Error::InvalidState);
        }
        if env.ledger().timestamp() < t.claim_by {
            return Err(Error::ClaimWindowOpen);
        }
        let remaining = t.pool_funded - load_paid_out(&env, id);
        if remaining > 0 {
            token::Client::new(&env, &t.token).transfer(&env.current_contract_address(), &t.sponsor, &remaining);
        }
        env.storage().persistent().remove(&DataKey::Claimable(id));
        t.state = State::Closed;
        save_tournament(&env, id, &t);
        UnclaimedRefunded {
            id,
            sponsor: t.sponsor.clone(),
            amount: remaining,
        }
        .publish(&env);
        Ok(remaining)
    }

    /// Organizer cancels before results are confirmed; the whole pool goes
    /// back to the sponsor. Returns the refunded amount.
    pub fn cancel(env: Env, id: u32) -> Result<i128, Error> {
        let mut t = load_tournament(&env, id)?;
        t.organizer.require_auth();
        cancel_inner(&env, id, &mut t, false)
    }

    /// If results_by passes without confirmed results, anyone can cancel and
    /// refund the sponsor. Returns the refunded amount.
    pub fn cancel_expired(env: Env, id: u32) -> Result<i128, Error> {
        let mut t = load_tournament(&env, id)?;
        if env.ledger().timestamp() < t.results_by {
            return Err(Error::ResultsDeadlineNotReached);
        }
        cancel_inner(&env, id, &mut t, true)
    }

    // ----------------------------------------------------------------- views

    /// Full tournament record.
    pub fn status(env: Env, id: u32) -> Result<Tournament, Error> {
        load_tournament(&env, id)
    }

    /// Prize still claimable by `player` (0 if none or already claimed).
    pub fn claimable(env: Env, id: u32, player: Address) -> i128 {
        load_claimable(&env, id).get(player).unwrap_or(0)
    }

    /// Prize already paid to `player` (0 if none).
    pub fn claimed(env: Env, id: u32, player: Address) -> i128 {
        load_claimed(&env, id).get(player).unwrap_or(0)
    }

    /// Total paid out to winners so far.
    pub fn paid_out(env: Env, id: u32) -> i128 {
        load_paid_out(&env, id)
    }

    /// Registered players (unordered).
    pub fn roster(env: Env, id: u32) -> Vec<Address> {
        load_roster(&env, id).keys()
    }

    pub fn is_registered(env: Env, id: u32, player: Address) -> bool {
        load_roster(&env, id).contains_key(player)
    }

    /// Submitted (or confirmed) ranking, first place first. Empty if none.
    pub fn ranking(env: Env, id: u32) -> Vec<Address> {
        load_ranking(&env, id)
    }

    /// Number of tournaments created so far (ids are 0..count).
    pub fn count(env: Env) -> u32 {
        env.storage().instance().get(&DataKey::Counter).unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Pure logic
// ---------------------------------------------------------------------------

/// Placement table rules: 1..=MAX_PLACEMENTS entries, all positive, sum 10_000.
pub fn validate_placements(placements: &Vec<u32>) -> Result<(), Error> {
    if placements.is_empty() || placements.len() > MAX_PLACEMENTS {
        return Err(Error::InvalidPlacements);
    }
    let mut sum: u64 = 0;
    for bps in placements.iter() {
        if bps == 0 {
            return Err(Error::InvalidPlacements);
        }
        sum += bps as u64;
    }
    if sum != BPS_DENOMINATOR as u64 {
        return Err(Error::InvalidPlacements);
    }
    Ok(())
}

/// Amount per placement using integer bps math. Every placement but the last
/// receives `floor(pool * bps / 10_000)`; the last placement receives whatever
/// is left, so the amounts always sum to exactly `pool` and rounding dust lands
/// on the last placement.
pub fn placement_amounts(env: &Env, pool: i128, placements: &Vec<u32>) -> Vec<i128> {
    let mut out: Vec<i128> = Vec::new(env);
    let n = placements.len();
    let mut distributed: i128 = 0;
    for (i, bps) in placements.iter().enumerate() {
        let amount = if (i as u32) == n - 1 {
            pool - distributed
        } else {
            pool * (bps as i128) / (BPS_DENOMINATOR as i128)
        };
        distributed += amount;
        out.push_back(amount);
    }
    out
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn confirm(env: &Env, id: u32, t: &mut Tournament, ranking: &Vec<Address>) {
    let amounts = placement_amounts(env, t.pool_funded, &t.placements);
    let mut claimable: Map<Address, i128> = Map::new(env);
    let mut payable_total: i128 = 0;
    for (i, player) in ranking.iter().enumerate() {
        let amount = amounts.get(i as u32).unwrap_or(0);
        if amount > 0 {
            claimable.set(player, amount);
            payable_total += amount;
        }
    }
    save_claimable(env, id, &claimable);
    save_claimed(env, id, &Map::new(env));
    t.state = State::Confirmed;
    save_tournament(env, id, t);
    ResultsConfirmed {
        id,
        ranking_len: ranking.len(),
        payable_total,
    }
    .publish(env);
}

fn cancel_inner(env: &Env, id: u32, t: &mut Tournament, expired: bool) -> Result<i128, Error> {
    match t.state {
        State::Created | State::Open | State::ResultsSubmitted => {}
        _ => return Err(Error::InvalidState),
    }
    let refunded = t.pool_funded;
    if refunded > 0 {
        token::Client::new(env, &t.token).transfer(&env.current_contract_address(), &t.sponsor, &refunded);
    }
    env.storage().persistent().remove(&DataKey::Ranking(id));
    t.state = State::Cancelled;
    save_tournament(env, id, t);
    TournamentCancelled {
        id,
        refunded,
        expired,
    }
    .publish(env);
    Ok(refunded)
}

fn next_id(env: &Env) -> u32 {
    let id: u32 = env.storage().instance().get(&DataKey::Counter).unwrap_or(0);
    env.storage().instance().set(&DataKey::Counter, &(id + 1));
    env.storage().instance().extend_ttl(TTL_THRESHOLD, TTL_EXTEND_TO);
    id
}

fn bump(env: &Env, key: &DataKey) {
    env.storage().persistent().extend_ttl(key, TTL_THRESHOLD, TTL_EXTEND_TO);
}

fn load_tournament(env: &Env, id: u32) -> Result<Tournament, Error> {
    env.storage().persistent().get(&DataKey::Tournament(id)).ok_or(Error::NotFound)
}

fn save_tournament(env: &Env, id: u32, t: &Tournament) {
    let key = DataKey::Tournament(id);
    env.storage().persistent().set(&key, t);
    bump(env, &key);
}

fn load_roster(env: &Env, id: u32) -> Map<Address, bool> {
    env.storage().persistent().get(&DataKey::Roster(id)).unwrap_or(Map::new(env))
}

fn save_roster(env: &Env, id: u32, roster: &Map<Address, bool>) {
    let key = DataKey::Roster(id);
    env.storage().persistent().set(&key, roster);
    bump(env, &key);
}

fn load_ranking(env: &Env, id: u32) -> Vec<Address> {
    env.storage().persistent().get(&DataKey::Ranking(id)).unwrap_or(Vec::new(env))
}

fn save_ranking(env: &Env, id: u32, ranking: &Vec<Address>) {
    let key = DataKey::Ranking(id);
    env.storage().persistent().set(&key, ranking);
    bump(env, &key);
}

fn load_claimable(env: &Env, id: u32) -> Map<Address, i128> {
    env.storage().persistent().get(&DataKey::Claimable(id)).unwrap_or(Map::new(env))
}

fn save_claimable(env: &Env, id: u32, m: &Map<Address, i128>) {
    let key = DataKey::Claimable(id);
    env.storage().persistent().set(&key, m);
    bump(env, &key);
}

fn load_claimed(env: &Env, id: u32) -> Map<Address, i128> {
    env.storage().persistent().get(&DataKey::Claimed(id)).unwrap_or(Map::new(env))
}

fn save_claimed(env: &Env, id: u32, m: &Map<Address, i128>) {
    let key = DataKey::Claimed(id);
    env.storage().persistent().set(&key, m);
    bump(env, &key);
}

fn load_paid_out(env: &Env, id: u32) -> i128 {
    env.storage().persistent().get(&DataKey::PaidOut(id)).unwrap_or(0)
}

fn save_paid_out(env: &Env, id: u32, v: i128) {
    let key = DataKey::PaidOut(id);
    env.storage().persistent().set(&key, &v);
    bump(env, &key);
}

#[cfg(test)]
mod test;
