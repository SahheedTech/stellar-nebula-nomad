/**
 * Client-side mirror of the on-chain wormhole rules (#631), so the UI can
 * preview outcomes before a transaction. The contract stays the source of
 * truth; keep in sync with src/wormhole_traveler.rs.
 */

export const MAX_STABILITY = 100;
export const STABILITY_LOSS_PER_JUMP = 10;
export const MAX_JUMP_RISK = 70;
export const TURBULENCE_SURCHARGE_PCT = 25;
export const EMERGENCY_EXIT_COST_PCT = 50;
export const EMERGENCY_EXIT_WINDOW_SECS = 300;

export type JumpOutcome = 'Clean' | 'Turbulent' | 'EmergencyExit';

export interface WormholeMapEntry {
  wormholeId: number;
  originNebula: number;
  destination: number;
  stability: number;
  risk: number;
  travelCost: number;
  isActive: boolean;
  expiresAt: number;
}

/** Mirrors `jump_risk`. */
export function jumpRisk(stability: number, weatherDanger: number): number {
  const instability = MAX_STABILITY - Math.min(stability, MAX_STABILITY);
  return Math.min(Math.floor(instability / 2) + Math.floor(weatherDanger / 5), MAX_JUMP_RISK);
}

/** Worst-case energy a jump can cost (turbulence surcharge). */
export function worstCaseEnergy(travelCost: number): number {
  return travelCost + Math.floor((travelCost * TURBULENCE_SURCHARGE_PCT) / 100);
}

/** Chance (percent) of each outcome for a given risk, mirroring `decide_outcome`. */
export function outcomeOdds(risk: number): Record<JumpOutcome, number> {
  const exit = Math.floor(risk / 3);
  return { EmergencyExit: exit, Turbulent: risk - exit, Clean: 100 - risk };
}

export function riskLabel(risk: number): 'Low' | 'Moderate' | 'High' | 'Extreme' {
  if (risk < 10) return 'Low';
  if (risk < 30) return 'Moderate';
  if (risk < 50) return 'High';
  return 'Extreme';
}
