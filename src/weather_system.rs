//! Dynamic weather system affecting gameplay (#553, completed in #630).
//!
//! Eleven weather types, each with mechanical effects on nebula visibility,
//! ship performance (speed, fuel, hull wear), combat power and resource
//! yields. See `docs/WEATHER_SYSTEM.md`.
//!
//! ## How weather is chosen
//!
//! Time is divided into fixed slots of [`WEATHER_SLOT_SECS`]. The weather of a
//! region for a slot is a deterministic weighted pick from the region's climate
//! (per-type weights, [`DEFAULT_CLIMATE`] unless an admin sets one) seeded by
//! `sha256(region ‖ slot ‖ season salt)`. Because the pick is deterministic,
//! [`generate_forecast`] predicts exactly what [`get_current_weather`] will
//! later realize. Forecast confidence falls with distance because an admin may
//! still change the climate or the season salt before the slot starts.
//!
//! ## Effects
//!
//! Every base modifier is a percentage scaled linearly by intensity (1-100)
//! and clamped to [`MIN_MODIFIER_PCT`, `MAX_MODIFIER_PCT`], so no weather can
//! zero out or explode a value. `PerfectConditions` carries no negatives.
//!
//! ## Storage Layout
//!
//! ```text
//! WeatherKey::Current(region)      → WeatherState (realized slot)
//! WeatherKey::Forecast(region)     → Vec<WeatherForecast> (last forecast)
//! WeatherKey::History(region, day) → Vec<WeatherType> (realized per day)
//! WeatherKey::SeasonalConfig       → u64 season salt
//! WeatherKey::Climate(region)      → Vec<u32> weights, one per type
//! ```

use soroban_sdk::{contracterror, contracttype, symbol_short, Address, Bytes, Env, Vec};

use crate::emergency_controls;

// ─── Constants ────────────────────────────────────────────────────────────────

/// Length of one weather slot (4 hours, six slots per day).
pub const WEATHER_SLOT_SECS: u64 = 4 * 3600;

/// Slots covered by a forecast (24 hours).
pub const FORECAST_HORIZON_SLOTS: u64 = 6;

/// Number of weather types (length of a climate weight vector).
pub const WEATHER_TYPE_COUNT: u32 = 11;

/// Default climate weights, in [`WeatherType::index`] order, summing to 200.
/// `PerfectConditions` gets 1/200 per slot: about once a month at 6 slots a day.
pub const DEFAULT_CLIMATE: [u32; 11] = [24, 20, 24, 20, 18, 16, 32, 8, 16, 21, 1];

/// Upper bound on the sum of climate weights.
pub const MAX_CLIMATE_WEIGHT_SUM: u32 = 10_000;

/// Lowest effective modifier: weather never removes more than 90%.
pub const MIN_MODIFIER_PCT: i32 = -90;

/// Highest effective modifier: weather never adds more than 300%.
pub const MAX_MODIFIER_PCT: i32 = 300;

/// Highest forecast confidence (the next slot).
pub const MAX_FORECAST_CONFIDENCE: u32 = 95;

/// Confidence lost per additional slot ahead.
pub const FORECAST_CONFIDENCE_DECAY: u32 = 10;

// ─── Weather Types ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracttype]
pub enum WeatherType {
    /// Reduce scanner range 50%, increase rare resource spawn 200%.
    IonStorm,
    /// Double resource values, increase danger levels.
    SolarFlare,
    /// Temporary visibility changes, affect navigation.
    NebulaDensityShift,
    /// Alter travel times between regions.
    GravitationalAnomaly,
    /// Boost XP gain 50%, cause ship degradation.
    CosmicRadiationBurst,
    /// Dangerous but resource-rich areas.
    MeteorShower,
    /// Affect ship speed and fuel consumption.
    StellarWind,
    /// Extreme risk/reward scenarios.
    BlackHoleProximity,
    /// Temporary fast-travel opportunities.
    WormholeInstability,
    /// Visual spectacles with special bonuses.
    AuroraPhenomenon,
    /// All bonuses active, no negatives. Roughly once per month.
    PerfectConditions,
}

impl WeatherType {
    /// Map a climate index (0-10) to its type; out-of-range wraps.
    pub fn from_index(index: u32) -> Self {
        match index % WEATHER_TYPE_COUNT {
            0 => Self::IonStorm,
            1 => Self::SolarFlare,
            2 => Self::NebulaDensityShift,
            3 => Self::GravitationalAnomaly,
            4 => Self::CosmicRadiationBurst,
            5 => Self::MeteorShower,
            6 => Self::StellarWind,
            7 => Self::BlackHoleProximity,
            8 => Self::WormholeInstability,
            9 => Self::AuroraPhenomenon,
            _ => Self::PerfectConditions,
        }
    }

    /// Position of this type in a climate weight vector.
    pub fn index(&self) -> u32 {
        match self {
            Self::IonStorm => 0,
            Self::SolarFlare => 1,
            Self::NebulaDensityShift => 2,
            Self::GravitationalAnomaly => 3,
            Self::CosmicRadiationBurst => 4,
            Self::MeteorShower => 5,
            Self::StellarWind => 6,
            Self::BlackHoleProximity => 7,
            Self::WormholeInstability => 8,
            Self::AuroraPhenomenon => 9,
            Self::PerfectConditions => 10,
        }
    }

    /// Scanner range / nebula visibility, in percent.
    pub fn scan_modifier(&self) -> i32 {
        match self {
            Self::IonStorm => -50,
            Self::SolarFlare => 20,
            Self::NebulaDensityShift => -10,
            Self::GravitationalAnomaly => 0,
            Self::CosmicRadiationBurst => 15,
            Self::MeteorShower => 30,
            Self::StellarWind => -5,
            Self::BlackHoleProximity => -20,
            Self::WormholeInstability => 10,
            Self::AuroraPhenomenon => 25,
            Self::PerfectConditions => 50,
        }
    }

    /// Resource yield, in percent.
    pub fn harvest_modifier(&self) -> i32 {
        match self {
            Self::IonStorm => 200,
            Self::SolarFlare => 100,
            Self::NebulaDensityShift => -10,
            Self::GravitationalAnomaly => 0,
            Self::CosmicRadiationBurst => 50,
            Self::MeteorShower => 80,
            Self::StellarWind => -20,
            Self::BlackHoleProximity => 150,
            Self::WormholeInstability => 0,
            Self::AuroraPhenomenon => 30,
            Self::PerfectConditions => 100,
        }
    }

    /// Experience gain, in percent.
    pub fn xp_modifier(&self) -> i32 {
        match self {
            Self::CosmicRadiationBurst => 50,
            Self::PerfectConditions => 25,
            Self::AuroraPhenomenon => 15,
            _ => 0,
        }
    }

    /// Ship speed, in percent (positive = faster).
    pub fn speed_modifier(&self) -> i32 {
        match self {
            Self::StellarWind => -30,
            Self::GravitationalAnomaly => 20,
            Self::WormholeInstability => 25,
            Self::BlackHoleProximity => -15,
            Self::MeteorShower => -10,
            Self::IonStorm => -5,
            Self::PerfectConditions => 10,
            _ => 0,
        }
    }

    /// Fuel / energy consumption, in percent (positive = costs more).
    pub fn fuel_modifier(&self) -> i32 {
        match self {
            Self::StellarWind => 25,
            Self::BlackHoleProximity => 40,
            Self::SolarFlare => 10,
            Self::GravitationalAnomaly => -10,
            Self::PerfectConditions => -10,
            _ => 0,
        }
    }

    /// Combat power, in percent.
    pub fn combat_modifier(&self) -> i32 {
        match self {
            Self::IonStorm => -20,
            Self::NebulaDensityShift => -15,
            Self::MeteorShower => -10,
            Self::SolarFlare => 15,
            Self::CosmicRadiationBurst => 10,
            Self::BlackHoleProximity => 25,
            Self::AuroraPhenomenon => 5,
            Self::PerfectConditions => 10,
            _ => 0,
        }
    }

    /// Hull wear per hour of exposure, in percent of max hull.
    pub fn hull_wear(&self) -> u32 {
        match self {
            Self::BlackHoleProximity => 5,
            Self::MeteorShower => 4,
            Self::CosmicRadiationBurst => 3,
            Self::SolarFlare => 2,
            Self::IonStorm => 1,
            _ => 0,
        }
    }

    pub fn danger_level(&self) -> u32 {
        match self {
            Self::BlackHoleProximity => 95,
            Self::MeteorShower => 80,
            Self::SolarFlare => 70,
            Self::CosmicRadiationBurst => 60,
            Self::IonStorm => 55,
            Self::WormholeInstability => 50,
            Self::StellarWind => 30,
            Self::NebulaDensityShift => 25,
            Self::GravitationalAnomaly => 20,
            Self::AuroraPhenomenon => 10,
            Self::PerfectConditions => 0,
        }
    }
}

// ─── Storage Keys ─────────────────────────────────────────────────────────────

#[derive(Clone)]
#[contracttype]
pub enum WeatherKey {
    /// Current weather per region: region_id -> WeatherState
    Current(u64),
    /// Weather forecast: region_id -> Vec<WeatherForecast>
    Forecast(u64),
    /// Weather history: (region_id, day) -> Vec<WeatherType>
    History(u64, u32),
    /// Season salt (u64) mixed into every slot seed.
    SeasonalConfig,
    /// Climate weights per region: region_id -> Vec<u32>
    Climate(u64),
}

// ─── Data Structs ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct WeatherState {
    pub region_id: u64,
    pub weather: WeatherType,
    pub intensity: u32,  // 1-100
    pub started_at: u64, // ledger timestamp
    pub ends_at: u64,    // ledger timestamp
}

#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct WeatherForecast {
    pub weather: WeatherType,
    pub intensity: u32,
    pub starts_at: u64,
    pub confidence: u32, // 0-100
}

/// Effective modifiers for a region right now, already scaled by intensity
/// and clamped. All percentages.
#[derive(Clone, Debug, PartialEq)]
#[contracttype]
pub struct WeatherImpact {
    pub scan_modifier: i32,
    pub harvest_modifier: i32,
    pub xp_modifier: i32,
    pub danger_level: u32,
    pub speed_modifier: i32,
    pub fuel_modifier: i32,
    pub combat_modifier: i32,
    pub hull_wear: u32,
}

// ─── Errors ───────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum WeatherError {
    InvalidRegion = 1,
    NoActiveWeather = 2,
    InvalidIntensity = 3,
    ForecastError = 4,
    /// Caller is not a contract admin.
    Unauthorized = 5,
    /// Climate weights must have one entry per type and a positive bounded sum.
    InvalidClimate = 6,
}

impl crate::error_standard::StandardContractError for WeatherError {
    fn descriptor(self) -> crate::error_standard::ErrorDescriptor {
        use crate::error_standard::ErrorKind;
        let (kind, retryable) = match self {
            Self::InvalidRegion | Self::InvalidIntensity | Self::InvalidClimate => {
                (ErrorKind::Validation, false)
            }
            Self::NoActiveWeather => (ErrorKind::NotFound, true),
            Self::ForecastError => (ErrorKind::Internal, true),
            Self::Unauthorized => (ErrorKind::Authorization, false),
        };
        crate::error_standard::ErrorDescriptor {
            module: "weather_system",
            code: self as u32,
            kind,
            retryable,
        }
    }
}

// ─── Deterministic slot weather ───────────────────────────────────────────────

/// Index of the weather slot containing `timestamp`.
pub fn slot_of(timestamp: u64) -> u64 {
    timestamp / WEATHER_SLOT_SECS
}

fn season_salt(env: &Env) -> u64 {
    env.storage()
        .persistent()
        .get::<WeatherKey, u64>(&WeatherKey::SeasonalConfig)
        .unwrap_or(0)
}

fn slot_seed(env: &Env, region_id: u64, slot: u64) -> u64 {
    let mut input = Bytes::from_slice(env, b"weather");
    input.extend_from_array(&region_id.to_be_bytes());
    input.extend_from_array(&slot.to_be_bytes());
    input.extend_from_array(&season_salt(env).to_be_bytes());
    let digest = env.crypto().sha256(&input).to_array();
    let mut seed = [0u8; 8];
    seed.copy_from_slice(&digest[0..8]);
    u64::from_be_bytes(seed)
}

/// Weighted pick of a weather type from `weights` using `roll`.
pub fn pick_weather(weights: &Vec<u32>, roll: u64) -> WeatherType {
    let total: u64 = weights.iter().map(u64::from).sum();
    if total == 0 {
        return WeatherType::PerfectConditions;
    }
    let mut target = roll % total;
    for (index, weight) in weights.iter().enumerate() {
        let weight = u64::from(weight);
        if target < weight {
            return WeatherType::from_index(index as u32);
        }
        target -= weight;
    }
    WeatherType::PerfectConditions
}

/// The weather a region will have during `slot` under the current climate and
/// season. Pure with respect to storage writes.
pub fn weather_for_slot(env: &Env, region_id: u64, slot: u64) -> WeatherState {
    let seed = slot_seed(env, region_id, slot);
    let weather = pick_weather(&get_region_climate(env, region_id), seed);
    let intensity = 20 + ((seed >> 16) % 81) as u32; // 20-100
    let started_at = slot.saturating_mul(WEATHER_SLOT_SECS);
    WeatherState {
        region_id,
        weather,
        intensity,
        started_at,
        ends_at: started_at.saturating_add(WEATHER_SLOT_SECS),
    }
}

// ─── Climate & season administration ──────────────────────────────────────────

fn require_admin(env: &Env, admin: &Address) -> Result<(), WeatherError> {
    admin.require_auth();
    if emergency_controls::get_admins(env).contains(admin) {
        Ok(())
    } else {
        Err(WeatherError::Unauthorized)
    }
}

/// Climate weights for a region, [`DEFAULT_CLIMATE`] unless one was set.
pub fn get_region_climate(env: &Env, region_id: u64) -> Vec<u32> {
    env.storage()
        .persistent()
        .get::<WeatherKey, Vec<u32>>(&WeatherKey::Climate(region_id))
        .unwrap_or_else(|| Vec::from_array(env, DEFAULT_CLIMATE))
}

/// Set a region's climate (admin only). Takes effect from the next slot that is
/// realized; the slot in progress keeps the weather it already has.
pub fn set_region_climate(
    env: &Env,
    admin: &Address,
    region_id: u64,
    weights: Vec<u32>,
) -> Result<(), WeatherError> {
    require_admin(env, admin)?;
    if weights.len() != WEATHER_TYPE_COUNT {
        return Err(WeatherError::InvalidClimate);
    }
    let total: u64 = weights.iter().map(u64::from).sum();
    if total == 0 || total > u64::from(MAX_CLIMATE_WEIGHT_SUM) {
        return Err(WeatherError::InvalidClimate);
    }
    env.storage()
        .persistent()
        .set(&WeatherKey::Climate(region_id), &weights);
    env.events().publish(
        (symbol_short!("weather"), symbol_short!("climate")),
        (region_id, weights),
    );
    Ok(())
}

/// Change the season salt (admin only), reshuffling every future slot.
pub fn set_season(env: &Env, admin: &Address, salt: u64) -> Result<(), WeatherError> {
    require_admin(env, admin)?;
    env.storage()
        .persistent()
        .set(&WeatherKey::SeasonalConfig, &salt);
    env.events()
        .publish((symbol_short!("weather"), symbol_short!("season")), salt);
    Ok(())
}

// ─── Core Weather Functions ───────────────────────────────────────────────────

/// Realize (and store) the weather for the region's current slot, recording it
/// in the daily history.
pub fn generate_weather(env: &Env, region_id: u64) -> WeatherState {
    let timestamp = env.ledger().timestamp();
    let state = weather_for_slot(env, region_id, slot_of(timestamp));
    env.storage()
        .persistent()
        .set(&WeatherKey::Current(region_id), &state);
    record_weather_history(env, region_id, (timestamp / 86_400) as u32, state.weather);
    env.events().publish(
        (symbol_short!("weather"), symbol_short!("changed")),
        (region_id, state.weather, state.intensity, state.ends_at),
    );
    state
}

/// Current weather for a region. Realizes a new slot when the stored one has
/// ended (or none exists yet).
pub fn get_current_weather(env: &Env, region_id: u64) -> WeatherState {
    let now = env.ledger().timestamp();
    match env
        .storage()
        .persistent()
        .get::<WeatherKey, WeatherState>(&WeatherKey::Current(region_id))
    {
        Some(state) if now >= state.started_at && now < state.ends_at => state,
        _ => generate_weather(env, region_id),
    }
}

/// Scale a base percentage by intensity (1-100) and clamp it.
pub fn scale_modifier(base_pct: i32, intensity: u32) -> i32 {
    let intensity = intensity.clamp(1, 100) as i32;
    (base_pct * intensity / 100).clamp(MIN_MODIFIER_PCT, MAX_MODIFIER_PCT)
}

/// Effective modifiers for `state`.
pub fn impact_of(state: &WeatherState) -> WeatherImpact {
    let w = state.weather;
    let i = state.intensity;
    WeatherImpact {
        scan_modifier: scale_modifier(w.scan_modifier(), i),
        harvest_modifier: scale_modifier(w.harvest_modifier(), i),
        xp_modifier: scale_modifier(w.xp_modifier(), i),
        danger_level: w.danger_level() * i.clamp(1, 100) / 100,
        speed_modifier: scale_modifier(w.speed_modifier(), i),
        fuel_modifier: scale_modifier(w.fuel_modifier(), i),
        combat_modifier: scale_modifier(w.combat_modifier(), i),
        hull_wear: w.hull_wear() * i.clamp(1, 100) / 100,
    }
}

/// Calculate the gameplay impact of current weather on a region.
pub fn get_weather_impact(env: &Env, region_id: u64) -> WeatherImpact {
    impact_of(&get_current_weather(env, region_id))
}

/// Apply a percentage modifier to `base`, never going below 10% of `base`.
pub fn apply_modifier(base: u64, pct: i32) -> u64 {
    let pct = pct.clamp(MIN_MODIFIER_PCT, MAX_MODIFIER_PCT);
    let factor = (100 + pct) as u64;
    base.saturating_mul(factor) / 100
}

// ─── Integration helpers (used by other systems) ──────────────────────────────

/// Nebula visibility in percent (10-100): how much of a nebula a scan reveals.
pub fn nebula_visibility(env: &Env, region_id: u64) -> u32 {
    let scan = get_weather_impact(env, region_id).scan_modifier;
    (100 + scan).clamp(10, 100) as u32
}

/// Scanner range after weather.
pub fn effective_scan_range(env: &Env, region_id: u64, base_range: u64) -> u64 {
    apply_modifier(base_range, get_weather_impact(env, region_id).scan_modifier)
}

/// Resource yield after weather.
pub fn effective_harvest_yield(env: &Env, region_id: u64, base_yield: u64) -> u64 {
    apply_modifier(
        base_yield,
        get_weather_impact(env, region_id).harvest_modifier,
    )
}

/// Experience after weather.
pub fn effective_xp(env: &Env, region_id: u64, base_xp: u64) -> u64 {
    apply_modifier(base_xp, get_weather_impact(env, region_id).xp_modifier)
}

/// Energy / fuel cost after weather.
pub fn effective_fuel_cost(env: &Env, region_id: u64, base_cost: u64) -> u64 {
    apply_modifier(base_cost, get_weather_impact(env, region_id).fuel_modifier)
}

/// Travel time after weather: faster ships take proportionally less time.
pub fn effective_travel_time(env: &Env, region_id: u64, base_secs: u64) -> u64 {
    let speed = get_weather_impact(env, region_id).speed_modifier;
    base_secs.saturating_mul(100) / (100 + speed).max(10) as u64
}

/// Combat power after weather.
pub fn effective_combat_power(env: &Env, region_id: u64, base_power: u64) -> u64 {
    apply_modifier(
        base_power,
        get_weather_impact(env, region_id).combat_modifier,
    )
}

// ─── Forecasting ──────────────────────────────────────────────────────────────

/// Confidence for a slot `slots_ahead` (1-based) in the future.
pub fn forecast_confidence(slots_ahead: u64) -> u32 {
    let decay = (slots_ahead.saturating_sub(1) as u32).saturating_mul(FORECAST_CONFIDENCE_DECAY);
    MAX_FORECAST_CONFIDENCE.saturating_sub(decay).max(30)
}

/// Forecast the next 24 hours ([`FORECAST_HORIZON_SLOTS`] slots) for a region.
/// Unless an admin changes the climate or season first, each entry is exactly
/// the weather [`get_current_weather`] realizes for that slot.
pub fn generate_forecast(env: &Env, region_id: u64) -> Vec<WeatherForecast> {
    let current = slot_of(env.ledger().timestamp());
    let mut forecasts = Vec::new(env);
    for ahead in 1..=FORECAST_HORIZON_SLOTS {
        let state = weather_for_slot(env, region_id, current + ahead);
        forecasts.push_back(WeatherForecast {
            weather: state.weather,
            intensity: state.intensity,
            starts_at: state.started_at,
            confidence: forecast_confidence(ahead),
        });
    }
    env.storage()
        .persistent()
        .set(&WeatherKey::Forecast(region_id), &forecasts);
    forecasts
}

/// Record weather history for analytics.
pub fn record_weather_history(env: &Env, region_id: u64, day: u32, weather: WeatherType) {
    let key = WeatherKey::History(region_id, day);
    let mut history = env
        .storage()
        .persistent()
        .get::<WeatherKey, Vec<WeatherType>>(&key)
        .unwrap_or_else(|| Vec::new(env));
    history.push_back(weather);
    env.storage().persistent().set(&key, &history);
}

/// Weather realized in a region on a given day (UTC day index).
pub fn get_weather_history(env: &Env, region_id: u64, day: u32) -> Vec<WeatherType> {
    env.storage()
        .persistent()
        .get::<WeatherKey, Vec<WeatherType>>(&WeatherKey::History(region_id, day))
        .unwrap_or_else(|| Vec::new(env))
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger};
    use soroban_sdk::{contract, contractimpl};

    #[contract]
    struct Stub;
    #[contractimpl]
    impl Stub {}

    const ALL: [WeatherType; 11] = [
        WeatherType::IonStorm,
        WeatherType::SolarFlare,
        WeatherType::NebulaDensityShift,
        WeatherType::GravitationalAnomaly,
        WeatherType::CosmicRadiationBurst,
        WeatherType::MeteorShower,
        WeatherType::StellarWind,
        WeatherType::BlackHoleProximity,
        WeatherType::WormholeInstability,
        WeatherType::AuroraPhenomenon,
        WeatherType::PerfectConditions,
    ];

    fn setup() -> (Env, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_timestamp(1_000 * WEATHER_SLOT_SECS + 60);
        let contract = env.register(Stub, ());
        let admin = Address::generate(&env);
        let a = admin.clone();
        env.as_contract(&contract, || {
            emergency_controls::initialize_admins(&env, Vec::from_array(&env, [a])).unwrap();
        });
        (env, contract, admin)
    }

    fn only(env: &Env, weather: WeatherType) -> Vec<u32> {
        let mut weights = Vec::new(env);
        for t in ALL {
            weights.push_back(u32::from(t == weather));
        }
        weights
    }

    #[test]
    fn index_round_trips_for_every_type() {
        for t in ALL {
            assert_eq!(WeatherType::from_index(t.index()), t);
        }
        assert_eq!(DEFAULT_CLIMATE.len() as u32, WEATHER_TYPE_COUNT);
        assert_eq!(DEFAULT_CLIMATE.iter().sum::<u32>(), 200);
    }

    #[test]
    fn perfect_conditions_has_no_negatives() {
        let p = WeatherType::PerfectConditions;
        assert!(p.scan_modifier() >= 0 && p.harvest_modifier() >= 0 && p.xp_modifier() >= 0);
        assert!(p.speed_modifier() >= 0 && p.combat_modifier() >= 0);
        assert!(p.fuel_modifier() <= 0, "lower fuel use is a bonus");
        assert_eq!(p.danger_level(), 0);
        assert_eq!(p.hull_wear(), 0);
    }

    #[test]
    fn effects_stay_within_balance_bounds_for_every_type_and_intensity() {
        for t in ALL {
            for intensity in [1u32, 20, 50, 80, 100] {
                let impact = impact_of(&WeatherState {
                    region_id: 1,
                    weather: t,
                    intensity,
                    started_at: 0,
                    ends_at: WEATHER_SLOT_SECS,
                });
                for m in [
                    impact.scan_modifier,
                    impact.harvest_modifier,
                    impact.xp_modifier,
                    impact.speed_modifier,
                    impact.fuel_modifier,
                    impact.combat_modifier,
                ] {
                    assert!(
                        (MIN_MODIFIER_PCT..=MAX_MODIFIER_PCT).contains(&m),
                        "{t:?} {intensity}"
                    );
                }
                assert!(impact.danger_level <= 100 && impact.hull_wear <= 5);
            }
        }
    }

    #[test]
    fn modifiers_scale_linearly_with_intensity() {
        // The old integer factor dropped every effect to zero below intensity 50.
        assert_eq!(scale_modifier(-50, 20), -10);
        assert_eq!(scale_modifier(-50, 100), -50);
        assert_eq!(scale_modifier(200, 50), 100);
        assert_eq!(scale_modifier(1_000, 100), MAX_MODIFIER_PCT);
        assert_eq!(apply_modifier(1_000, -50), 500);
        assert_eq!(apply_modifier(1_000, -500), 100);
        assert_eq!(apply_modifier(1_000, 200), 3_000);
    }

    #[test]
    fn weighted_pick_respects_climate() {
        let env = Env::default();
        for t in ALL {
            let weights = only(&env, t);
            for roll in [0u64, 1, 7, u64::MAX] {
                assert_eq!(pick_weather(&weights, roll), t);
            }
        }
        assert_eq!(
            pick_weather(&Vec::new(&env), 5),
            WeatherType::PerfectConditions
        );
    }

    #[test]
    fn default_climate_makes_perfect_conditions_rare() {
        let (env, contract, _) = setup();
        env.as_contract(&contract, || {
            let mut perfect = 0u32;
            let mut seen = [false; 11];
            for slot in 0..2_000u64 {
                let w = weather_for_slot(&env, 7, slot).weather;
                seen[w.index() as usize] = true;
                if w == WeatherType::PerfectConditions {
                    perfect += 1;
                }
            }
            assert!(
                perfect < 40,
                "perfect conditions too common: {perfect}/2000"
            );
            assert!(seen[..10].iter().all(|s| *s), "every regular type occurs");
        });
    }

    #[test]
    fn current_weather_is_stable_within_a_slot_and_changes_after() {
        let (env, contract, _) = setup();
        env.as_contract(&contract, || {
            let first = get_current_weather(&env, 3);
            env.ledger().set_timestamp(first.ends_at - 1);
            assert_eq!(get_current_weather(&env, 3), first);
            env.ledger().set_timestamp(first.ends_at);
            let next = get_current_weather(&env, 3);
            assert_eq!(next.started_at, first.ends_at);
            assert_eq!(next.ends_at - next.started_at, WEATHER_SLOT_SECS);
            assert!((20..=100).contains(&next.intensity));
        });
    }

    #[test]
    fn forecast_predicts_the_weather_that_is_realized() {
        let (env, contract, _) = setup();
        env.as_contract(&contract, || {
            let forecast = generate_forecast(&env, 11);
            assert_eq!(forecast.len() as u64, FORECAST_HORIZON_SLOTS);
            let mut last_confidence = 101;
            for entry in forecast.iter() {
                assert!(entry.confidence < last_confidence);
                last_confidence = entry.confidence;
                env.ledger().set_timestamp(entry.starts_at + 1);
                let actual = get_current_weather(&env, 11);
                assert_eq!(actual.weather, entry.weather);
                assert_eq!(actual.intensity, entry.intensity);
            }
            assert_eq!(forecast.get(0).unwrap().confidence, MAX_FORECAST_CONFIDENCE);
        });
    }

    #[test]
    fn climate_controls_future_weather_and_is_admin_only() {
        let (env, contract, admin) = setup();
        let stranger = Address::generate(&env);
        let run = |f: &dyn Fn() -> Result<(), WeatherError>| env.as_contract(&contract, f);
        assert_eq!(
            run(&|| set_region_climate(&env, &stranger, 5, only(&env, WeatherType::StellarWind))),
            Err(WeatherError::Unauthorized)
        );
        assert_eq!(
            run(&|| {
                let mut bad = only(&env, WeatherType::StellarWind);
                bad.pop_back();
                set_region_climate(&env, &admin, 5, bad)
            }),
            Err(WeatherError::InvalidClimate)
        );
        assert_eq!(
            run(&|| set_region_climate(&env, &admin, 5, Vec::from_array(&env, [0u32; 11]))),
            Err(WeatherError::InvalidClimate)
        );
        run(&|| set_region_climate(&env, &admin, 5, only(&env, WeatherType::StellarWind))).unwrap();
        env.as_contract(&contract, || {
            for entry in generate_forecast(&env, 5).iter() {
                assert_eq!(entry.weather, WeatherType::StellarWind);
            }
            let impact = get_weather_impact(&env, 5);
            assert!(impact.speed_modifier < 0 && impact.fuel_modifier > 0);
        });
    }

    #[test]
    fn season_change_reshuffles_the_forecast() {
        let (env, contract, admin) = setup();
        let before = env.as_contract(&contract, || generate_forecast(&env, 9));
        env.as_contract(&contract, || set_season(&env, &admin, 42))
            .unwrap();
        let after = env.as_contract(&contract, || generate_forecast(&env, 9));
        assert_ne!(before, after);
        assert_eq!(
            env.as_contract(&contract, || set_season(&env, &Address::generate(&env), 1)),
            Err(WeatherError::Unauthorized)
        );
    }

    #[test]
    fn effects_apply_to_visibility_ships_combat_and_resources() {
        let (env, contract, admin) = setup();
        for (region, weather) in [
            (1u64, WeatherType::IonStorm),
            (2, WeatherType::PerfectConditions),
            (3, WeatherType::StellarWind),
        ] {
            env.as_contract(&contract, || {
                set_region_climate(&env, &admin, region, only(&env, weather))
            })
            .unwrap();
        }
        env.as_contract(&contract, || {
            assert!(nebula_visibility(&env, 1) < 100);
            assert_eq!(nebula_visibility(&env, 2), 100);
            assert!(effective_scan_range(&env, 1, 1_000) < 1_000);
            assert!(effective_harvest_yield(&env, 1, 1_000) > 1_000);
            assert!(effective_combat_power(&env, 1, 1_000) < 1_000);
            assert!(effective_combat_power(&env, 2, 1_000) > 1_000);
            assert!(effective_fuel_cost(&env, 3, 1_000) > 1_000);
            assert!(effective_travel_time(&env, 3, 1_000) > 1_000);
            assert!(effective_travel_time(&env, 2, 1_000) < 1_000);
            assert!(effective_xp(&env, 2, 1_000) > 1_000);
        });
    }

    #[test]
    fn realized_weather_is_recorded_in_history() {
        let (env, contract, _) = setup();
        env.as_contract(&contract, || {
            let day = (env.ledger().timestamp() / 86_400) as u32;
            let first = get_current_weather(&env, 4);
            get_current_weather(&env, 4);
            let history = get_weather_history(&env, 4, day);
            assert_eq!(history.len(), 1);
            assert_eq!(history.get(0).unwrap(), first.weather);
        });
    }
}
