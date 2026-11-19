import React from 'react';
import {
  WEATHER_LABELS,
  WeatherForecast,
  WeatherImpact,
  WeatherState,
  effectTone,
  formatModifier,
  visibilityPct,
} from '../utils/weather';

export interface WeatherPanelProps {
  current: WeatherState;
  impact: WeatherImpact;
  forecast: WeatherForecast[];
  /** Ledger time in seconds, used for "ends in". */
  now: number;
}

const EFFECTS: Array<[keyof WeatherImpact, string]> = [
  ['scanModifier', 'Visibility / scans'],
  ['speedModifier', 'Ship speed'],
  ['fuelModifier', 'Fuel use'],
  ['combatModifier', 'Combat power'],
  ['harvestModifier', 'Resource yield'],
  ['xpModifier', 'Experience'],
];

const TONE_COLOURS = { good: '#10B981', bad: '#F87171', neutral: '#94A3B8' };

function hoursUntil(ts: number, now: number): string {
  const hours = Math.max(0, Math.round((ts - now) / 3600));
  return hours === 0 ? '<1h' : `${hours}h`;
}

/** Current regional weather, its gameplay effects and the 24-hour forecast (#630). */
export const WeatherPanel: React.FC<WeatherPanelProps> = ({ current, impact, forecast, now }) => {
  const meta = WEATHER_LABELS[current.weather];
  return (
    <section aria-label="Weather" style={{ padding: 16, borderRadius: 8, background: '#0F172A', color: '#F8FAFC' }}>
      <header style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
        <span style={{ fontSize: '2rem' }} aria-hidden>
          {meta.icon}
        </span>
        <div>
          <h2 style={{ margin: 0, fontSize: '1.1rem' }}>{meta.label}</h2>
          <span style={{ color: '#94A3B8', fontSize: '0.8rem' }}>
            Region {current.regionId} · intensity {current.intensity} · ends in {hoursUntil(current.endsAt, now)}
          </span>
        </div>
      </header>

      <dl style={{ display: 'grid', gridTemplateColumns: '1fr auto', gap: '4px 12px', margin: '12px 0' }}>
        <dt>Nebula visibility</dt>
        <dd data-testid="visibility" style={{ margin: 0 }}>
          {visibilityPct(impact)}%
        </dd>
        {EFFECTS.map(([key, label]) => (
          <React.Fragment key={key}>
            <dt>{label}</dt>
            <dd data-testid={`effect-${key}`} style={{ margin: 0, color: TONE_COLOURS[effectTone(key, impact[key])] }}>
              {formatModifier(impact[key])}
            </dd>
          </React.Fragment>
        ))}
        <dt>Danger</dt>
        <dd style={{ margin: 0 }}>{impact.dangerLevel}/100</dd>
        <dt>Hull wear</dt>
        <dd style={{ margin: 0 }}>{impact.hullWear}%/h</dd>
      </dl>

      <h3 style={{ fontSize: '0.9rem', margin: '8px 0' }}>Next 24 hours</h3>
      <ol aria-label="Forecast" style={{ listStyle: 'none', display: 'flex', gap: 8, padding: 0, margin: 0 }}>
        {forecast.map((slot) => (
          <li
            key={slot.startsAt}
            title={`${WEATHER_LABELS[slot.weather].label}, intensity ${slot.intensity}`}
            style={{ textAlign: 'center', fontSize: '0.75rem', color: '#CBD5E1' }}
          >
            <div aria-hidden style={{ fontSize: '1.25rem' }}>
              {WEATHER_LABELS[slot.weather].icon}
            </div>
            <div>{WEATHER_LABELS[slot.weather].label}</div>
            <div>in {hoursUntil(slot.startsAt, now)}</div>
            <div style={{ color: '#64748B' }}>{slot.confidence}% sure</div>
          </li>
        ))}
      </ol>
    </section>
  );
};
