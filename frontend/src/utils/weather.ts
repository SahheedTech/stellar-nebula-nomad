/**
 * Client-side mirror of the on-chain weather rules (#630), so the UI can
 * preview outcomes before a transaction. The contract stays the source of
 * truth; keep in sync with src/weather_system.rs.
 */

export type WeatherType =
  | 'IonStorm'
  | 'SolarFlare'
  | 'NebulaDensityShift'
  | 'GravitationalAnomaly'
  | 'CosmicRadiationBurst'
  | 'MeteorShower'
  | 'StellarWind'
  | 'BlackHoleProximity'
  | 'WormholeInstability'
  | 'AuroraPhenomenon'
  | 'PerfectConditions';

export interface WeatherState {
  regionId: number;
  weather: WeatherType;
  intensity: number;
  startedAt: number;
  endsAt: number;
}

export interface WeatherForecast {
  weather: WeatherType;
  intensity: number;
  startsAt: number;
  confidence: number;
}

/** Effective modifiers in percent, as returned by `get_weather_effects`. */
export interface WeatherImpact {
  scanModifier: number;
  harvestModifier: number;
  xpModifier: number;
  dangerLevel: number;
  speedModifier: number;
  fuelModifier: number;
  combatModifier: number;
  hullWear: number;
}

export const WEATHER_LABELS: Record<WeatherType, { label: string; icon: string }> = {
  IonStorm: { label: 'Ion Storm', icon: '⚡' },
  SolarFlare: { label: 'Solar Flare', icon: '☀️' },
  NebulaDensityShift: { label: 'Density Shift', icon: '🌫️' },
  GravitationalAnomaly: { label: 'Gravitational Anomaly', icon: '🌀' },
  CosmicRadiationBurst: { label: 'Radiation Burst', icon: '☢️' },
  MeteorShower: { label: 'Meteor Shower', icon: '☄️' },
  StellarWind: { label: 'Stellar Wind', icon: '💨' },
  BlackHoleProximity: { label: 'Black Hole Proximity', icon: '🕳️' },
  WormholeInstability: { label: 'Wormhole Instability', icon: '🌌' },
  AuroraPhenomenon: { label: 'Aurora', icon: '🌈' },
  PerfectConditions: { label: 'Perfect Conditions', icon: '✨' },
};

/** Effects whose sign is bad for the player when positive. */
const COST_EFFECTS = new Set<keyof WeatherImpact>(['fuelModifier', 'dangerLevel', 'hullWear']);

export function formatModifier(pct: number): string {
  if (pct === 0) return '±0%';
  return `${pct > 0 ? '+' : ''}${pct}%`;
}

/** "good" | "bad" | "neutral" for colouring an effect. */
export function effectTone(effect: keyof WeatherImpact, value: number): 'good' | 'bad' | 'neutral' {
  if (value === 0) return 'neutral';
  const positiveIsGood = !COST_EFFECTS.has(effect);
  return (value > 0) === positiveIsGood ? 'good' : 'bad';
}

/** Nebula visibility in percent, mirroring `nebula_visibility` (10-100). */
export function visibilityPct(impact: WeatherImpact): number {
  return Math.min(100, Math.max(10, 100 + impact.scanModifier));
}
