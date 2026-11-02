//! Which graphics card each per-GPU metric belongs to. Metrics, history and
//! cells number cards by slot. The sampler owns this table and sends it with
//! every snapshot, so nothing else has to list the adapters and risk numbering
//! them differently.

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::metric::{MetricKey, Snapshot};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuAdapter {
    pub name: String,
    pub luid: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuSlot {
    /// Told apart from identical cards: the second "RTX 4090" is "RTX 4090 #2".
    pub name: String,
    pub luid: u64,
    pub present: bool,
    model: String,
    /// Changes when another LUID takes the slot.
    generation: u32,
}

/// A card keeps its slot and name while the process runs, even while absent.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GpuSlots(Vec<GpuSlot>);

impl GpuSlots {
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    pub fn listing(adapters: &[GpuAdapter]) -> Self {
        let mut slots = Self::new();
        slots.update(adapters);
        slots
    }

    /// Brings the table up to date with what the system lists now. An adapter
    /// with an unknown LUID takes the first absent slot of its model, since a
    /// driver reset or a replug gives a card a new LUID; otherwise a new slot.
    pub fn update(&mut self, adapters: &[GpuAdapter]) {
        for slot in &mut self.0 {
            slot.present = adapters.iter().any(|a| a.luid == slot.luid);
        }
        for adapter in adapters {
            if self.0.iter().any(|s| s.luid == adapter.luid) {
                continue;
            }
            if let Some(slot) = self
                .0
                .iter_mut()
                .find(|s| !s.present && s.model == adapter.name)
            {
                slot.luid = adapter.luid;
                slot.present = true;
                slot.generation = slot.generation.wrapping_add(1);
            } else if self.0.len() <= usize::from(u8::MAX) {
                let same = self.0.iter().filter(|s| s.model == adapter.name).count();
                self.0.push(GpuSlot {
                    name: match same {
                        0 => adapter.name.clone(),
                        n => format!("{} #{}", adapter.name, n + 1),
                    },
                    luid: adapter.luid,
                    present: true,
                    model: adapter.name.clone(),
                    generation: 0,
                });
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn present(&self) -> impl Iterator<Item = (u8, &GpuSlot)> {
        (0..=u8::MAX).zip(&self.0).filter(|(_, s)| s.present)
    }

    /// The slot of a card that is there now.
    pub fn slot_of(&self, luid: u64) -> Option<u8> {
        self.present()
            .find(|(_, s)| s.luid == luid)
            .map(|(slot, _)| slot)
    }

    /// The slot of a card, by the name saved in the settings, that is there now.
    pub fn slot_named(&self, name: &str) -> Option<u8> {
        self.present()
            .find(|(_, s)| s.name == name)
            .map(|(slot, _)| slot)
    }

    /// A card's name in this table, whether or not it is there now.
    pub fn name_of(&self, luid: u64) -> Option<&str> {
        self.0
            .iter()
            .find(|s| s.luid == luid)
            .map(|s| s.name.as_str())
    }

    pub fn present_luids(&self) -> BTreeSet<u64> {
        self.present().map(|(_, s)| s.luid).collect()
    }

    /// Slots that hold a different card in `newer`, whose history is not this card's.
    pub fn replaced_by(&self, newer: &GpuSlots) -> Vec<u8> {
        (0..=u8::MAX)
            .zip(self.0.iter().zip(&newer.0))
            .filter(|(_, (old, new))| old.generation != new.generation)
            .map(|(slot, _)| slot)
            .collect()
    }

    /// Files readings under the slot of the card they came from. Only cards
    /// there now get a reading.
    pub fn publish(&self, readings: &GpuReadings, out: &mut Snapshot) {
        for (slot, card) in self.present() {
            if let Some(load) = &readings.load
                && readings.seen.contains(&card.luid)
            {
                let busy = load.get(&card.luid).copied().unwrap_or(0.0);
                out.set(MetricKey::GpuAdapterUtil(slot), busy);
            }
            let mut set = |key: MetricKey, values: &HashMap<u64, f64>| {
                if let Some(&value) = values.get(&card.luid) {
                    out.set(key, value);
                }
            };
            set(
                MetricKey::GpuAdapterDedicatedBytes(slot),
                &readings.dedicated,
            );
            set(MetricKey::GpuAdapterSharedBytes(slot), &readings.shared);
            set(MetricKey::GpuAdapterTempC(slot), &readings.temp_c);
            set(
                MetricKey::GpuAdapterMemoryPercent(slot),
                &readings.memory_percent,
            );
        }
    }
}

/// What the sources read this tick, by LUID, for the sampler to file by slot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GpuReadings {
    pub seen: BTreeSet<u64>,
    /// `None` when the engines weren't read. A seen adapter missing from the
    /// map has no process on it, so it is idle.
    pub load: Option<HashMap<u64, f64>>,
    pub dedicated: HashMap<u64, f64>,
    pub shared: HashMap<u64, f64>,
    pub temp_c: HashMap<u64, f64>,
    pub memory_percent: HashMap<u64, f64>,
}

#[cfg(test)]
pub(crate) fn adapter(name: &str, luid: u64) -> GpuAdapter {
    GpuAdapter {
        name: name.into(),
        luid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(slots: &GpuSlots) -> Vec<(&str, bool)> {
        slots
            .0
            .iter()
            .map(|s| (s.name.as_str(), s.present))
            .collect()
    }

    #[test]
    fn identical_cards_are_numbered_and_found_by_luid() {
        let slots = GpuSlots::listing(&[
            adapter("RTX 4090", 1),
            adapter("Radeon", 2),
            adapter("RTX 4090", 3),
            adapter("RTX 4090", 4),
        ]);
        assert_eq!(
            names(&slots),
            [
                ("RTX 4090", true),
                ("Radeon", true),
                ("RTX 4090 #2", true),
                ("RTX 4090 #3", true)
            ]
        );
        assert_eq!(slots.slot_of(3), Some(2));
        assert_eq!(slots.slot_named("RTX 4090 #3"), Some(3));
    }

    #[test]
    fn a_card_keeps_its_number_when_an_identical_one_goes() {
        let mut slots = GpuSlots::listing(&[adapter("RTX 4090", 1), adapter("RTX 4090", 2)]);
        slots.update(&[adapter("RTX 4090", 2)]);
        assert_eq!(
            names(&slots),
            [("RTX 4090", false), ("RTX 4090 #2", true)],
            "the numbering doesn't shift"
        );
        assert_eq!(slots.slot_of(1), None);
        assert_eq!(slots.slot_of(2), Some(1));
        assert_eq!(slots.slot_named("RTX 4090"), None);
        assert_eq!(slots.slot_named("RTX 4090 #2"), Some(1));
    }

    #[test]
    fn a_replug_under_the_same_luid_is_the_same_card() {
        let both = [adapter("RTX 4090", 1), adapter("Radeon", 2)];
        let before = GpuSlots::listing(&both);
        let mut slots = before.clone();
        slots.update(&both[1..]);
        slots.update(&both);
        assert_eq!(slots, before);
        assert!(before.replaced_by(&slots).is_empty());
    }

    #[test]
    fn a_new_luid_takes_the_absent_slot_of_its_model_and_counts_as_replaced() {
        let before = GpuSlots::listing(&[adapter("RTX 4090", 1), adapter("Radeon", 2)]);
        let mut slots = before.clone();
        slots.update(&[adapter("Radeon", 2)]);
        slots.update(&[adapter("Radeon", 2), adapter("RTX 4090", 9)]);
        assert_eq!(slots.slot_of(9), Some(0));
        assert_eq!(slots.slot_of(1), None);
        assert_eq!(names(&slots), [("RTX 4090", true), ("Radeon", true)]);
        assert_eq!(before.replaced_by(&slots), [0]);
    }

    #[test]
    fn a_new_luid_does_not_take_the_slot_of_a_card_still_there() {
        let mut slots = GpuSlots::listing(&[adapter("RTX 4090", 1), adapter("RTX 4090", 2)]);
        slots.update(&[adapter("RTX 4090", 3), adapter("RTX 4090", 2)]);
        assert_eq!(slots.slot_of(2), Some(1));
        assert_eq!(slots.slot_of(3), Some(0));
        slots.update(&[
            adapter("RTX 4090", 4),
            adapter("RTX 4090", 3),
            adapter("RTX 4090", 2),
        ]);
        assert_eq!(slots.slot_of(4), Some(2), "a third card gets its own");
        assert_eq!(slots.slot_named("RTX 4090 #3"), Some(2));
    }

    #[test]
    fn a_different_card_gets_a_new_slot() {
        let mut slots = GpuSlots::listing(&[adapter("RTX 4090", 1)]);
        slots.update(&[adapter("RTX 5090", 2)]);
        assert_eq!(names(&slots), [("RTX 4090", false), ("RTX 5090", true)]);
    }

    #[test]
    fn listing_order_does_not_move_a_card() {
        let mut slots = GpuSlots::listing(&[adapter("RTX 4090", 1), adapter("Radeon", 2)]);
        slots.update(&[adapter("Radeon", 2), adapter("RTX 4090", 1)]);
        assert_eq!(slots.slot_of(1), Some(0));
        assert_eq!(slots.slot_of(2), Some(1));
    }

    #[test]
    fn readings_go_to_the_card_they_came_from_and_only_if_it_is_there() {
        let mut slots = GpuSlots::listing(&[adapter("RTX 4090", 1), adapter("Radeon", 2)]);
        slots.update(&[adapter("Radeon", 2)]);
        let readings = GpuReadings {
            seen: BTreeSet::from([1, 2]),
            load: Some(HashMap::from([(1, 90.0)])),
            dedicated: HashMap::from([(1, 8.0), (2, 2.0)]),
            shared: HashMap::from([(2, 3.0)]),
            temp_c: HashMap::from([(1, 80.0), (2, 40.0)]),
            memory_percent: HashMap::from([(1, 75.0), (2, 25.0)]),
        };
        let mut s = Snapshot::default();
        slots.publish(&readings, &mut s);
        for key in [
            MetricKey::GpuAdapterUtil(0),
            MetricKey::GpuAdapterMemoryPercent(0),
            MetricKey::GpuAdapterDedicatedBytes(0),
            MetricKey::GpuAdapterTempC(0),
        ] {
            assert_eq!(s.get(key), None, "{key:?}");
        }
        assert_eq!(s.get(MetricKey::GpuAdapterUtil(1)), Some(0.0), "idle");
        assert_eq!(s.get(MetricKey::GpuAdapterDedicatedBytes(1)), Some(2.0));
        assert_eq!(s.get(MetricKey::GpuAdapterSharedBytes(1)), Some(3.0));
        assert_eq!(s.get(MetricKey::GpuAdapterTempC(1)), Some(40.0));
        assert_eq!(s.get(MetricKey::GpuAdapterMemoryPercent(1)), Some(25.0));
    }

    #[test]
    fn no_engine_reading_is_not_reported_as_idle() {
        let slots = GpuSlots::listing(&[adapter("RTX 4090", 1)]);
        let mut s = Snapshot::default();
        let readings = GpuReadings {
            seen: BTreeSet::from([1]),
            ..GpuReadings::default()
        };
        slots.publish(&readings, &mut s);
        assert_eq!(s.get(MetricKey::GpuAdapterUtil(0)), None);
    }

    #[test]
    fn a_card_the_sources_cannot_see_is_not_reported_as_idle() {
        let slots = GpuSlots::listing(&[adapter("RTX 4090", 1)]);
        let mut s = Snapshot::default();
        let readings = GpuReadings {
            seen: BTreeSet::from([2]),
            load: Some(HashMap::new()),
            ..GpuReadings::default()
        };
        slots.publish(&readings, &mut s);
        assert_eq!(s.get(MetricKey::GpuAdapterUtil(0)), None);
    }

    #[test]
    fn the_table_survives_json() {
        let mut slots = GpuSlots::listing(&[adapter("RTX 4090", u64::MAX), adapter("RTX 4090", 2)]);
        slots.update(&[adapter("RTX 4090", 2)]);
        let text = serde_json::to_string(&slots).unwrap();
        assert_eq!(serde_json::from_str::<GpuSlots>(&text).unwrap(), slots);
    }
}
