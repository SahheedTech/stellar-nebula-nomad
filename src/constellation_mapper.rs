//! Constellation discovery and mapping state (#72, completed in #632).
//!
//! Explorers record star patterns (immutable once recorded) and annotate them
//! once with a name, an anchor region and lore. Other players **recognize**
//! those patterns from partial, noisy observations, use them as **navigation
//! aids** toward the anchor region, and **sight** them for discovery bonuses
//! that build a personal **collection** with milestone rewards. Sightings need
//! a clear sky: weather visibility in the observer's region must be at least
//! [`MIN_SIGHTING_VISIBILITY`]. See `docs/CONSTELLATION_MAPPING.md`.
use soroban_sdk::{contracterror, contracttype, symbol_short, Address, BytesN, Env, String, Vec};

use crate::resource_minter::{self, ResourceType};
use crate::weather_system;

/// Minimum number of stars required to form a valid constellation.
pub const MIN_STARS: u32 = 3;

/// Maximum patterns matched per `match_constellation` call.
pub const MAX_MATCH_BURST: u32 = 15;

/// Most recent constellations considered by [`recognize_pattern`].
pub const MAX_RECOGNITION_SCAN: u32 = 50;

/// Share of a constellation's stars an observation must contain to recognize it.
pub const MIN_RECOGNITION_PCT: u32 = 60;

/// Weather visibility (percent) needed to sight a constellation.
pub const MIN_SIGHTING_VISIBILITY: u32 = 70;

/// Stellar Dust awarded for a player's first sighting of a constellation.
pub const SIGHTING_BONUS: u64 = 50;

/// Multiplier for the very first sighting of a constellation by anyone.
pub const FIRST_SIGHTING_MULTIPLIER: u64 = 2;

/// Collection sizes that award a one-time milestone bonus, and the bonuses.
pub const COLLECTION_MILESTONES: [(u32, u64); 3] = [(5, 200), (10, 500), (25, 1_500)];

/// Upper bound on a player's collection.
pub const MAX_COLLECTION_SIZE: u32 = 500;

/// Name and lore length limits, in bytes.
pub const MAX_NAME_LEN: u32 = 32;
pub const MAX_LORE_LEN: u32 = 512;

// ─── Storage Keys ──────────────────────────────────────────────────────────────

#[derive(Clone)]
#[contracttype]
pub enum ConstellationKey {
    /// Auto-incrementing constellation ID.
    ConstellationCounter,
    /// Constellation data by ID.
    Constellation(u64),
    /// Name, anchor region and lore by ID (set once by the recorder).
    Meta(u64),
    /// Constellation IDs a player has sighted, in sighting order.
    Collection(Address),
    /// First player (other than the recorder) to sight a constellation.
    FirstSighter(u64),
    /// Number of collection milestones a player has been paid for.
    Milestones(Address),
    /// Sightings of other players' constellations (milestones count these).
    BonusSightings(Address),
}

// ─── Errors ─────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ConstellationError {
    /// No known constellation matched the observed pattern.
    NoMatchFound = 1,
    /// Star vector is below the minimum count.
    TooFewStars = 2,
    /// Attempted to mutate an immutable historical record.
    ImmutableRecord = 3,
    /// Requested burst size exceeds MAX_MATCH_BURST.
    BurstTooLarge = 4,
    /// Weather visibility in the region is too low to sight stars.
    SkyObscured = 5,
    /// Only the recorder may annotate a constellation.
    Unauthorized = 6,
    /// The constellation already has a name, anchor and lore.
    AlreadyAnnotated = 7,
    /// Name or lore is empty or too long.
    InvalidMetadata = 8,
    /// The player's collection is full.
    CollectionFull = 9,
    /// Crediting the bonus failed (balance overflow).
    RewardFailed = 10,
}

impl crate::error_standard::StandardContractError for ConstellationError {
    fn descriptor(self) -> crate::error_standard::ErrorDescriptor {
        use crate::error_standard::ErrorKind;
        let (kind, retryable) = match self {
            Self::NoMatchFound => (ErrorKind::NotFound, false),
            Self::TooFewStars => (ErrorKind::Validation, false),
            Self::ImmutableRecord => (ErrorKind::Conflict, false),
            Self::BurstTooLarge | Self::CollectionFull => (ErrorKind::ResourceLimit, false),
            Self::SkyObscured => (ErrorKind::Conflict, true),
            Self::Unauthorized => (ErrorKind::Authorization, false),
            Self::AlreadyAnnotated => (ErrorKind::Conflict, false),
            Self::InvalidMetadata => (ErrorKind::Validation, false),
            Self::RewardFailed => (ErrorKind::Internal, false),
        };
        crate::error_standard::ErrorDescriptor {
            module: "constellation_mapper",
            code: self as u32,
            kind,
            retryable,
        }
    }
}

// ─── Data Types ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct Constellation {
    pub id: u64,
    pub recorder: Address,
    /// Ordered list of star hashes (coordinate + spectral fingerprint).
    pub stars: Vec<BytesN<32>>,
    pub recorded_at: u64,
    /// Optional name (stored as first star's hash parity for on-chain cheapness).
    pub star_count: u32,
}

#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct MatchResult {
    pub constellation_id: u64,
    /// Number of stars that coincided with the query.
    pub matched_stars: u32,
    pub total_stars: u32,
}

/// Name, navigation anchor and lore of a constellation.
#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct ConstellationMeta {
    pub name: String,
    /// Region (nebula) the constellation points navigators toward.
    pub anchor_region: u64,
    pub lore: String,
}

/// A recognized pattern and how confident the recognition is.
#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct Recognition {
    pub constellation_id: u64,
    pub matched_stars: u32,
    pub total_stars: u32,
    /// Share of the constellation's stars present in the observation.
    pub confidence_pct: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracttype]
pub enum Heading {
    /// The navigator is already in the anchor region.
    Arrived,
    /// The anchor lies at a higher region index.
    Ascending,
    /// The anchor lies at a lower region index.
    Descending,
    /// The constellation has no anchor yet.
    Unknown,
}

/// Where a recognized constellation points.
#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct NavigationAid {
    pub recognition: Recognition,
    pub name: String,
    pub anchor_region: u64,
    pub heading: Heading,
    /// Region steps to the anchor (0 when arrived or unknown).
    pub distance: u64,
}

/// Result of a sighting.
#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct SightingResult {
    pub recognition: Recognition,
    /// First time this player sighted the constellation.
    pub new_sighting: bool,
    /// First time anyone (other than the recorder) sighted it.
    pub first_sighter: bool,
    /// Stellar Dust credited (sighting bonus plus any milestone bonus).
    pub bonus_awarded: u64,
    pub collection_size: u32,
}

/// A player's collection progress.
#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct CollectionProgress {
    pub sighted: Vec<u64>,
    pub sighted_count: u32,
    pub total_constellations: u64,
    /// sighted / total, in percent.
    pub completion_pct: u32,
    /// Collection size of the next unpaid milestone (0 when all are paid).
    pub next_milestone: u32,
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Count how many elements of `observed` appear in `known`.
///
/// Both slices are small (≤ 50 stars), so O(n*m) is acceptable within gas
/// limits for on-chain execution.
fn count_matching_stars(observed: &Vec<BytesN<32>>, known: &Vec<BytesN<32>>) -> u32 {
    let mut count = 0u32;
    for obs in observed.iter() {
        for kn in known.iter() {
            if obs == kn {
                count += 1;
                break;
            }
        }
    }
    count
}

// ─── Public API ─────────────────────────────────────────────────────────────

/// Record a new constellation pattern on-chain.
///
/// Records are immutable after creation to preserve historical navigation data.
/// Returns the assigned constellation ID.
pub fn record_constellation(
    env: &Env,
    recorder: &Address,
    stars: &Vec<BytesN<32>>,
) -> Result<u64, ConstellationError> {
    recorder.require_auth();

    if stars.len() < MIN_STARS {
        return Err(ConstellationError::TooFewStars);
    }

    let id: u64 = env
        .storage()
        .instance()
        .get(&ConstellationKey::ConstellationCounter)
        .unwrap_or(0);

    let now = env.ledger().timestamp();
    let star_count = stars.len();

    let constellation = Constellation {
        id,
        recorder: recorder.clone(),
        stars: stars.clone(),
        recorded_at: now,
        star_count,
    };

    env.storage()
        .persistent()
        .set(&ConstellationKey::Constellation(id), &constellation);
    env.storage()
        .instance()
        .set(&ConstellationKey::ConstellationCounter, &(id + 1));

    env.events().publish(
        (symbol_short!("const"), symbol_short!("record")),
        (recorder.clone(), id, star_count, now),
    );

    Ok(id)
}

/// Find the known constellation with the highest star overlap with `observed`.
///
/// Returns the best [`MatchResult`], or [`ConstellationError::NoMatchFound`]
/// if no constellation shares at least one star with the query.
pub fn match_constellation(
    env: &Env,
    observed: &Vec<BytesN<32>>,
) -> Result<MatchResult, ConstellationError> {
    if observed.len() < MIN_STARS {
        return Err(ConstellationError::TooFewStars);
    }

    let total: u64 = env
        .storage()
        .instance()
        .get(&ConstellationKey::ConstellationCounter)
        .unwrap_or(0);

    if total == 0 {
        return Err(ConstellationError::NoMatchFound);
    }

    // Cap the search to MAX_MATCH_BURST most-recently recorded constellations.
    let search_from = total.saturating_sub(MAX_MATCH_BURST as u64);

    let mut best_id: u64 = 0;
    let mut best_matches: u32 = 0;
    let mut best_total: u32 = 0;
    let mut found = false;

    for id in search_from..total {
        let constellation: Constellation = match env
            .storage()
            .persistent()
            .get(&ConstellationKey::Constellation(id))
        {
            Some(c) => c,
            None => continue,
        };

        let matched = count_matching_stars(observed, &constellation.stars);
        if matched > best_matches {
            best_matches = matched;
            best_id = id;
            best_total = constellation.star_count;
            found = true;
        }
    }

    if !found || best_matches == 0 {
        return Err(ConstellationError::NoMatchFound);
    }

    let result = MatchResult {
        constellation_id: best_id,
        matched_stars: best_matches,
        total_stars: best_total,
    };

    env.events().publish(
        (symbol_short!("const"), symbol_short!("matched")),
        (best_id, best_matches, best_total),
    );

    Ok(result)
}

/// Match multiple observed patterns in one call (burst mode).
///
/// Returns a list of the best [`MatchResult`] for each pattern.
/// Patterns that yield no match are silently skipped (caller should check
/// result length vs input length).
pub fn match_constellations_batch(
    env: &Env,
    patterns: &Vec<Vec<BytesN<32>>>,
) -> Result<Vec<MatchResult>, ConstellationError> {
    if patterns.len() > MAX_MATCH_BURST {
        return Err(ConstellationError::BurstTooLarge);
    }

    let mut results: Vec<MatchResult> = Vec::new(env);
    for pattern in patterns.iter() {
        if let Ok(r) = match_constellation(env, &pattern) {
            results.push_back(r);
        }
    }

    Ok(results)
}

/// Retrieve a stored constellation by ID.
pub fn get_constellation(
    env: &Env,
    constellation_id: u64,
) -> Result<Constellation, ConstellationError> {
    env.storage()
        .persistent()
        .get(&ConstellationKey::Constellation(constellation_id))
        .ok_or(ConstellationError::NoMatchFound)
}

/// Return the total number of recorded constellations.
pub fn get_constellation_count(env: &Env) -> u64 {
    env.storage()
        .instance()
        .get(&ConstellationKey::ConstellationCounter)
        .unwrap_or(0)
}

// ─── Pattern recognition ────────────────────────────────────────────────────

/// Recognize an observed pattern among the most recent
/// [`MAX_RECOGNITION_SCAN`] constellations. Order does not matter and the
/// observation may contain extra stars; it must contain at least
/// [`MIN_RECOGNITION_PCT`] of a constellation's stars. The most confident
/// match wins, then the larger constellation.
pub fn recognize_pattern(
    env: &Env,
    observed: &Vec<BytesN<32>>,
) -> Result<Recognition, ConstellationError> {
    if observed.len() < MIN_STARS {
        return Err(ConstellationError::TooFewStars);
    }
    let total = get_constellation_count(env);
    let mut best: Option<Recognition> = None;
    for id in total.saturating_sub(u64::from(MAX_RECOGNITION_SCAN))..total {
        let Ok(constellation) = get_constellation(env, id) else {
            continue;
        };
        let matched = count_matching_stars(observed, &constellation.stars);
        let confidence_pct = matched * 100 / constellation.star_count.max(1);
        if confidence_pct < MIN_RECOGNITION_PCT {
            continue;
        }
        let candidate = Recognition {
            constellation_id: id,
            matched_stars: matched,
            total_stars: constellation.star_count,
            confidence_pct,
        };
        let better = match &best {
            None => true,
            Some(b) => {
                (candidate.confidence_pct, candidate.total_stars)
                    > (b.confidence_pct, b.total_stars)
            }
        };
        if better {
            best = Some(candidate);
        }
    }
    best.ok_or(ConstellationError::NoMatchFound)
}

// ─── Lore & navigation ──────────────────────────────────────────────────────

/// Name a constellation, anchor it to a region and attach its lore. Only the
/// recorder may do this, once, keeping the record immutable in spirit.
pub fn annotate_constellation(
    env: &Env,
    recorder: &Address,
    constellation_id: u64,
    name: String,
    anchor_region: u64,
    lore: String,
) -> Result<ConstellationMeta, ConstellationError> {
    recorder.require_auth();
    let constellation = get_constellation(env, constellation_id)?;
    if constellation.recorder != *recorder {
        return Err(ConstellationError::Unauthorized);
    }
    let key = ConstellationKey::Meta(constellation_id);
    if env.storage().persistent().has(&key) {
        return Err(ConstellationError::AlreadyAnnotated);
    }
    if name.len() == 0 || name.len() > MAX_NAME_LEN || lore.len() > MAX_LORE_LEN {
        return Err(ConstellationError::InvalidMetadata);
    }
    let meta = ConstellationMeta {
        name,
        anchor_region,
        lore,
    };
    env.storage().persistent().set(&key, &meta);
    env.events().publish(
        (symbol_short!("const"), symbol_short!("lore")),
        (constellation_id, anchor_region),
    );
    Ok(meta)
}

/// Name, anchor and lore of a constellation, if annotated.
pub fn get_constellation_meta(env: &Env, constellation_id: u64) -> Option<ConstellationMeta> {
    env.storage()
        .persistent()
        .get(&ConstellationKey::Meta(constellation_id))
}

/// Recognize an observed pattern and say where it points from `current_region`.
pub fn navigation_aid(
    env: &Env,
    observed: &Vec<BytesN<32>>,
    current_region: u64,
) -> Result<NavigationAid, ConstellationError> {
    let recognition = recognize_pattern(env, observed)?;
    let Some(meta) = get_constellation_meta(env, recognition.constellation_id) else {
        return Ok(NavigationAid {
            recognition,
            name: String::from_str(env, ""),
            anchor_region: 0,
            heading: Heading::Unknown,
            distance: 0,
        });
    };
    let (heading, distance) = if meta.anchor_region == current_region {
        (Heading::Arrived, 0)
    } else if meta.anchor_region > current_region {
        (Heading::Ascending, meta.anchor_region - current_region)
    } else {
        (Heading::Descending, current_region - meta.anchor_region)
    };
    Ok(NavigationAid {
        recognition,
        name: meta.name,
        anchor_region: meta.anchor_region,
        heading,
        distance,
    })
}

// ─── Sightings, bonuses & collection ────────────────────────────────────────

fn milestones_paid(env: &Env, player: &Address) -> u32 {
    env.storage()
        .persistent()
        .get(&ConstellationKey::Milestones(player.clone()))
        .unwrap_or(0)
}

/// Constellation IDs a player has sighted.
pub fn get_sighted(env: &Env, player: &Address) -> Vec<u64> {
    env.storage()
        .persistent()
        .get(&ConstellationKey::Collection(player.clone()))
        .unwrap_or_else(|| Vec::new(env))
}

/// First player (other than the recorder) to sight a constellation.
pub fn get_first_sighter(env: &Env, constellation_id: u64) -> Option<Address> {
    env.storage()
        .persistent()
        .get(&ConstellationKey::FirstSighter(constellation_id))
}

/// Sight a constellation from `region_id`. A player's first sighting of a
/// constellation adds it to their collection and pays [`SIGHTING_BONUS`]
/// (doubled for the first sighter overall); reaching a collection milestone
/// pays its bonus once. Recorders collect their own constellations without a
/// bonus, and those sightings do not count toward milestones, so recording
/// patterns cannot farm rewards directly.
pub fn sight_constellation(
    env: &Env,
    observer: &Address,
    observed: &Vec<BytesN<32>>,
    region_id: u64,
) -> Result<SightingResult, ConstellationError> {
    observer.require_auth();
    if weather_system::nebula_visibility(env, region_id) < MIN_SIGHTING_VISIBILITY {
        return Err(ConstellationError::SkyObscured);
    }
    let recognition = recognize_pattern(env, observed)?;
    let id = recognition.constellation_id;
    let mut sighted = get_sighted(env, observer);

    if sighted.contains(id) {
        return Ok(SightingResult {
            recognition,
            new_sighting: false,
            first_sighter: false,
            bonus_awarded: 0,
            collection_size: sighted.len(),
        });
    }
    if sighted.len() >= MAX_COLLECTION_SIZE {
        return Err(ConstellationError::CollectionFull);
    }
    sighted.push_back(id);
    env.storage()
        .persistent()
        .set(&ConstellationKey::Collection(observer.clone()), &sighted);

    let is_recorder = get_constellation(env, id)?.recorder == *observer;
    let first_key = ConstellationKey::FirstSighter(id);
    let first_sighter = !is_recorder && !env.storage().persistent().has(&first_key);
    if first_sighter {
        env.storage().persistent().set(&first_key, observer);
    }

    let mut bonus = 0u64;
    if !is_recorder {
        bonus = if first_sighter {
            SIGHTING_BONUS * FIRST_SIGHTING_MULTIPLIER
        } else {
            SIGHTING_BONUS
        };
        let counter_key = ConstellationKey::BonusSightings(observer.clone());
        let bonus_sightings: u32 = env.storage().persistent().get(&counter_key).unwrap_or(0) + 1;
        env.storage()
            .persistent()
            .set(&counter_key, &bonus_sightings);
        let paid = milestones_paid(env, observer);
        if let Some((size, milestone_bonus)) = COLLECTION_MILESTONES.get(paid as usize) {
            if bonus_sightings >= *size {
                bonus += milestone_bonus;
                env.storage()
                    .persistent()
                    .set(&ConstellationKey::Milestones(observer.clone()), &(paid + 1));
            }
        }
        resource_minter::credit_balance(env, observer, &ResourceType::StellarDust, bonus)
            .map_err(|_| ConstellationError::RewardFailed)?;
    }

    env.events().publish(
        (symbol_short!("const"), symbol_short!("sighted")),
        (observer.clone(), id, first_sighter, bonus),
    );

    Ok(SightingResult {
        recognition,
        new_sighting: true,
        first_sighter,
        bonus_awarded: bonus,
        collection_size: sighted.len(),
    })
}

/// A player's collection progress toward every recorded constellation.
pub fn get_collection(env: &Env, player: &Address) -> CollectionProgress {
    let sighted = get_sighted(env, player);
    let total = get_constellation_count(env);
    let count = sighted.len();
    let completion_pct = if total == 0 {
        0
    } else {
        (u64::from(count) * 100 / total) as u32
    };
    let next_milestone = COLLECTION_MILESTONES
        .get(milestones_paid(env, player) as usize)
        .map_or(0, |(size, _)| *size);
    CollectionProgress {
        sighted,
        sighted_count: count,
        total_constellations: total,
        completion_pct,
        next_milestone,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emergency_controls;
    use crate::weather_system::WeatherType;
    use soroban_sdk::testutils::{Address as _, Ledger};
    use soroban_sdk::{contract, contractimpl};

    #[contract]
    struct Stub;
    #[contractimpl]
    impl Stub {}

    const CLEAR: u64 = 1;
    const STORMY: u64 = 2;

    struct Fx {
        env: Env,
        contract: Address,
        admin: Address,
        recorder: Address,
    }

    impl Fx {
        fn run<T>(&self, f: impl FnOnce() -> T) -> T {
            self.env.as_contract(&self.contract, f)
        }

        fn stars(&self, ids: &[u8]) -> Vec<BytesN<32>> {
            let mut v = Vec::new(&self.env);
            for id in ids {
                v.push_back(BytesN::from_array(&self.env, &[*id; 32]));
            }
            v
        }

        fn record(&self, ids: &[u8]) -> u64 {
            let stars = self.stars(ids);
            self.run(|| record_constellation(&self.env, &self.recorder, &stars).unwrap())
        }

        fn weather(&self, region: u64, weather: WeatherType) {
            self.run(|| {
                let mut weights = Vec::new(&self.env);
                for i in 0..weather_system::WEATHER_TYPE_COUNT {
                    weights.push_back(u32::from(i == weather.index()));
                }
                weather_system::set_region_climate(&self.env, &self.admin, region, weights)
                    .unwrap();
            });
        }

        fn sight(
            &self,
            who: &Address,
            ids: &[u8],
            region: u64,
        ) -> Result<SightingResult, ConstellationError> {
            let stars = self.stars(ids);
            self.run(|| sight_constellation(&self.env, who, &stars, region))
        }

        fn dust(&self, who: &Address) -> u64 {
            self.run(|| resource_minter::balance_of(&self.env, who, &ResourceType::StellarDust))
        }
    }

    fn fx() -> Fx {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(500_000);
        let contract = env.register(Stub, ());
        let admin = Address::generate(&env);
        let recorder = Address::generate(&env);
        let a = admin.clone();
        env.as_contract(&contract, || {
            emergency_controls::initialize_admins(&env, Vec::from_array(&env, [a])).unwrap();
        });
        let f = Fx {
            env,
            contract,
            admin,
            recorder,
        };
        f.weather(CLEAR, WeatherType::PerfectConditions);
        f.weather(STORMY, WeatherType::IonStorm);
        f
    }

    #[test]
    fn recognizes_partial_noisy_and_reordered_observations() {
        let f = fx();
        let orion = f.record(&[1, 2, 3, 4, 5, 6]);
        let lyra = f.record(&[10, 11, 12]);
        f.run(|| {
            let r = recognize_pattern(&f.env, &f.stars(&[4, 99, 1, 3, 2])).unwrap();
            assert_eq!(r.constellation_id, orion);
            assert_eq!(
                (r.matched_stars, r.total_stars, r.confidence_pct),
                (4, 6, 66)
            );
            let r = recognize_pattern(&f.env, &f.stars(&[12, 10, 11, 1])).unwrap();
            assert_eq!((r.constellation_id, r.confidence_pct), (lyra, 100));
            assert_eq!(
                recognize_pattern(&f.env, &f.stars(&[1, 2, 3, 97, 98])),
                Err(ConstellationError::NoMatchFound),
                "half of the stars is below the recognition threshold"
            );
            assert_eq!(
                recognize_pattern(&f.env, &f.stars(&[1, 2])),
                Err(ConstellationError::TooFewStars)
            );
        });
    }

    #[test]
    fn annotation_is_recorder_only_validated_and_once() {
        let f = fx();
        let id = f.record(&[1, 2, 3]);
        let name = String::from_str(&f.env, "The Lantern");
        let lore = String::from_str(&f.env, "Lit by the first nomads to cross the Veil.");
        let stranger = Address::generate(&f.env);
        f.run(|| {
            assert_eq!(
                annotate_constellation(&f.env, &stranger, id, name.clone(), 9, lore.clone()),
                Err(ConstellationError::Unauthorized)
            );
        });
        f.run(|| {
            assert_eq!(
                annotate_constellation(
                    &f.env,
                    &f.recorder,
                    id,
                    String::from_str(&f.env, ""),
                    9,
                    lore.clone()
                ),
                Err(ConstellationError::InvalidMetadata)
            );
        });
        let meta = f.run(|| {
            annotate_constellation(&f.env, &f.recorder, id, name.clone(), 9, lore.clone()).unwrap()
        });
        assert_eq!(meta.anchor_region, 9);
        assert_eq!(f.run(|| get_constellation_meta(&f.env, id)), Some(meta));
        f.run(|| {
            assert_eq!(
                annotate_constellation(&f.env, &f.recorder, id, name, 3, lore),
                Err(ConstellationError::AlreadyAnnotated)
            );
        });
    }

    #[test]
    fn navigation_aid_points_toward_the_anchor() {
        let f = fx();
        let id = f.record(&[1, 2, 3, 4]);
        let unnamed = f.record(&[20, 21, 22]);
        f.run(|| {
            annotate_constellation(
                &f.env,
                &f.recorder,
                id,
                String::from_str(&f.env, "Wayfinder"),
                10,
                String::from_str(&f.env, ""),
            )
            .unwrap()
        });
        f.run(|| {
            let observed = f.stars(&[1, 2, 3]);
            let aid = navigation_aid(&f.env, &observed, 4).unwrap();
            assert_eq!(
                (aid.heading, aid.distance, aid.anchor_region),
                (Heading::Ascending, 6, 10)
            );
            assert_eq!(aid.name, String::from_str(&f.env, "Wayfinder"));
            assert_eq!(
                navigation_aid(&f.env, &observed, 10).unwrap().heading,
                Heading::Arrived
            );
            let back = navigation_aid(&f.env, &observed, 12).unwrap();
            assert_eq!((back.heading, back.distance), (Heading::Descending, 2));
            let unknown = navigation_aid(&f.env, &f.stars(&[20, 21, 22]), 1).unwrap();
            assert_eq!(unknown.recognition.constellation_id, unnamed);
            assert_eq!(unknown.heading, Heading::Unknown);
        });
    }

    #[test]
    fn sightings_pay_discovery_bonuses_once() {
        let f = fx();
        let id = f.record(&[1, 2, 3]);
        let first = Address::generate(&f.env);
        let second = Address::generate(&f.env);

        let own = f.sight(&f.recorder, &[1, 2, 3], CLEAR).unwrap();
        assert!(own.new_sighting && !own.first_sighter);
        assert_eq!(own.bonus_awarded, 0);

        let a = f.sight(&first, &[3, 2, 1], CLEAR).unwrap();
        assert!(a.new_sighting && a.first_sighter);
        assert_eq!(a.bonus_awarded, SIGHTING_BONUS * FIRST_SIGHTING_MULTIPLIER);
        assert_eq!(f.dust(&first), SIGHTING_BONUS * FIRST_SIGHTING_MULTIPLIER);
        assert_eq!(f.run(|| get_first_sighter(&f.env, id)), Some(first.clone()));

        let b = f.sight(&second, &[1, 2, 3], CLEAR).unwrap();
        assert!(b.new_sighting && !b.first_sighter);
        assert_eq!(f.dust(&second), SIGHTING_BONUS);

        let again = f.sight(&first, &[1, 2, 3], CLEAR).unwrap();
        assert!(!again.new_sighting);
        assert_eq!(again.bonus_awarded, 0);
        assert_eq!(f.dust(&first), SIGHTING_BONUS * FIRST_SIGHTING_MULTIPLIER);
    }

    #[test]
    fn collection_tracks_progress_and_pays_milestones() {
        let f = fx();
        let player = Address::generate(&f.env);
        let patterns: [[u8; 3]; 6] = [
            [1, 2, 3],
            [4, 5, 6],
            [7, 8, 9],
            [10, 11, 12],
            [13, 14, 15],
            [16, 17, 18],
        ];
        for p in patterns {
            f.record(&p);
        }
        let mut total_bonus = 0;
        for (n, p) in patterns.iter().take(5).enumerate() {
            let r = f.sight(&player, p, CLEAR).unwrap();
            let expected = SIGHTING_BONUS * FIRST_SIGHTING_MULTIPLIER
                + if n == 4 {
                    COLLECTION_MILESTONES[0].1
                } else {
                    0
                };
            assert_eq!(r.bonus_awarded, expected, "sighting {n}");
            assert_eq!(r.collection_size, n as u32 + 1);
            total_bonus += expected;
        }
        assert_eq!(f.dust(&player), total_bonus);
        let progress = f.run(|| get_collection(&f.env, &player));
        assert_eq!(progress.sighted_count, 5);
        assert_eq!(progress.total_constellations, 6);
        assert_eq!(progress.completion_pct, 83);
        assert_eq!(progress.next_milestone, COLLECTION_MILESTONES[1].0);
    }

    #[test]
    fn recorders_cannot_farm_milestones_with_their_own_patterns() {
        let f = fx();
        for base in [1u8, 4, 7, 10, 13] {
            f.record(&[base, base + 1, base + 2]);
            let r = f
                .sight(&f.recorder, &[base, base + 1, base + 2], CLEAR)
                .unwrap();
            assert_eq!(r.bonus_awarded, 0);
        }
        assert_eq!(f.dust(&f.recorder), 0);
        assert_eq!(
            f.run(|| get_collection(&f.env, &f.recorder)).next_milestone,
            COLLECTION_MILESTONES[0].0
        );
    }

    #[test]
    fn storms_can_hide_the_sky() {
        let f = fx();
        f.record(&[1, 2, 3]);
        let player = Address::generate(&f.env);
        // Find an ion-storm slot strong enough to drop visibility below the bar.
        let mut obscured = false;
        for _ in 0..200 {
            if f.run(|| weather_system::nebula_visibility(&f.env, STORMY)) < MIN_SIGHTING_VISIBILITY
            {
                obscured = true;
                break;
            }
            let next = f.env.ledger().timestamp() + weather_system::WEATHER_SLOT_SECS;
            f.env.ledger().set_timestamp(next);
        }
        assert!(obscured, "a strong ion storm should occur within 200 slots");
        assert_eq!(
            f.sight(&player, &[1, 2, 3], STORMY),
            Err(ConstellationError::SkyObscured)
        );
        // Clear skies always allow a sighting.
        assert!(f.sight(&player, &[1, 2, 3], CLEAR).unwrap().new_sighting);
    }
}
