import React from 'react';
import { render, screen } from '@testing-library/react';
import { ConstellationAtlas } from '../views/ConstellationAtlas';
import { headingText } from '../utils/constellations';

describe('constellation rule mirrors', () => {
  test('heading text', () => {
    expect(headingText('Ascending', 1, 7)).toBe('Region 7 lies 1 step ahead');
    expect(headingText('Arrived', 0, 7)).toBe('You are in region 7');
  });
});

describe('ConstellationAtlas', () => {
  const entries = [
    { constellationId: 0, name: 'The Lantern', lore: 'Lit by the first nomads.', anchorRegion: 9, starCount: 5, sighted: true },
    { constellationId: 1, name: 'Hidden', lore: 'Secret lore', anchorRegion: 2, starCount: 3, sighted: false },
  ];

  test('shows collection progress, lore only for sighted patterns and navigation', () => {
    render(
      <ConstellationAtlas
        entries={entries}
        totalConstellations={2}
        visibility={90}
        lastBonus={100}
        navigation={{ constellationId: 0, name: 'The Lantern', confidencePct: 80, heading: 'Ascending', distance: 3, anchorRegion: 9 }}
        onSight={jest.fn()}
      />,
    );
    expect(screen.getByTestId('collection-progress').textContent).toContain('1 / 2 collected (50%)');
    expect(screen.getByText('Lit by the first nomads.')).toBeDefined();
    expect(screen.queryByText('Secret lore')).toBeNull();
    expect(screen.getByText('Unknown pattern')).toBeDefined();
    expect(screen.getByText(/Region 9 lies 3 steps ahead/)).toBeDefined();
    expect(screen.getByText('Discovery bonus: +100 Stellar Dust')).toBeDefined();
  });

  test('disables sighting when the sky is obscured', () => {
    render(<ConstellationAtlas entries={entries} totalConstellations={2} visibility={50} onSight={jest.fn()} />);
    expect((screen.getByText('Sight constellation') as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByRole('alert').textContent).toContain('Visibility 50%');
  });
});
