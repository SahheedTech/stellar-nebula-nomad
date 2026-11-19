/**
 * Client-side mirror of the on-chain constellation rules (#632), so the UI can
 * preview outcomes before a transaction. The contract stays the source of
 * truth; keep in sync with src/constellation_mapper.rs.
 */

export const MIN_RECOGNITION_PCT = 60;
export const MIN_SIGHTING_VISIBILITY = 70;
export const COLLECTION_MILESTONES: Array<[number, number]> = [
  [5, 200],
  [10, 500],
  [25, 1500],
];

export type Heading = 'Arrived' | 'Ascending' | 'Descending' | 'Unknown';

export function headingText(heading: Heading, distance: number, anchor: number): string {
  switch (heading) {
    case 'Arrived':
      return `You are in region ${anchor}`;
    case 'Ascending':
      return `Region ${anchor} lies ${distance} step${distance === 1 ? '' : 's'} ahead`;
    case 'Descending':
      return `Region ${anchor} lies ${distance} step${distance === 1 ? '' : 's'} behind`;
    default:
      return 'This constellation has not been charted yet';
  }
}

export function canSight(visibility: number): boolean {
  return visibility >= MIN_SIGHTING_VISIBILITY;
}
