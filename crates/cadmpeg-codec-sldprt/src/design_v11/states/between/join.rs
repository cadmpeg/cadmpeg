// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, HashMap};

use cadmpeg_ir::design::feature::{Feature, FeatureOperation, OperationFamily};
use cadmpeg_ir::features::FeatureId;

use super::{stateless, Unread};
use crate::records::{FeatureHistory, FeatureInputLane, ObjectId};
use crate::rollback::{MarkChain, MarkRecord};

pub(super) struct Joined {
    pub(super) states: BTreeMap<u32, FeatureId>,
    pub(super) unread: Unread,
    pub(super) prefix: Prefix,
    pub(super) tip: Option<FeatureId>,
    pub(super) order: Vec<FeatureId>,
}

#[derive(Default)]
pub(super) struct Prefix {
    ranks: HashMap<FeatureId, usize>,
    marks: HashMap<u32, usize>,
}

impl Prefix {
    pub(super) fn includes(&self, mark: u32, feature: &FeatureId) -> bool {
        self.rank(feature)
            .zip(self.cutoff(mark))
            .is_some_and(|(rank, cutoff)| rank <= cutoff)
    }

    pub(super) fn cutoff(&self, mark: u32) -> Option<usize> {
        self.marks.get(&mark).copied()
    }

    pub(super) fn rank(&self, feature: &FeatureId) -> Option<usize> {
        self.ranks.get(feature).copied()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rank {
    Unique(usize),
    Ambiguous,
}

fn set_rank(rank: &mut Rank, other: Rank) {
    if *rank != other {
        *rank = Rank::Ambiguous;
    }
}

#[derive(Clone, Copy)]
enum Object {
    Feature(usize),
    Other,
    Ambiguous,
}

pub(super) struct FeatureIndex<'a> {
    features: &'a [Feature],
    by_object: HashMap<u32, Object>,
    imported: Vec<bool>,
}

impl<'a> FeatureIndex<'a> {
    pub(super) fn new(features: &'a [Feature], histories: &[FeatureHistory]) -> Self {
        let index = features
            .iter()
            .enumerate()
            .filter_map(|(index, feature)| Some((feature.native_ref.as_deref()?, index)))
            .collect::<HashMap<_, _>>();
        let mut by_object = HashMap::new();
        let mut imported = vec![false; features.len()];
        for record in histories.iter().flat_map(|history| &history.features) {
            let index = index.get(record.id.as_str()).copied();
            if let Some(index) = index {
                imported[index] |= record.input_class.as_deref()
                    == Some(crate::design_v11::operation::BASE_BODY_CLASS);
            }
            if let Some(object) = record.source_value() {
                by_object
                    .entry(object)
                    .and_modify(|named| *named = Object::Ambiguous)
                    .or_insert(index.map_or(Object::Other, Object::Feature));
            }
        }
        Self {
            features,
            by_object,
            imported,
        }
    }

    pub(super) fn join(
        &self,
        chain: &MarkChain,
        records: &[MarkRecord],
        lane: &FeatureInputLane,
    ) -> Joined {
        let objects = lane
            .names
            .iter()
            .filter_map(|name| Some((name.offset, name.object_id.and_then(ObjectId::value)?)));
        self.join_objects(chain, records, objects)
    }

    pub(super) fn join_objects(
        &self,
        chain: &MarkChain,
        records: &[MarkRecord],
        objects: impl IntoIterator<Item = (u64, u32)>,
    ) -> Joined {
        let mut ranks = HashMap::<u32, Rank>::new();
        for (rank, &mark) in chain.main.iter().rev().enumerate() {
            ranks
                .entry(mark)
                .and_modify(|previous| set_rank(previous, Rank::Unique(rank)))
                .or_insert(Rank::Unique(rank));
        }
        let mut objects = objects.into_iter().peekable();
        let mut holder = None;
        let mut nested = Vec::new();
        let mut assigned = vec![None::<Rank>; self.features.len()];
        let mut seen_holder = vec![false; self.features.len()];
        let mut at_rank = vec![None::<usize>; chain.main.len()];
        let mut unread = Unread::default();
        let mut malformed = Vec::new();

        for record in records {
            let Some(offset) = u64::try_from(record.at).ok() else {
                continue;
            };
            while objects.peek().is_some_and(|(at, _)| *at < offset) {
                let Some((_, object)) = objects.next() else {
                    break;
                };
                match self.by_object.get(&object) {
                    Some(Object::Feature(index)) if self.holds_state(*index) => {
                        holder = Some(*index);
                        nested.clear();
                    }
                    Some(Object::Feature(index)) => nested.push(*index),
                    Some(Object::Other) => {}
                    Some(Object::Ambiguous) | None => {
                        holder = None;
                        nested.clear();
                    }
                }
            }
            let (Some(holder), Some(&rank)) = (holder, ranks.get(&record.mark)) else {
                continue;
            };
            let nested = nested.drain(..);
            let feature = &self.features[holder].id;
            let Rank::Unique(rank) = rank else {
                if !self.imported[holder] {
                    unread.feature(
                        "their persistent PMARK ID occurs more than once on the main line",
                        feature,
                    );
                }
                continue;
            };
            if !record.valid {
                if !self.imported[holder] && record.mark != chain.current {
                    malformed.push((holder, rank));
                }
                continue;
            }
            for index in std::iter::once(holder)
                .chain(nested.filter(|&index| !stateless(&self.features[index])))
            {
                match &mut assigned[index] {
                    Some(previous) => set_rank(previous, Rank::Unique(rank)),
                    slot @ None => *slot = Some(Rank::Unique(rank)),
                }
            }
            at_rank[rank] = Some(holder);
            seen_holder[holder] = true;
        }

        let mut prefix = Prefix {
            marks: ranks
                .into_iter()
                .filter_map(|(mark, rank)| match rank {
                    Rank::Unique(rank) => Some((mark, rank)),
                    Rank::Ambiguous => None,
                })
                .collect(),
            ..Prefix::default()
        };
        let mut members = vec![Vec::new(); chain.main.len()];
        for (index, rank) in assigned.into_iter().enumerate() {
            let feature = &self.features[index].id;
            match rank {
                Some(Rank::Unique(rank)) => {
                    prefix.ranks.insert(feature.clone(), rank);
                    if at_rank[rank] != Some(index) {
                        members[rank].push(feature.clone());
                    }
                }
                Some(Rank::Ambiguous) if seen_holder[index] && !self.imported[index] => {
                    unread.feature("their feature holds records of two marks", feature);
                }
                _ => {}
            }
        }
        let mut states = BTreeMap::new();
        let mut tip = None;
        for (rank, (&mark, holder)) in chain.main.iter().rev().zip(at_rank).enumerate() {
            let Some(holder) = holder else {
                continue;
            };
            let feature = &self.features[holder].id;
            if prefix.ranks.get(feature) != Some(&rank) {
                continue;
            }
            members[rank].push(feature.clone());
            if self.imported[holder] {
                continue;
            }
            if rank + 1 == chain.main.len() {
                tip = Some(feature.clone());
            } else {
                states.insert(mark, feature.clone());
            }
        }
        for (holder, rank) in malformed {
            let feature = &self.features[holder].id;
            if prefix.rank(feature) != Some(rank) {
                unread.feature(
                    "their record names a main-line PMARK and has a malformed head",
                    feature,
                );
            }
        }
        Joined {
            states,
            unread,
            prefix,
            tip,
            order: members.into_iter().flatten().collect(),
        }
    }

    fn holds_state(&self, index: usize) -> bool {
        let feature = &self.features[index];
        !stateless(feature)
            && !matches!(
                feature.definition.operation,
                FeatureOperation::Unresolved {
                    family: OperationFamily::Thread
                }
            )
    }
}
