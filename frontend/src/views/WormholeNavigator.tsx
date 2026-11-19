import React from 'react';
import {
  EMERGENCY_EXIT_WINDOW_SECS,
  JumpOutcome,
  WormholeMapEntry,
  outcomeOdds,
  riskLabel,
  worstCaseEnergy,
} from '../utils/wormholes';

export interface LastJump {
  wormholeId: number;
  outcome: JumpOutcome;
  location: number;
  traveledAt: number;
  energyConsumed: number;
}

export interface WormholeNavigatorProps {
  currentNebula: number;
  /** The explorer's discovered wormholes (`get_wormhole_map`). */
  map: WormholeMapEntry[];
  shipEnergy: number;
  lastJump?: LastJump;
  /** Ledger time in seconds. */
  now: number;
  onScan: () => void;
  onJump: (wormholeId: number) => void;
  onEmergencyExit: () => void;
}

const OUTCOME_TEXT: Record<JumpOutcome, string> = {
  Clean: 'Clean jump',
  Turbulent: 'Turbulent jump (surcharge paid)',
  EmergencyExit: 'Emergency exit: thrown back to the origin',
};

/** Wormhole discovery, map, risk preview, jumps and emergency exit (#631). */
export const WormholeNavigator: React.FC<WormholeNavigatorProps> = ({
  currentNebula,
  map,
  shipEnergy,
  lastJump,
  now,
  onScan,
  onJump,
  onEmergencyExit,
}) => {
  const exitOpen =
    lastJump !== undefined &&
    lastJump.outcome !== 'EmergencyExit' &&
    now <= lastJump.traveledAt + EMERGENCY_EXIT_WINDOW_SECS;

  return (
    <section aria-label="Wormhole navigator" style={{ padding: 16, borderRadius: 8, background: '#0F172A', color: '#F8FAFC' }}>
      <header style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <h2 style={{ margin: 0, fontSize: '1.1rem' }}>Wormholes near nebula {currentNebula}</h2>
        <button type="button" onClick={onScan}>
          Scan for wormholes
        </button>
      </header>
      <p style={{ color: '#94A3B8', fontSize: '0.8rem' }}>Energy: {shipEnergy}</p>

      {lastJump && (
        <div role="status" style={{ padding: 8, borderRadius: 6, background: '#1E293B', marginBottom: 12 }}>
          {OUTCOME_TEXT[lastJump.outcome]} · now in nebula {lastJump.location} · {lastJump.energyConsumed} energy
          {exitOpen && (
            <button type="button" onClick={onEmergencyExit} style={{ marginLeft: 8 }}>
              Emergency exit
            </button>
          )}
        </div>
      )}

      {map.length === 0 ? (
        <p>No wormholes charted yet. Scan a nebula to discover them.</p>
      ) : (
        <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: '0.85rem' }}>
          <thead>
            <tr>
              <th align="left">Route</th>
              <th align="left">Stability</th>
              <th align="left">Risk</th>
              <th align="left">Energy</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {map.map((w) => {
              const odds = outcomeOdds(w.risk);
              const fromHere = w.originNebula === currentNebula;
              const affordable = shipEnergy >= w.travelCost;
              const canJump = w.isActive && fromHere && affordable;
              return (
                <tr key={w.wormholeId} data-testid={`wormhole-${w.wormholeId}`} style={{ opacity: w.isActive ? 1 : 0.5 }}>
                  <td>
                    {w.originNebula} → {w.destination}
                  </td>
                  <td>
                    <meter min={0} max={100} value={w.stability} aria-label="stability" /> {w.stability}
                  </td>
                  <td title={`clean ${odds.Clean}% · turbulent ${odds.Turbulent}% · exit ${odds.EmergencyExit}%`}>
                    {riskLabel(w.risk)} ({w.risk}%)
                  </td>
                  <td>
                    {w.travelCost}–{worstCaseEnergy(w.travelCost)}
                  </td>
                  <td>
                    <button
                      type="button"
                      disabled={!canJump}
                      onClick={() => onJump(w.wormholeId)}
                      title={
                        !w.isActive
                          ? 'Collapsed'
                          : !fromHere
                            ? `Travel to nebula ${w.originNebula} first`
                            : !affordable
                              ? 'Not enough energy'
                              : 'Jump'
                      }
                    >
                      Jump
                    </button>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
    </section>
  );
};
