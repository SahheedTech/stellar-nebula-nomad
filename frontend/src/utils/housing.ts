/**
 * Client-side mirror of the on-chain housing rules (#629), so the UI can
 * preview outcomes before a transaction. The contract stays the source of
 * truth; keep in sync with src/player_housing.rs.
 */

export type HouseSize = 'Small' | 'Medium' | 'Large';

export const THEMES_BY_SIZE: Record<HouseSize, number> = { Small: 5, Medium: 10, Large: 20 };
export const MAX_DISPLAY_SLOTS: Record<HouseSize, number> = { Small: 3, Medium: 6, Large: 12 };
export const SLOTS_PER_DISPLAY_CASE = 3;

/** Mirrors `display_slots`: 3 per placed display case, capped by house size. */
export function displaySlots(size: HouseSize, displayCases: number): number {
  return Math.min(displayCases * SLOTS_PER_DISPLAY_CASE, MAX_DISPLAY_SLOTS[size]);
}

export function formatRating(scaled: number): string {
  return scaled === 0 ? 'Not rated' : `${(scaled / 100).toFixed(1)} ★`;
}
