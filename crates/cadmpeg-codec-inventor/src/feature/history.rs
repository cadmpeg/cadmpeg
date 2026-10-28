// SPDX-License-Identifier: Apache-2.0
//! Browser-ordered feature history of one `PmDc` segment.
//!
//! A model root owns one feature label. That label's participant list is the
//! ordered top level of the browser tree. Wrapper nodes own labels
//! whose participants are nested nodes. A feature label lists the sketches
//! the feature consumes; those sketches precede the feature in the history.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::{
    FeatureInventory, PmDcFeature, PmDcFeatureLabel, PmDcFeatureTerminator, PmDcPatternFeature,
};
use crate::design::scope::{root_role, BrepSegmentName, BrepSegmentRole, PmDcModelRoot};
use crate::design::DesignInventory;

/// Content-header flag bit of a suppressed feature or sketch record.
pub(crate) const SUPPRESSED_FLAG: u32 = 0x0000_0004;

/// Kind of a record that occupies a history position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoryNodeKind {
    Feature,
    Pattern,
    Sketch,
}

/// Position and state of one record in the browser history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HistoryPlacement {
    pub(crate) ordinal: u64,
    pub(crate) kind: HistoryNodeKind,
    pub(crate) below_end_of_part: bool,
    pub(crate) model_history: bool,
    pub(crate) model_root: u32,
}

/// Ordered history placements keyed by record ordinal in one segment.
#[derive(Debug, Default)]
pub(crate) struct History {
    pub(super) placements: HashMap<u32, HistoryPlacement>,
    pub(super) order: Vec<u32>,
    pub(super) next_ordinal: u64,
}

impl History {
    pub(crate) fn placement(&self, ordinal: u32) -> Option<HistoryPlacement> {
        self.placements.get(&ordinal).copied()
    }
}

struct Walk<'a, 'b> {
    ctx: &'b DecodeContext<'a>,
    labels: &'b HashMap<u32, &'b PmDcFeatureLabel>,
    kinds: &'b HashMap<u32, Kind>,
    visited: HashSet<u32>,
    history: History,
    model_root: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Node(HistoryNodeKind),
    EndOfPart,
}

/// Build the browser history of `token` from its model roots.
///
/// The segment must have exactly one document model root; otherwise it has
/// no history. The document model root is walked first, then the
/// flat-pattern model roots in record order. A root whose B-rep segment does
/// not resolve, or that does not own exactly one label, contributes no history.
pub(super) fn build(
    ctx: &DecodeContext<'_>,
    token: &str,
    inventory: &FeatureInventory,
    design: &DesignInventory,
    sketches: &[crate::sketch::PmDcSketch],
) -> Result<History, CodecError> {
    build_records(
        ctx,
        token,
        &HistoryRecords {
            features: &inventory.features,
            patterns: &inventory.pattern_features,
            labels: &inventory.labels,
            terminators: &inventory.terminators,
            sketches,
            roots: &design.model_roots,
            segments: &design.brep_segments,
        },
    )
}

pub(crate) struct HistoryRecords<'a> {
    pub(crate) features: &'a [PmDcFeature],
    pub(crate) patterns: &'a [PmDcPatternFeature],
    pub(crate) labels: &'a [PmDcFeatureLabel],
    pub(crate) terminators: &'a [PmDcFeatureTerminator],
    pub(crate) sketches: &'a [crate::sketch::PmDcSketch],
    pub(crate) roots: &'a [PmDcModelRoot],
    pub(crate) segments: &'a [BrepSegmentName],
}

pub(crate) fn build_records(
    ctx: &DecodeContext<'_>,
    token: &str,
    records: &HistoryRecords<'_>,
) -> Result<History, CodecError> {
    let mut roots = records
        .roots
        .iter()
        .filter(|root| root.identity.segment_token.as_str() == token)
        .filter_map(|root| Some((root_role(root, records.segments)?, root)))
        .collect::<Vec<_>>();
    if roots
        .iter()
        .filter(|(role, _)| *role == BrepSegmentRole::Document)
        .count()
        != 1
    {
        return Ok(History::default());
    }
    roots.sort_unstable_by_key(|(role, root)| {
        (
            *role != BrepSegmentRole::Document,
            root.identity.record_ordinal,
        )
    });
    let mut labels = HashMap::new();
    let mut duplicate_owners = HashSet::new();
    for label in records
        .labels
        .iter()
        .filter(|label| label.identity.segment_token.as_str() == token)
    {
        let Some(owner) = label.header.owner.record_ordinal() else {
            continue;
        };
        if labels.insert(owner, label).is_some() {
            duplicate_owners.insert(owner);
        }
    }
    for owner in &duplicate_owners {
        labels.remove(owner);
    }
    let mut kinds = HashMap::new();
    let mut add_kind = |ordinal: u32, kind: Kind| {
        kinds.insert(ordinal, kind);
    };
    for record in records
        .features
        .iter()
        .filter(|record| record.identity.segment_token.as_str() == token)
    {
        add_kind(
            record.identity.record_ordinal,
            Kind::Node(HistoryNodeKind::Feature),
        );
    }
    for record in records
        .patterns
        .iter()
        .filter(|record| record.identity.segment_token.as_str() == token)
    {
        add_kind(
            record.identity.record_ordinal,
            Kind::Node(HistoryNodeKind::Pattern),
        );
    }
    for record in records
        .sketches
        .iter()
        .filter(|record| record.identity.segment_token.as_str() == token)
    {
        add_kind(
            record.identity.record_ordinal,
            Kind::Node(HistoryNodeKind::Sketch),
        );
    }
    for record in records
        .terminators
        .iter()
        .filter(|record| record.identity.segment_token.as_str() == token)
    {
        add_kind(record.identity.record_ordinal, Kind::EndOfPart);
    }
    let mut walk = Walk {
        ctx,
        labels: &labels,
        kinds: &kinds,
        visited: HashSet::new(),
        history: History {
            next_ordinal: 1,
            ..History::default()
        },
        model_root: 0,
    };
    for (role, root) in roots {
        let Some(label) = labels.get(&root.identity.record_ordinal) else {
            continue;
        };
        let model = role == BrepSegmentRole::Document;
        walk.model_root = root.identity.record_ordinal;
        let members = label
            .participants
            .references()
            .iter()
            .filter_map(|reference| reference.record_ordinal())
            .collect::<Vec<_>>();
        walk.members(&members, 0, model, &mut false)?;
    }
    Ok(walk.history)
}

impl Walk<'_, '_> {
    fn members(
        &mut self,
        members: &[u32],
        depth: usize,
        model: bool,
        below: &mut bool,
    ) -> Result<(), CodecError> {
        let _depth = self.ctx.enter_nested("walk Inventor feature history")?;
        for &member in members {
            if !self.visited.insert(member) {
                continue;
            }
            match self.kinds.get(&member).copied() {
                Some(Kind::EndOfPart) => {
                    if depth == 0 {
                        *below = true;
                    }
                }
                Some(Kind::Node(kind)) => {
                    if kind != HistoryNodeKind::Sketch {
                        let consumed = self
                            .labels
                            .get(&member)
                            .map(|label| {
                                label
                                    .participants
                                    .references()
                                    .iter()
                                    .filter_map(|reference| reference.record_ordinal())
                                    .filter(|ordinal| {
                                        self.kinds.get(ordinal)
                                            == Some(&Kind::Node(HistoryNodeKind::Sketch))
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        let mut nested_below = *below;
                        self.members(&consumed, depth + 1, model, &mut nested_below)?;
                    }
                    self.place(member, kind, *below, model)?;
                }
                None => {
                    let Some(label) = self.labels.get(&member) else {
                        continue;
                    };
                    let nested = label
                        .participants
                        .references()
                        .iter()
                        .filter_map(|reference| reference.record_ordinal())
                        .collect::<Vec<_>>();
                    let mut nested_below = *below;
                    self.members(&nested, depth + 1, model, &mut nested_below)?;
                }
            }
        }
        Ok(())
    }

    fn place(
        &mut self,
        member: u32,
        kind: HistoryNodeKind,
        below_end_of_part: bool,
        model_history: bool,
    ) -> Result<(), CodecError> {
        let ordinal = self.history.next_ordinal;
        self.history.next_ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| CodecError::malformed("Inventor feature history ordinal overflows"))?;
        self.history.placements.insert(
            member,
            HistoryPlacement {
                ordinal,
                kind,
                below_end_of_part,
                model_history,
                model_root: self.model_root,
            },
        );
        self.history.order.push(member);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
