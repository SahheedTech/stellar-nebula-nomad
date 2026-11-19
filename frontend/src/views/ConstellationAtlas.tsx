import React from 'react';
import { COLLECTION_MILESTONES, Heading, canSight, headingText } from '../utils/constellations';

export interface AtlasEntry {
  constellationId: number;
  name: string;
  lore: string;
  anchorRegion: number;
  starCount: number;
  sighted: boolean;
}

export interface NavigationAidView {
  constellationId: number;
  name: string;
  confidencePct: number;
  heading: Heading;
  distance: number;
  anchorRegion: number;
}

export interface ConstellationAtlasProps {
  entries: AtlasEntry[];
  totalConstellations: number;
  /** Weather visibility in the observer's region, percent. */
  visibility: number;
  navigation?: NavigationAidView;
  lastBonus?: number;
  onSight: () => void;
}

/** Collection, lore, navigation and sighting for constellations (#632). */
export const ConstellationAtlas: React.FC<ConstellationAtlasProps> = ({
  entries,
  totalConstellations,
  visibility,
  navigation,
  lastBonus,
  onSight,
}) => {
  const sighted = entries.filter((e) => e.sighted).length;
  const pct = totalConstellations === 0 ? 0 : Math.floor((sighted * 100) / totalConstellations);
  const next = COLLECTION_MILESTONES.find(([size]) => size > sighted);
  const clear = canSight(visibility);

  return (
    <section aria-label="Constellation atlas" style={{ padding: 16, borderRadius: 8, background: '#020617', color: '#F8FAFC' }}>
      <header style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <h2 style={{ margin: 0, fontSize: '1.1rem' }}>Constellation atlas</h2>
        <button type="button" onClick={onSight} disabled={!clear} title={clear ? 'Sight the sky' : 'The sky is obscured'}>
          Sight constellation
        </button>
      </header>

      <p data-testid="collection-progress">
        {sighted} / {totalConstellations} collected ({pct}%)
        {next && ` · next reward at ${next[0]} (+${next[1]} dust)`}
      </p>
      <progress max={100} value={pct} aria-label="collection progress" style={{ width: '100%' }} />
      {!clear && <p role="alert">Visibility {visibility}%: the sky is too clouded to sight stars.</p>}
      {lastBonus !== undefined && lastBonus > 0 && <p role="status">Discovery bonus: +{lastBonus} Stellar Dust</p>}

      {navigation && (
        <div aria-label="Navigation aid" style={{ padding: 8, borderRadius: 6, background: '#1E293B', margin: '12px 0' }}>
          <strong>{navigation.name || `Constellation #${navigation.constellationId}`}</strong> recognized (
          {navigation.confidencePct}% match). {headingText(navigation.heading, navigation.distance, navigation.anchorRegion)}
        </div>
      )}

      <ul style={{ listStyle: 'none', padding: 0, display: 'grid', gap: 8 }}>
        {entries.map((e) => (
          <li
            key={e.constellationId}
            data-testid={`constellation-${e.constellationId}`}
            style={{ padding: 8, borderRadius: 6, border: '1px solid #334155', opacity: e.sighted ? 1 : 0.55 }}
          >
            <strong>{e.sighted ? e.name || `#${e.constellationId}` : 'Unknown pattern'}</strong>
            <span style={{ color: '#94A3B8', fontSize: '0.75rem' }}> · {e.starCount} stars</span>
            {e.sighted && e.lore && <p style={{ margin: '4px 0 0', fontSize: '0.8rem', color: '#CBD5E1' }}>{e.lore}</p>}
          </li>
        ))}
      </ul>
    </section>
  );
};
