// SPDX-License-Identifier: Apache-2.0
//! The design records that restate each record of an arena the CADIR wire
//! omits ([`crate::document::ArenaName::WIRE_OMITTED`]), and the wire fields
//! of the record each leaves out.
//!
//! A projection states the restatements and charges nothing. A write that
//! omits the record's arena charges the record by them (§3.2): the fields
//! that every restater it writes leaves out, and the whole record when it
//! writes none of them. A cut is computed by comparing what the record holds
//! with what its restater holds.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::design::dimension::Dimension;
use crate::design::feature::{Feature, FeatureOperation};
use crate::design::names::TopoName;
use crate::design::refs::{EntityRef, Evaluated, Member, Position};
use crate::document::CadIr;
use crate::features::{Feature as ModelFeature, FeatureId, FeatureOperation as ModelOperation};
use crate::ids::BodyId;
use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use crate::schema::ArenaRecord;

use super::fields::{Field, ModelFeatureField};

/// What restates a record.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Restater {
    /// The design record with this identity.
    Record(String),
    /// The name table of the current geometry (§5.7).
    Names,
}

/// The restaters of one record, each with the wire keys of the record's
/// fields it leaves out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Restating(BTreeMap<Restater, BTreeSet<&'static str>>);

impl Restating {
    /// Each restater with the fields it leaves out.
    pub fn restaters(&self) -> impl Iterator<Item = (&Restater, &BTreeSet<&'static str>)> {
        self.0.iter()
    }

    /// The fields a write leaves out when it writes the restaters `writes`
    /// accepts: those every written restater leaves out. `None` where it
    /// writes none of them.
    pub fn cut_by(&self, writes: impl Fn(&Restater) -> bool) -> Option<BTreeSet<&'static str>> {
        self.0
            .iter()
            .filter(|(by, _)| writes(by))
            .map(|(_, cut)| cut.clone())
            .reduce(|left, right| left.intersection(&right).copied().collect())
    }
}

/// The restatement of each restated record, by the record's identity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Restatements(BTreeMap<String, Restating>);

impl Restatements {
    /// States that `by` restates the record `legacy` without its wire fields
    /// `cut`. Statements by one restater add up their cut fields; statements
    /// by different restaters stay apart.
    pub fn record(
        &mut self,
        legacy: &str,
        by: Restater,
        cut: impl IntoIterator<Item = &'static str>,
    ) {
        self.0
            .entry(legacy.to_owned())
            .or_default()
            .0
            .entry(by)
            .or_default()
            .extend(cut);
    }

    /// The restatement of the record `legacy`.
    #[must_use]
    pub fn get(&self, legacy: &str) -> Option<&Restating> {
        self.0.get(legacy)
    }

    pub(crate) fn append(&mut self, other: &mut Self) {
        for edge in std::mem::take(other).into_edges() {
            self.insert_edge(edge);
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
        self.0.into_iter().flat_map(|(legacy, restating)| {
            restating
                .0
                .into_iter()
                .map(move |(by, cut)| RestatementEdge {
                    legacy: legacy.clone(),
                    by,
                    cut,
                })
        })
    }

    pub(crate) fn insert_edge(&mut self, edge: RestatementEdge) {
        self.record(&edge.legacy, edge.by, edge.cut);
    }
}

/// One restatement by one restater, as
/// [`crate::document::Model::extend_rewritten`] hands it to a rewrite.
pub(crate) struct RestatementEdge {
    legacy: String,
    by: Restater,
    cut: BTreeSet<&'static str>,
}

impl RewriteIdentities for RestatementEdge {
    fn visit_identity_references(
        &self,
        _ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        visitor(&self.legacy)?;
        match &self.by {
            Restater::Record(id) => visitor(id),
            Restater::Names => Ok(()),
        }
    }

    fn rewrite_identities(
        self,
        _ctx: &DecodeContext<'_>,
        map: &mut IdentityMap<'_>,
    ) -> Result<Self, CodecError> {
        let by = match self.by {
            Restater::Record(id) => Restater::Record(map.identity(&id)?),
            Restater::Names => Restater::Names,
        };
        Ok(Self {
            legacy: map.identity(&self.legacy)?,
            by,
            cut: self.cut,
        })
    }
}

/// The feature that produced each body, where a design record states it.
struct Facts<'ir> {
    produced: HashSet<(&'ir str, &'ir str)>,
}

impl<'ir> Facts<'ir> {
    fn of(ir: &'ir CadIr) -> Self {
        let mut produced = HashSet::new();
        for (entity, entry) in ir.design.names.entries() {
            if let (EntityRef::Body(body), Some(TopoName::Generated { feature, .. })) =
                (entity, &entry.name)
            {
                produced.insert((body.as_str(), feature.as_str()));
            }
        }
        for evaluation in &ir.design.evaluations {
            if let (Evaluated::State(Position::After(Member::Feature(feature))), Some(content)) =
                (&evaluation.of, &evaluation.content)
            {
                for body in &content.delta.records.bodies {
                    produced.insert((body.id.as_str(), feature.as_str()));
                }
            }
        }
        for scope in &ir.design.scopes {
            for current in &scope.current {
                if let Position::After(Member::Feature(feature)) = &current.at {
                    for body in &current.bodies {
                        produced.insert((body.as_str(), feature.as_str()));
                    }
                }
            }
        }
        Self { produced }
    }
}

/// The source fields of `model`, unless `held`: a native record, the
/// full-fidelity record that `model` or its restater names by `native_ref`,
/// holds them.
fn source_cut(model: &ModelFeature, held: bool) -> impl Iterator<Item = &'static str> {
    let fields = [
        (
            ModelFeatureField::SourceProperties,
            !model.source_properties.is_empty(),
        ),
        (ModelFeatureField::SourceTag, model.source_tag.is_some()),
        (ModelFeatureField::SourceText, model.source_text.is_some()),
        (
            ModelFeatureField::SourceContent,
            !model.source_content.is_empty(),
        ),
    ];
    fields
        .into_iter()
        .filter(move |(_, stated)| *stated && !held)
        .map(|(field, _)| field.key())
}

/// The name of `model` where it states one and `stated` differs from it.
fn name_cut(model: &ModelFeature, stated: Option<&str>) -> Option<&'static str> {
    model
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty() && Some(*name) != stated)
        .map(|_| ModelFeatureField::Name.key())
}

/// `definition`, with `dependencies` where `model` lists some (D26: the
/// dependencies derive from the definition's references).
fn definition_cut(model: &ModelFeature) -> impl Iterator<Item = &'static str> {
    std::iter::once(ModelFeatureField::Definition.key())
        .chain((!model.dependencies.is_empty()).then(|| ModelFeatureField::Dependencies.key()))
}

/// States the fields each design feature of `ir` leaves out of the model
/// feature it restates under its identity, by comparing the two records.
///
/// # Errors
///
/// A resource refusal while the model feature's references are read.
pub fn restate_features(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<(), CodecError> {
    let cut = {
        let facts = Facts::of(ir);
        let model: HashMap<&str, &ModelFeature> = ir
            .model
            .features
            .iter()
            .map(|feature| (feature.record_identity(), feature))
            .collect();
        let mut cut = Vec::new();
        for feature in &ir.design.features {
            let id = feature.record_identity();
            let Some(legacy) = model.get(id) else {
                continue;
            };
            let fields = feature_cut(ctx, feature, legacy, &facts)?;
            if !fields.is_empty() {
                cut.push((id.to_owned(), fields));
            }
        }
        cut
    };
    for (id, fields) in cut {
        let by = Restater::Record(id.clone());
        ir.model.restated.record(&id, by, fields);
    }
    Ok(())
}

/// Every string a design record's wire form holds.
fn wire_strings(feature: &Feature) -> HashSet<String> {
    fn collect(value: serde_json::Value, into: &mut HashSet<String>) {
        match value {
            serde_json::Value::String(text) => {
                into.insert(text);
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    collect(item, into);
                }
            }
            serde_json::Value::Object(fields) => {
                for item in fields.into_values() {
                    collect(item, into);
                }
            }
            serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            }
        }
    }
    let mut strings = HashSet::new();
    if let Ok(value) = serde_json::to_value(feature) {
        collect(value, &mut strings);
    }
    strings
}

/// Whether a model operation holds anything beyond its kind.
fn holds_content(operation: &ModelOperation) -> bool {
    match operation {
        ModelOperation::Unresolved { .. } => false,
        ModelOperation::Native { parameters, .. } => !parameters.is_empty(),
        other => match serde_json::to_value(other) {
            Ok(serde_json::Value::Object(fields)) => fields.get("value").is_some_and(|value| {
                !matches!(value, serde_json::Value::Null)
                    && value.as_object().is_none_or(|fields| !fields.is_empty())
                    && value.as_array().is_none_or(|items| !items.is_empty())
            }),
            Ok(_) => false,
            Err(_) => true,
        },
    }
}

/// Whether `design` states the definition of `model`: what its operation
/// holds beyond its kind, its native parameters by name, and every reference
/// and state-local member it holds.
fn definition_stated(
    ctx: &DecodeContext<'_>,
    design: &Feature,
    model: &ModelFeature,
    strings: &HashSet<String>,
) -> Result<bool, CodecError> {
    let definition = model.evaluation.definition();
    let operation = definition.operation();
    let shown = match &design.definition.operation {
        FeatureOperation::Unresolved { .. } => !holds_content(operation),
        FeatureOperation::Native {
            native_options,
            source_settings,
            ..
        } => {
            let names = native_options
                .keys()
                .chain(source_settings.keys())
                .filter_map(|name| name.as_str().split_once(':').map(|(_, key)| key))
                .collect::<HashSet<_>>();
            match operation {
                ModelOperation::Native { parameters, .. } => {
                    parameters.keys().all(|key| names.contains(key.as_str()))
                }
                other => !names.is_empty() || !holds_content(other),
            }
        }
        _ => !matches!(
            operation,
            ModelOperation::Native { parameters, .. } if !parameters.is_empty()
        ),
    };
    if !shown {
        return Ok(false);
    }
    let mut stated = true;
    let mut visit = |id: &str| {
        stated &= strings.contains(id);
        Ok(())
    };
    definition.visit_identity_references(ctx, &mut visit)?;
    definition.visit_local_members(ctx, &mut visit)?;
    Ok(stated)
}

fn feature_cut(
    ctx: &DecodeContext<'_>,
    design: &Feature,
    model: &ModelFeature,
    facts: &Facts<'_>,
) -> Result<Vec<&'static str>, CodecError> {
    let strings = wire_strings(design);
    let mut cut = Vec::new();
    if !definition_stated(ctx, design, model, &strings)? {
        cut.extend(definition_cut(model));
    }
    let id = design.id.as_str();
    if model
        .evaluation
        .outputs()
        .iter()
        .any(|body| !facts.produced.contains(&(body.as_str(), id)))
    {
        cut.push(ModelFeatureField::Outputs.key());
    }
    cut.extend(name_cut(model, design.name.as_deref()));
    let differs = match (&design.suppressed, model.suppressed) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(Dimension::Literal(stated)), Some(model)) => *stated != model,
        (Some(Dimension::Parameter(_) | Dimension::Negated(_)), Some(_)) => false,
    };
    if differs {
        cut.push(ModelFeatureField::Suppressed.key());
    }
    cut.extend(source_cut(
        model,
        model.native_ref.is_some() || design.native_ref.is_some(),
    ));
    Ok(cut)
}

/// What an ordered container or folder states of a model tree node it
/// restates under another identity.
#[derive(Debug, Clone, Default)]
pub struct NodeStatement {
    /// The members, in order.
    pub members: Vec<FeatureId>,
    /// The member the restater states as active, such as a Body's `Tip`.
    pub active: Option<FeatureId>,
    /// The restater's name.
    pub name: Option<String>,
    /// Whether the restater states the node's suppression.
    pub suppressed: bool,
    /// The bodies the restater states as the node's result.
    pub bodies: Vec<BodyId>,
}

/// States that each restater restates the model tree node of each entry of
/// `nodes`, without the fields the node holds and its statement does not.
/// A node with no model feature is skipped.
pub fn restate_tree_nodes(
    ir: &mut CadIr,
    nodes: impl IntoIterator<Item = (FeatureId, Restater, NodeStatement)>,
) {
    let cut = {
        let model: HashMap<&str, &ModelFeature> = ir
            .model
            .features
            .iter()
            .map(|feature| (feature.record_identity(), feature))
            .collect();
        nodes
            .into_iter()
            .filter_map(|(node, by, stated)| {
                let legacy = model.get(node.as_str())?;
                Some((node, by, tree_node_cut(legacy, &stated)))
            })
            .collect::<Vec<_>>()
    };
    for (node, by, fields) in cut {
        ir.model.restated.record(node.as_str(), by, fields);
    }
}

fn tree_node_cut(model: &ModelFeature, stated: &NodeStatement) -> Vec<&'static str> {
    let mut cut = Vec::new();
    let definition = match model.evaluation.definition().operation() {
        ModelOperation::TreeNode { children, .. } => {
            **children == *stated.members && children.active_child() == &stated.active
        }
        _ => false,
    };
    if !definition {
        cut.extend(definition_cut(model));
    }
    if model
        .evaluation
        .outputs()
        .iter()
        .any(|body| !stated.bodies.contains(body))
    {
        cut.push(ModelFeatureField::Outputs.key());
    }
    cut.extend(name_cut(model, stated.name.as_deref()));
    if model.suppressed.is_some() && !stated.suppressed {
        cut.push(ModelFeatureField::Suppressed.key());
    }
    cut.extend(source_cut(model, model.native_ref.is_some()));
    cut
}
