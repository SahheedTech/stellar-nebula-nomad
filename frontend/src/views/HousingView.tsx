import React from 'react';
import {
  HouseSize,
  THEMES_BY_SIZE,
  displaySlots,
  formatRating,
} from '../utils/housing';

export interface PlacedItem {
  furnitureId: number;
  name: string;
  category: string;
  x: number;
  y: number;
  rotation: number;
  layer: number;
}

export interface DisplayedAchievement {
  slot: number;
  achievementId: number;
  title: string;
}

export interface HouseShowcaseView {
  owner: string;
  size: HouseSize;
  access: 'Public' | 'Private' | 'FriendsOnly';
  wallTheme: number;
  floorTheme: number;
  lightingTheme: number;
  totalVisits: number;
  averageRating: number;
  placements: PlacedItem[];
  displays: DisplayedAchievement[];
  banner?: { allianceName: string };
  storage: { capacity: number; used: number; expansions: number; nextExpansionCost: number };
}

export interface HousingViewProps {
  showcase: HouseShowcaseView;
  /** True when the connected wallet owns this house. */
  isOwner: boolean;
  onThemeChange?: (wall: number, floor: number, lighting: number) => void;
  onExpandStorage?: () => void;
  onHangBanner?: () => void;
  onRemoveFurniture?: (furnitureId: number) => void;
  onVisit?: () => void;
  onRate?: (rating: number) => void;
}

/** A house: layout, display cases, guild banner, storage and visitors (#629). */
export const HousingView: React.FC<HousingViewProps> = ({
  showcase,
  isOwner,
  onThemeChange,
  onExpandStorage,
  onHangBanner,
  onRemoveFurniture,
  onVisit,
  onRate,
}) => {
  const maxTheme = THEMES_BY_SIZE[showcase.size];
  const cases = showcase.placements.filter((p) => p.category === 'Achievement').length;
  const slots = displaySlots(showcase.size, cases);
  const themes = Array.from({ length: maxTheme }, (_, i) => i + 1);

  return (
    <section aria-label="House" style={{ padding: 16, borderRadius: 8, background: '#111827', color: '#F8FAFC' }}>
      <header>
        <h2 style={{ margin: 0, fontSize: '1.1rem' }}>
          {showcase.size} house · {showcase.access === 'FriendsOnly' ? 'Friends only' : showcase.access}
        </h2>
        <span style={{ color: '#94A3B8', fontSize: '0.8rem' }}>
          {showcase.totalVisits} visits · {formatRating(showcase.averageRating)}
        </span>
        {showcase.banner && (
          <div aria-label="Guild banner" style={{ marginTop: 8, fontWeight: 700 }}>
            🏳️ {showcase.banner.allianceName}
          </div>
        )}
      </header>

      {isOwner && onThemeChange && (
        <fieldset style={{ margin: '12px 0', border: '1px solid #334155' }}>
          <legend>Themes (1–{maxTheme})</legend>
          {(['Walls', 'Floor', 'Lighting'] as const).map((label, i) => {
            const current = [showcase.wallTheme, showcase.floorTheme, showcase.lightingTheme];
            return (
              <label key={label} style={{ marginRight: 12 }}>
                {label}{' '}
                <select
                  aria-label={label}
                  value={current[i]}
                  onChange={(e) => {
                    const next = [...current];
                    next[i] = Number(e.target.value);
                    onThemeChange(next[0], next[1], next[2]);
                  }}
                >
                  {themes.map((t) => (
                    <option key={t} value={t}>
                      {t}
                    </option>
                  ))}
                </select>
              </label>
            );
          })}
        </fieldset>
      )}

      <h3 style={{ fontSize: '0.9rem' }}>Achievement displays ({showcase.displays.length}/{slots})</h3>
      {slots === 0 ? (
        <p>Place a display case to show achievements.</p>
      ) : (
        <ol style={{ display: 'flex', gap: 8, listStyle: 'none', padding: 0 }}>
          {Array.from({ length: slots }, (_, slot) => {
            const shown = showcase.displays.find((d) => d.slot === slot);
            return (
              <li key={slot} data-testid={`display-slot-${slot}`} style={{ padding: 8, border: '1px dashed #475569', borderRadius: 6 }}>
                {shown ? `🏆 ${shown.title}` : 'Empty'}
              </li>
            );
          })}
        </ol>
      )}

      <h3 style={{ fontSize: '0.9rem' }}>Furniture ({showcase.placements.length})</h3>
      <ul style={{ padding: 0, listStyle: 'none' }}>
        {showcase.placements.map((p, i) => (
          <li key={`${p.furnitureId}-${i}`}>
            {p.name} at ({p.x}, {p.y}) · {p.rotation}° · layer {p.layer}
            {isOwner && onRemoveFurniture && (
              <button type="button" onClick={() => onRemoveFurniture(p.furnitureId)} style={{ marginLeft: 8 }}>
                Store
              </button>
            )}
          </li>
        ))}
      </ul>

      <p data-testid="storage">
        Storage {showcase.storage.used}/{showcase.storage.capacity} · {showcase.storage.expansions} expansions
      </p>
      {isOwner && (
        <div style={{ display: 'flex', gap: 8 }}>
          {onExpandStorage && (
            <button type="button" onClick={onExpandStorage} disabled={showcase.storage.nextExpansionCost === 0}>
              {showcase.storage.nextExpansionCost === 0
                ? 'Storage maxed'
                : `Expand storage (${showcase.storage.nextExpansionCost} dust)`}
            </button>
          )}
          {onHangBanner && (
            <button type="button" onClick={onHangBanner}>
              Hang guild banner
            </button>
          )}
        </div>
      )}
      {!isOwner && (
        <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          {onVisit && (
            <button type="button" onClick={onVisit}>
              Visit
            </button>
          )}
          {onRate &&
            [1, 2, 3, 4, 5].map((stars) => (
              <button key={stars} type="button" aria-label={`Rate ${stars} stars`} onClick={() => onRate(stars * 100)}>
                {'★'.repeat(stars)}
              </button>
            ))}
        </div>
      )}
    </section>
  );
};
