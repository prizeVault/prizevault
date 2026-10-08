extern crate std;

use super::*;
use soroban_sdk::testutils::{
    storage::{Instance as _, Persistent as _},
    Address as _, AuthorizedFunction, AuthorizedInvocation, Events as _, Ledger as _, MockAuth,
    MockAuthInvoke,
};
use soroban_sdk::{xdr, IntoVal, Symbol};
use std::string::String;
use std::vec::Vec as StdVec;

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

/// 2026-09-21T14:13:20Z: a plausible "now" for a tournament created this week.
const T0: u64 = 1_790_000_000;
const DAY: u64 = 86_400;
/// USDC on Stellar has 7 decimals.
const USDC: i128 = 10_000_000;
const REGISTER_BY: u64 = T0 + 7 * DAY;
const RESULTS_BY: u64 = T0 + 14 * DAY;
const CLAIM_BY: u64 = T0 + 44 * DAY;

/// The seed placement table (data/seed/placements.json): 16 places, 10_000 bps.
const PLACEMENTS_16: [u32; 16] = [
    3000, 1800, 1200, 900, 600, 600, 400, 400, 200, 200, 200, 200, 75, 75, 75, 75,
];

struct Fx {
    env: Env,
    contract: Address,
    token: Address,
    organizer: Address,
    sponsor: Address,
    referee: Address,
}

impl Fx {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(T0);
        let token_admin = Address::generate(&env);
        let sac = env.register_stellar_asset_contract_v2(token_admin);
        let contract = env.register(PrizeVault, ());
        Fx {
            organizer: Address::generate(&env),
            sponsor: Address::generate(&env),
            referee: Address::generate(&env),
            token: sac.address(),
            contract,
            env,
        }
    }

    fn client(&self) -> PrizeVaultClient<'_> {
        PrizeVaultClient::new(&self.env, &self.contract)
    }

    fn token(&self) -> token::Client<'_> {
        token::Client::new(&self.env, &self.token)
    }

    fn mint(&self, who: &Address, amount: i128) {
        token::StellarAssetClient::new(&self.env, &self.token).mint(who, &amount);
    }

    fn set_time(&self, t: u64) {
        self.env.ledger().set_timestamp(t);
    }

    fn bps(&self, table: &[u32]) -> Vec<u32> {
        Vec::from_slice(&self.env, table)
    }

    fn placements16(&self) -> Vec<u32> {
        self.bps(&PLACEMENTS_16)
    }

    fn create(&self, pool: i128, placements: &Vec<u32>, referee: Option<Address>) -> u32 {
        self.client().create(
            &self.organizer,
            &self.sponsor,
            &self.token,
            &pool,
            placements,
            &REGISTER_BY,
            &RESULTS_BY,
            &CLAIM_BY,
            &referee,
        )
    }

    /// create + sponsor funds the whole pool + organizer opens registration.
    fn create_open(&self, pool: i128, placements: &Vec<u32>, referee: Option<Address>) -> u32 {
        let id = self.create(pool, placements, referee);
        self.mint(&self.sponsor, pool);
        self.client().fund(&id, &self.sponsor, &pool);
        self.client().open_registration(&id);
        id
    }

    fn register_players(&self, id: u32, n: u32) -> Vec<Address> {
        let mut players = Vec::new(&self.env);
        for _ in 0..n {
            let p = Address::generate(&self.env);
            self.client().register(&id, &p);
            players.push_back(p);
        }
        players
    }

    /// Names (first topic) of the PrizeVault events published by the last
    /// invocation (token transfer events are filtered out).
    fn event_names(&self) -> StdVec<String> {
        self.env
            .events()
            .all()
            .filter_by_contract(&self.contract)
            .events()
            .iter()
            .map(|e| {
                let xdr::ContractEventBody::V0(v0) = &e.body;
                match v0.topics.as_slice().first() {
                    Some(xdr::ScVal::Symbol(s)) => s.0.to_utf8_string_lossy(),
                    _ => String::from("?"),
                }
            })
            .collect()
    }

    /// Switch the env to enforcing mode with no signatures at all.
    fn deny_all_auth(&self) {
        self.env.mock_auths(&[]);
    }

    /// Only `who` signs `fn_name(args)` on the contract.
    fn only_auth(&self, who: &Address, fn_name: &str, args: soroban_sdk::Vec<soroban_sdk::Val>) {
        self.env.mock_auths(&[MockAuth {
            address: who,
            invoke: &MockAuthInvoke {
                contract: &self.contract,
                fn_name,
                args,
                sub_invokes: &[],
            },
        }]);
    }
}

fn sum(v: &Vec<i128>) -> i128 {
    v.iter().sum()
}

// ---------------------------------------------------------------------------
// Pure logic
// ---------------------------------------------------------------------------

#[test]
fn placement_amounts_sum_to_pool_and_dust_goes_to_last() {
    let fx = Fx::new();
    let pool: i128 = 77_777_777_777; // 7,777.7777777 USDC: forces rounding dust
    let amounts = placement_amounts(&fx.env, pool, &fx.placements16());
    assert_eq!(amounts.len(), 16);
    assert_eq!(amounts.get(0).unwrap(), 23_333_333_333);
    assert_eq!(amounts.get(1).unwrap(), 13_999_999_999);
    assert_eq!(amounts.get(12).unwrap(), 583_333_333);
    // last placement = its floor share (583_333_333) + 7 stroops of dust
    assert_eq!(amounts.get(15).unwrap(), 583_333_340);
    assert_eq!(sum(&amounts), pool);

    // a single placement takes everything
    let one = placement_amounts(&fx.env, 12_345, &fx.bps(&[10_000]));
    assert_eq!(one.get(0).unwrap(), 12_345);

    // tiny pool: floor shares are zero, everything lands on the last placement
    let tiny = placement_amounts(&fx.env, 3, &fx.bps(&[5000, 3000, 2000]));
    assert_eq!(tiny.get(0).unwrap(), 1);
    assert_eq!(tiny.get(1).unwrap(), 0);
    assert_eq!(tiny.get(2).unwrap(), 2);
}

#[test]
fn validate_placements_rules() {
    let fx = Fx::new();
    assert_eq!(validate_placements(&fx.placements16()), Ok(()));
    assert_eq!(validate_placements(&fx.bps(&[10_000])), Ok(()));
    assert_eq!(validate_placements(&fx.bps(&[])), Err(Error::InvalidPlacements));
    assert_eq!(validate_placements(&fx.bps(&[5000, 4999])), Err(Error::InvalidPlacements));
    assert_eq!(validate_placements(&fx.bps(&[5000, 5001])), Err(Error::InvalidPlacements));
    assert_eq!(validate_placements(&fx.bps(&[10_000, 0])), Err(Error::InvalidPlacements));
    // 257 positive entries summing to 10_000 exceed MAX_PLACEMENTS
    let mut too_many = [1u32; 257];
    too_many[0] = 10_000 - 256;
    assert_eq!(validate_placements(&fx.bps(&too_many)), Err(Error::InvalidPlacements));
}

// ---------------------------------------------------------------------------
// create
// ---------------------------------------------------------------------------

#[test]
fn create_stores_tournament_and_emits_event() {
    let fx = Fx::new();
    let id = fx.create(5_000 * USDC, &fx.placements16(), Some(fx.referee.clone()));
    assert_eq!(id, 0);
    assert_eq!(fx.event_names(), ["tournament_created"]);

    let t = fx.client().status(&id);
    assert_eq!(t.organizer, fx.organizer);
    assert_eq!(t.sponsor, fx.sponsor);
    assert_eq!(t.token, fx.token);
    assert_eq!(t.pool_required, 5_000 * USDC);
    assert_eq!(t.pool_funded, 0);
    assert_eq!(t.placements, fx.placements16());
    assert_eq!(t.register_by, REGISTER_BY);
    assert_eq!(t.results_by, RESULTS_BY);
    assert_eq!(t.claim_by, CLAIM_BY);
    assert_eq!(t.referee, Some(fx.referee.clone()));
    assert_eq!(t.state, State::Created);

    let second = fx.create(100 * USDC, &fx.bps(&[10_000]), None);
    assert_eq!(second, 1);
    assert_eq!(fx.client().count(), 2);
    assert_eq!(fx.client().status(&second).referee, None);
}

#[test]
fn create_rejects_placements_not_summing_to_10000() {
    let fx = Fx::new();
    let bad = fx.bps(&[5000, 3000, 1990]); // 9_990
    let res = fx.client().try_create(
        &fx.organizer, &fx.sponsor, &fx.token, &(100 * USDC), &bad,
        &REGISTER_BY, &RESULTS_BY, &CLAIM_BY, &None,
    );
    assert_eq!(res, Err(Ok(Error::InvalidPlacements)));
    assert_eq!(fx.client().count(), 0);
}

#[test]
fn create_rejects_bad_deadlines() {
    let fx = Fx::new();
    let c = fx.client();
    let p = fx.placements16();
    // register_by in the past
    assert_eq!(
        c.try_create(&fx.organizer, &fx.sponsor, &fx.token, &(100 * USDC), &p, &(T0 - 1), &RESULTS_BY, &CLAIM_BY, &None),
        Err(Ok(Error::InvalidDeadlines))
    );
    // register_by == now is also too late
    assert_eq!(
        c.try_create(&fx.organizer, &fx.sponsor, &fx.token, &(100 * USDC), &p, &T0, &RESULTS_BY, &CLAIM_BY, &None),
        Err(Ok(Error::InvalidDeadlines))
    );
    // results_by not after register_by
    assert_eq!(
        c.try_create(&fx.organizer, &fx.sponsor, &fx.token, &(100 * USDC), &p, &REGISTER_BY, &REGISTER_BY, &CLAIM_BY, &None),
        Err(Ok(Error::InvalidDeadlines))
    );
    // claim_by not after results_by
    assert_eq!(
        c.try_create(&fx.organizer, &fx.sponsor, &fx.token, &(100 * USDC), &p, &REGISTER_BY, &RESULTS_BY, &RESULTS_BY, &None),
        Err(Ok(Error::InvalidDeadlines))
    );
}

#[test]
fn create_rejects_bad_pool_amount() {
    let fx = Fx::new();
    let c = fx.client();
    let p = fx.placements16();
    for pool in [0i128, -1, MAX_POOL + 1] {
        assert_eq!(
            c.try_create(&fx.organizer, &fx.sponsor, &fx.token, &pool, &p, &REGISTER_BY, &RESULTS_BY, &CLAIM_BY, &None),
            Err(Ok(Error::InvalidAmount))
        );
    }
}

#[test]
fn create_requires_organizer_auth() {
    let fx = Fx::new();
    fx.deny_all_auth();
    let res = fx.client().try_create(
        &fx.organizer, &fx.sponsor, &fx.token, &(100 * USDC), &fx.placements16(),
        &REGISTER_BY, &RESULTS_BY, &CLAIM_BY, &None,
    );
    assert!(res.is_err());
    // the sponsor signing instead of the organizer does not help
    fx.only_auth(
        &fx.sponsor,
        "create",
        (fx.organizer.clone(), fx.sponsor.clone(), fx.token.clone(), 100 * USDC, fx.placements16(), REGISTER_BY, RESULTS_BY, CLAIM_BY, None::<Address>).into_val(&fx.env),
    );
    let res = fx.client().try_create(
        &fx.organizer, &fx.sponsor, &fx.token, &(100 * USDC), &fx.placements16(),
        &REGISTER_BY, &RESULTS_BY, &CLAIM_BY, &None,
    );
    assert!(res.is_err());
}

// ---------------------------------------------------------------------------
// fund
// ---------------------------------------------------------------------------

#[test]
fn fund_transfers_tokens_and_tracks_pool_in_tranches() {
    let fx = Fx::new();
    let id = fx.create(1_000 * USDC, &fx.placements16(), None);
    fx.mint(&fx.sponsor, 1_000 * USDC);

    fx.client().fund(&id, &fx.sponsor, &(400 * USDC));
    assert_eq!(fx.event_names(), ["pool_funded"]);
    // the sponsor authorised fund() and, nested in it, the token transfer
    assert_eq!(
        fx.env.auths(),
        std::vec![(
            fx.sponsor.clone(),
            AuthorizedInvocation {
                function: AuthorizedFunction::Contract((
                    fx.contract.clone(),
                    Symbol::new(&fx.env, "fund"),
                    (id, fx.sponsor.clone(), 400 * USDC).into_val(&fx.env),
                )),
                sub_invocations: std::vec![AuthorizedInvocation {
                    function: AuthorizedFunction::Contract((
                        fx.token.clone(),
                        Symbol::new(&fx.env, "transfer"),
                        (fx.sponsor.clone(), fx.contract.clone(), 400 * USDC).into_val(&fx.env),
                    )),
                    sub_invocations: std::vec![],
                }],
            }
        )]
    );
    assert_eq!(fx.client().status(&id).pool_funded, 400 * USDC);
    assert_eq!(fx.token().balance(&fx.contract), 400 * USDC);
    assert_eq!(fx.token().balance(&fx.sponsor), 600 * USDC);

    // a co-sponsor tops up the rest
    let cosponsor = Address::generate(&fx.env);
    fx.mint(&cosponsor, 600 * USDC);
    fx.client().fund(&id, &cosponsor, &(600 * USDC));
    assert_eq!(fx.client().status(&id).pool_funded, 1_000 * USDC);
    assert_eq!(fx.token().balance(&fx.contract), 1_000 * USDC);
    assert_eq!(fx.client().status(&id).state, State::Created);
}

#[test]
fn fund_rejects_overfunding() {
    let fx = Fx::new();
    let id = fx.create(1_000 * USDC, &fx.placements16(), None);
    fx.mint(&fx.sponsor, 2_000 * USDC);
    fx.client().fund(&id, &fx.sponsor, &(999 * USDC));
    // 2 USDC would overshoot the announced pool by 1 USDC
    assert_eq!(
        fx.client().try_fund(&id, &fx.sponsor, &(2 * USDC)),
        Err(Ok(Error::Overfunded))
    );
    fx.client().fund(&id, &fx.sponsor, &(1 * USDC));
    assert_eq!(
        fx.client().try_fund(&id, &fx.sponsor, &1),
        Err(Ok(Error::Overfunded))
    );
    assert_eq!(fx.token().balance(&fx.contract), 1_000 * USDC);
}

#[test]
fn fund_rejects_zero_amount_unknown_id_and_wrong_state() {
    let fx = Fx::new();
    let id = fx.create(100 * USDC, &fx.placements16(), None);
    fx.mint(&fx.sponsor, 200 * USDC);
    assert_eq!(fx.client().try_fund(&id, &fx.sponsor, &0), Err(Ok(Error::InvalidAmount)));
    assert_eq!(fx.client().try_fund(&id, &fx.sponsor, &-5), Err(Ok(Error::InvalidAmount)));
    assert_eq!(fx.client().try_fund(&42, &fx.sponsor, &1), Err(Ok(Error::NotFound)));
    fx.client().fund(&id, &fx.sponsor, &(100 * USDC));
    fx.client().open_registration(&id);
    assert_eq!(fx.client().try_fund(&id, &fx.sponsor, &1), Err(Ok(Error::InvalidState)));
}

#[test]
fn fund_requires_from_auth() {
    let fx = Fx::new();
    let id = fx.create(100 * USDC, &fx.placements16(), None);
    fx.mint(&fx.sponsor, 100 * USDC);
    fx.deny_all_auth();
    assert!(fx.client().try_fund(&id, &fx.sponsor, &(100 * USDC)).is_err());
    assert_eq!(fx.token().balance(&fx.contract), 0);
}

// ---------------------------------------------------------------------------
// open_registration (proof of funds)
// ---------------------------------------------------------------------------

#[test]
fn open_registration_fails_when_underfunded() {
    let fx = Fx::new();
    let id = fx.create(5_000 * USDC, &fx.placements16(), None);
    // nothing funded
    assert_eq!(fx.client().try_open_registration(&id), Err(Ok(Error::Underfunded)));
    // 99.99% funded is still not proof of funds
    fx.mint(&fx.sponsor, 5_000 * USDC);
    fx.client().fund(&id, &fx.sponsor, &(5_000 * USDC - 1));
    assert_eq!(fx.client().try_open_registration(&id), Err(Ok(Error::Underfunded)));
    assert_eq!(fx.client().status(&id).state, State::Created);
    // and players cannot register into a tournament that is not open
    let p = Address::generate(&fx.env);
    assert_eq!(fx.client().try_register(&id, &p), Err(Ok(Error::InvalidState)));
}

#[test]
fn open_registration_succeeds_when_fully_funded() {
    let fx = Fx::new();
    let id = fx.create(5_000 * USDC, &fx.placements16(), None);
    fx.mint(&fx.sponsor, 5_000 * USDC);
    fx.client().fund(&id, &fx.sponsor, &(5_000 * USDC));
    fx.client().open_registration(&id);
    assert_eq!(fx.event_names(), ["registration_opened"]);
    assert_eq!(
        fx.env.auths(),
        std::vec![(
            fx.organizer.clone(),
            AuthorizedInvocation {
                function: AuthorizedFunction::Contract((
                    fx.contract.clone(),
                    Symbol::new(&fx.env, "open_registration"),
                    (id,).into_val(&fx.env),
                )),
                sub_invocations: std::vec![],
            }
        )]
    );
    let t = fx.client().status(&id);
    assert_eq!(t.state, State::Open);
    assert_eq!(t.pool_funded, t.pool_required);
    // opening twice is a state error
    assert_eq!(fx.client().try_open_registration(&id), Err(Ok(Error::InvalidState)));
}

#[test]
fn open_registration_requires_organizer_auth() {
    let fx = Fx::new();
    let id = fx.create(100 * USDC, &fx.placements16(), None);
    fx.mint(&fx.sponsor, 100 * USDC);
    fx.client().fund(&id, &fx.sponsor, &(100 * USDC));
    // sponsor signs instead of organizer
    fx.only_auth(&fx.sponsor, "open_registration", (id,).into_val(&fx.env));
    assert!(fx.client().try_open_registration(&id).is_err());
    fx.deny_all_auth();
    assert!(fx.client().try_open_registration(&id).is_err());
    assert_eq!(fx.client().status(&id).state, State::Created);
}

#[test]
fn open_registration_fails_after_register_by() {
    let fx = Fx::new();
    let id = fx.create(100 * USDC, &fx.placements16(), None);
    fx.mint(&fx.sponsor, 100 * USDC);
    fx.client().fund(&id, &fx.sponsor, &(100 * USDC));
    fx.set_time(REGISTER_BY);
    assert_eq!(fx.client().try_open_registration(&id), Err(Ok(Error::RegistrationClosed)));
}

// ---------------------------------------------------------------------------
// register
// ---------------------------------------------------------------------------

#[test]
fn register_adds_player_to_roster() {
    let fx = Fx::new();
    let id = fx.create_open(100 * USDC, &fx.placements16(), None);
    let p1 = Address::generate(&fx.env);
    let p2 = Address::generate(&fx.env);
    fx.client().register(&id, &p1);
    assert_eq!(fx.event_names(), ["player_registered"]);
    assert_eq!(
        fx.env.auths(),
        std::vec![(
            p1.clone(),
            AuthorizedInvocation {
                function: AuthorizedFunction::Contract((
                    fx.contract.clone(),
                    Symbol::new(&fx.env, "register"),
                    (id, p1.clone()).into_val(&fx.env),
                )),
                sub_invocations: std::vec![],
            }
        )]
    );
    fx.client().register(&id, &p2);
    assert!(fx.client().is_registered(&id, &p1));
    assert!(fx.client().is_registered(&id, &p2));
    assert!(!fx.client().is_registered(&id, &fx.organizer));
    assert_eq!(fx.client().roster(&id).len(), 2);
}

#[test]
fn register_rejects_duplicate_player() {
    let fx = Fx::new();
    let id = fx.create_open(100 * USDC, &fx.placements16(), None);
    let p = Address::generate(&fx.env);
    fx.client().register(&id, &p);
    assert_eq!(fx.client().try_register(&id, &p), Err(Ok(Error::AlreadyRegistered)));
    assert_eq!(fx.client().roster(&id).len(), 1);
}

#[test]
fn register_rejects_after_register_by() {
    let fx = Fx::new();
    let id = fx.create_open(100 * USDC, &fx.placements16(), None);
    let early = Address::generate(&fx.env);
    fx.set_time(REGISTER_BY - 1);
    fx.client().register(&id, &early);
    fx.set_time(REGISTER_BY);
    let late = Address::generate(&fx.env);
    assert_eq!(fx.client().try_register(&id, &late), Err(Ok(Error::RegistrationClosed)));
}

#[test]
fn register_requires_player_auth() {
    let fx = Fx::new();
    let id = fx.create_open(100 * USDC, &fx.placements16(), None);
    let p = Address::generate(&fx.env);
    // organizer cannot register a player on their behalf
    fx.only_auth(&fx.organizer, "register", (id, p.clone()).into_val(&fx.env));
    assert!(fx.client().try_register(&id, &p).is_err());
    fx.deny_all_auth();
    assert!(fx.client().try_register(&id, &p).is_err());
    assert!(!fx.client().is_registered(&id, &p));
}

// ---------------------------------------------------------------------------
// submit_results
// ---------------------------------------------------------------------------

#[test]
fn submit_results_without_referee_confirms_immediately() {
    let fx = Fx::new();
    let pool = 1_000 * USDC;
    let id = fx.create_open(pool, &fx.bps(&[6000, 3000, 1000]), None);
    let players = fx.register_players(id, 5);
    fx.set_time(REGISTER_BY);
    let ranking = Vec::from_array(
        &fx.env,
        [players.get(3).unwrap(), players.get(0).unwrap(), players.get(4).unwrap()],
    );
    fx.client().submit_results(&id, &ranking);
    assert_eq!(fx.event_names(), ["results_submitted", "results_confirmed"]);
    let t = fx.client().status(&id);
    assert_eq!(t.state, State::Confirmed);
    assert_eq!(fx.client().ranking(&id), ranking);
    assert_eq!(fx.client().claimable(&id, &players.get(3).unwrap()), 600 * USDC);
    assert_eq!(fx.client().claimable(&id, &players.get(0).unwrap()), 300 * USDC);
    assert_eq!(fx.client().claimable(&id, &players.get(4).unwrap()), 100 * USDC);
    assert_eq!(fx.client().claimable(&id, &players.get(1).unwrap()), 0);
    // confirm/reject do not apply without a referee
    assert_eq!(fx.client().try_confirm_results(&id), Err(Ok(Error::NoReferee)));
    assert_eq!(fx.client().try_reject_results(&id), Err(Ok(Error::NoReferee)));
}

#[test]
fn submit_results_with_referee_waits_for_confirmation() {
    let fx = Fx::new();
    let id = fx.create_open(1_000 * USDC, &fx.bps(&[7000, 3000]), Some(fx.referee.clone()));
    let players = fx.register_players(id, 3);
    fx.set_time(REGISTER_BY);
    let ranking = Vec::from_array(&fx.env, [players.get(1).unwrap(), players.get(2).unwrap()]);
    fx.client().submit_results(&id, &ranking);
    assert_eq!(fx.event_names(), ["results_submitted"]);
    assert_eq!(fx.client().status(&id).state, State::ResultsSubmitted);
    assert_eq!(fx.client().claimable(&id, &players.get(1).unwrap()), 0);
    // nobody can claim yet
    assert_eq!(
        fx.client().try_claim(&id, &players.get(1).unwrap()),
        Err(Ok(Error::InvalidState))
    );
}

#[test]
fn submit_results_rejects_while_registration_is_open() {
    let fx = Fx::new();
    let id = fx.create_open(100 * USDC, &fx.bps(&[10_000]), None);
    let players = fx.register_players(id, 2);
    fx.set_time(REGISTER_BY - 1);
    let ranking = Vec::from_array(&fx.env, [players.get(0).unwrap()]);
    assert_eq!(
        fx.client().try_submit_results(&id, &ranking),
        Err(Ok(Error::RegistrationStillOpen))
    );
}

#[test]
fn submit_results_rejects_after_results_by() {
    let fx = Fx::new();
    let id = fx.create_open(100 * USDC, &fx.bps(&[10_000]), None);
    let players = fx.register_players(id, 2);
    fx.set_time(RESULTS_BY);
    let ranking = Vec::from_array(&fx.env, [players.get(0).unwrap()]);
    assert_eq!(
        fx.client().try_submit_results(&id, &ranking),
        Err(Ok(Error::ResultsDeadlinePassed))
    );
    assert_eq!(fx.client().status(&id).state, State::Open);
}

#[test]
fn submit_results_rejects_unregistered_duplicate_empty_and_too_long() {
    let fx = Fx::new();
    let id = fx.create_open(100 * USDC, &fx.bps(&[5000, 3000, 2000]), None);
    let players = fx.register_players(id, 4);
    fx.set_time(REGISTER_BY);
    let c = fx.client();
    let p0 = players.get(0).unwrap();
    let p1 = players.get(1).unwrap();
    let p2 = players.get(2).unwrap();
    let stranger = Address::generate(&fx.env);

    assert_eq!(
        c.try_submit_results(&id, &Vec::from_array(&fx.env, [p0.clone(), stranger])),
        Err(Ok(Error::NotRegistered))
    );
    assert_eq!(
        c.try_submit_results(&id, &Vec::from_array(&fx.env, [p0.clone(), p1.clone(), p0.clone()])),
        Err(Ok(Error::DuplicateInRanking))
    );
    assert_eq!(
        c.try_submit_results(&id, &Vec::new(&fx.env)),
        Err(Ok(Error::InvalidRanking))
    );
    assert_eq!(
        c.try_submit_results(
            &id,
            &Vec::from_array(&fx.env, [p0.clone(), p1.clone(), p2.clone(), players.get(3).unwrap()])
        ),
        Err(Ok(Error::InvalidRanking))
    );
    // nothing leaked into storage
    assert_eq!(c.ranking(&id).len(), 0);
    assert_eq!(c.status(&id).state, State::Open);
}

#[test]
fn submit_results_requires_organizer_auth() {
    let fx = Fx::new();
    let id = fx.create_open(100 * USDC, &fx.bps(&[10_000]), None);
    let players = fx.register_players(id, 1);
    fx.set_time(REGISTER_BY);
    let ranking = Vec::from_array(&fx.env, [players.get(0).unwrap()]);
    // the winner signing their own victory is not enough
    fx.only_auth(&players.get(0).unwrap(), "submit_results", (id, ranking.clone()).into_val(&fx.env));
    assert!(fx.client().try_submit_results(&id, &ranking).is_err());
    fx.only_auth(&fx.referee, "submit_results", (id, ranking.clone()).into_val(&fx.env));
    assert!(fx.client().try_submit_results(&id, &ranking).is_err());
    assert_eq!(fx.client().status(&id).state, State::Open);
}

#[test]
fn shorter_ranking_leaves_unfilled_placements_for_the_sponsor() {
    let fx = Fx::new();
    let pool: i128 = 10_000_000_001; // 1,000.0000001 USDC
    let id = fx.create_open(pool, &fx.bps(&[5000, 3000, 2000]), None);
    let players = fx.register_players(id, 2);
    fx.set_time(REGISTER_BY);
    fx.client().submit_results(&id, &players);
    let p0 = players.get(0).unwrap();
    let p1 = players.get(1).unwrap();
    assert_eq!(fx.client().claimable(&id, &p0), 5_000_000_000);
    assert_eq!(fx.client().claimable(&id, &p1), 3_000_000_000);
    fx.client().claim(&id, &p0);
    fx.client().claim(&id, &p1);
    fx.set_time(CLAIM_BY);
    // third placement (2_000_000_000 + 1 stroop of dust) had no winner
    assert_eq!(fx.client().refund_unclaimed(&id), 2_000_000_001);
    assert_eq!(fx.token().balance(&fx.sponsor), 2_000_000_001);
    assert_eq!(fx.token().balance(&fx.contract), 0);
}

// ---------------------------------------------------------------------------
// confirm_results / reject_results
// ---------------------------------------------------------------------------

#[test]
fn confirm_results_by_referee_makes_prizes_claimable() {
    let fx = Fx::new();
    let id = fx.create_open(1_000 * USDC, &fx.bps(&[7000, 3000]), Some(fx.referee.clone()));
    let players = fx.register_players(id, 2);
    fx.set_time(REGISTER_BY);
    fx.client().submit_results(&id, &players);
    fx.client().confirm_results(&id);
    assert_eq!(fx.event_names(), ["results_confirmed"]);
    assert_eq!(
        fx.env.auths(),
        std::vec![(
            fx.referee.clone(),
            AuthorizedInvocation {
                function: AuthorizedFunction::Contract((
                    fx.contract.clone(),
                    Symbol::new(&fx.env, "confirm_results"),
                    (id,).into_val(&fx.env),
                )),
                sub_invocations: std::vec![],
            }
        )]
    );
    assert_eq!(fx.client().status(&id).state, State::Confirmed);
    assert_eq!(fx.client().claimable(&id, &players.get(0).unwrap()), 700 * USDC);
    assert_eq!(fx.client().claimable(&id, &players.get(1).unwrap()), 300 * USDC);
    // once confirmed, results are final
    assert_eq!(fx.client().try_confirm_results(&id), Err(Ok(Error::InvalidState)));
    assert_eq!(fx.client().try_reject_results(&id), Err(Ok(Error::InvalidState)));
    assert_eq!(fx.client().try_submit_results(&id, &players), Err(Ok(Error::InvalidState)));
}

#[test]
fn confirm_results_requires_referee_auth() {
    let fx = Fx::new();
    let id = fx.create_open(1_000 * USDC, &fx.bps(&[10_000]), Some(fx.referee.clone()));
    let players = fx.register_players(id, 1);
    fx.set_time(REGISTER_BY);
    fx.client().submit_results(&id, &players);
    // organizer cannot confirm their own results
    fx.only_auth(&fx.organizer, "confirm_results", (id,).into_val(&fx.env));
    assert!(fx.client().try_confirm_results(&id).is_err());
    fx.only_auth(&fx.organizer, "reject_results", (id,).into_val(&fx.env));
    assert!(fx.client().try_reject_results(&id).is_err());
    fx.deny_all_auth();
    assert!(fx.client().try_confirm_results(&id).is_err());
    assert_eq!(fx.client().status(&id).state, State::ResultsSubmitted);
}

#[test]
fn confirm_results_rejects_wrong_state_and_late_confirmation() {
    let fx = Fx::new();
    let id = fx.create_open(1_000 * USDC, &fx.bps(&[10_000]), Some(fx.referee.clone()));
    let players = fx.register_players(id, 1);
    fx.set_time(REGISTER_BY);
    // nothing submitted yet
    assert_eq!(fx.client().try_confirm_results(&id), Err(Ok(Error::InvalidState)));
    assert_eq!(fx.client().try_reject_results(&id), Err(Ok(Error::InvalidState)));
    fx.client().submit_results(&id, &players);
    fx.set_time(RESULTS_BY);
    assert_eq!(fx.client().try_confirm_results(&id), Err(Ok(Error::ResultsDeadlinePassed)));
    assert_eq!(fx.client().status(&id).state, State::ResultsSubmitted);
}

#[test]
fn referee_reject_then_resubmit_then_confirm() {
    let fx = Fx::new();
    let pool = 2_500 * USDC;
    let id = fx.create_open(pool, &fx.bps(&[6000, 4000]), Some(fx.referee.clone()));
    let players = fx.register_players(id, 3);
    let p0 = players.get(0).unwrap();
    let p1 = players.get(1).unwrap();
    let p2 = players.get(2).unwrap();
    fx.set_time(REGISTER_BY + DAY);

    // organizer submits the wrong bracket
    fx.client().submit_results(&id, &Vec::from_array(&fx.env, [p0.clone(), p1.clone()]));
    assert_eq!(fx.client().status(&id).state, State::ResultsSubmitted);

    // referee vetoes
    fx.client().reject_results(&id);
    assert_eq!(fx.event_names(), ["results_rejected"]);
    assert_eq!(fx.client().status(&id).state, State::Open);
    assert_eq!(fx.client().ranking(&id).len(), 0);
    // registration stays closed even though the state is Open again
    let latecomer = Address::generate(&fx.env);
    assert_eq!(fx.client().try_register(&id, &latecomer), Err(Ok(Error::RegistrationClosed)));

    // organizer resubmits, referee confirms
    let fixed = Vec::from_array(&fx.env, [p2.clone(), p0.clone()]);
    fx.client().submit_results(&id, &fixed);
    fx.client().confirm_results(&id);
    assert_eq!(fx.client().status(&id).state, State::Confirmed);
    assert_eq!(fx.client().ranking(&id), fixed);
    assert_eq!(fx.client().claimable(&id, &p2), 1_500 * USDC);
    assert_eq!(fx.client().claimable(&id, &p0), 1_000 * USDC);
    assert_eq!(fx.client().claimable(&id, &p1), 0);
}

#[test]
fn organizer_can_overwrite_submission_before_referee_decides() {
    let fx = Fx::new();
    let id = fx.create_open(100 * USDC, &fx.bps(&[10_000]), Some(fx.referee.clone()));
    let players = fx.register_players(id, 2);
    fx.set_time(REGISTER_BY);
    fx.client().submit_results(&id, &Vec::from_array(&fx.env, [players.get(0).unwrap()]));
    fx.client().submit_results(&id, &Vec::from_array(&fx.env, [players.get(1).unwrap()]));
    assert_eq!(fx.client().ranking(&id), Vec::from_array(&fx.env, [players.get(1).unwrap()]));
    fx.client().confirm_results(&id);
    assert_eq!(fx.client().claimable(&id, &players.get(1).unwrap()), 100 * USDC);
    assert_eq!(fx.client().claimable(&id, &players.get(0).unwrap()), 0);
}

// ---------------------------------------------------------------------------
// claim
// ---------------------------------------------------------------------------

#[test]
fn claim_pays_winner_once() {
    let fx = Fx::new();
    let id = fx.create_open(1_000 * USDC, &fx.bps(&[7000, 3000]), None);
    let players = fx.register_players(id, 2);
    let p0 = players.get(0).unwrap();
    let p1 = players.get(1).unwrap();
    fx.set_time(REGISTER_BY);
    fx.client().submit_results(&id, &players);

    assert_eq!(fx.client().claim(&id, &p0), 700 * USDC);
    assert_eq!(fx.event_names(), ["prize_claimed"]);
    assert_eq!(
        fx.env.auths(),
        std::vec![(
            p0.clone(),
            AuthorizedInvocation {
                function: AuthorizedFunction::Contract((
                    fx.contract.clone(),
                    Symbol::new(&fx.env, "claim"),
                    (id, p0.clone()).into_val(&fx.env),
                )),
                sub_invocations: std::vec![],
            }
        )]
    );
    assert_eq!(fx.token().balance(&p0), 700 * USDC);
    assert_eq!(fx.token().balance(&fx.contract), 300 * USDC);
    assert_eq!(fx.client().claimable(&id, &p0), 0);
    assert_eq!(fx.client().claimed(&id, &p0), 700 * USDC);
    assert_eq!(fx.client().paid_out(&id), 700 * USDC);
    // second claim pays nothing
    assert_eq!(fx.client().try_claim(&id, &p0), Err(Ok(Error::NothingToClaim)));
    assert_eq!(fx.token().balance(&p0), 700 * USDC);
    // p1 untouched
    assert_eq!(fx.client().claimable(&id, &p1), 300 * USDC);
}

#[test]
fn claim_rejects_non_winner_and_unranked_registrant() {
    let fx = Fx::new();
    let id = fx.create_open(1_000 * USDC, &fx.bps(&[10_000]), None);
    let players = fx.register_players(id, 2);
    fx.set_time(REGISTER_BY);
    fx.client().submit_results(&id, &Vec::from_array(&fx.env, [players.get(0).unwrap()]));
    assert_eq!(
        fx.client().try_claim(&id, &players.get(1).unwrap()),
        Err(Ok(Error::NothingToClaim))
    );
    let stranger = Address::generate(&fx.env);
    assert_eq!(fx.client().try_claim(&id, &stranger), Err(Ok(Error::NothingToClaim)));
}

#[test]
fn claim_rejects_after_claim_by() {
    let fx = Fx::new();
    let id = fx.create_open(1_000 * USDC, &fx.bps(&[10_000]), None);
    let players = fx.register_players(id, 1);
    fx.set_time(REGISTER_BY);
    fx.client().submit_results(&id, &players);
    fx.set_time(CLAIM_BY);
    assert_eq!(
        fx.client().try_claim(&id, &players.get(0).unwrap()),
        Err(Ok(Error::ClaimWindowClosed))
    );
    // the prize is still recorded and goes back to the sponsor
    assert_eq!(fx.client().claimable(&id, &players.get(0).unwrap()), 1_000 * USDC);
    assert_eq!(fx.client().refund_unclaimed(&id), 1_000 * USDC);
}

#[test]
fn claim_requires_player_auth() {
    let fx = Fx::new();
    let id = fx.create_open(1_000 * USDC, &fx.bps(&[10_000]), None);
    let players = fx.register_players(id, 1);
    let p0 = players.get(0).unwrap();
    fx.set_time(REGISTER_BY);
    fx.client().submit_results(&id, &players);
    // the organizer cannot claim on the winner's behalf
    fx.only_auth(&fx.organizer, "claim", (id, p0.clone()).into_val(&fx.env));
    assert!(fx.client().try_claim(&id, &p0).is_err());
    fx.deny_all_auth();
    assert!(fx.client().try_claim(&id, &p0).is_err());
    assert_eq!(fx.token().balance(&p0), 0);
    assert_eq!(fx.client().claimable(&id, &p0), 1_000 * USDC);
}

// ---------------------------------------------------------------------------
// refund_unclaimed
// ---------------------------------------------------------------------------

#[test]
fn refund_unclaimed_rejects_before_claim_by_and_wrong_state() {
    let fx = Fx::new();
    let id = fx.create_open(1_000 * USDC, &fx.bps(&[10_000]), None);
    let players = fx.register_players(id, 1);
    // Open: nothing to refund yet
    assert_eq!(fx.client().try_refund_unclaimed(&id), Err(Ok(Error::InvalidState)));
    fx.set_time(REGISTER_BY);
    fx.client().submit_results(&id, &players);
    fx.set_time(CLAIM_BY - 1);
    assert_eq!(fx.client().try_refund_unclaimed(&id), Err(Ok(Error::ClaimWindowOpen)));
    assert_eq!(fx.token().balance(&fx.sponsor), 0);
}

#[test]
fn refund_unclaimed_returns_remainder_closes_and_needs_no_auth() {
    let fx = Fx::new();
    let id = fx.create_open(1_000 * USDC, &fx.bps(&[5000, 3000, 2000]), None);
    let players = fx.register_players(id, 3);
    fx.set_time(REGISTER_BY);
    fx.client().submit_results(&id, &players);
    fx.client().claim(&id, &players.get(1).unwrap());
    fx.set_time(CLAIM_BY);

    fx.deny_all_auth(); // anyone (even an unsigned call) can sweep
    assert_eq!(fx.client().refund_unclaimed(&id), 700 * USDC);
    assert_eq!(fx.event_names(), ["unclaimed_refunded"]);
    assert_eq!(fx.token().balance(&fx.sponsor), 700 * USDC);
    assert_eq!(fx.token().balance(&fx.contract), 0);
    let t = fx.client().status(&id);
    assert_eq!(t.state, State::Closed);
    assert_eq!(fx.client().claimable(&id, &players.get(0).unwrap()), 0);
    assert_eq!(fx.client().paid_out(&id), 300 * USDC);
    // closed is terminal
    assert_eq!(fx.client().try_refund_unclaimed(&id), Err(Ok(Error::InvalidState)));
    fx.env.mock_all_auths();
    assert_eq!(fx.client().try_claim(&id, &players.get(0).unwrap()), Err(Ok(Error::InvalidState)));
    assert_eq!(fx.client().try_cancel(&id), Err(Ok(Error::InvalidState)));
}

// ---------------------------------------------------------------------------
// cancel / cancel_expired
// ---------------------------------------------------------------------------

#[test]
fn cancel_refunds_sponsor_from_created_open_and_submitted() {
    let fx = Fx::new();
    let p = fx.bps(&[10_000]);

    // Created, partially funded
    let a = fx.create(300 * USDC, &p, None);
    fx.mint(&fx.sponsor, 300 * USDC);
    fx.client().fund(&a, &fx.sponsor, &(120 * USDC));
    assert_eq!(fx.client().cancel(&a), 120 * USDC);
    assert_eq!(fx.event_names(), ["tournament_cancelled"]);
    assert_eq!(fx.client().status(&a).state, State::Cancelled);
    assert_eq!(fx.token().balance(&fx.sponsor), 300 * USDC);

    // Created, never funded: refund of zero still cancels
    let z = fx.create(50 * USDC, &p, None);
    assert_eq!(fx.client().cancel(&z), 0);
    assert_eq!(fx.client().status(&z).state, State::Cancelled);

    // Open
    let b = fx.create_open(500 * USDC, &p, None);
    fx.register_players(b, 4);
    assert_eq!(fx.client().cancel(&b), 500 * USDC);
    assert_eq!(fx.token().balance(&fx.sponsor), 800 * USDC);

    // ResultsSubmitted (referee has not confirmed)
    let c = fx.create_open(200 * USDC, &p, Some(fx.referee.clone()));
    let players = fx.register_players(c, 2);
    fx.set_time(REGISTER_BY);
    fx.client().submit_results(&c, &Vec::from_array(&fx.env, [players.get(0).unwrap()]));
    assert_eq!(fx.client().cancel(&c), 200 * USDC);
    assert_eq!(fx.token().balance(&fx.sponsor), 1_000 * USDC);
    assert_eq!(fx.token().balance(&fx.contract), 0);
    assert_eq!(fx.client().ranking(&c).len(), 0);
    // cancelled is terminal
    assert_eq!(fx.client().try_fund(&a, &fx.sponsor, &1), Err(Ok(Error::InvalidState)));
    assert_eq!(fx.client().try_cancel(&a), Err(Ok(Error::InvalidState)));
}

#[test]
fn cancel_rejects_after_results_confirmed() {
    let fx = Fx::new();
    let id = fx.create_open(1_000 * USDC, &fx.bps(&[10_000]), None);
    let players = fx.register_players(id, 1);
    fx.set_time(REGISTER_BY);
    fx.client().submit_results(&id, &players);
    assert_eq!(fx.client().try_cancel(&id), Err(Ok(Error::InvalidState)));
    fx.set_time(RESULTS_BY + DAY);
    assert_eq!(fx.client().try_cancel_expired(&id), Err(Ok(Error::InvalidState)));
    assert_eq!(fx.token().balance(&fx.contract), 1_000 * USDC);
}

#[test]
fn cancel_requires_organizer_auth() {
    let fx = Fx::new();
    let id = fx.create_open(1_000 * USDC, &fx.bps(&[10_000]), None);
    // not even the sponsor whose money it is
    fx.only_auth(&fx.sponsor, "cancel", (id,).into_val(&fx.env));
    assert!(fx.client().try_cancel(&id).is_err());
    fx.deny_all_auth();
    assert!(fx.client().try_cancel(&id).is_err());
    assert_eq!(fx.client().status(&id).state, State::Open);
    assert_eq!(fx.token().balance(&fx.contract), 1_000 * USDC);
}

#[test]
fn cancel_expired_only_after_results_by_and_needs_no_auth() {
    let fx = Fx::new();
    let id = fx.create_open(1_000 * USDC, &fx.bps(&[10_000]), Some(fx.referee.clone()));
    let players = fx.register_players(id, 2);
    fx.set_time(RESULTS_BY - 1);
    assert_eq!(fx.client().try_cancel_expired(&id), Err(Ok(Error::ResultsDeadlineNotReached)));

    // organizer submitted but the referee never confirmed
    fx.client().submit_results(&id, &Vec::from_array(&fx.env, [players.get(0).unwrap()]));
    fx.set_time(RESULTS_BY);
    fx.deny_all_auth();
    assert_eq!(fx.client().cancel_expired(&id), 1_000 * USDC);
    assert_eq!(fx.event_names(), ["tournament_cancelled"]);
    assert_eq!(fx.client().status(&id).state, State::Cancelled);
    assert_eq!(fx.token().balance(&fx.sponsor), 1_000 * USDC);
    assert_eq!(fx.token().balance(&fx.contract), 0);
}

#[test]
fn cancel_expired_rescues_funds_when_organizer_disappears_before_opening() {
    let fx = Fx::new();
    let id = fx.create(1_000 * USDC, &fx.bps(&[10_000]), None);
    fx.mint(&fx.sponsor, 1_000 * USDC);
    fx.client().fund(&id, &fx.sponsor, &(1_000 * USDC));
    // organizer never calls open_registration
    fx.set_time(RESULTS_BY);
    fx.deny_all_auth();
    assert_eq!(fx.client().cancel_expired(&id), 1_000 * USDC);
    assert_eq!(fx.token().balance(&fx.sponsor), 1_000 * USDC);
}

// ---------------------------------------------------------------------------
// views, storage, isolation
// ---------------------------------------------------------------------------

#[test]
fn status_of_unknown_id_is_not_found() {
    let fx = Fx::new();
    assert_eq!(fx.client().try_status(&7), Err(Ok(Error::NotFound)));
    assert_eq!(fx.client().try_open_registration(&7), Err(Ok(Error::NotFound)));
    assert_eq!(fx.client().try_cancel_expired(&7), Err(Ok(Error::NotFound)));
    assert_eq!(fx.client().claimable(&7, &fx.sponsor), 0);
    assert_eq!(fx.client().roster(&7).len(), 0);
    assert_eq!(fx.client().count(), 0);
}

#[test]
fn ttl_is_extended_on_write() {
    let fx = Fx::new();
    let id = fx.create_open(100 * USDC, &fx.bps(&[10_000]), None);
    let p = fx.register_players(id, 1).get(0).unwrap();
    fx.env.as_contract(&fx.contract, || {
        let s = fx.env.storage();
        assert!(s.persistent().get_ttl(&DataKey::Tournament(id)) >= TTL_EXTEND_TO - 1);
        assert!(s.persistent().get_ttl(&DataKey::Roster(id)) >= TTL_EXTEND_TO - 1);
        assert!(s.persistent().get_ttl(&DataKey::PaidOut(id)) >= TTL_EXTEND_TO - 1);
        assert!(s.instance().get_ttl() >= TTL_EXTEND_TO - 1);
    });
    assert!(fx.client().is_registered(&id, &p));
}

#[test]
fn tournaments_are_isolated_from_each_other() {
    let fx = Fx::new();
    let a = fx.create_open(1_000 * USDC, &fx.bps(&[10_000]), None);
    let b = fx.create_open(250 * USDC, &fx.bps(&[10_000]), None);
    assert_eq!(fx.token().balance(&fx.contract), 1_250 * USDC);
    let pa = fx.register_players(a, 2);
    let pb = fx.register_players(b, 2);
    assert!(!fx.client().is_registered(&a, &pb.get(0).unwrap()));

    // cancelling A must not touch B's escrow
    fx.client().cancel(&a);
    assert_eq!(fx.token().balance(&fx.sponsor), 1_000 * USDC);
    assert_eq!(fx.token().balance(&fx.contract), 250 * USDC);
    assert_eq!(fx.client().status(&b).state, State::Open);

    fx.set_time(REGISTER_BY);
    // A's players cannot be ranked in B
    assert_eq!(
        fx.client().try_submit_results(&b, &Vec::from_array(&fx.env, [pa.get(0).unwrap()])),
        Err(Ok(Error::NotRegistered))
    );
    fx.client().submit_results(&b, &Vec::from_array(&fx.env, [pb.get(1).unwrap()]));
    assert_eq!(fx.client().claim(&b, &pb.get(1).unwrap()), 250 * USDC);
    assert_eq!(fx.token().balance(&fx.contract), 0);
}

// ---------------------------------------------------------------------------
// Scenario: a 64-player bracket with 16 placements, partial claims and sweep
// ---------------------------------------------------------------------------

#[test]
fn scenario_64_player_bracket() {
    let fx = Fx::new();
    let pool: i128 = 77_777_777_777; // 7,777.7777777 USDC: forces rounding dust
    let placements = fx.placements16();

    std::println!("== PrizeVault scenario: 64-player bracket, 16 placements ==");
    std::println!("pool_required = {} stroops (7,777.7777777 USDC)", pool);

    // 1. create, fund in two tranches, open registration (proof of funds)
    let id = fx.create(pool, &placements, Some(fx.referee.clone()));
    fx.mint(&fx.sponsor, pool);
    fx.client().fund(&id, &fx.sponsor, &(50_000_000_000));
    assert_eq!(fx.client().try_open_registration(&id), Err(Ok(Error::Underfunded)));
    fx.client().fund(&id, &fx.sponsor, &(pool - 50_000_000_000));
    fx.client().open_registration(&id);
    std::println!("registration opened: pool_funded = {} / {}", fx.client().status(&id).pool_funded, pool);
    assert_eq!(fx.token().balance(&fx.contract), pool);

    // 2. 64 players register over the week; the 65th is late
    let players = fx.register_players(id, 64);
    assert_eq!(fx.client().roster(&id).len(), 64);
    fx.set_time(REGISTER_BY);
    let late = Address::generate(&fx.env);
    assert_eq!(fx.client().try_register(&id, &late), Err(Ok(Error::RegistrationClosed)));
    std::println!("roster closed with {} players", fx.client().roster(&id).len());

    // 3. bracket resolves; the organizer's first export has a duplicate entry
    //    (seed file data/seed/results.json reproduces this) and is rejected
    let winner = |i: u32| players.get(i).unwrap();
    let mut bad: Vec<Address> = Vec::new(&fx.env);
    for i in 0..16u32 {
        // seat 0 is exported twice (1st and 12th place)
        bad.push_back(winner(if i == 11 { 0 } else { i * 3 }));
    }
    fx.set_time(REGISTER_BY + 2 * DAY);
    assert_eq!(fx.client().try_submit_results(&id, &bad), Err(Ok(Error::DuplicateInRanking)));
    std::println!("duplicate ranking rejected");

    // 4. corrected ranking: seats 0,3,6,...,45 finish 1st..16th
    let mut ranking: Vec<Address> = Vec::new(&fx.env);
    for i in 0..16u32 {
        ranking.push_back(winner(i * 3));
    }
    fx.client().submit_results(&id, &ranking);
    assert_eq!(fx.client().status(&id).state, State::ResultsSubmitted);

    // 5. referee confirms
    fx.client().confirm_results(&id);
    assert_eq!(fx.client().status(&id).state, State::Confirmed);
    let amounts = placement_amounts(&fx.env, pool, &placements);
    assert_eq!(sum(&amounts), pool);
    let mut expected_total: i128 = 0;
    for i in 0..16u32 {
        let a = fx.client().claimable(&id, &winner(i * 3));
        assert_eq!(a, amounts.get(i).unwrap());
        expected_total += a;
        std::println!("  place {:>2}: {:>5} bps -> {:>14} stroops", i + 1, placements.get(i).unwrap(), a);
    }
    assert_eq!(expected_total, pool);
    assert_eq!(amounts.get(15).unwrap(), 583_333_340); // 583_333_333 + 7 dust

    // 6. partial claims: 11 of 16 winners claim within the window
    let claimers: [u32; 11] = [0, 1, 2, 4, 7, 8, 9, 12, 13, 14, 15];
    let mut claimed_total: i128 = 0;
    for &i in claimers.iter() {
        fx.set_time(REGISTER_BY + 3 * DAY + (i as u64) * 3_600);
        let paid = fx.client().claim(&id, &winner(i * 3));
        assert_eq!(fx.token().balance(&winner(i * 3)), paid);
        claimed_total += paid;
    }
    assert_eq!(claimed_total, 59_888_888_891);
    assert_eq!(fx.client().paid_out(&id), claimed_total);
    // a registered non-winner and a double claimer get nothing
    assert_eq!(fx.client().try_claim(&id, &winner(1)), Err(Ok(Error::NothingToClaim)));
    assert_eq!(fx.client().try_claim(&id, &winner(0)), Err(Ok(Error::NothingToClaim)));
    // sweeping early is refused
    assert_eq!(fx.client().try_refund_unclaimed(&id), Err(Ok(Error::ClaimWindowOpen)));
    std::println!("{} winners claimed {} stroops", claimers.len(), claimed_total);

    // 7. claim window closes: places 4, 6, 7, 11, 12 never claimed
    fx.set_time(CLAIM_BY);
    let unclaimed_expected: i128 = amounts.get(3).unwrap()
        + amounts.get(5).unwrap()
        + amounts.get(6).unwrap()
        + amounts.get(10).unwrap()
        + amounts.get(11).unwrap();
    assert_eq!(unclaimed_expected, 17_888_888_886);
    assert_eq!(fx.client().try_claim(&id, &winner(3 * 3)), Err(Ok(Error::ClaimWindowClosed)));

    let refunded = fx.client().refund_unclaimed(&id);
    assert_eq!(refunded, unclaimed_expected);
    assert_eq!(refunded, pool - claimed_total);
    assert_eq!(fx.token().balance(&fx.sponsor), unclaimed_expected);
    assert_eq!(fx.token().balance(&fx.contract), 0);
    assert_eq!(fx.client().status(&id).state, State::Closed);
    std::println!("unclaimed {} stroops refunded to sponsor; vault balance 0; state Closed", refunded);
}
