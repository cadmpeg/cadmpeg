// SPDX-License-Identifier: Apache-2.0
//! Ordered value parts retain their original positions when a part is absent.

use super::ValueRecord;
use serde::{Deserialize, Serialize};

/// Held value records and the complete number of source positions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ValueSlots {
    held: Vec<ValueRecord>,
    slots: usize,
}

impl ValueSlots {
    pub(crate) const EMPTY: Self = Self {
        held: Vec::new(),
        slots: 0,
    };

    pub(crate) fn empty_ref() -> &'static Self {
        static EMPTY: ValueSlots = ValueSlots::EMPTY;
        &EMPTY
    }

    /// Admits records whose ordinals name their original source positions.
    pub(crate) fn new(held: Vec<ValueRecord>, slots: usize) -> Result<Self, String> {
        let value = Self { held, slots };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), String> {
        let mut seen = std::collections::BTreeSet::new();
        let mut previous = None;
        for value in &self.held {
            if value.order >= self.slots || previous.is_some_and(|order| order >= value.order) {
                return Err("value ordinals must increase within their source slots".into());
            }
            if value.parent.is_some_and(|parent| !seen.contains(&parent)) {
                return Err("value parent must name an earlier held value slot".into());
            }
            if let Some(index) = value.link_index {
                let parent = value
                    .parent
                    .and_then(|parent| self.held.iter().find(|value| value.order == parent));
                if !parent.is_some_and(|parent| {
                    matches!(
                        (value.tag.as_str(), parent.tag.as_str()),
                        ("XLink", "XLinks" | "XLinkSubList") | ("Link", "LinkList" | "LinkSubList")
                    ) && parent
                        .attributes
                        .get("count")
                        .and_then(|text| text.parse::<usize>().ok())
                        .is_some_and(|count| index < count)
                }) {
                    return Err(
                        "link_index must name a position inside its registered reference collection".into(),
                    );
                }
                if self
                    .held
                    .iter()
                    .take_while(|peer| peer.order < value.order)
                    .any(|peer| peer.parent == value.parent && peer.link_index == Some(index))
                {
                    return Err(
                        "reference source ordinals must be unique within their parent".into(),
                    );
                }
            }
            if let Some(index) = value.item_index {
                let parent = value
                    .parent
                    .and_then(|order| self.held.iter().find(|row| row.order == order));
                if value.tag != "Enum"
                    || !parent.is_some_and(|parent| {
                        parent.tag == "CustomEnumList"
                            && parent
                                .attributes
                                .get("count")
                                .and_then(|text| text.parse::<usize>().ok())
                                .is_some_and(|count| index < count)
                    })
                {
                    return Err(
                        "item_index must name an Enum position inside CustomEnumList".into(),
                    );
                }
                if self
                    .held
                    .iter()
                    .take_while(|peer| peer.order < value.order)
                    .any(|peer| peer.parent == value.parent && peer.item_index == Some(index))
                {
                    return Err("Enum item ordinals must be unique within their parent".into());
                }
            }
            seen.insert(value.order);
            previous = Some(value.order);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn clear(&mut self) {
        self.held.clear();
        self.slots = 0;
    }

    pub(crate) fn slot_count(&self) -> usize {
        self.slots
    }

    pub(crate) fn has_missing(&self) -> bool {
        self.slots != self.held.len()
    }

    /// Removing a held part preserves its position as an explicit null.
    pub(crate) fn retain_mut(&mut self, predicate: impl FnMut(&mut ValueRecord) -> bool) {
        self.held.retain_mut(predicate);
    }
}

impl From<Vec<ValueRecord>> for ValueSlots {
    fn from(held: Vec<ValueRecord>) -> Self {
        let slots = held.last().map_or(0, |value| value.order + 1);
        Self { held, slots }
    }
}

impl std::ops::Deref for ValueSlots {
    type Target = [ValueRecord];
    fn deref(&self) -> &Self::Target {
        &self.held
    }
}

impl std::ops::DerefMut for ValueSlots {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.held
    }
}

impl<'a> IntoIterator for &'a ValueSlots {
    type Item = &'a ValueRecord;
    type IntoIter = std::slice::Iter<'a, ValueRecord>;
    fn into_iter(self) -> Self::IntoIter {
        self.held.iter()
    }
}

impl<'a> IntoIterator for &'a mut ValueSlots {
    type Item = &'a mut ValueRecord;
    type IntoIter = std::slice::IterMut<'a, ValueRecord>;
    fn into_iter(self) -> Self::IntoIter {
        self.held.iter_mut()
    }
}

impl Serialize for ValueSlots {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.validate().map_err(serde::ser::Error::custom)?;
        let mut held = self.held.iter().peekable();
        serializer.collect_seq((0..self.slots).map(|order| {
            if held.peek().is_some_and(|value| value.order == order) {
                held.next()
            } else {
                None
            }
        }))
    }
}

impl<'de> Deserialize<'de> for ValueSlots {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = Vec::<Option<ValueRecord>>::deserialize(deserializer)?;
        let slots = wire.len();
        let mut held = Vec::new();
        for (order, value) in wire.into_iter().enumerate() {
            if let Some(value) = value {
                if value.order != order {
                    return Err(serde::de::Error::custom(
                        "value order disagrees with its slot",
                    ));
                }
                held.push(value);
            }
        }
        Self::new(held, slots).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests;
