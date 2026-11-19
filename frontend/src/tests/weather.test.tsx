import React from 'react';
import { render, screen } from '@testing-library/react';
import { WeatherPanel } from '../views/WeatherPanel';
import { WeatherImpact, effectTone, formatModifier, visibilityPct } from '../utils/weather';

const calm: WeatherImpact = {
  scanModifier: 0,
  harvestModifier: 0,
  xpModifier: 0,
  dangerLevel: 0,
  speedModifier: 0,
  fuelModifier: 0,
  combatModifier: 0,
  hullWear: 0,
};

describe('weather rule mirrors', () => {
  test('weather helpers format and colour effects', () => {
    expect(formatModifier(25)).toBe('+25%');
    expect(formatModifier(-30)).toBe('-30%');
    expect(formatModifier(0)).toBe('±0%');
    expect(effectTone('fuelModifier', 20)).toBe('bad');
    expect(effectTone('speedModifier', 20)).toBe('good');
    expect(visibilityPct({ ...calm, scanModifier: -50 })).toBe(50);
    expect(visibilityPct({ ...calm, scanModifier: 40 })).toBe(100);
  });
});

describe('WeatherPanel', () => {
  test('shows current weather, effects and forecast', () => {
    render(
      <WeatherPanel
        now={0}
        current={{ regionId: 3, weather: 'IonStorm', intensity: 80, startedAt: 0, endsAt: 4 * 3600 }}
        impact={{ ...calm, scanModifier: -40, harvestModifier: 160, fuelModifier: 10 }}
        forecast={[
          { weather: 'StellarWind', intensity: 50, startsAt: 4 * 3600, confidence: 95 },
          { weather: 'AuroraPhenomenon', intensity: 30, startsAt: 8 * 3600, confidence: 85 },
        ]}
      />,
    );
    expect(screen.getByText('Ion Storm')).toBeDefined();
    expect(screen.getByTestId('visibility').textContent).toBe('60%');
    expect(screen.getByTestId('effect-harvestModifier').textContent).toBe('+160%');
    expect(screen.getByText('95% sure')).toBeDefined();
    expect(screen.getByText('Aurora')).toBeDefined();
  });
});
