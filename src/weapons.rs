//! Experimental iconic weapon creation for the verified 277/2500 save layout.
//!
//! TweakDB presets supply the native parts; inventory identity and StatsSystem
//! entries are created together. Story facts and equipped slots are never edited.
use crate::{
    binary::{put32, string, vlq, Reader},
    package::{Class, Package},
    save::{fnv64, tdb, Item, Save},
};
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use std::{collections::HashSet, sync::OnceLock};

pub const PLAYER_INVENTORY: u64 = 1;
pub const STASH: u64 = 1_000_000;
pub const APARTMENT_STASH: u64 = 0xE5F556FCBB62A706;
pub fn is_stash(inventory: u64) -> bool {
    matches!(inventory, STASH | APARTMENT_STASH)
}
pub fn is_owned_inventory(inventory: u64) -> bool {
    inventory == PLAYER_INVENTORY || is_stash(inventory)
}

/// Initial upgrade stage. Quality and +/++ follow the same native counter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WeaponQuality {
    #[default]
    PlayerLevel,
    Tier1,
    Tier5PlusPlus,
}
impl WeaponQuality {
    pub const ALL: [Self; 3] = [Self::PlayerLevel, Self::Tier1, Self::Tier5PlusPlus];
    pub fn upgrade_count(self, level: u32) -> u32 {
        match self {
            Self::Tier1 => 0,
            Self::Tier5PlusPlus => 10,
            // 2.5 quality_curves.iconic_level_to_random_range_mult_new:
            // level 0/9/17/25/33 -> quality 0/1/2/3/4 (constant interpolation).
            Self::PlayerLevel => match level {
                0..=8 => 0,
                9..=16 => 2,
                17..=24 => 4,
                25..=32 => 6,
                _ => 8,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct WeaponPart {
    pub record: u64,
    pub slot: u64,
}
#[derive(Debug, Deserialize)]
pub struct Weapon {
    pub key: String,
    pub name: String,
    pub name_en: String,
    pub record: String,
    pub kind: String,
    pub description: String,
    pub description_en: String,
    pub source: String,
    pub source_en: String,
    pub aliases: Vec<u64>,
    pub group: String,
    pub limit: usize,
    pub image: String,
    pub quest_sensitive: bool,
    pub dual_wield: bool,
    pub parts: Vec<WeaponPart>,
}
impl Weapon {
    pub fn matches(&self, id: u64) -> bool {
        id == tdb(&self.record) || self.aliases.contains(&id)
    }
    pub fn owned(&self, items: &[Item], inventory: Option<u64>) -> usize {
        items
            .iter()
            .filter(|i| {
                is_owned_inventory(i.inventory)
                    && inventory.is_none_or(|inv| {
                        if inv == STASH {
                            is_stash(i.inventory)
                        } else {
                            inv == i.inventory
                        }
                    })
                    && self.matches(i.id)
            })
            .count()
    }
    pub fn group_owned(&self, items: &[Item]) -> usize {
        // Count each physical entry once, even if aliases overlap.
        items
            .iter()
            .filter(|i| {
                is_owned_inventory(i.inventory)
                    && catalog()
                        .iter()
                        .any(|w| w.group == self.group && w.matches(i.id))
            })
            .count()
    }
}
pub fn catalog() -> &'static [Weapon] {
    static CATALOG: OnceLock<Vec<Weapon>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/iconic_weapons.json"))
            .expect("Valid iconic weapon catalog")
    })
}

/// The engine's item identity hash (TweakDBID, random seed, unique counter).
pub fn item_hash(id: u64, seed: u32, unique: u16) -> u64 {
    let c = 0xc6a4a7935bd1e995u64;
    let mut t;
    if unique == 0 {
        t = (seed as u64).wrapping_mul(c);
        t ^= t >> 47;
    } else {
        t = (unique as u64).wrapping_mul(c);
        t = ((t ^ (t >> 47)).wrapping_mul(c) ^ seed as u64).wrapping_mul(0x35a98f4d286a90b9);
        t ^= t >> 47;
    }
    (t.wrapping_mul(c) ^ id).wrapping_mul(c)
}
fn header(id: u64, seed: u32) -> Vec<u8> {
    let mut bytes = id.to_le_bytes().to_vec();
    bytes.extend(seed.to_le_bytes());
    bytes.push(1); // Extended, non-stackable item.
    bytes.extend([0; 3]); // UniqueCounter=0; fresh, globally unused seed identifies it.
    bytes
}
fn part_bytes(part: &WeaponPart, seed: u32, children: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = header(part.record, seed);
    bytes.extend(string("None"));
    bytes.extend(part.slot.to_le_bytes());
    bytes.extend(vlq(children.len() as i32));
    for child in children {
        bytes.extend(child);
    }
    bytes.extend(0u32.to_le_bytes());
    bytes.extend(0u64.to_le_bytes()); // Loot pool.
    bytes.extend(0u32.to_le_bytes());
    bytes.extend(f32::MAX.to_le_bytes()); // Native part default, not an equip level.
    bytes
}
fn constant(name: &str, value: f32) -> Vec<u8> {
    let mut bytes = string("gameConstantStatModifier");
    bytes.extend(fnv64(name).to_le_bytes());
    bytes.extend(0u32.to_le_bytes()); // Additive.
    bytes.extend(value.to_le_bytes());
    bytes
}
fn indices(values: &[u16]) -> Vec<u8> {
    let mut bytes = (values.len() as u32).to_le_bytes().to_vec();
    for v in values {
        bytes.extend(v.to_le_bytes());
    }
    bytes
}
fn append_u64(pool: &mut Vec<u8>, value: u64) -> Result<u16> {
    let mut r = Reader::new(pool, 0);
    let count = r.u32()? as usize;
    ensure!(pool.len() == 4 + count * 8, "Invalid stats record pool");
    for index in 0..count {
        if r.u64()? == value {
            return Ok(index as u16);
        }
    }
    ensure!(count < u16::MAX as usize, "Stats record pool full");
    pool.extend(value.to_le_bytes());
    put32(pool, 0, (count + 1) as u32)?;
    Ok(count as u16)
}

impl Save {
    /// Add exactly one experimental weapon to V. This method enforces the same
    /// ownership and variant limits as the UI, and commits only on full success.
    pub fn add_iconic_weapon(&mut self, key: &str) -> Result<()> {
        self.add_iconic_weapon_with_quality(key, WeaponQuality::PlayerLevel)
    }

    pub fn add_iconic_weapon_with_quality(
        &mut self,
        key: &str,
        quality: WeaponQuality,
    ) -> Result<()> {
        ensure!(
            self.version == 277 && self.game == 2500,
            "Only save 277/game 2500 is supported"
        );
        let weapon = catalog()
            .iter()
            .find(|w| w.key == key)
            .context("Unknown weapon")?;
        let items = self.items()?;
        ensure!(
            weapon.group_owned(&items) < weapon.limit,
            "Weapon limit reached across inventory and stash"
        );
        ensure!(!weapon.parts.is_empty(), "Weapon parts missing");
        let mut candidate = self.clone();
        let mut p = Package::from_save(&candidate, "StatsSystem")?;
        p.ensure_names(
            &[
                "entityHash",
                "Uint64",
                "seed",
                "Uint32",
                "recordIDIndex",
                "Uint16",
                "idType",
                "gameStatIDType",
                "ItemID",
                "bufferSliceIndices",
                "array:Uint16",
                "inactiveStatsIndices",
                "offset",
                "combined",
                "LootLevel",
                "IsItemPlus",
                "RandomCurveInput",
            ]
            .iter()
            .map(|s| (*s).to_owned())
            .collect::<Vec<_>>(),
        )?;
        let (root, mut c) = p.chunk("gameStatsStateMapStructure")?;
        let mut rows = p.array(
            &c.field(&p, "valuesOptimized")
                .context("Stats rows missing")?
                .data,
        )?;
        let mut slices = p.array(
            &c.field(&p, "sharedBufferSlices")
                .context("Stats slices missing")?
                .data,
        )?;
        let mut buffer = c
            .field(&p, "sharedDataBuffer")
            .context("Stats buffer missing")?
            .data
            .clone();
        ensure!(
            buffer.len() == Reader::new(&buffer, 0).u32()? as usize + 4,
            "Invalid stats buffer length"
        );
        let mut records = c
            .field(&p, "sharedRecordId")
            .context("Stats records missing")?
            .data
            .clone();
        let record_index = append_u64(&mut records, tdb(&weapon.record))?;
        let mut inactive = c
            .field(&p, "sharedInactiveStats")
            .context("Stats enum pool missing")?
            .data
            .clone();
        let mut inactive_reader = Reader::new(&inactive, 0);
        let count = inactive_reader.u32()? as usize;
        ensure!(
            inactive.len() == 4 + count * 2,
            "Invalid inactive stat pool"
        );
        let mut inactive_ids = (0..count)
            .map(|_| inactive_reader.u16())
            .collect::<Result<Vec<_>>>()?;
        let mut inactive_indices = Vec::new();
        for name in ["LootLevel", "IsItemPlus", "RandomCurveInput"] {
            let id = p.name_id(name)?;
            let index = match inactive_ids.iter().position(|v| *v == id) {
                Some(i) => i,
                None => {
                    inactive_ids.push(id);
                    inactive_ids.len() - 1
                }
            };
            ensure!(index < u16::MAX as usize, "Inactive stat pool full");
            inactive_indices.push(index as u16);
        }
        inactive = indices(&inactive_ids);
        let mut used_seeds = HashSet::new();
        let mut used_hashes = HashSet::new();
        for row in &rows {
            if let Some(f) = row.field(&p, "seed") {
                used_seeds.insert(Reader::new(&f.data, 0).u32()?);
            }
            if let Some(f) = row.field(&p, "entityHash") {
                used_hashes.insert(Reader::new(&f.data, 0).u64()?);
            }
        }
        // Include part identities from every item (also identities with no stats row).
        for item in &items {
            let n = self
                .nodes
                .iter()
                .find(|n| n.offset == item.offset)
                .context("Item node missing")?;
            let (_, seeds) = validate_item(&self.data[n.offset + 4..n.offset + n.size])?;
            used_seeds.extend(seeds);
        }
        let mut next_seed = 3u32;
        let mut fresh = |record: u64| -> Result<u32> {
            loop {
                let seed = next_seed;
                next_seed = next_seed
                    .checked_add(1)
                    .context("Item seed space exhausted")?;
                let hash = item_hash(record, seed, 0);
                if !used_seeds.contains(&seed) && !used_hashes.contains(&hash) {
                    used_seeds.insert(seed);
                    used_hashes.insert(hash);
                    return Ok(seed);
                }
            }
        };
        let seed = fresh(tdb(&weapon.record))?;
        let mut parts = Vec::new();
        for part in &weapon.parts {
            parts.push((part, fresh(part.record)?));
        }
        let children = parts[1..]
            .iter()
            .map(|(part, seed)| part_bytes(part, *seed, &[]))
            .collect::<Vec<_>>();
        let identity = header(tdb(&weapon.record), seed);
        let mut payload = identity.clone();
        payload.push(0); // No injected quest flag.
        payload.extend(0u32.to_le_bytes()); // Creation time.
        payload.extend([0; 16]); // Additional info: no loot pool, no level restriction.
        payload.extend(part_bytes(parts[0].0, parts[0].1, &children));
        validate_item(&payload)?;
        let level = self
            .player_stats()?
            .into_iter()
            .find(|s| s.key == "Level")
            .context("Player level missing")?
            .value;
        let mut modifiers = constant("LootLevel", level.min(60) as f32);
        // Suppress the independent level-based quality roll. Otherwise an item
        // can appear Tier 5 while WasItemUpgraded=0 still charges Tier 1 costs.
        // All bundled presets have the native upgrade-count -> Quality curve.
        // Inactive + saved RCI=0 keeps that curve as the sole quality source,
        // including after an in-game upgrade; do not pin Quality to a constant.
        modifiers.extend(constant("RandomCurveInput", 0.));
        modifiers.extend(constant(
            "WasItemUpgraded",
            quality.upgrade_count(level) as f32,
        ));
        // Keep the native iconic upgrade curve so future upgrades can produce +/++.
        modifiers.extend(string("gameCurveStatModifier"));
        modifiers.extend(fnv64("IsItemPlus").to_le_bytes());
        modifiers.extend(0u32.to_le_bytes());
        modifiers.extend(string("quality_curves"));
        modifiers.extend(string("iconic_upgrades_amount_to_plus"));
        modifiers.extend(fnv64("WasItemUpgraded").to_le_bytes());
        modifiers.extend(0u32.to_le_bytes());
        ensure!(slices.len() < u16::MAX as usize, "Stats slice pool full");
        let slice_index = slices.len() as u16;
        let mut slice = Class { fields: Vec::new() };
        slice.set(
            &p,
            "offset",
            "Uint32",
            ((buffer.len() - 4) as u32).to_le_bytes().to_vec(),
        )?;
        slice.set(
            &p,
            "combined",
            "Uint32",
            (modifiers.len() as u32).to_le_bytes().to_vec(),
        )?;
        slices.push(slice);
        buffer.extend(modifiers);
        let buffer_len = (buffer.len() - 4) as u32;
        put32(&mut buffer, 0, buffer_len)?;
        let mut row = Class { fields: Vec::new() };
        row.set(
            &p,
            "entityHash",
            "Uint64",
            item_hash(tdb(&weapon.record), seed, 0)
                .to_le_bytes()
                .to_vec(),
        )?;
        row.set(&p, "seed", "Uint32", seed.to_le_bytes().to_vec())?;
        row.set(
            &p,
            "recordIDIndex",
            "Uint16",
            record_index.to_le_bytes().to_vec(),
        )?;
        row.set(
            &p,
            "idType",
            "gameStatIDType",
            p.name_id("ItemID")?.to_le_bytes().to_vec(),
        )?;
        row.set(
            &p,
            "bufferSliceIndices",
            "array:Uint16",
            indices(&[slice_index]),
        )?;
        row.set(
            &p,
            "inactiveStatsIndices",
            "array:Uint16",
            indices(&inactive_indices),
        )?;
        rows.push(row);
        c.set(
            &p,
            "valuesOptimized",
            "array:gameSavedStatsDataOptimized",
            Package::array_bytes(&rows),
        )?;
        c.set(
            &p,
            "sharedBufferSlices",
            "array:gameBufferSlice",
            Package::array_bytes(&slices),
        )?;
        c.set(&p, "sharedRecordId", "array:TweakDBID", records)?;
        c.set(
            &p,
            "sharedInactiveStats",
            "array:gamedataStatType",
            inactive,
        )?;
        c.set(&p, "sharedDataBuffer", "DataBuffer", buffer)?;
        p.replace_chunk(root, c)?;
        p.write(&mut candidate, "StatsSystem")?;
        candidate.insert_inventory_item(PLAYER_INVENTORY, &identity, &payload)?;
        *self = candidate;
        Ok(())
    }
}

/// Validate the full known inventory item shape, including nested attachment IDs.
/// Unknown/trailing data stops addition rather than risking identity collisions.
fn validate_item(bytes: &[u8]) -> Result<(u64, Vec<u32>)> {
    let mut r = Reader::new(bytes, 0);
    let mut seeds = Vec::new();
    let id = r.u64()?;
    let seed = r.u32()?;
    seeds.push(seed);
    let structure = r.u8()?;
    ensure!(structure <= 3, "Unknown item structure");
    r.bytes(3)?;
    r.bytes(5)?;
    if structure & 2 != 0 || structure == 0 && seed == 2 {
        r.u32()?;
    }
    if structure & 1 != 0 || structure == 0 && seed != 2 {
        r.bytes(16)?;
        read_part(&mut r, &mut seeds, 0)?;
    }
    ensure!(r.pos == bytes.len(), "Unsupported item data trailer");
    Ok((id, seeds))
}
fn read_part(r: &mut Reader<'_>, seeds: &mut Vec<u32>, depth: usize) -> Result<()> {
    ensure!(depth < 32, "Item attachment nesting too deep");
    r.u64()?;
    seeds.push(r.u32()?);
    r.bytes(4)?;
    r.string()?;
    r.u64()?;
    let count = r.vlq()?;
    ensure!((0..1000).contains(&count), "Invalid attachment count");
    for _ in 0..count {
        read_part(r, seeds, depth + 1)?;
    }
    r.bytes(20)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Save {
        Save::load(std::path::Path::new("saveExample")).unwrap()
    }
    fn without_catalog_ownership(mut s: Save) -> Save {
        // Simulate a save with no owned iconics without deleting any item nodes.
        for item in s.items().unwrap() {
            if is_owned_inventory(item.inventory) && catalog().iter().any(|w| w.matches(item.id)) {
                for offset in [item.offset - 16, item.offset + 4] {
                    s.data[offset..offset + 8]
                        .copy_from_slice(&tdb("Items.test_unowned").to_le_bytes());
                }
            }
        }
        s
    }
    fn stats(s: &Save) -> (Package, Class) {
        let p = Package::from_save(s, "StatsSystem").unwrap();
        let (_, c) = p.chunk("gameStatsStateMapStructure").unwrap();
        (p, c)
    }
    #[test]
    fn all_catalog_weapons_roundtrip_and_preserve_existing_data() {
        if !std::path::Path::new("saveExample/sav.dat").exists() {
            return;
        }
        let original = without_catalog_ownership(sample());
        assert_eq!(catalog().len(), 60);
        let mut edited = original.clone();
        let (p, c) = stats(&original);
        let before_rows = p
            .array(&c.field(&p, "valuesOptimized").unwrap().data)
            .unwrap();
        for w in catalog() {
            if w.key == "mq007_skippy_post_quest" {
                let mut alternate = original.clone();
                alternate.add_iconic_weapon(&w.key).unwrap();
                let alternate = Save::parse(alternate.encode().unwrap()).unwrap();
                assert_eq!(w.owned(&alternate.items().unwrap(), None), 1);
                continue;
            }
            assert!(!w.parts.is_empty(), "{}", w.record);
            assert_eq!(w.limit, if w.dual_wield { 2 } else { 1 });
            edited
                .add_iconic_weapon(&w.key)
                .unwrap_or_else(|e| panic!("{}: {e:#}", w.record));
        }
        let reopened = Save::parse(edited.encode().unwrap()).unwrap();
        assert_eq!(
            reopened.items().unwrap().len(),
            original.items().unwrap().len() + 59
        );
        for w in catalog() {
            assert_eq!(
                w.owned(&reopened.items().unwrap(), None),
                if w.key == "mq007_skippy_post_quest" {
                    0
                } else {
                    1
                },
                "{}",
                w.record
            );
        }
        let old_items = original.items().unwrap();
        let new_items = reopened.items().unwrap();
        for inv in old_items
            .iter()
            .map(|i| i.inventory)
            .collect::<HashSet<_>>()
        {
            for (a, b) in old_items
                .iter()
                .filter(|i| i.inventory == inv)
                .zip(new_items.iter().filter(|i| i.inventory == inv))
            {
                let an = original
                    .nodes
                    .iter()
                    .find(|n| n.offset == a.offset)
                    .unwrap();
                let bn = reopened
                    .nodes
                    .iter()
                    .find(|n| n.offset == b.offset)
                    .unwrap();
                assert_eq!(
                    original.data[an.offset + 4..an.offset + an.size],
                    reopened.data[bn.offset + 4..bn.offset + bn.size]
                );
            }
        }
        let (q, d) = stats(&reopened);
        let rows = q
            .array(&d.field(&q, "valuesOptimized").unwrap().data)
            .unwrap();
        assert_eq!(rows.len(), before_rows.len() + 59);
        for (a, b) in before_rows.iter().zip(&rows) {
            assert_eq!(a.encode(), b.encode());
        }
        // All leaf node payloads except the stats package and new items remain exact.
        for (n, other) in original
            .nodes
            .iter()
            .filter(|n| n.name != "itemData")
            .zip(reopened.nodes.iter().filter(|n| n.name != "itemData"))
        {
            assert_eq!(n.name, other.name);
            if n.child == u32::MAX && n.name != "StatsSystem" {
                assert_eq!(
                    original.data[n.offset + 4..n.offset + n.size],
                    reopened.data[other.offset + 4..other.offset + other.size],
                    "{}",
                    n.name
                );
            }
        }
        for field in ["sharedDataBuffer", "sharedRecordId", "sharedInactiveStats"] {
            let before = &c.field(&p, field).unwrap().data;
            let after = &d.field(&q, field).unwrap().data;
            assert_eq!(
                &before[4..],
                &after[4..before.len()],
                "Changed existing {field}"
            );
        }
        let old_slices = p
            .array(&c.field(&p, "sharedBufferSlices").unwrap().data)
            .unwrap();
        let new_slices = q
            .array(&d.field(&q, "sharedBufferSlices").unwrap().data)
            .unwrap();
        for (a, b) in old_slices.iter().zip(new_slices) {
            assert_eq!(a.encode(), b.encode());
        }
        let hashes = rows
            .iter()
            .filter_map(|row| row.field(&q, "entityHash"))
            .map(|f| Reader::new(&f.data, 0).u64().unwrap())
            .collect::<Vec<_>>();
        for item in reopened
            .items()
            .unwrap()
            .iter()
            .filter(|i| catalog().iter().any(|w| tdb(&w.record) == i.id))
        {
            let seed = Reader::new(&reopened.data, item.offset + 12).u32().unwrap();
            assert_eq!(
                hashes
                    .iter()
                    .filter(|h| **h == item_hash(item.id, seed, 0))
                    .count(),
                1
            );
        }
    }
    #[test]
    fn limits_include_stash_aliases_and_skippy_variants_and_fail_atomically() {
        if !std::path::Path::new("saveExample/sav.dat").exists() {
            return;
        }
        let mut s = without_catalog_ownership(sample());
        s.add_iconic_weapon("Preset_Liberty_Yorinobu").unwrap();
        // Move first copy into the stash with the shared insertion primitive;
        // mask the original to simulate V depositing it.
        let item = s
            .items()
            .unwrap()
            .into_iter()
            .find(|i| i.id == tdb("Items.Preset_Liberty_Yorinobu"))
            .unwrap();
        let n = s.nodes.iter().find(|n| n.offset == item.offset).unwrap();
        let payload = s.data[n.offset + 4..n.offset + n.size].to_vec();
        s.insert_inventory_item(STASH, &payload[..16], &payload)
            .unwrap();
        let item = s
            .items()
            .unwrap()
            .into_iter()
            .find(|i| i.id == tdb("Items.Preset_Liberty_Yorinobu") && i.inventory == 1)
            .unwrap();
        for offset in [item.offset - 16, item.offset + 4] {
            s.data[offset..offset + 8].copy_from_slice(&tdb("Items.test_unowned").to_le_bytes());
        }
        s.add_iconic_weapon("Preset_Liberty_Yorinobu").unwrap();
        let w = catalog()
            .iter()
            .find(|w| w.key == "Preset_Liberty_Yorinobu")
            .unwrap();
        assert_eq!(w.group_owned(&s.items().unwrap()), 2);
        let before = s.data.clone();
        assert!(s.add_iconic_weapon(&w.key).is_err());
        assert_eq!(s.data, before);
        s.add_iconic_weapon("Preset_Katana_Saburo").unwrap();
        let before = s.data.clone();
        assert!(s.add_iconic_weapon("Preset_Katana_Saburo").is_err());
        assert_eq!(s.data, before);
        s.add_iconic_weapon("mq007_skippy").unwrap();
        let before = s.data.clone();
        assert!(s.add_iconic_weapon("mq007_skippy_post_quest").is_err());
        assert_eq!(s.data, before);
        assert!(s.add_iconic_weapon("not_a_weapon").is_err());
        assert_eq!(s.data, before);
        Save::parse(s.encode().unwrap()).unwrap();
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    #[test]
    fn stats_name_pool_can_grow_and_malformed_stats_fail_without_mutation() {
        if !std::path::Path::new("saveExample/sav.dat").exists() {
            return;
        }
        let mut save = Save::load(std::path::Path::new("saveExample")).unwrap();
        let mut p = Package::from_save(&save, "StatsSystem").unwrap();
        let chunks = p
            .chunks
            .iter()
            .map(|(_, a, b)| p.bytes[*a..*b].to_vec())
            .collect::<Vec<_>>();
        p.ensure_names(&["experimental_weapon_test_name".into()])
            .unwrap();
        for (old, (_, a, b)) in chunks.iter().zip(&p.chunks) {
            assert_eq!(old, &p.bytes[*a..*b]);
        }
        p.write(&mut save, "StatsSystem").unwrap();
        Save::parse(save.encode().unwrap()).unwrap();
        save.add_iconic_weapon("Preset_Liberty_Yorinobu").unwrap();
        let mut p = Package::from_save(&save, "StatsSystem").unwrap();
        let (root, mut c) = p.chunk("gameStatsStateMapStructure").unwrap();
        let id = p.name_id("sharedDataBuffer").unwrap();
        c.fields.retain(|f| f.name != id);
        p.replace_chunk(root, c).unwrap();
        p.write(&mut save, "StatsSystem").unwrap();
        let before = save.data.clone();
        let nodes = save.nodes.clone();
        assert!(save.add_iconic_weapon("Preset_Liberty_Yorinobu").is_err());
        assert_eq!(save.data, before);
        assert_eq!(save.nodes, nodes);
    }
}

#[cfg(test)]
mod quality_tests {
    use super::*;
    use std::collections::HashMap;
    fn native_curves() -> HashMap<String, Vec<[f32; 2]>> {
        serde_json::from_str(include_str!("../assets/weapon_quality_curves_25.json")).unwrap()
    }
    fn constant_curve(points: &[[f32; 2]], input: f32) -> f32 {
        points
            .iter()
            .rev()
            .find(|p| input >= p[0])
            .unwrap_or(&points[0])[1]
    }
    #[test]
    fn initial_stage_and_all_upgrades_match_native_25_quality_curves() {
        let curves = native_curves();
        let level_curve = &curves["iconic_level_to_random_range_mult_new"];
        let quality_curve = &curves["iconic_upgrades_amount_to_quality"];
        let plus_curve = &curves["iconic_upgrades_amount_to_plus"];
        // No random roll at RCI=0: quality comes only from the upgrade counter.
        assert_eq!(constant_curve(&curves["quality_distribution_new"], 0.), 0.);
        for level in 1..=100 {
            let rank = constant_curve(level_curve, level as f32);
            let stage = WeaponQuality::PlayerLevel.upgrade_count(level);
            assert_eq!(constant_curve(quality_curve, stage as f32), rank);
            assert_eq!(constant_curve(plus_curve, stage as f32), 0.);
            assert_eq!(WeaponQuality::Tier1.upgrade_count(level), 0);
            assert_eq!(WeaponQuality::Tier5PlusPlus.upgrade_count(level), 10);
        }
        let stages = [
            (0., 0.),
            (0., 1.),
            (1., 0.),
            (1., 1.),
            (2., 0.),
            (2., 1.),
            (3., 0.),
            (3., 1.),
            (4., 0.),
            (4., 1.),
            (4., 2.),
        ];
        for (stage, (tier, plus)) in stages.into_iter().enumerate() {
            assert_eq!(constant_curve(quality_curve, stage as f32), tier);
            assert_eq!(constant_curve(plus_curve, stage as f32), plus);
        }
        // Regression for the user's level-39 Pride: Tier 5 must start at stage 8,
        // not stage 0 (which previously charged components for Tier 1).
        assert_eq!(WeaponQuality::PlayerLevel.upgrade_count(39), 8);
    }
    #[test]
    fn all_spawn_modes_serialize_counter_and_disable_independent_quality_roll() {
        if !std::path::Path::new("saveExample/sav.dat").exists() {
            return;
        }
        let original = Save::load(std::path::Path::new("saveExample")).unwrap();
        for level in [1, 27, 39, 60] {
            for quality in WeaponQuality::ALL {
                let mut save = original.clone();
                save.set_player_stat("Progression", "Level", level).unwrap();
                save.add_iconic_weapon_with_quality("Preset_Liberty_Yorinobu", quality)
                    .unwrap();
                let save = Save::parse(save.encode().unwrap()).unwrap();
                let p = Package::from_save(&save, "StatsSystem").unwrap();
                let (_, c) = p.chunk("gameStatsStateMapStructure").unwrap();
                let rows = p
                    .array(&c.field(&p, "valuesOptimized").unwrap().data)
                    .unwrap();
                let row = rows.last().unwrap();
                let pool = &c.field(&p, "sharedInactiveStats").unwrap().data;
                let mut r = Reader::new(&row.field(&p, "inactiveStatsIndices").unwrap().data, 0);
                let n = r.u32().unwrap();
                let mut names = Vec::new();
                for _ in 0..n {
                    let index = r.u16().unwrap() as usize;
                    let name = Reader::new(pool, 4 + 2 * index).u16().unwrap();
                    names.push(p.names[name as usize].as_str());
                }
                assert!(names.contains(&"RandomCurveInput"));
                assert!(names.contains(&"IsItemPlus"));
                assert!(
                    !names.contains(&"Quality"),
                    "Quality must follow its live upgrade curve"
                );
                let slices = p
                    .array(&c.field(&p, "sharedBufferSlices").unwrap().data)
                    .unwrap();
                let mut r = Reader::new(&row.field(&p, "bufferSliceIndices").unwrap().data, 0);
                assert_eq!(r.u32().unwrap(), 1);
                let slice = &slices[r.u16().unwrap() as usize];
                let combined = slice.integer(&p, "combined").unwrap();
                assert_eq!(combined >> 30, 0);
                let start = slice.integer(&p, "offset").unwrap() as usize + 4;
                let end = start + combined as usize;
                let mut r = Reader::new(&c.field(&p, "sharedDataBuffer").unwrap().data, start);
                let mut constants = HashMap::new();
                for _ in 0..3 {
                    assert_eq!(r.string().unwrap(), "gameConstantStatModifier");
                    let stat = r.u64().unwrap();
                    assert_eq!(r.u32().unwrap(), 0);
                    constants.insert(stat, f32::from_bits(r.u32().unwrap()));
                }
                assert_eq!(constants[&fnv64("LootLevel")], level as f32);
                assert_eq!(constants[&fnv64("RandomCurveInput")], 0.);
                assert_eq!(
                    constants[&fnv64("WasItemUpgraded")],
                    quality.upgrade_count(level) as f32
                );
                assert_eq!(r.string().unwrap(), "gameCurveStatModifier");
                assert_eq!(r.u64().unwrap(), fnv64("IsItemPlus"));
                assert_eq!(r.u32().unwrap(), 0);
                assert_eq!(r.string().unwrap(), "quality_curves");
                assert_eq!(r.string().unwrap(), "iconic_upgrades_amount_to_plus");
                assert_eq!(r.u64().unwrap(), fnv64("WasItemUpgraded"));
                assert_eq!(r.u32().unwrap(), 0);
                assert_eq!(r.pos, end);
            }
        }
    }
}
