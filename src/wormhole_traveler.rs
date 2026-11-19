//! Inter-nebula wormhole travel (#77, completed in #631).
//!
//! Wormholes give fast travel between nebulae. Each one has a **stability**
//! (0-100) that falls with every jump and with time; at 0 it collapses. Low
//! stability makes a jump cheaper but riskier: every jump rolls against a risk
//! derived from stability and the origin's weather. A failed roll means
//! turbulence (extra energy); a bad failure, or the collapse of the wormhole
//! mid-jump, triggers the **emergency exit** that throws the ship back to the
//! origin for half the cost. Explorers **discover** wormholes by scanning a
//! nebula (weather visibility hides some signatures) and keep a personal
//! **map** of what they found. See `docs/WORMHOLE_TRAVEL.md`.

use crate::energy_manager::{consume_energy, get_energy_balance};
use crate::ship_nft::{DataKey as ShipDataKey, ShipNft};
use crate::weather_system::{self, WeatherType};
use soroban_sdk::{contracterror, contracttype, symbol_short, Address, Bytes, BytesN, Env, Vec};

// Travel cost multipliers for balanced progression
pub const BASE_TRAVEL_COST: u32 = 50;
pub const DISTANCE_MULTIPLIER: u32 = 2;
pub const MAX_SIMULTANEOUS_WORMHOLES: u32 = 5;
pub const WORMHOLE_LIFETIME_SECS: u64 = 3600; // 1 hour

/// Stability of a freshly opened wormhole in calm weather.
pub const MAX_STABILITY: u32 = 100;
/// Stability lost per jump.
pub const STABILITY_LOSS_PER_JUMP: u32 = 10;
/// Stability lost per started 10 minutes since opening.
pub const STABILITY_LOSS_PER_10_MIN: u32 = 2;
/// Risk never exceeds this, so a jump is never a certain failure.
pub const MAX_JUMP_RISK: u32 = 70;
/// Extra energy paid on a turbulent jump, in percent of the cost.
pub const TURBULENCE_SURCHARGE_PCT: u32 = 25;
/// Share of the cost paid when the emergency exit fires, in percent.
pub const EMERGENCY_EXIT_COST_PCT: u32 = 50;
/// How long after arriving a ship may bail out back through the wormhole.
pub const EMERGENCY_EXIT_WINDOW_SECS: u64 = 300;
/// Wormholes remembered per explorer map.
pub const MAX_MAP_ENTRIES: u32 = 100;

#[derive(Clone)]
#[contracttype]
pub enum WormholeKey {
    Wormhole(u64),      // wormhole_id -> Wormhole
    ActiveWormholes,    // -> Vec<u64> (active wormhole IDs)
    WormholeCount,      // -> u64 (next wormhole ID)
    TravelHistory(u64), // ship_id -> Vec<TravelRecord>
    Stability(u64),     // wormhole_id -> WormholeStatus
    Map(Address),       // explorer -> Vec<u64> discovered wormhole IDs
    Discoverer(u64),    // wormhole_id -> Address (first explorer to find it)
    Location(u64),      // ship_id -> nebula the ship is in
    LastJump(u64),      // ship_id -> TravelOutcome
}

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum WormholeError {
    InvalidDestination = 1,
    InsufficientEnergy = 2,
    ShipNotFound = 3,
    WormholeNotFound = 4,
    WormholeExpired = 5,
    MaxWormholesReached = 6,
    UnauthorizedTravel = 7,
    SameNebulaTravel = 8,
    WormholeClosed = 9,
    EnergyManagerError = 10,
    /// The ship is not in the wormhole's origin nebula.
    ShipNotAtOrigin = 11,
    /// The ship has no jump it could bail out of.
    NoRecentJump = 12,
    /// The emergency exit window after arrival has passed.
    ExitWindowClosed = 13,
}

impl crate::error_standard::StandardContractError for WormholeError {
    fn descriptor(self) -> crate::error_standard::ErrorDescriptor {
        use crate::error_standard::ErrorKind;
        let (kind, retryable) = match self {
            Self::InvalidDestination | Self::SameNebulaTravel | Self::ShipNotAtOrigin => {
                (ErrorKind::Validation, false)
            }
            Self::NoRecentJump => (ErrorKind::NotFound, false),
            Self::ExitWindowClosed => (ErrorKind::Conflict, false),
            Self::InsufficientEnergy | Self::MaxWormholesReached => {
                (ErrorKind::ResourceLimit, false)
            }
            Self::ShipNotFound | Self::WormholeNotFound => (ErrorKind::NotFound, false),
            Self::WormholeExpired | Self::WormholeClosed => (ErrorKind::Conflict, false),
            Self::UnauthorizedTravel => (ErrorKind::Authorization, false),
            Self::EnergyManagerError => (ErrorKind::Internal, false),
        };
        crate::error_standard::ErrorDescriptor {
            module: "wormhole_traveler",
            code: self as u32,
            kind,
            retryable,
        }
    }
}

#[derive(Clone, Debug)]
#[contracttype]
pub struct Wormhole {
    pub wormhole_id: u64,
    pub origin_nebula: u64,
    pub destination: u64,
    pub creator: Address,
    pub created_at: u64,
    pub expires_at: u64,
    pub is_active: bool,
    pub travel_cost: u32,
    pub verifiable_link: BytesN<32>,
}

/// Mutable health of a wormhole, kept apart from [`Wormhole`] so records
/// stored before #631 still decode.
#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct WormholeStatus {
    /// Stability when opened (before time decay).
    pub base_stability: u32,
    pub traversals: u32,
    pub collapsed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracttype]
pub enum JumpOutcome {
    /// Arrived at the destination at normal cost.
    Clean,
    /// Arrived, but paid a turbulence surcharge.
    Turbulent,
    /// Thrown back to the origin by the emergency exit for half the cost.
    EmergencyExit,
}

/// Full result of a jump; `record` is what the travel history keeps.
#[derive(Clone, Debug)]
#[contracttype]
pub struct TravelOutcome {
    pub record: TravelRecord,
    pub outcome: JumpOutcome,
    /// Risk of the jump in percent.
    pub risk: u32,
    /// The roll (0-99) that decided it; below `risk` is a failure.
    pub roll: u32,
    pub stability_after: u32,
    pub collapsed: bool,
    /// Nebula the ship ended in.
    pub location: u64,
}

/// One entry of an explorer's wormhole map.
#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct WormholeMapEntry {
    pub wormhole_id: u64,
    pub origin_nebula: u64,
    pub destination: u64,
    pub stability: u32,
    pub risk: u32,
    pub travel_cost: u32,
    pub is_active: bool,
    pub expires_at: u64,
}

#[derive(Clone, Debug)]
#[contracttype]
pub struct TravelRecord {
    pub ship_id: u64,
    pub wormhole_id: u64,
    pub origin_nebula: u64,
    pub destination: u64,
    pub traveled_at: u64,
    pub energy_consumed: u32,
}

/// Open a new wormhole between two nebulae with verifiable travel link
pub fn open_wormhole(
    env: &Env,
    creator: Address,
    origin_nebula: u64,
    destination: u64,
) -> Result<u64, WormholeError> {
    creator.require_auth();

    // Validate destinations
    if origin_nebula == destination {
        return Err(WormholeError::SameNebulaTravel);
    }

    // Check maximum simultaneous wormholes
    let active_wormholes = env
        .storage()
        .persistent()
        .get::<WormholeKey, Vec<u64>>(&WormholeKey::ActiveWormholes)
        .unwrap_or(Vec::new(env));

    if active_wormholes.len() >= MAX_SIMULTANEOUS_WORMHOLES.try_into().unwrap() {
        return Err(WormholeError::MaxWormholesReached);
    }

    // Generate wormhole ID
    let wormhole_id = env
        .storage()
        .persistent()
        .get::<WormholeKey, u64>(&WormholeKey::WormholeCount)
        .unwrap_or(0);

    // Calculate travel cost based on distance
    let distance = if destination > origin_nebula {
        destination - origin_nebula
    } else {
        origin_nebula - destination
    };
    let travel_cost = BASE_TRAVEL_COST + ((distance as u32) * DISTANCE_MULTIPLIER);

    // Generate verifiable travel link using ledger seed
    let mut link_data = [0u8; 32];
    let ledger_seq = env.ledger().sequence();
    let timestamp = env.ledger().timestamp();

    // Create deterministic hash from origin, destination, and ledger data
    // Use simple XOR-based hash for verifiability
    for (i, byte) in origin_nebula.to_be_bytes().iter().enumerate() {
        link_data[i % 32] ^= byte;
    }
    for (i, byte) in destination.to_be_bytes().iter().enumerate() {
        link_data[(i + 8) % 32] ^= byte;
    }
    for (i, byte) in ledger_seq.to_be_bytes().iter().enumerate() {
        link_data[(i + 16) % 32] ^= byte;
    }
    for (i, byte) in timestamp.to_be_bytes().iter().enumerate() {
        link_data[(i + 24) % 32] ^= byte;
    }

    let verifiable_link = BytesN::from_array(env, &link_data);

    // Wormhole-instability weather at the origin tears new wormholes open weaker.
    let origin_weather = weather_system::get_current_weather(env, origin_nebula);
    let base_stability = if origin_weather.weather == WeatherType::WormholeInstability {
        MAX_STABILITY - origin_weather.intensity / 2
    } else {
        MAX_STABILITY
    };
    env.storage().persistent().set(
        &WormholeKey::Stability(wormhole_id),
        &WormholeStatus {
            base_stability,
            traversals: 0,
            collapsed: false,
        },
    );

    let current_time = env.ledger().timestamp();
    let wormhole = Wormhole {
        wormhole_id,
        origin_nebula,
        destination,
        creator: creator.clone(),
        created_at: current_time,
        expires_at: current_time + WORMHOLE_LIFETIME_SECS,
        is_active: true,
        travel_cost,
        verifiable_link: verifiable_link.clone(),
    };

    // Store wormhole
    env.storage()
        .persistent()
        .set(&WormholeKey::Wormhole(wormhole_id), &wormhole);

    // Update wormhole count
    env.storage()
        .persistent()
        .set(&WormholeKey::WormholeCount, &(wormhole_id + 1));

    // Add to active list
    let mut updated_active = active_wormholes;
    updated_active.push_back(wormhole_id);
    env.storage()
        .persistent()
        .set(&WormholeKey::ActiveWormholes, &updated_active);

    // Emit event
    env.events().publish(
        (symbol_short!("wormhole"), symbol_short!("opened")),
        (
            wormhole_id,
            origin_nebula,
            destination,
            creator,
            verifiable_link,
        ),
    );

    Ok(wormhole_id)
}

/// Traverse an existing wormhole with energy cost validation and state sync.
/// Returns the travel record; [`jump_through_wormhole`] returns the full outcome.
pub fn traverse_wormhole(
    env: &Env,
    traveler: Address,
    ship_id: u64,
    wormhole_id: u64,
) -> Result<TravelRecord, WormholeError> {
    jump_through_wormhole(env, traveler, ship_id, wormhole_id).map(|outcome| outcome.record)
}

/// Current stability of a wormhole (0 once collapsed or expired).
pub fn current_stability(env: &Env, wormhole: &Wormhole) -> u32 {
    let status = get_wormhole_status(env, wormhole.wormhole_id);
    if status.collapsed || !wormhole.is_active {
        return 0;
    }
    let now = env.ledger().timestamp();
    if now > wormhole.expires_at {
        return 0;
    }
    let tens_of_minutes = (now.saturating_sub(wormhole.created_at) / 600) as u32;
    status
        .base_stability
        .saturating_sub(status.traversals.saturating_mul(STABILITY_LOSS_PER_JUMP))
        .saturating_sub(tens_of_minutes.saturating_mul(STABILITY_LOSS_PER_10_MIN))
}

/// Risk of a jump in percent from stability and the origin's weather danger.
pub fn jump_risk(stability: u32, weather_danger: u32) -> u32 {
    let instability = MAX_STABILITY.saturating_sub(stability.min(MAX_STABILITY));
    (instability / 2 + weather_danger / 5).min(MAX_JUMP_RISK)
}

/// Energy a jump costs at `stability`: unstable wormholes are cheaper (the
/// reward for taking the risk), down to 60% of the base cost at stability 0.
pub fn stability_adjusted_cost(base_cost: u32, stability: u32) -> u32 {
    let stability = stability.min(MAX_STABILITY);
    let pct = 60 + 40 * stability / MAX_STABILITY;
    (base_cost.saturating_mul(pct) / 100).max(1)
}

/// Decide a jump: `roll` below `risk / 3` (or a collapse) fires the emergency
/// exit, below `risk` is turbulence, anything else is clean.
pub fn decide_outcome(risk: u32, roll: u32, collapses: bool) -> JumpOutcome {
    if collapses || roll < risk / 3 {
        JumpOutcome::EmergencyExit
    } else if roll < risk {
        JumpOutcome::Turbulent
    } else {
        JumpOutcome::Clean
    }
}

fn jump_roll(env: &Env, ship_id: u64, wormhole_id: u64, traversals: u32) -> u32 {
    let mut input = Bytes::from_slice(env, b"wormhole-jump");
    input.extend_from_array(&ship_id.to_be_bytes());
    input.extend_from_array(&wormhole_id.to_be_bytes());
    input.extend_from_array(&traversals.to_be_bytes());
    input.extend_from_array(&env.ledger().sequence().to_be_bytes());
    input.extend_from_array(&env.ledger().timestamp().to_be_bytes());
    let digest = env.crypto().sha256(&input).to_array();
    u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]) % 100
}

/// Energy a ship would pay to jump through `wormhole` now (before turbulence).
pub fn current_travel_cost(env: &Env, wormhole: &Wormhole) -> u32 {
    let stability = current_stability(env, wormhole);
    let adjusted = stability_adjusted_cost(wormhole.travel_cost, stability);
    let weathered =
        weather_system::effective_fuel_cost(env, wormhole.origin_nebula, u64::from(adjusted));
    (weathered.min(u64::from(u32::MAX)) as u32).max(1)
}

fn verify_ship_owner(env: &Env, traveler: &Address, ship_id: u64) -> Result<(), WormholeError> {
    let ship = env
        .storage()
        .persistent()
        .get::<ShipDataKey, ShipNft>(&ShipDataKey::Ship(ship_id))
        .ok_or(WormholeError::ShipNotFound)?;
    if ship.owner != *traveler {
        return Err(WormholeError::UnauthorizedTravel);
    }
    Ok(())
}

fn push_history(env: &Env, record: &TravelRecord) {
    let key = WormholeKey::TravelHistory(record.ship_id);
    let mut travel_history = env
        .storage()
        .persistent()
        .get::<WormholeKey, Vec<TravelRecord>>(&key)
        .unwrap_or(Vec::new(env));
    travel_history.push_back(record.clone());
    // Keep only last 100 travel records per ship
    if travel_history.len() > 100 {
        travel_history.pop_front();
    }
    env.storage().persistent().set(&key, &travel_history);
}

fn collapse(env: &Env, wormhole: &Wormhole) {
    let mut closed = wormhole.clone();
    closed.is_active = false;
    env.storage()
        .persistent()
        .set(&WormholeKey::Wormhole(wormhole.wormhole_id), &closed);
    let active = get_active_wormholes(env);
    let mut remaining = Vec::new(env);
    for id in active.iter() {
        if id != wormhole.wormhole_id {
            remaining.push_back(id);
        }
    }
    env.storage()
        .persistent()
        .set(&WormholeKey::ActiveWormholes, &remaining);
    env.events().publish(
        (symbol_short!("wormhole"), symbol_short!("collapse")),
        wormhole.wormhole_id,
    );
}

/// Jump through a wormhole, applying stability, risk, weather and the
/// emergency exit. The ship must be owned by `traveler` and, if its location
/// is known, be in the wormhole's origin nebula.
pub fn jump_through_wormhole(
    env: &Env,
    traveler: Address,
    ship_id: u64,
    wormhole_id: u64,
) -> Result<TravelOutcome, WormholeError> {
    traveler.require_auth();

    let wormhole = env
        .storage()
        .persistent()
        .get::<WormholeKey, Wormhole>(&WormholeKey::Wormhole(wormhole_id))
        .ok_or(WormholeError::WormholeNotFound)?;

    let mut status = get_wormhole_status(env, wormhole_id);
    if !wormhole.is_active || status.collapsed {
        return Err(WormholeError::WormholeClosed);
    }

    let current_time = env.ledger().timestamp();
    if current_time > wormhole.expires_at {
        return Err(WormholeError::WormholeExpired);
    }

    verify_ship_owner(env, &traveler, ship_id)?;

    if let Some(location) = get_ship_location(env, ship_id) {
        if location != wormhole.origin_nebula {
            return Err(WormholeError::ShipNotAtOrigin);
        }
    }

    let stability = current_stability(env, &wormhole);
    let cost = current_travel_cost(env, &wormhole);
    let danger = weather_system::get_weather_impact(env, wormhole.origin_nebula).danger_level;
    let risk = jump_risk(stability, danger);
    let roll = jump_roll(env, ship_id, wormhole_id, status.traversals);
    let stability_after = stability.saturating_sub(STABILITY_LOSS_PER_JUMP);
    let collapses = stability_after == 0;
    let outcome = decide_outcome(risk, roll, collapses);

    let energy_balance =
        get_energy_balance(env, ship_id).map_err(|_| WormholeError::EnergyManagerError)?;
    if energy_balance.current < cost {
        return Err(WormholeError::InsufficientEnergy);
    }
    let energy = match outcome {
        JumpOutcome::Clean => cost,
        JumpOutcome::Turbulent => cost
            .saturating_add(cost * TURBULENCE_SURCHARGE_PCT / 100)
            .min(energy_balance.current),
        JumpOutcome::EmergencyExit => (cost * EMERGENCY_EXIT_COST_PCT / 100).max(1),
    };
    consume_energy(env, ship_id, energy).map_err(|_| WormholeError::EnergyManagerError)?;

    status.traversals = status.traversals.saturating_add(1);
    status.collapsed = collapses;
    env.storage()
        .persistent()
        .set(&WormholeKey::Stability(wormhole_id), &status);
    if collapses {
        collapse(env, &wormhole);
    }

    let location = if outcome == JumpOutcome::EmergencyExit {
        wormhole.origin_nebula
    } else {
        wormhole.destination
    };
    env.storage()
        .persistent()
        .set(&WormholeKey::Location(ship_id), &location);

    let record = TravelRecord {
        ship_id,
        wormhole_id,
        origin_nebula: wormhole.origin_nebula,
        destination: wormhole.destination,
        traveled_at: current_time,
        energy_consumed: energy,
    };
    push_history(env, &record);

    let result = TravelOutcome {
        record,
        outcome,
        risk,
        roll,
        stability_after,
        collapsed: collapses,
        location,
    };
    env.storage()
        .persistent()
        .set(&WormholeKey::LastJump(ship_id), &result);

    // Emit travel completion event
    env.events().publish(
        (symbol_short!("wormhole"), symbol_short!("traversed")),
        (
            ship_id,
            wormhole_id,
            wormhole.origin_nebula,
            location,
            traveler,
        ),
    );
    if outcome == JumpOutcome::EmergencyExit {
        env.events().publish(
            (symbol_short!("wormhole"), symbol_short!("ejected")),
            (ship_id, wormhole_id, energy),
        );
    }

    Ok(result)
}

/// Bail out back through the wormhole a ship just came through, within
/// [`EMERGENCY_EXIT_WINDOW_SECS`] of arriving and while the wormhole is open.
/// Costs [`EMERGENCY_EXIT_COST_PCT`] of the current travel cost.
pub fn emergency_exit(
    env: &Env,
    traveler: Address,
    ship_id: u64,
) -> Result<TravelOutcome, WormholeError> {
    traveler.require_auth();
    verify_ship_owner(env, &traveler, ship_id)?;

    let last: TravelOutcome = env
        .storage()
        .persistent()
        .get(&WormholeKey::LastJump(ship_id))
        .ok_or(WormholeError::NoRecentJump)?;
    if last.outcome == JumpOutcome::EmergencyExit || last.location != last.record.destination {
        return Err(WormholeError::NoRecentJump);
    }
    let now = env.ledger().timestamp();
    if now
        > last
            .record
            .traveled_at
            .saturating_add(EMERGENCY_EXIT_WINDOW_SECS)
    {
        return Err(WormholeError::ExitWindowClosed);
    }
    let wormhole =
        get_wormhole(env, last.record.wormhole_id).ok_or(WormholeError::WormholeNotFound)?;
    let status = get_wormhole_status(env, wormhole.wormhole_id);
    if !wormhole.is_active || status.collapsed {
        return Err(WormholeError::WormholeClosed);
    }
    if now > wormhole.expires_at {
        return Err(WormholeError::WormholeExpired);
    }

    let energy = (current_travel_cost(env, &wormhole) * EMERGENCY_EXIT_COST_PCT / 100).max(1);
    consume_energy(env, ship_id, energy).map_err(|e| match e {
        crate::energy_manager::EnergyError::InsufficientEnergy => WormholeError::InsufficientEnergy,
        _ => WormholeError::EnergyManagerError,
    })?;

    let location = wormhole.origin_nebula;
    env.storage()
        .persistent()
        .set(&WormholeKey::Location(ship_id), &location);

    let record = TravelRecord {
        ship_id,
        wormhole_id: wormhole.wormhole_id,
        origin_nebula: wormhole.destination,
        destination: wormhole.origin_nebula,
        traveled_at: now,
        energy_consumed: energy,
    };
    push_history(env, &record);

    let result = TravelOutcome {
        record,
        outcome: JumpOutcome::EmergencyExit,
        risk: 0,
        roll: 0,
        stability_after: current_stability(env, &wormhole),
        collapsed: false,
        location,
    };
    env.storage()
        .persistent()
        .set(&WormholeKey::LastJump(ship_id), &result);
    env.events().publish(
        (symbol_short!("wormhole"), symbol_short!("ejected")),
        (ship_id, wormhole.wormhole_id, energy),
    );
    Ok(result)
}

// ─── Discovery & mapping ──────────────────────────────────────────────────────

/// Detection signature (0-99) of a wormhole: scans reveal it only when the
/// nebula's visibility is above it, so storms hide faint wormholes.
pub fn detection_signature(env: &Env, wormhole_id: u64) -> u32 {
    let mut input = Bytes::from_slice(env, b"wormhole-signature");
    input.extend_from_array(&wormhole_id.to_be_bytes());
    let digest = env.crypto().sha256(&input).to_array();
    // Signatures fall in 0-89 so that clear skies (100% visibility) reveal all.
    u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]]) % 90
}

/// Scan a nebula for active wormholes touching it. Every wormhole the scan
/// detects is added to the explorer's map; the first explorer to detect a
/// wormhole (other than its creator) is recorded as its discoverer.
pub fn scan_for_wormholes(env: &Env, explorer: Address, nebula_id: u64) -> Vec<u64> {
    explorer.require_auth();
    let visibility = weather_system::nebula_visibility(env, nebula_id);
    let mut found = Vec::new(env);
    let mut map = get_discovered_wormholes(env, &explorer);

    for id in get_active_wormholes(env).iter() {
        let Some(wormhole) = get_wormhole(env, id) else {
            continue;
        };
        if wormhole.origin_nebula != nebula_id && wormhole.destination != nebula_id {
            continue;
        }
        if current_stability(env, &wormhole) == 0 || detection_signature(env, id) >= visibility {
            continue;
        }
        found.push_back(id);
        if !map.contains(id) {
            map.push_back(id);
            if map.len() > MAX_MAP_ENTRIES {
                map.pop_front();
            }
        }
        let discoverer_key = WormholeKey::Discoverer(id);
        if wormhole.creator != explorer && !env.storage().persistent().has(&discoverer_key) {
            env.storage().persistent().set(&discoverer_key, &explorer);
            env.events().publish(
                (symbol_short!("wormhole"), symbol_short!("found")),
                (id, explorer.clone(), nebula_id),
            );
        }
    }

    env.storage()
        .persistent()
        .set(&WormholeKey::Map(explorer.clone()), &map);
    found
}

/// Wormhole IDs in an explorer's map (oldest first).
pub fn get_discovered_wormholes(env: &Env, explorer: &Address) -> Vec<u64> {
    env.storage()
        .persistent()
        .get(&WormholeKey::Map(explorer.clone()))
        .unwrap_or(Vec::new(env))
}

/// First explorer (other than the creator) to detect a wormhole.
pub fn get_discoverer(env: &Env, wormhole_id: u64) -> Option<Address> {
    env.storage()
        .persistent()
        .get(&WormholeKey::Discoverer(wormhole_id))
}

fn map_entry(env: &Env, wormhole: &Wormhole) -> WormholeMapEntry {
    let stability = current_stability(env, wormhole);
    let danger = weather_system::get_weather_impact(env, wormhole.origin_nebula).danger_level;
    WormholeMapEntry {
        wormhole_id: wormhole.wormhole_id,
        origin_nebula: wormhole.origin_nebula,
        destination: wormhole.destination,
        stability,
        risk: jump_risk(stability, danger),
        travel_cost: current_travel_cost(env, wormhole),
        is_active: stability > 0,
        expires_at: wormhole.expires_at,
    }
}

/// An explorer's map with each wormhole's live stability, risk and cost.
pub fn get_wormhole_map(env: &Env, explorer: &Address) -> Vec<WormholeMapEntry> {
    let mut entries = Vec::new(env);
    for id in get_discovered_wormholes(env, explorer).iter() {
        if let Some(wormhole) = get_wormhole(env, id) {
            entries.push_back(map_entry(env, &wormhole));
        }
    }
    entries
}

/// Open routes leaving a nebula, with live stability, risk and cost.
pub fn get_routes_from(env: &Env, nebula_id: u64) -> Vec<WormholeMapEntry> {
    let mut routes = Vec::new(env);
    for id in get_active_wormholes(env).iter() {
        if let Some(wormhole) = get_wormhole(env, id) {
            if wormhole.origin_nebula == nebula_id && current_stability(env, &wormhole) > 0 {
                routes.push_back(map_entry(env, &wormhole));
            }
        }
    }
    routes
}

/// Stability status of a wormhole (full stability if it predates #631).
pub fn get_wormhole_status(env: &Env, wormhole_id: u64) -> WormholeStatus {
    env.storage()
        .persistent()
        .get(&WormholeKey::Stability(wormhole_id))
        .unwrap_or(WormholeStatus {
            base_stability: MAX_STABILITY,
            traversals: 0,
            collapsed: false,
        })
}

/// Nebula a ship is in, if it has travelled since #631.
pub fn get_ship_location(env: &Env, ship_id: u64) -> Option<u64> {
    env.storage()
        .persistent()
        .get(&WormholeKey::Location(ship_id))
}

/// The most recent jump (or emergency exit) of a ship.
pub fn get_last_jump(env: &Env, ship_id: u64) -> Option<TravelOutcome> {
    env.storage()
        .persistent()
        .get(&WormholeKey::LastJump(ship_id))
}

/// Get wormhole details by ID
pub fn get_wormhole(env: &Env, wormhole_id: u64) -> Option<Wormhole> {
    env.storage()
        .persistent()
        .get::<WormholeKey, Wormhole>(&WormholeKey::Wormhole(wormhole_id))
}

/// Get all active wormholes
pub fn get_active_wormholes(env: &Env) -> Vec<u64> {
    env.storage()
        .persistent()
        .get::<WormholeKey, Vec<u64>>(&WormholeKey::ActiveWormholes)
        .unwrap_or(Vec::new(env))
}

/// Get travel history for a ship
pub fn get_travel_history(env: &Env, ship_id: u64) -> Vec<TravelRecord> {
    env.storage()
        .persistent()
        .get::<WormholeKey, Vec<TravelRecord>>(&WormholeKey::TravelHistory(ship_id))
        .unwrap_or(Vec::new(env))
}

/// Close expired wormholes (maintenance function)
pub fn cleanup_expired_wormholes(env: &Env) -> u32 {
    let mut cleaned_count = 0;
    let current_time = env.ledger().timestamp();

    let active_wormholes = env
        .storage()
        .persistent()
        .get::<WormholeKey, Vec<u64>>(&WormholeKey::ActiveWormholes)
        .unwrap_or(Vec::new(env));

    let mut updated_active = Vec::new(env);

    for wormhole_id in active_wormholes.iter() {
        if let Some(wormhole) = get_wormhole(env, wormhole_id) {
            if current_time <= wormhole.expires_at && wormhole.is_active {
                updated_active.push_back(wormhole_id);
            } else {
                // Mark as inactive
                let mut updated_wormhole = wormhole;
                updated_wormhole.is_active = false;
                env.storage()
                    .persistent()
                    .set(&WormholeKey::Wormhole(wormhole_id), &updated_wormhole);
                cleaned_count += 1;
            }
        }
    }

    env.storage()
        .persistent()
        .set(&WormholeKey::ActiveWormholes, &updated_active);

    cleaned_count
}

/// Calculate travel cost between two nebulae
pub fn calculate_travel_cost(origin_nebula: u64, destination: u64) -> u32 {
    if origin_nebula == destination {
        return 0;
    }

    let distance = if destination > origin_nebula {
        destination - origin_nebula
    } else {
        origin_nebula - destination
    };

    BASE_TRAVEL_COST + ((distance as u32) * DISTANCE_MULTIPLIER)
}

/// Verify wormhole link integrity
pub fn verify_wormhole_link(env: &Env, wormhole_id: u64, provided_link: BytesN<32>) -> bool {
    if let Some(wormhole) = get_wormhole(env, wormhole_id) {
        wormhole.verifiable_link == provided_link
    } else {
        false
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emergency_controls;
    use crate::energy_manager;
    use crate::ship_nft;
    use soroban_sdk::testutils::{Address as _, Ledger};
    use soroban_sdk::{contract, contractimpl, Symbol};

    #[contract]
    struct Stub;
    #[contractimpl]
    impl Stub {}

    struct Fx {
        env: Env,
        contract: Address,
        admin: Address,
        pilot: Address,
    }

    impl Fx {
        fn run<T>(&self, f: impl FnOnce() -> T) -> T {
            self.env.as_contract(&self.contract, f)
        }

        fn ship(&self, owner: &Address) -> u64 {
            self.run(|| {
                ship_nft::mint_ship(
                    &self.env,
                    owner,
                    &Symbol::new(&self.env, "explorer"),
                    &Bytes::new(&self.env),
                )
                .unwrap()
                .id
            })
        }

        fn weather(&self, nebula: u64, weather: WeatherType) {
            self.run(|| {
                let mut weights = Vec::new(&self.env);
                for i in 0..weather_system::WEATHER_TYPE_COUNT {
                    weights.push_back(u32::from(i == weather.index()));
                }
                weather_system::set_region_climate(&self.env, &self.admin, nebula, weights)
                    .unwrap();
            });
        }

        fn open(&self, origin: u64, destination: u64) -> u64 {
            self.run(|| open_wormhole(&self.env, self.admin.clone(), origin, destination).unwrap())
        }

        fn energy(&self, ship: u64) -> u32 {
            self.run(|| {
                energy_manager::get_energy_balance(&self.env, ship)
                    .unwrap()
                    .current
            })
        }
    }

    fn fx() -> Fx {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(1_000_000);
        let contract = env.register(Stub, ());
        let admin = Address::generate(&env);
        let pilot = Address::generate(&env);
        let a = admin.clone();
        env.as_contract(&contract, || {
            emergency_controls::initialize_admins(&env, Vec::from_array(&env, [a])).unwrap();
        });
        Fx {
            env,
            contract,
            admin,
            pilot,
        }
    }

    #[test]
    fn risk_cost_and_outcome_rules() {
        assert_eq!(jump_risk(100, 0), 0);
        assert_eq!(jump_risk(60, 0), 20);
        assert_eq!(jump_risk(60, 95), 39);
        assert_eq!(jump_risk(0, 100), MAX_JUMP_RISK);
        assert_eq!(stability_adjusted_cost(100, 100), 100);
        assert_eq!(stability_adjusted_cost(100, 50), 80);
        assert_eq!(stability_adjusted_cost(100, 0), 60);
        assert_eq!(decide_outcome(0, 0, false), JumpOutcome::Clean);
        assert_eq!(decide_outcome(30, 5, false), JumpOutcome::EmergencyExit);
        assert_eq!(decide_outcome(30, 20, false), JumpOutcome::Turbulent);
        assert_eq!(decide_outcome(30, 30, false), JumpOutcome::Clean);
        assert_eq!(decide_outcome(0, 99, true), JumpOutcome::EmergencyExit);
    }

    #[test]
    fn clean_jump_moves_the_ship_and_charges_the_cost() {
        let f = fx();
        f.weather(1, WeatherType::PerfectConditions);
        let ship = f.ship(&f.pilot);
        let id = f.open(1, 6);
        let before = f.energy(ship);
        let expected = f.run(|| current_travel_cost(&f.env, &get_wormhole(&f.env, id).unwrap()));
        assert!(
            expected < calculate_travel_cost(1, 6),
            "perfect conditions save fuel"
        );

        let outcome = f.run(|| jump_through_wormhole(&f.env, f.pilot.clone(), ship, id).unwrap());
        assert_eq!(outcome.outcome, JumpOutcome::Clean);
        assert_eq!(outcome.risk, 0);
        assert_eq!(outcome.location, 6);
        assert_eq!(outcome.record.energy_consumed, expected);
        assert_eq!(f.energy(ship), before - expected);
        assert_eq!(f.run(|| get_ship_location(&f.env, ship)), Some(6));
        assert_eq!(f.run(|| get_travel_history(&f.env, ship)).len(), 1);
        assert_eq!(
            outcome.stability_after,
            MAX_STABILITY - STABILITY_LOSS_PER_JUMP
        );
    }

    #[test]
    fn ship_must_be_at_the_origin_and_owned() {
        let f = fx();
        f.weather(1, WeatherType::PerfectConditions);
        let ship = f.ship(&f.pilot);
        let id = f.open(1, 2);
        f.run(|| {
            assert_eq!(
                jump_through_wormhole(&f.env, Address::generate(&f.env), ship, id).unwrap_err(),
                WormholeError::UnauthorizedTravel
            );
        });
        f.run(|| jump_through_wormhole(&f.env, f.pilot.clone(), ship, id).unwrap());
        f.run(|| {
            assert_eq!(
                jump_through_wormhole(&f.env, f.pilot.clone(), ship, id).unwrap_err(),
                WormholeError::ShipNotAtOrigin
            );
        });
    }

    #[test]
    fn stability_decays_with_jumps_and_time_until_collapse() {
        let f = fx();
        f.weather(1, WeatherType::PerfectConditions);
        let id = f.open(1, 2);
        let jumps = MAX_STABILITY / STABILITY_LOSS_PER_JUMP;
        for n in 1..=jumps {
            let ship = f.ship(&f.pilot);
            let outcome =
                f.run(|| jump_through_wormhole(&f.env, f.pilot.clone(), ship, id).unwrap());
            assert_eq!(
                outcome.stability_after,
                MAX_STABILITY - n * STABILITY_LOSS_PER_JUMP
            );
            assert_eq!(outcome.collapsed, n == jumps);
            if n == jumps {
                assert_eq!(outcome.outcome, JumpOutcome::EmergencyExit);
                assert_eq!(outcome.location, 1);
            }
        }
        let ship = f.ship(&f.pilot);
        f.run(|| {
            assert_eq!(
                jump_through_wormhole(&f.env, f.pilot.clone(), ship, id).unwrap_err(),
                WormholeError::WormholeClosed
            );
            assert!(!get_active_wormholes(&f.env).contains(id));
        });

        let fresh = f.open(1, 3);
        f.env
            .ledger()
            .set_timestamp(f.env.ledger().timestamp() + 30 * 60);
        let stability = f.run(|| current_stability(&f.env, &get_wormhole(&f.env, fresh).unwrap()));
        assert_eq!(stability, MAX_STABILITY - 3 * STABILITY_LOSS_PER_10_MIN);
    }

    #[test]
    fn dangerous_weather_makes_jumps_risky_and_outcomes_follow_the_roll() {
        let f = fx();
        f.weather(1, WeatherType::BlackHoleProximity);
        let id = f.open(1, 2);
        let mut seen_risky = false;
        for _ in 0..8 {
            let ship = f.ship(&f.pilot);
            let wormhole = f.run(|| get_wormhole(&f.env, id).unwrap());
            let cost = f.run(|| current_travel_cost(&f.env, &wormhole));
            let before = f.energy(ship);
            let o = f.run(|| jump_through_wormhole(&f.env, f.pilot.clone(), ship, id).unwrap());
            assert!(o.risk > 0);
            assert_eq!(o.outcome, decide_outcome(o.risk, o.roll, o.collapsed));
            let paid = before - f.energy(ship);
            match o.outcome {
                JumpOutcome::Clean => assert_eq!(paid, cost),
                JumpOutcome::Turbulent => {
                    seen_risky = true;
                    assert_eq!(paid, cost + cost * TURBULENCE_SURCHARGE_PCT / 100);
                    assert_eq!(o.location, 2);
                }
                JumpOutcome::EmergencyExit => {
                    seen_risky = true;
                    assert_eq!(paid, cost * EMERGENCY_EXIT_COST_PCT / 100);
                    assert_eq!(o.location, 1);
                }
            }
            f.env
                .ledger()
                .set_sequence_number(f.env.ledger().sequence() + 1);
        }
        assert!(seen_risky, "eight risky jumps should not all be clean");
    }

    #[test]
    fn emergency_exit_returns_through_the_wormhole_within_the_window() {
        let f = fx();
        f.weather(1, WeatherType::PerfectConditions);
        f.weather(2, WeatherType::PerfectConditions);
        let id = f.open(1, 2);
        let ship = f.ship(&f.pilot);
        f.run(|| {
            assert_eq!(
                emergency_exit(&f.env, f.pilot.clone(), ship).unwrap_err(),
                WormholeError::NoRecentJump
            );
        });
        f.run(|| jump_through_wormhole(&f.env, f.pilot.clone(), ship, id).unwrap());
        let before = f.energy(ship);
        let cost = f.run(|| current_travel_cost(&f.env, &get_wormhole(&f.env, id).unwrap()));
        let exit = f.run(|| emergency_exit(&f.env, f.pilot.clone(), ship).unwrap());
        assert_eq!(exit.outcome, JumpOutcome::EmergencyExit);
        assert_eq!(exit.location, 1);
        assert_eq!(
            before - f.energy(ship),
            cost * EMERGENCY_EXIT_COST_PCT / 100
        );
        f.run(|| {
            assert_eq!(
                emergency_exit(&f.env, f.pilot.clone(), ship).unwrap_err(),
                WormholeError::NoRecentJump
            );
        });

        let other = f.ship(&f.pilot);
        f.run(|| jump_through_wormhole(&f.env, f.pilot.clone(), other, id).unwrap());
        f.env
            .ledger()
            .set_timestamp(f.env.ledger().timestamp() + EMERGENCY_EXIT_WINDOW_SECS + 1);
        f.run(|| {
            assert_eq!(
                emergency_exit(&f.env, f.pilot.clone(), other).unwrap_err(),
                WormholeError::ExitWindowClosed
            );
        });
    }

    #[test]
    fn scans_discover_wormholes_and_build_the_map() {
        let f = fx();
        f.weather(1, WeatherType::PerfectConditions);
        let explorer = Address::generate(&f.env);
        let a = f.open(1, 5);
        let b = f.open(7, 1);
        let elsewhere = f.open(8, 9);
        let found = f.run(|| scan_for_wormholes(&f.env, explorer.clone(), 1));
        assert_eq!(found, Vec::from_array(&f.env, [a, b]));
        assert!(!found.contains(elsewhere));
        assert_eq!(f.run(|| get_discoverer(&f.env, a)), Some(explorer.clone()));
        f.run(|| scan_for_wormholes(&f.env, f.admin.clone(), 8));
        assert_eq!(
            f.run(|| get_discoverer(&f.env, elsewhere)),
            None,
            "creators don't discover"
        );

        let map = f.run(|| get_wormhole_map(&f.env, &explorer));
        assert_eq!(map.len(), 2);
        let entry = map.get(0).unwrap();
        assert_eq!((entry.origin_nebula, entry.destination), (1, 5));
        assert_eq!(entry.stability, MAX_STABILITY);
        assert!(entry.is_active);
        f.run(|| scan_for_wormholes(&f.env, explorer.clone(), 1));
        assert_eq!(
            f.run(|| get_discovered_wormholes(&f.env, &explorer)).len(),
            2,
            "no duplicates"
        );

        let routes = f.run(|| get_routes_from(&f.env, 1));
        assert_eq!(routes.len(), 1);
        assert_eq!(routes.get(0).unwrap().wormhole_id, a);
    }

    #[test]
    fn storms_hide_faint_wormholes_from_scans() {
        let f = fx();
        f.weather(3, WeatherType::IonStorm);
        let ids = [
            f.open(3, 4),
            f.open(3, 5),
            f.open(3, 6),
            f.open(3, 7),
            f.open(3, 8),
        ];
        let explorer = Address::generate(&f.env);
        let found = f.run(|| scan_for_wormholes(&f.env, explorer.clone(), 3));
        let visibility = f.run(|| weather_system::nebula_visibility(&f.env, 3));
        assert!(visibility < 100);
        for id in ids {
            let visible = f.run(|| detection_signature(&f.env, id)) < visibility;
            assert_eq!(found.contains(id), visible);
        }
    }

    #[test]
    fn wormhole_instability_weather_opens_weaker_wormholes() {
        let f = fx();
        f.weather(4, WeatherType::WormholeInstability);
        let id = f.open(4, 5);
        let status = f.run(|| get_wormhole_status(&f.env, id));
        let intensity = f.run(|| weather_system::get_current_weather(&f.env, 4).intensity);
        assert_eq!(status.base_stability, MAX_STABILITY - intensity / 2);
        assert!(status.base_stability < MAX_STABILITY);
    }
}
