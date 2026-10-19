// SPDX-License-Identifier: Apache-2.0
//! Lazy indexes for immutable design inputs.

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};

use cadmpeg_ir::features::FeatureId;

use crate::native::{ObjectRecord, PropertyRecord};

/// The preceding feature at the first occurrence of a member in the first
/// body that supplies a predecessor. Repeated members do not replace it.
pub(super) struct BodyPredecessors<'a> {
    objects: &'a [ObjectRecord],
    features: &'a HashMap<&'a str, FeatureId>,
    properties: &'a HashMap<&'a str, Vec<&'a PropertyRecord>>,
    entries: OnceCell<HashMap<&'a str, &'a FeatureId>>,
}

impl<'a> BodyPredecessors<'a> {
    pub(super) fn new(
        objects: &'a [ObjectRecord],
        features: &'a HashMap<&'a str, FeatureId>,
        properties: &'a HashMap<&'a str, Vec<&'a PropertyRecord>>,
    ) -> Self {
        Self {
            objects,
            features,
            properties,
            entries: OnceCell::new(),
        }
    }

    pub(super) fn get(&self, member: &str) -> Option<&'a FeatureId> {
        self.entries
            .get_or_init(|| self.build())
            .get(member)
            .copied()
    }

    fn build(&self) -> HashMap<&'a str, &'a FeatureId> {
        let mut predecessors = HashMap::new();
        for object in self.objects {
            let Some(members) = self
                .properties
                .get(object.id().as_str())
                .and_then(|owned| super::body_membership_property(owned))
            else {
                continue;
            };
            let mut previous = None;
            let mut seen = HashSet::new();
            for member in members
                .links()
                .iter()
                .flatten()
                .filter_map(crate::native::LinkTarget::object)
            {
                if seen.insert(member) {
                    if let Some(previous) = previous {
                        predecessors.entry(member).or_insert(previous);
                    }
                }
                if let Some(feature) = self.features.get(member) {
                    previous = Some(feature);
                }
            }
        }
        predecessors
    }
}

#[cfg(test)]
mod tests;
