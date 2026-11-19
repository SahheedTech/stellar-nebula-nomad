import React from 'react';
import { fireEvent, render, screen } from '@testing-library/react';
import { HousingView, HouseShowcaseView } from '../views/HousingView';
import { displaySlots } from '../utils/housing';

describe('housing rule mirrors', () => {
  test('display slots follow cases and house size', () => {
    expect(displaySlots('Small', 0)).toBe(0);
    expect(displaySlots('Small', 2)).toBe(3);
    expect(displaySlots('Large', 3)).toBe(9);
  });
});

describe('HousingView', () => {
  const showcase: HouseShowcaseView = {
    owner: 'GOWNER',
    size: 'Small',
    access: 'Public',
    wallTheme: 1,
    floorTheme: 2,
    lightingTheme: 3,
    totalVisits: 12,
    averageRating: 450,
    placements: [
      { furnitureId: 7, name: 'Trophy Case', category: 'Achievement', x: 10, y: 10, rotation: 0, layer: 0 },
      { furnitureId: 8, name: 'Lamp', category: 'Lighting', x: 20, y: 10, rotation: 90, layer: 1 },
    ],
    displays: [{ slot: 1, achievementId: 1, title: 'First Scan' }],
    banner: { allianceName: 'Nomads' },
    storage: { capacity: 10, used: 4, expansions: 0, nextExpansionCost: 1000 },
  };

  test('owner sees display slots, banner, storage and controls', () => {
    const onTheme = jest.fn();
    const onExpand = jest.fn();
    render(<HousingView showcase={showcase} isOwner onThemeChange={onTheme} onExpandStorage={onExpand} />);
    expect(screen.getByText('4.5 ★', { exact: false })).toBeDefined();
    expect(screen.getByLabelText('Guild banner').textContent).toContain('Nomads');
    expect(screen.getByTestId('display-slot-1').textContent).toContain('First Scan');
    expect(screen.getByTestId('display-slot-0').textContent).toBe('Empty');
    expect(screen.queryByTestId('display-slot-3')).toBeNull();
    expect(screen.getByTestId('storage').textContent).toContain('4/10');
    fireEvent.change(screen.getByLabelText('Walls'), { target: { value: '5' } });
    expect(onTheme).toHaveBeenCalledWith(5, 2, 3);
    fireEvent.click(screen.getByText('Expand storage (1000 dust)'));
    expect(onExpand).toHaveBeenCalled();
    expect((screen.getByLabelText('Walls') as HTMLSelectElement).options.length).toBe(5);
  });

  test('visitors can visit and rate', () => {
    const onRate = jest.fn();
    const onVisit = jest.fn();
    render(<HousingView showcase={showcase} isOwner={false} onVisit={onVisit} onRate={onRate} />);
    fireEvent.click(screen.getByText('Visit'));
    fireEvent.click(screen.getByLabelText('Rate 4 stars'));
    expect(onVisit).toHaveBeenCalled();
    expect(onRate).toHaveBeenCalledWith(400);
    expect(screen.queryByText(/Expand storage/)).toBeNull();
  });
});
