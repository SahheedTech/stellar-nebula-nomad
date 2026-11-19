# Wormhole Travel

Issues: #77 (initial), #631 (completion). Module: `src/wormhole_traveler.rs`. UI: `frontend/src/views/WormholeNavigator.tsx`.

## Lifecycle

1. **Open:** `open_wormhole(creator, origin, destination)`. It lives for 1 hour, with at most 5 active at once.
   - Base cost is `50 + 2 × distance` energy.
   - It starts at stability 100. If the origin's weather is Wormhole Instability, it starts at `100 − intensity / 2`.
2. **Discover:** `scan_for_wormholes(explorer, nebula)` finds active wormholes starting or ending in that nebula.
   - Each wormhole has a detection signature (0–89). A scan only sees it when the nebula's weather visibility is above the signature, so storms hide faint wormholes.
   - Everything found is added to the explorer's **map** (up to 100 entries).
   - The first explorer other than the creator to find a wormhole is recorded as its discoverer.
3. **Jump:** `traverse_wormhole` / `jump_through_wormhole(traveler, ship, wormhole)`.
4. **Collapse:** when a jump takes stability to 0, the wormhole closes and leaves the active list.

## Stability

`stability = base − 10 × jumps − 2 × (minutes since opening / 10)`, and 0 once collapsed or expired.

## Cost, risk and reward

- **Cost:** `base cost × (60 + 40 × stability / 100) / 100`, then the origin's weather fuel modifier. An unstable wormhole is up to 40% cheaper.
- **Risk:** `(100 − stability) / 2 + origin weather danger / 5`, capped at 70%.
- **Roll:** each jump rolls 0–99 from `sha256(ship ‖ wormhole ‖ jumps ‖ ledger sequence ‖ timestamp)`.

| Roll | Outcome | Energy | Ship ends in |
| --- | --- | --- | --- |
| `< risk / 3`, or the jump collapses the wormhole | **Emergency exit** | 50% of cost | Origin |
| `< risk` | **Turbulent** | cost + 25% | Destination |
| otherwise | **Clean** | cost | Destination |

A fresh wormhole in calm weather has 0% risk, so its jumps are always clean at the base cost.

## Emergency exits

- **Automatic:** a severe failure, or a collapse mid-jump, throws the ship back to the origin. It pays half the cost.
- **Manual:** `wormhole_emergency_exit(traveler, ship)` takes the ship back through the wormhole it just used. It must be within 5 minutes of arriving and while the wormhole is open, and it costs half the current travel cost.

## Location

Each jump records the ship's nebula (`get_ship_location`). A ship whose location is known can only jump from the wormhole's origin (`ShipNotAtOrigin`). Ships that haven't travelled since this change may jump from anywhere, as before.

## Contract entry points

Existing: `open_wormhole`, `traverse_wormhole`, `get_wormhole`, `get_active_wormholes`, `get_travel_history`, `cleanup_expired_wormholes`, `calculate_travel_cost`, `verify_wormhole_link`.

New:

| Entry point | Returns |
| --- | --- |
| `jump_through_wormhole(traveler, ship_id, wormhole_id)` | `TravelOutcome` (outcome, risk, roll, stability after, collapsed, location) |
| `wormhole_emergency_exit(traveler, ship_id)` | `TravelOutcome` |
| `scan_for_wormholes(explorer, nebula_id)` | Detected IDs |
| `get_wormhole_map(explorer)` | `WormholeMapEntry` per discovered wormhole (live stability, risk, cost) |
| `get_wormhole_routes(nebula_id)` | Open routes leaving a nebula |

New errors (`WormholeError`): `ShipNotAtOrigin` 11, `NoRecentJump` 12, `ExitWindowClosed` 13.

## Storage

Stability lives under `WormholeKey::Stability(id)`, separate from the `Wormhole` record, so wormholes stored before this change still decode. They are treated as stability 100 with no jumps.
