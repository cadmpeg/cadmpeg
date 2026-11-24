// SPDX-License-Identifier: Apache-2.0
//! The rules on names (§5.7, part 10 "Names").
//!
//! Name tables are checked in their resolved states: `Design.names` against
//! the current geometry, and each stored evaluation's entries against that
//! evaluation's state. The rules on what an `Instance` name or a hole role
//! names apply to every name, those of picks included.

use std::collections::{BTreeSet, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::design::dimension::Dimension;
use crate::design::evaluation::Evaluation;
use crate::design::feature::FeatureOperation;
use crate::design::names::{
    GenerationRole, Generator, InstanceIndex, NameEntry, NameKind, TopoName,
};
use crate::design::ops::body::{PatternInstances, TransformMode};
use crate::design::ops::shared::{
    CircularLayout, CompositeStage, DimensionTable, LinearLayout, PatternLayout, PatternRun,
    RunSides, ScaleLayout, Spacing, UpToFill,
};
use crate::design::options::SymmetricMeasure;
use crate::design::refs::{EntityRef, Evaluated};
use crate::design::selection::{PickTarget, Picks};
use crate::features::FeatureId;
use crate::report::check::Finding;

use super::index::{DesignIndex, StateAt};
use super::resolved::Arena;
use super::states::States;
use super::Held;

/// Every name nested in `name`, `name` first.
pub(super) fn nested(name: &TopoName) -> Vec<&TopoName> {
    let mut found = Vec::new();
    let mut stack = vec![name];
    while let Some(name) = stack.pop() {
        found.push(name);
        match name {
            TopoName::Generated { generator, .. } => {
                if let Some(Generator::Entity(inner)) = generator {
                    stack.push(inner);
                }
            }
            TopoName::Instance { of: inner, .. }
            | TopoName::Modified { from: inner, .. }
            | TopoName::Split { from: inner, .. } => stack.push(inner),
            TopoName::Blend { between, .. } => stack.extend(between.iter().rev()),
            TopoName::Bounded { faces, ends, .. } => {
                stack.extend(ends.iter().rev());
                stack.extend(faces.iter().rev());
            }
            TopoName::Meeting { faces, .. } | TopoName::Merged { from: faces, .. } => {
                stack.extend(faces.iter().rev());
            }
            TopoName::Root { .. } => {}
        }
    }
    found
}

/// Every feature `name` and the names nested in it mention.
pub(super) fn mentioned_features(name: &TopoName) -> Vec<&FeatureId> {
    nested(name)
        .into_iter()
        .filter_map(|name| match name {
            TopoName::Generated { feature, .. }
            | TopoName::Instance { feature, .. }
            | TopoName::Blend { feature, .. }
            | TopoName::Modified { feature, .. } => Some(feature),
            TopoName::Split { feature, .. } | TopoName::Merged { feature, .. } => feature.as_ref(),
            TopoName::Bounded { .. } | TopoName::Meeting { .. } | TopoName::Root { .. } => None,
        })
        .collect()
}

/// The kinds a name derives (§5.7): one kind, or several where the members
/// of a `Merged` name disagree, or none for a `Merged` name of no members.
pub(super) fn derived_kinds(name: &TopoName) -> BTreeSet<NameKind> {
    let mut kinds = BTreeSet::new();
    let mut stack = vec![name];
    while let Some(name) = stack.pop() {
        match name {
            TopoName::Generated { kind, .. } => {
                kinds.insert(*kind);
            }
            TopoName::Blend { .. } => {
                kinds.insert(NameKind::Face);
            }
            TopoName::Bounded { .. } => {
                kinds.insert(NameKind::Edge);
            }
            TopoName::Meeting { .. } => {
                kinds.insert(NameKind::Vertex);
            }
            TopoName::Root { .. } => {
                kinds.insert(NameKind::Body);
            }
            TopoName::Instance { of: inner, .. }
            | TopoName::Modified { from: inner, .. }
            | TopoName::Split { from: inner, .. } => stack.push(inner),
            TopoName::Merged { from, .. } => stack.extend(from),
        }
    }
    kinds
}

/// Checks the name tables and every name the design states.
pub(super) fn check<'a>(
    ctx: &DecodeContext<'_>,
    index: &DesignIndex<'a>,
    states: &States<'a>,
    held: &[Held<'a>],
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("IR design name validation")?;
    let ir = index.records.ir;
    let extent_features: Vec<_> = states
        .extents
        .iter()
        .map(|extent| index.state_features(StateAt::Position(&extent.state.at)))
        .collect();
    for (entity, entry) in ir.design.names.entries() {
        let holds = states.geometry_holds(entity);
        check_entry(
            entity,
            entry,
            holds,
            "the current geometry",
            entity.id(),
            findings,
        );
        for (extent, features) in states.extents.iter().zip(&extent_features) {
            if extent.records.contains(entity.id()) {
                if let Some(features) = features {
                    check_order(entry, features, entity.id(), findings);
                }
            }
        }
    }
    for evaluation in &ir.design.evaluations {
        let Some(content) = &evaluation.content else {
            continue;
        };
        let features = evaluation_features(index, states, evaluation);
        for (entity, entry) in &content.delta.names {
            let Some(entry) = entry else {
                continue;
            };
            let id = evaluation.id.as_str();
            if let Some(holds) = states.holds(id, entity) {
                check_entry(entity, entry, holds, "its resolved state", id, findings);
            }
            if let Some(features) = &features {
                check_order(entry, features, id, findings);
            }
        }
    }
    for (name, at) in every_name(ir, held) {
        for inner in nested(name) {
            check_instance(index, inner, at, findings);
            check_hole_role(index, inner, at, findings);
        }
    }
    Ok(())
}

/// Every name of the name tables and of the design's picks, with the record
/// a finding about it names.
fn every_name<'a>(
    ir: &'a crate::document::CadIr,
    held: &'a [Held<'a>],
) -> Vec<(&'a TopoName, &'a str)> {
    let mut found = Vec::new();
    for (entity, entry) in ir.design.names.entries() {
        found.extend(entry.name.iter().map(|name| (name, entity.id())));
    }
    for evaluation in &ir.design.evaluations {
        for entry in evaluation
            .content
            .iter()
            .flat_map(|content| content.delta.names.values())
            .flatten()
        {
            found.extend(entry.name.iter().map(|name| (name, evaluation.id.as_str())));
        }
    }
    for publication in &ir.design.publications {
        if let PickTarget::Entity(name) = &publication.target.target {
            found.push((name, publication.id.as_str()));
        }
    }
    for record in held {
        for selection in &record.selections {
            for pick in selection.picks() {
                if let PickTarget::Entity(name) = &pick.target {
                    found.push((name, record.holder.record()));
                }
            }
        }
    }
    found
}

/// The features a stored evaluation's state depends on, with the features of
/// the prototypes its instance paths place. `None` where its position is in
/// no container.
fn evaluation_features<'a>(
    index: &DesignIndex<'a>,
    states: &States<'a>,
    evaluation: &'a Evaluation,
) -> Option<HashSet<&'a str>> {
    let mut features = match &evaluation.of {
        Evaluated::State(position) => (*index.state_features(StateAt::Position(position))?).clone(),
        Evaluated::Tool(feature) => index.tool_features(feature.as_str())?,
    };
    for path in states.instance_paths(evaluation.id.as_str()) {
        for definition in index.path_prototypes(path) {
            features.extend(index.scope_features(definition));
        }
    }
    Some(features)
}

fn check_entry(
    entity: &EntityRef,
    entry: &NameEntry,
    holds: bool,
    state: &str,
    at: &str,
    findings: &mut Vec<Finding>,
) {
    if !holds {
        super::error(
            findings,
            at,
            format_args!(
                "name entry for {} names an entity that {state} does not hold",
                entity.id()
            ),
        );
    }
    if entry.name.is_none() && entry.native.is_none() {
        super::error(
            findings,
            at,
            format_args!(
                "name entry for {} holds neither a name nor a native name",
                entity.id()
            ),
        );
    }
    let Some(name) = &entry.name else {
        return;
    };
    let kinds = derived_kinds(name);
    if kinds.len() > 1 {
        super::error(
            findings,
            at,
            format_args!("name of {} merges entities of different kinds", entity.id()),
        );
    } else if let Some(kind) = kinds.first() {
        if Arena::of_entity(entity).0.name_kind() != Some(*kind) {
            super::error(
                findings,
                at,
                format_args!(
                    "name of {} derives kind {kind:?}, which is not the entity's kind",
                    entity.id()
                ),
            );
        }
    }
}

fn check_order(entry: &NameEntry, features: &HashSet<&str>, at: &str, findings: &mut Vec<Finding>) {
    let Some(name) = &entry.name else {
        return;
    };
    for feature in mentioned_features(name) {
        if !features.contains(feature.as_str()) {
            super::error(
                findings,
                at,
                format_args!(
                    "name mentions feature {}, which the named state does not depend on",
                    feature.as_str()
                ),
            );
        }
    }
}

/// An `Instance` name names a pattern with implicit instances by a valid
/// index, a copying `TransformBodies` by `[k]` within its count, or an
/// `Extract` by `[1]`. A name of a feature the document does not hold is not
/// checked: a pick by it stays when that feature is removed, and no longer
/// resolves (§4, D51), and a table name that mentions it breaks the order
/// rule.
fn check_instance(index: &DesignIndex<'_>, name: &TopoName, at: &str, findings: &mut Vec<Finding>) {
    let TopoName::Instance {
        feature, instance, ..
    } = name
    else {
        return;
    };
    let valid = match index.operation(feature.as_str()) {
        None => None,
        Some(FeatureOperation::Unresolved { .. } | FeatureOperation::Native { .. }) => None,
        Some(FeatureOperation::Pattern(pattern)) => match &pattern.instances {
            PatternInstances::Implicit(_) => valid_pattern_index(&pattern.layout, instance),
            PatternInstances::Members(_) => Some(false),
        },
        Some(FeatureOperation::TransformBodies(transform)) => match &transform.mode {
            Some(TransformMode::Copy { count, .. }) => Some(match instance.0.as_slice() {
                [k] => {
                    *k >= 1 && literal(count).is_none_or(|count| i64::from(*k) <= i64::from(count))
                }
                _ => false,
            }),
            _ => Some(false),
        },
        Some(FeatureOperation::Extract(_)) => Some(instance.0 == [1]),
        Some(_) => Some(false),
    };
    if valid == Some(false) {
        super::error(
            findings,
            at,
            format_args!(
                "instance name {:?} of feature {} names no implicit copy of that feature",
                instance.0,
                feature.as_str()
            ),
        );
    }
}

fn literal<T: Copy>(dimension: &Dimension<T>) -> Option<T> {
    match dimension {
        Dimension::Literal(value) => Some(*value),
        Dimension::Parameter(_) | Dimension::Negated(_) => None,
    }
}

/// Whether `instance` is an instance index of an implicit pattern of
/// `layout` (§6.5, part 10 "Design"): one coordinate per run present, not
/// all zeros, each in its run's coordinate range where the run's counts are
/// literal, and a zero coordinate under a literal `seed_only: true`. `None`
/// where the layout's instance set is not stated here.
fn valid_pattern_index(layout: &PatternLayout, instance: &InstanceIndex) -> Option<bool> {
    let coordinates = &instance.0;
    let index = layout_coordinates(layout)?;
    if coordinates.len() != index.coordinates.len()
        || coordinates.iter().all(|coordinate| *coordinate == 0)
    {
        return Some(false);
    }
    let in_range = coordinates
        .iter()
        .zip(&index.coordinates)
        .all(|(value, coordinate)| coordinate.admits(i64::from(*value)));
    let seed_only = index.seed_only.iter().all(|&first| {
        coordinates[first] == 0 || coordinates.get(first + 1).is_some_and(|value| *value == 0)
    });
    Some(in_range && seed_only)
}

/// The coordinates an instance index of a layout has.
struct LayoutCoordinates<'a> {
    /// One per run, in index order.
    coordinates: Vec<Coordinate<'a>>,
    /// The position of the first coordinate of each two-run `Linear` with a
    /// literal `seed_only: true`.
    seed_only: Vec<usize>,
}

/// The values one coordinate of an instance index takes.
enum Coordinate<'a> {
    /// Every value from the first to the second, both included; `None`
    /// bounds nothing on its side.
    Range(Option<i64>, Option<i64>),
    /// A row's `instance` of a table.
    Row(&'a DimensionTable),
}

impl Coordinate<'_> {
    const ANY: Self = Self::Range(None, None);

    const fn between(low: i64, high: i64) -> Self {
        Self::Range(Some(low), Some(high))
    }

    fn admits(&self, value: i64) -> bool {
        match self {
            Self::Range(low, high) => {
                low.is_none_or(|low| low <= value) && high.is_none_or(|high| value <= high)
            }
            Self::Row(table) => table
                .rows
                .iter()
                .any(|row| i64::from(row.instance) == value),
        }
    }
}

impl LayoutCoordinates<'_> {
    fn linear(&mut self, layout: &LinearLayout) {
        if layout.second.is_some() && matches!(layout.seed_only, Some(Dimension::Literal(true))) {
            self.seed_only.push(self.coordinates.len());
        }
        self.coordinates
            .push(run_coordinate(&layout.first.run, false));
        self.coordinates.extend(
            layout
                .second
                .iter()
                .map(|second| run_coordinate(&second.run, false)),
        );
    }

    fn circular(&mut self, layout: &CircularLayout) {
        self.coordinates
            .push(run_coordinate(&layout.angular, false));
        self.coordinates.extend(
            layout
                .radial
                .iter()
                .map(|radial| run_coordinate(radial, false)),
        );
    }

    fn mirror(&mut self) {
        self.coordinates.push(Coordinate::between(0, 1));
    }

    fn scale(&mut self, layout: &ScaleLayout) {
        let last = literal(&layout.count).map(|count| i64::from(count) - 1);
        self.coordinates.push(Coordinate::Range(Some(0), last));
    }
}

/// The coordinates of an instance index of `layout`, in declaration order.
/// `None` where the layout does not state its runs.
fn layout_coordinates(layout: &PatternLayout) -> Option<LayoutCoordinates<'_>> {
    let mut found = LayoutCoordinates {
        coordinates: Vec::new(),
        seed_only: Vec::new(),
    };
    match layout {
        PatternLayout::Linear(linear) => found.linear(linear),
        PatternLayout::Circular(circular) => found.circular(circular),
        PatternLayout::Path { run, .. } => found.coordinates.push(run_coordinate(run, true)),
        PatternLayout::Points { points, .. } => {
            let last = match &points.picks {
                Picks::Named(picks) => i64::try_from(picks.as_slice().len()).ok(),
                _ => None,
            };
            found.coordinates.push(Coordinate::Range(Some(1), last));
        }
        PatternLayout::Dimensions(runs) => {
            found.coordinates.extend(runs.iter().map(|run| {
                literal(&run.count).map_or(Coordinate::ANY, |count| {
                    Coordinate::between(0, i64::from(count) - 1)
                })
            }));
        }
        PatternLayout::Table(table) => found.coordinates.push(Coordinate::Row(table)),
        PatternLayout::Mirror(_) => found.mirror(),
        PatternLayout::Scale(scale) => found.scale(scale),
        PatternLayout::Composite(stages) => {
            for stage in stages {
                match stage {
                    CompositeStage::Linear(linear) => found.linear(linear),
                    CompositeStage::Circular(circular) => found.circular(circular),
                    CompositeStage::Mirror(_) => found.mirror(),
                    CompositeStage::Scale(scale) => found.scale(scale),
                    CompositeStage::Native(_) => return None,
                }
            }
        }
        PatternLayout::Follow(_) | PatternLayout::Chain(_) | PatternLayout::Native(_) => {
            return None
        }
    }
    Some(found)
}

/// The coordinate range of a run whose count, and seed where `sides` reads
/// it, are literal (§6.5, Coordinate range). A run whose `sides` is not
/// stated reaches at most `last` and has no lower bound: an unread side may
/// be a `Reverse` arm whose count the run does not state. In a `Path` run
/// (`on_path`) an unread side may also be `Against`, which reaches the seed,
/// so the bound is the greater of `last` and the seed, and there is none for
/// a seed that is not literal. That carve-out is interim, gap
/// 20261119-ERR1-C-H6-2: §6.5 does not yet state it.
fn run_coordinate<'a, Q>(run: &PatternRun<Q>, on_path: bool) -> Coordinate<'a> {
    let Some(last) = spacing_count(&run.spacing).map(|count| i64::from(count) - 1) else {
        return Coordinate::ANY;
    };
    let seed = match &run.seed {
        None => Some(0),
        Some(seed) => literal(seed).map(i64::from),
    };
    match &run.sides {
        Some(RunSides::One) => seed.map_or(Coordinate::ANY, |seed| {
            Coordinate::between(-seed, last - seed)
        }),
        Some(RunSides::Against) => seed.map_or(Coordinate::ANY, |seed| {
            Coordinate::between(seed - last, seed)
        }),
        Some(RunSides::Symmetric {
            count: Some(SymmetricMeasure::Total),
            ..
        }) => {
            let reverse = last.div_euclid(2);
            Coordinate::between(-reverse, last - reverse)
        }
        Some(RunSides::Symmetric { .. }) => Coordinate::between(-last, last),
        Some(RunSides::Reverse(reverse)) => seed.map_or(Coordinate::ANY, |seed| {
            let low = spacing_count(reverse).map(|count| (1 - i64::from(count)).min(-seed));
            Coordinate::Range(low, Some(last - seed))
        }),
        None if on_path => Coordinate::Range(None, seed.map(|seed| last.max(seed))),
        None => Coordinate::Range(None, Some(last)),
    }
}

/// A spacing's count, the seed included, where it is literal.
fn spacing_count<Q>(spacing: &Spacing<Q>) -> Option<u32> {
    match spacing {
        Spacing::Pitch { count, .. }
        | Spacing::Span { count, .. }
        | Spacing::Fit { count }
        | Spacing::UpTo {
            fill: UpToFill::Count(count),
            ..
        } => literal(count),
        Spacing::List(spacings) => u32::try_from(spacings.len()).ok()?.checked_add(1),
        Spacing::PitchSpan { .. }
        | Spacing::UpTo {
            fill: UpToFill::Pitch(_),
            ..
        }
        | Spacing::Native(_) => None,
    }
}

/// `GenerationRole::Hole` occurs only in a `Generated` face name of a hole.
/// As for `Instance` names, a name of a feature the document does not hold
/// is not checked.
fn check_hole_role(
    index: &DesignIndex<'_>,
    name: &TopoName,
    at: &str,
    findings: &mut Vec<Finding>,
) {
    let TopoName::Generated {
        kind,
        feature,
        role: Some(GenerationRole::Hole(_)),
        ..
    } = name
    else {
        return;
    };
    let of_hole = match index.operation(feature.as_str()) {
        None => return,
        Some(FeatureOperation::Hole(_) | FeatureOperation::Native { .. }) => true,
        Some(FeatureOperation::Unresolved { family }) => {
            *family == crate::design::feature::OperationFamily::Hole
        }
        Some(_) => false,
    };
    if *kind != NameKind::Face || !of_hole {
        super::error(
            findings,
            at,
            format_args!(
                "hole role on a {kind:?} name of feature {}, which is not a hole's face",
                feature.as_str()
            ),
        );
    }
}
