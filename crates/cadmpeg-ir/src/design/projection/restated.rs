// SPDX-License-Identifier: Apache-2.0
//! The design record that restates each record of an arena the CADIR wire
//! omits ([`crate::document::ArenaName::WIRE_OMITTED`]), and the wire fields
//! of the record it leaves out.
//!
//! A projection states the restatement and charges nothing. A write that
//! omits the record's arena charges the record by it (§3.2): nothing when the
//! write carries the restater and the restater leaves no field out, the cut
//! fields when it leaves some out, and the whole record when the write omits
//! the restater.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::design::feature::{Feature, FeatureOperation};
use crate::document::CadIr;
use crate::features::FeatureOperation as ModelOperation;
use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use crate::schema::ArenaRecord;

use super::fields::{Field, ModelFeatureField};

/// What restates a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Restater {
    /// The design record with this identity.
    Record(String),
    /// The name table of the current geometry (§5.7).
    Names,
}

/// The restatement of one record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restating {
    by: Restater,
    cut: BTreeSet<&'static str>,
}

impl Restating {
    /// What restates the record.
    #[must_use]
    pub const fn by(&self) -> &Restater {
        &self.by
    }

    /// The wire keys of the record's fields the restater leaves out.
    #[must_use]
    pub const fn cut(&self) -> &BTreeSet<&'static str> {
        &self.cut
    }
}

/// The restatement of each restated record, by the record's identity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Restatements(BTreeMap<String, Restating>);

impl Restatements {
    /// States that `by` restates the record `legacy` without its wire fields
    /// `cut`. The record keeps the restater stated first and takes the cut
    /// fields of every statement.
    pub fn record(
        &mut self,
        legacy: &str,
        by: Restater,
        cut: impl IntoIterator<Item = &'static str>,
    ) {
        self.0
            .entry(legacy.to_owned())
            .or_insert_with(|| Restating {
                by,
                cut: BTreeSet::new(),
            })
            .cut
            .extend(cut);
    }

    /// The restatement of the record `legacy`.
    #[must_use]
    pub fn get(&self, legacy: &str) -> Option<&Restating> {
        self.0.get(legacy)
    }

    pub(crate) fn append(&mut self, other: &mut Self) {
        for (legacy, restating) in std::mem::take(&mut other.0) {
            self.record(&legacy, restating.by, restating.cut);
        }
    }

    /// The restatements of the records whose identities `ids` holds.
    pub(crate) fn select(&self, ids: &BTreeSet<String>) -> Self {
        Self(
            self.0
                .iter()
                .filter(|(legacy, _)| ids.contains(*legacy))
                .map(|(legacy, restating)| (legacy.clone(), restating.clone()))
                .collect(),
        )
    }

    pub(crate) fn into_edges(self) -> impl Iterator<Item = RestatementEdge> {
        self.0
            .into_iter()
            .map(|(legacy, restating)| RestatementEdge { legacy, restating })
    }

    pub(crate) fn insert_edge(&mut self, edge: RestatementEdge) {
        self.record(&edge.legacy, edge.restating.by, edge.restating.cut);
    }
}

/// One restatement, as [`crate::document::Model::extend_rewritten`] hands it
/// to a rewrite.
pub(crate) struct RestatementEdge {
    legacy: String,
    restating: Restating,
}

impl RewriteIdentities for RestatementEdge {
    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visitor(&self.legacy)?;
        match &self.restating.by {
            Restater::Record(id) => visitor(id),
            Restater::Names => Ok(()),
        }
    }

    fn rewrite_identities(
        self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_>,
    ) -> Result<Self, CodecError> {
        let by = match self.restating.by {
            Restater::Record(id) => Restater::Record(map.identity(&id)?),
            Restater::Names => Restater::Names,
        };
        Ok(Self {
            legacy: map.identity(&self.legacy)?,
            restating: Restating {
                by,
                cut: self.restating.cut,
            },
        })
    }
}

/// States the fields each design feature of `ir` leaves out of the model
/// feature it restates under its identity: the operands of an operation it
/// states as `Unresolved` or as `Native` without options (§3.3), and a name
/// or suppression state it does not state.
pub fn restate_features(ir: &mut CadIr) {
    let model: HashMap<&str, &crate::features::Feature> = ir
        .model
        .features
        .iter()
        .map(|feature| (feature.record_identity(), feature))
        .collect();
    let cut: Vec<(String, Vec<&'static str>)> = ir
        .design
        .features
        .iter()
        .filter_map(|feature| {
            let id = feature.record_identity();
            let fields = feature_cut(feature, model.get(id)?);
            (!fields.is_empty()).then(|| (id.to_owned(), fields))
        })
        .collect();
    for (id, fields) in cut {
        let by = Restater::Record(id.clone());
        ir.model.restated.record(&id, by, fields);
    }
}

fn feature_cut(design: &Feature, model: &crate::features::Feature) -> Vec<&'static str> {
    let mut cut = Vec::new();
    let operands_unstated = match &design.definition.operation {
        FeatureOperation::Unresolved { .. } => true,
        FeatureOperation::Native { native_options, .. } => native_options.is_empty(),
        _ => false,
    };
    let operands_held = match model.evaluation.definition().operation() {
        ModelOperation::Unresolved { .. }
        | ModelOperation::TreeNode { .. }
        | ModelOperation::StoredGeometry {}
        | ModelOperation::BaseFeature { .. } => false,
        ModelOperation::Native { parameters, .. } => !parameters.is_empty(),
        _ => true,
    };
    if operands_unstated && operands_held {
        cut.push(ModelFeatureField::Definition.key());
    }
    if design.name.is_none()
        && model
            .name
            .as_deref()
            .is_some_and(|name| !name.trim().is_empty())
    {
        cut.push(ModelFeatureField::Name.key());
    }
    if design.suppressed.is_none() && model.suppressed.is_some() {
        cut.push(ModelFeatureField::Suppressed.key());
    }
    cut
}
