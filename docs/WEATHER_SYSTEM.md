# Weather System

Issues: #553 (initial), #630 (completion). Module: `src/weather_system.rs`. UI: `frontend/src/views/WeatherPanel.tsx`.

## Weather types and effects

Each type has base modifiers, in percent. The effective value is `base × intensity / 100`, clamped to −90%…+300%, so weather never zeroes out or explodes a value.

| Type | Visibility/scan | Speed | Fuel | Combat | Yield | XP | Danger | Hull wear/h |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Ion Storm | −50 | −5 | 0 | −20 | +200 | 0 | 55 | 1 |
| Solar Flare | +20 | 0 | +10 | +15 | +100 | 0 | 70 | 2 |
| Nebula Density Shift | −10 | 0 | 0 | −15 | −10 | 0 | 25 | 0 |
| Gravitational Anomaly | 0 | +20 | −10 | 0 | 0 | 0 | 20 | 0 |
| Cosmic Radiation Burst | +15 | 0 | 0 | +10 | +50 | +50 | 60 | 3 |
| Meteor Shower | +30 | −10 | 0 | −10 | +80 | 0 | 80 | 4 |
| Stellar Wind | −5 | −30 | +25 | 0 | −20 | 0 | 30 | 0 |
| Black Hole Proximity | −20 | −15 | +40 | +25 | +150 | 0 | 95 | 5 |
| Wormhole Instability | +10 | +25 | 0 | 0 | 0 | 0 | 50 | 0 |
| Aurora | +25 | 0 | 0 | +5 | +30 | +15 | 10 | 0 |
| Perfect Conditions | +50 | +10 | −10 | +10 | +100 | +25 | 0 | 0 |

Perfect Conditions has no negatives (lower fuel use is a bonus).

## How weather is chosen

- Time is split into 4-hour slots (`WEATHER_SLOT_SECS`), six per day.
- A region's weather for a slot is a weighted pick from its **climate**: one weight per type. `DEFAULT_CLIMATE` sums to 200 and gives Perfect Conditions weight 1, so it appears about once a month.
- The pick and the intensity (20–100) come from `sha256("weather" ‖ region ‖ slot ‖ season salt)`. The result is deterministic, and nobody can choose it by timing a transaction.
- `get_current_weather` realizes the current slot the first time it is read, stores it, and appends it to the day's history. Weather stays fixed for the rest of the slot.

## Forecast

`generate_forecast` returns the next six slots (24 hours). Because the weather is deterministic, each entry is exactly what will be realized for that slot, unless an admin changes the climate (`set_region_climate`) or the season salt (`set_season`) first. That is what the confidence expresses: 95% for the next slot, falling by 10 points per slot (minimum 30).

## Integration helpers

Other systems apply weather through these helpers:

| Helper | Use |
| --- | --- |
| `nebula_visibility(region)` | 10–100%. Used by wormhole scans and constellation sightings |
| `effective_scan_range` | Scanner range |
| `effective_harvest_yield` | Resource yields |
| `effective_xp` | Experience |
| `effective_fuel_cost` | Energy cost; used by wormhole travel |
| `effective_travel_time` | Travel time (faster ships take less) |
| `effective_combat_power` | Combat outcomes |
| `get_weather_impact(region).danger_level` | Wormhole jump risk |

## Contract entry points

| Entry point | Notes |
| --- | --- |
| `get_region_weather(region_id)` | Current `WeatherState` (realizes a new slot if needed) |
| `get_weather_forecast(region_id)` | Six `WeatherForecast` entries |
| `get_weather_effects(region_id)` | `WeatherImpact`, already scaled and clamped |
| `set_region_climate(admin, region_id, weights)` | Admin only (emergency-controls admin set). 11 weights, sum 1–10,000 |

Errors (`WeatherError`): `InvalidRegion` 1, `NoActiveWeather` 2, `InvalidIntensity` 3, `ForecastError` 4, `Unauthorized` 5, `InvalidClimate` 6.

## Balance notes

- Every effect is clamped to −90%…+300% at any intensity; a unit test checks all 11 types at intensities 1–100.
- An intensity-20 storm now has a small effect. Previously the integer factor `intensity / 50` gave zero effect below intensity 50 and double effect at 100.
