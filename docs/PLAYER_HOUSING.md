# Player Housing

Issues: #532 (initial), #629 (completion). Module: `src/player_housing.rs`. UI: `frontend/src/views/HousingView.tsx`.

## Houses and customization

- `initialize_house(owner)` creates a Small, public house. The owner needs a player profile.
- `upgrade_house` moves to Medium (5,000 dust), then Large (15,000 dust).
- `customize_house` / `customize_theme` sets wall, floor and lighting themes:

| Size | Themes | Furniture placed | Display slots (max) | Base storage |
| --- | ---: | ---: | ---: | ---: |
| Small | 1–5 | 50 | 3 | 10 |
| Medium | 1–10 | 100 | 6 | 20 |
| Large | 1–20 | 200 | 12 | 40 |

## Furniture

- **Lifecycle:** `define_furniture` (catalog) → `purchase_furniture` (debits the cost, adds the item to storage) → `place_furniture` (takes it out of storage) → `remove_furniture` (back into storage).
- **Ownership:** placing requires an unplaced copy you own (`NotOwned`).
- **Positions:** two items can't share an `(x, y, layer)` position (`PositionOccupied`).
- **Bounds:** coordinates are 0–999, and rotation is 0, 90, 180 or 270.

## Storage expansion

- Capacity = base by size + 10 per expansion + 5 per placed Storage-category item. Purchases and removals need a free slot (`StorageFull`).
- `expand_house_storage` / `expand_storage` adds 10 slots. Expansion *n* costs `1,000 × n` dust, up to 5 expansions (`StorageMaxed`).

## Achievement display cases

- Each placed Achievement-category item provides 3 display slots, up to the size maximum.
- `display_house_achievement(owner, achievement_id, slot)` shows an achievement the owner has unlocked (`AchievementNotEarned` otherwise). It replaces whatever the slot held.
- Removing a display case drops the slots it provided.

## Guild banners

- `hang_guild_banner(owner)` displays the owner's alliance name (`NotInGuild` if the owner isn't in one).
- The banner hides itself once the owner leaves that alliance.

## Visitors

- **Access modes:** **Public**, **Private** (owner only) or **FriendsOnly**.
- **Friends:** the owner, players on the guest list (`add_house_guest`, up to 50), and members of the owner's guild.
- **Visitor log:** keeps the latest 50 visits and drops the oldest.
- **Ratings:** visitors must visit before rating (`MustVisitFirst`). Ratings are 100–500, one per visitor, and the visit is marked as rated.

## Showcase

`get_house_showcase(owner)` returns everything a visitor sees in one read: the house, the furniture layout, the achievement displays, the guild banner and the storage summary. It feeds `HousingView`.

## New errors (`HousingError`)

`InvalidTheme` 18, `StorageFull` 19, `NotOwned` 20, `StorageMaxed` 21, `AchievementNotEarned` 22, `NoDisplayCase` 23, `NotInGuild` 24, `MustVisitFirst` 25, `GuestListFull` 26.

## Compatibility

Furniture placed before this change stays placed. Removing it now returns it to storage, so the owner can place it again.
