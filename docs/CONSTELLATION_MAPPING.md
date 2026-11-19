# Constellation Mapping

Issues: #72 (initial), #632 (completion). Module: `src/constellation_mapper.rs`. UI: `frontend/src/views/ConstellationAtlas.tsx`. Lore: `docs/lore/CONSTELLATIONS.md`.

## Recording and lore

- `record_constellation(recorder, stars)` stores a pattern of at least 3 star hashes. Records are immutable.
- `annotate_constellation(recorder, id, name, anchor_region, lore)` names the pattern, anchors it to a region and attaches its story.
  - Only the recorder may do this, and only once.
  - The name must be 1–32 bytes and the lore at most 512 bytes.

## Pattern recognition

`recognize_constellation(observed)` compares the observation with the 50 most recently recorded constellations.
- Star order doesn't matter, and extra (noise) stars are fine.
- The observation must contain at least **60%** of a constellation's stars.
- The most confident match wins; ties go to the larger constellation.
- The result includes `confidence_pct`.

The original `match_constellation` (any overlap among the last 15) is unchanged.

## Navigation aids

`get_navigation_aid(observed, current_region)` recognizes the pattern and points toward its anchor region:
- **Arrived**, **Ascending** or **Descending**, plus the number of region steps;
- **Unknown** if the pattern hasn't been annotated yet.

## Sightings, bonuses and collection

`sight_constellation(observer, observed, region_id)`:

- **Weather:** the region's weather visibility must be at least 70% (`SkyObscured` otherwise). A strong ion storm hides the sky.
- **First sighting:** the constellation joins the player's collection, and the player earns **50 Stellar Dust**, or **100** if no one else has sighted it yet.
- **Milestones:** reaching 5, 10 and 25 sightings pays 200, 500 and 1,500 dust, once each.
- **Repeats:** later sightings of the same constellation pay nothing.
- **Recorders:** sighting your own constellation adds it to your collection but pays nothing and doesn't count toward milestones. So recording patterns can't farm rewards directly. A recorder could still sight their patterns from a second account; the per-pair once-only rule limits that to 100 dust per pattern.

`get_constellation_collection(player)` returns the sighted IDs, the count, the total recorded, the completion percentage and the next unpaid milestone.

## Errors (`ConstellationError`)

`NoMatchFound` 1, `TooFewStars` 2, `ImmutableRecord` 3, `BurstTooLarge` 4, `SkyObscured` 5, `Unauthorized` 6, `AlreadyAnnotated` 7, `InvalidMetadata` 8, `CollectionFull` 9, `RewardFailed` 10.
