// SPDX-License-Identifier: Apache-2.0
//! Structured names of alternate states on the scope's shared timeline.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_ir::design::evaluation::Evaluation;
use cadmpeg_ir::design::feature::Feature;
use cadmpeg_ir::design::names::{NameEntry, TopoName};
use cadmpeg_ir::design::refs::{EntityRef, Evaluated, Member, Position};
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::SldprtLossCode;

pub(super) struct Timeline<'a>(HashMap<&'a FeatureId, usize>);

impl<'a> Timeline<'a> {
    pub(super) fn new(features: &'a [Feature]) -> Self {
        Self(
            features
                .iter()
                .enumerate()
                .map(|(rank, feature)| (&feature.id, rank))
                .collect(),
        )
    }

    /// Rebase the name deltas of one alternate configuration, from its
    /// Empty-based tip back through its earlier states. The native chain's
    /// order and records are already stated. A producer that its CADIR
    /// position cannot name remains in the native part of the entry.
    pub(super) fn project(
        &self,
        tip: &mut Evaluation,
        earlier: &mut [Evaluation],
    ) -> Option<LossNote> {
        let configuration = tip.configuration.clone()?;
        let mut names = Names::default();
        for evaluation in std::iter::once(tip).chain(earlier) {
            let Some(content) = &mut evaluation.content else {
                continue;
            };
            let cutoff = match &evaluation.of {
                Evaluated::State(Position::Start(_)) => 0,
                Evaluated::State(Position::After(Member::Feature(feature))) => {
                    self.0.get(feature).map_or(0, |rank| rank + 1)
                }
                _ => self.0.len(),
            };
            names.advance(self, cutoff, &mut content.delta.names);
        }
        (!names.native_only.is_empty()).then(|| {
            let producers = names.native_only.into_iter().collect::<Vec<_>>().join(", ");
            SldprtLossCode::DesignStateNameNative.note(format!(
                "Configuration {configuration} states the names of producers {producers} +                 only as native names where its stored order differs from the shared timeline."
            ))
        })
    }
}

// Keep the native names through cutoff changes, since an older native
// state can be after a later member of the shared timeline.
#[derive(Default)]
struct Names {
    native: BTreeMap<EntityRef, NameEntry>,
    by_rank: BTreeMap<usize, BTreeSet<EntityRef>>,
    cutoff: usize,
    native_only: BTreeSet<String>,
}

impl Names {
    fn advance(
        &mut self,
        timeline: &Timeline<'_>,
        cutoff: usize,
        delta: &mut BTreeMap<EntityRef, Option<NameEntry>>,
    ) {
        let changes = std::mem::take(delta);
        let crossed = cutoff.min(self.cutoff)..cutoff.max(self.cutoff);
        for (rank, entities) in self.by_rank.range(crossed) {
            for entity in entities {
                if !changes.contains_key(entity) {
                    let entry = &self.native[entity];
                    delta.insert(
                        entity.clone(),
                        projected(entry, *rank < cutoff, &mut self.native_only),
                    );
                }
            }
        }
        for (entity, next) in changes {
            let previous = self.native.remove(&entity);
            if let Some(rank) = previous.as_ref().and_then(|entry| rank(timeline, entry)) {
                if let std::collections::btree_map::Entry::Occupied(mut slot) =
                    self.by_rank.entry(rank)
                {
                    slot.get_mut().remove(&entity);
                    if slot.get().is_empty() {
                        slot.remove();
                    }
                }
            }
            let before = previous
                .as_ref()
                .and_then(|entry| visible(timeline, self.cutoff, entry));
            let after = next.as_ref().and_then(|entry| {
                let rank = rank(timeline, entry);
                if let Some(rank) = rank {
                    self.by_rank.entry(rank).or_default().insert(entity.clone());
                }
                projected(
                    entry,
                    rank.is_none_or(|rank| rank < cutoff),
                    &mut self.native_only,
                )
            });
            if before != after {
                delta.insert(entity.clone(), after);
            }
            if let Some(next) = next {
                self.native.insert(entity, next);
            }
        }
        self.cutoff = cutoff;
    }
}

fn rank(timeline: &Timeline<'_>, entry: &NameEntry) -> Option<usize> {
    match entry.name.as_ref()? {
        TopoName::Generated { feature, .. } => {
            Some(timeline.0.get(feature).copied().unwrap_or(usize::MAX))
        }
        _ => None,
    }
}

fn visible(timeline: &Timeline<'_>, cutoff: usize, entry: &NameEntry) -> Option<NameEntry> {
    if rank(timeline, entry).is_none_or(|rank| rank < cutoff) {
        return Some(entry.clone());
    }
    entry.native.clone().map(|native| NameEntry {
        name: None,
        native: Some(native),
    })
}

fn projected(
    entry: &NameEntry,
    full: bool,
    native_only: &mut BTreeSet<String>,
) -> Option<NameEntry> {
    if full {
        return Some(entry.clone());
    }
    if let Some(TopoName::Generated { feature, .. }) = &entry.name {
        native_only.insert(feature.to_string());
    }
    entry.native.clone().map(|native| NameEntry {
        name: None,
        native: Some(native),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::design::names::{NameKind, NativeName, NativeNameValue};
    use cadmpeg_ir::ids::FaceId;

    #[test]
    fn crossing_shared_positions_restates_inherited_names_without_topology() {
        let producer = FeatureId::mint("sldprt:model:feature#producer").unwrap();
        let entity = EntityRef::Face(FaceId::mint("sldprt:brep:face#f").unwrap());
        let timeline = Timeline(HashMap::from([(&producer, 1)]));
        let entry = NameEntry {
            name: Some(TopoName::Generated {
                kind: NameKind::Face,
                feature: producer.clone(),
                role: None,
                generator: None,
                index: None,
            }),
            native: Some(NativeName {
                codec: cadmpeg_ir::codec_id!("sldprt"),
                value: NativeNameValue::Integers(vec![1, 2, 3]),
            }),
        };
        let mut names = Names::default();
        let mut tip = BTreeMap::from([(entity.clone(), Some(entry.clone()))]);
        names.advance(&timeline, 1, &mut tip);
        let kept = tip[&entity].as_ref().unwrap();
        assert!(kept.name.is_none());
        assert_eq!(kept.native, entry.native);

        let mut restored = BTreeMap::new();
        names.advance(&timeline, 2, &mut restored);
        assert_eq!(restored[&entity], Some(entry.clone()));

        let mut cleared = BTreeMap::new();
        names.advance(&timeline, 1, &mut cleared);
        assert_eq!(cleared[&entity], tip[&entity]);

        let mut removed = BTreeMap::from([(entity.clone(), None)]);
        names.advance(&timeline, 2, &mut removed);
        assert_eq!(removed, BTreeMap::from([(entity, None)]));
        assert_eq!(names.native_only, BTreeSet::from([producer.to_string()]));
    }
}
