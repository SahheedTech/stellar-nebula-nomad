import React from 'react';
import { fireEvent, render, screen } from '@testing-library/react';
import { WormholeNavigator } from '../views/WormholeNavigator';
import { jumpRisk, outcomeOdds, worstCaseEnergy } from '../utils/wormholes';

describe('wormhole rule mirrors', () => {
  test('jump risk matches the contract formula and cap', () => {
    expect(jumpRisk(100, 0)).toBe(0);
    expect(jumpRisk(60, 0)).toBe(20);
    expect(jumpRisk(60, 95)).toBe(39);
    expect(jumpRisk(0, 100)).toBe(70);
    expect(outcomeOdds(30)).toEqual({ EmergencyExit: 10, Turbulent: 20, Clean: 70 });
    expect(worstCaseEnergy(100)).toBe(125);
  });
});

describe('WormholeNavigator', () => {
  const map = [
    { wormholeId: 1, originNebula: 4, destination: 9, stability: 90, risk: 5, travelCost: 60, isActive: true, expiresAt: 9999 },
    { wormholeId: 2, originNebula: 7, destination: 4, stability: 40, risk: 30, travelCost: 40, isActive: true, expiresAt: 9999 },
    { wormholeId: 3, originNebula: 4, destination: 2, stability: 0, risk: 50, travelCost: 30, isActive: false, expiresAt: 9999 },
  ];

  test('lets the pilot jump only through open routes from the current nebula', () => {
    const onJump = jest.fn();
    render(
      <WormholeNavigator
        currentNebula={4}
        map={map}
        shipEnergy={100}
        now={0}
        onScan={jest.fn()}
        onJump={onJump}
        onEmergencyExit={jest.fn()}
      />,
    );
    const buttons = screen.getAllByText('Jump') as HTMLButtonElement[];
    expect(buttons.map((b) => b.disabled)).toEqual([false, true, true]);
    fireEvent.click(buttons[0]);
    expect(onJump).toHaveBeenCalledWith(1);
    expect(screen.getByText('High (30%)')).toBeDefined();
    expect(screen.getByText('Low (5%)')).toBeDefined();
  });

  test('offers the emergency exit only inside the window', () => {
    const onExit = jest.fn();
    const lastJump = { wormholeId: 1, outcome: 'Clean' as const, location: 9, traveledAt: 1000, energyConsumed: 60 };
    const { rerender } = render(
      <WormholeNavigator
        currentNebula={9}
        map={map}
        shipEnergy={40}
        lastJump={lastJump}
        now={1100}
        onScan={jest.fn()}
        onJump={jest.fn()}
        onEmergencyExit={onExit}
      />,
    );
    fireEvent.click(screen.getByText('Emergency exit'));
    expect(onExit).toHaveBeenCalled();
    rerender(
      <WormholeNavigator
        currentNebula={9}
        map={map}
        shipEnergy={40}
        lastJump={lastJump}
        now={1400}
        onScan={jest.fn()}
        onJump={jest.fn()}
        onEmergencyExit={onExit}
      />,
    );
    expect(screen.queryByText('Emergency exit')).toBeNull();
  });
});
