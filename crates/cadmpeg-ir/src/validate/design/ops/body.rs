// SPDX-License-Identifier: Apache-2.0
//! Patterns, template instances, and body operations (§6.5).

use std::collections::BTreeSet;

use cadmpeg_core::text::NonBlankString;

use super::picks::{self, Class};
use super::{assembly, error, Site};
use crate::design::container::Container;
use crate::design::dimension::Dimension;
use crate::design::feature::{Feature, FeatureOperation, OperationFamily};
use crate::design::names::InstanceIndex;
use crate::design::ops::body::{
    Combine, DeleteFaces, Flex, Graft, InstanceChange, Kept, MoveFaces, Pattern, PatternInstances,
    PatternSeeds, ReplaceFaces, Split, SplitMethod, SplitMode, SplitTool, TemplateInstance,
    TransformBodies, TransformMode, TransformStep, Transformation,
};
use crate::design::ops::shared::{
    CircularLayout, CompositeStage, InstanceOrientation, JointKind, LinearLayout, PatternLayout,
    PatternRun, RunSides, ScaleLayout, Spacing, UpToFill,
};
use crate::design::options::{BooleanOp, ProjectionDirection};
use crate::design::selection::{OrderedSelection, PickDomain, PickTarget};
use crate::features::{FeatureId, ParameterId};
use crate::report::check::Finding;

use super::super::records::{DesignRecords, MemberKey};

/// Which spacings a run admits: `Fit` in a circular angular run, a path
/// run, and a circular sketch pattern; `UpTo` in a linear run.
#[derive(Clone, Copy)]
pub(super) struct RunKind {
    pub(super) fit: bool,
    pub(super) up_to: bool,
}

/// A spacing's counts are at least 1, a `List` is nonempty, and
/// `Fit` and `UpTo` occur only where the run admits them.
pub(super) fn check_spacing<Q>(
    site: Site<'_>,
    field: &str,
    spacing: &Spacing<Q>,
    kind: RunKind,
    findings: &mut Vec<Finding>,
) {
    if let Some(count) = spacing_count(spacing) {
        site.bound(findings, |values| {
            (values.get(count)? == 0)
                .then(|| format!("{field} has a count of 0; a count is at least 1"))
        });
    }
    match spacing {
        Spacing::List(values) if values.is_empty() => error(
            findings,
            site.entity,
            format_args!("{field} is an empty List"),
        ),
        Spacing::Fit { .. } if !kind.fit => error(
            findings,
            site.entity,
            format_args!("{field} is Fit, which occurs only in a circular angular run, a path run, or a circular sketch pattern"),
        ),
        Spacing::UpTo { .. } if !kind.up_to => error(
            findings,
            site.entity,
            format_args!("{field} is UpTo, which occurs only in a LinearRun"),
        ),
        _ => {}
    }
}

/// The count a spacing states.
fn spacing_count<Q>(spacing: &Spacing<Q>) -> Option<&Dimension<u32>> {
    match spacing {
        Spacing::Pitch { count, .. } | Spacing::Span { count, .. } | Spacing::Fit { count } => {
            Some(count)
        }
        Spacing::UpTo {
            fill: UpToFill::Count(count),
            ..
        } => Some(count),
        _ => None,
    }
}

/// The literal count a spacing states.
fn literal_count<Q>(spacing: &Spacing<Q>) -> Option<u32> {
    match spacing_count(spacing)? {
        Dimension::Literal(count) => Some(*count),
        _ => None,
    }
}

/// The parameters a seed feature's definition binds by a dimension: the ids
/// its definition holds, and the ids the records of the sketches it owns or
/// names hold.
fn seed_bindings<'a>(index: &DesignRecords<'a>, seeds: &[&'a Feature]) -> BTreeSet<String> {
    let design = &index.ir.design;
    let mut held = BTreeSet::new();
    for seed in seeds {
        let definition = super::wire_strings(&seed.definition);
        let owned =
            |sketch: &str, owner: &str| owner == seed.id.as_str() || definition.contains(sketch);
        let sketches: BTreeSet<&str> = design
            .sketches
            .iter()
            .filter(|sketch| owned(sketch.id.as_str(), sketch.owner.as_str()))
            .map(|sketch| sketch.id.as_str())
            .collect();
        for constraint in design
            .sketch_constraints
            .iter()
            .filter(|constraint| sketches.contains(constraint.sketch.as_str()))
        {
            held.extend(super::wire_strings(&constraint.relation));
        }
        for entity in design
            .sketch_entities
            .iter()
            .filter(|entity| sketches.contains(entity.sketch.as_str()))
        {
            held.extend(super::wire_strings(&entity.definition));
        }
        held.extend(definition);
    }
    held
}

/// The seed features a `Features` seed selection picks by their results.
fn seed_features<'a>(index: &DesignRecords<'a>, seeds: &PatternSeeds) -> Option<Vec<&'a Feature>> {
    let PatternSeeds::Features { features, .. } = seeds else {
        return None;
    };
    picks::named(features)?
        .iter()
        .map(|pick| match &pick.target {
            PickTarget::FeatureResult(feature) => index.features.get(feature.as_str()).copied(),
            _ => None,
        })
        .collect()
}

pub(super) fn check_pattern(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    pattern: &Pattern,
    findings: &mut Vec<Finding>,
) {
    match &pattern.seeds {
        PatternSeeds::Features { features, .. } => {
            let allowed = |class| class == Class::FeatureResult;
            if let Some(class) = picks::refused(features, allowed) {
                error(
                    findings,
                    site.entity,
                    format_args!("a Features seed picks {class}; it picks feature results"),
                );
            }
        }
        PatternSeeds::Faces(faces) => {
            if let Some(class) = picks::refused(faces, picks::is_face) {
                error(
                    findings,
                    site.entity,
                    format_args!("a Faces seed picks {class}, not a face"),
                );
            }
        }
        PatternSeeds::Bodies { bodies, .. } => {
            if let Some(class) = picks::refused(bodies, picks::is_body) {
                error(
                    findings,
                    site.entity,
                    format_args!("a Bodies seed picks {class}, not a body"),
                );
            }
        }
    }
    let seeds = seed_features(index, &pattern.seeds);
    let bound = seeds.as_ref().map(|seeds| seed_bindings(index, seeds));
    check_layout(
        index,
        site,
        &pattern.layout,
        false,
        bound.as_ref(),
        findings,
    );
    match &pattern.instances {
        PatternInstances::Implicit(overrides) => {
            if matches!(pattern.layout, PatternLayout::Mirror(_)) && !overrides.is_empty() {
                error(
                    findings,
                    site.entity,
                    format_args!("a Mirror layout states instance overrides"),
                );
            }
            let indices: Vec<&InstanceIndex> = overrides.iter().map(|o| &o.index).collect();
            check_indices(index, site, &pattern.layout, &indices, findings);
            for instance in overrides {
                if let InstanceChange::Varied { parameters } = &instance.change {
                    if parameters.is_empty() {
                        error(
                            findings,
                            site.entity,
                            format_args!("an instance override's Varied.parameters is empty"),
                        );
                    }
                    for parameter in parameters.keys() {
                        if bound
                            .as_ref()
                            .is_some_and(|bound| !bound.contains(parameter.as_str()))
                        {
                            error(
                                findings,
                                site.entity,
                                format_args!(
                                    "Varied names {}, which no seed definition binds",
                                    parameter.as_str()
                                ),
                            );
                        }
                    }
                }
            }
        }
        PatternInstances::Members(members) => {
            let indices: Vec<&InstanceIndex> = members.iter().map(|m| &m.index).collect();
            check_indices(index, site, &pattern.layout, &indices, findings);
            let seeds = seeds.unwrap_or_default();
            let per_member = match &pattern.seeds {
                PatternSeeds::Features { features, .. } => picks::count(features),
                _ => None,
            };
            for member in members {
                if per_member.is_some_and(|count| member.features.len() != count) {
                    error(
                        findings,
                        site.entity,
                        format_args!("a pattern member lists {} features; it lists one per seed feature pick", member.features.len()),
                    );
                }
            }
            let listed: Vec<&str> = members
                .iter()
                .flat_map(|member| &member.features)
                .map(FeatureId::as_str)
                .collect();
            if listed
                .iter()
                .any(|feature| seeds.iter().any(|seed| seed.id.as_str() == *feature))
            {
                error(
                    findings,
                    site.entity,
                    format_args!("a pattern member is one of the pattern's seeds"),
                );
            }
            check_generated_members(index, site, "pattern member", &listed, findings);
        }
    }
}

/// Generated members exist, are not driven, and follow the generating
/// feature contiguously in its container.
fn check_generated_members(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    what: &str,
    listed: &[&str],
    findings: &mut Vec<Finding>,
) {
    for &feature in listed {
        match index.features.get(feature) {
            None => error(
                findings,
                site.entity,
                format_args!("{what} {feature} is no feature of the design"),
            ),
            Some(record) if record.driven_by.is_some() => error(
                findings,
                site.entity,
                format_args!("{what} {feature} has driven_by"),
            ),
            Some(_) => {}
        }
    }
    let Some(container) = index.parents.get(&MemberKey::Feature(site.feature)) else {
        return;
    };
    let Some(Container::Ordered { members, .. }) = index.containers.get(container) else {
        return;
    };
    let keys: Vec<MemberKey<'_>> = members.iter().map(MemberKey::of).collect();
    let Some(at) = keys
        .iter()
        .position(|key| *key == MemberKey::Feature(site.feature))
    else {
        return;
    };
    let wanted: BTreeSet<&str> = listed.iter().copied().collect();
    let following: BTreeSet<&str> = keys
        .iter()
        .skip(at + 1)
        .take(wanted.len())
        .filter_map(|key| match key {
            MemberKey::Feature(id) => Some(*id),
            MemberKey::Container(_) => None,
        })
        .collect();
    if following != wanted {
        error(
            findings,
            site.entity,
            format_args!("the {what}s do not follow the feature contiguously in its container"),
        );
    }
}

/// The coordinates an index of the layout holds, where the layout decides
/// them (§6.5): a `Composite` the sum of its stages', a `Follow` its
/// followed pattern's. `follow` is whether a `Follow` is resolved; a
/// followed pattern's own `Follow` is not, since no pattern follows a
/// follower.
fn coordinates(index: &DesignRecords<'_>, layout: &PatternLayout, follow: bool) -> Option<usize> {
    match layout {
        PatternLayout::Linear(linear) => Some(1 + usize::from(linear.second.is_some())),
        PatternLayout::Circular(circular) => Some(1 + usize::from(circular.radial.is_some())),
        PatternLayout::Path { .. }
        | PatternLayout::Chain(_)
        | PatternLayout::Points { .. }
        | PatternLayout::Table(_)
        | PatternLayout::Mirror(_)
        | PatternLayout::Scale(_) => Some(1),
        PatternLayout::Dimensions(runs) => Some(runs.len()),
        PatternLayout::Composite(stages) => stages
            .iter()
            .map(|stage| match stage {
                CompositeStage::Linear(linear) => Some(1 + usize::from(linear.second.is_some())),
                CompositeStage::Circular(circular) => {
                    Some(1 + usize::from(circular.radial.is_some()))
                }
                CompositeStage::Mirror(_) | CompositeStage::Scale(_) => Some(1),
                CompositeStage::Native(_) => None,
            })
            .sum(),
        PatternLayout::Follow(followed) if follow => {
            coordinates(index, followed_layout(index, followed)?, false)
        }
        PatternLayout::Follow(_) | PatternLayout::Native(_) => None,
    }
}

/// The layout of the pattern a `Follow` picks, where it picks one feature
/// of this definition that states a pattern operation.
fn followed_layout<'a>(
    index: &DesignRecords<'a>,
    followed: &OrderedSelection,
) -> Option<&'a PatternLayout> {
    let [pick] = picks::named(followed)? else {
        return None;
    };
    let PickTarget::Feature(target) = &pick.target else {
        return None;
    };
    if pick.context.is_some() {
        return None;
    }
    match &index.features.get(target.as_str())?.definition.operation {
        FeatureOperation::Pattern(pattern) => Some(&pattern.layout),
        FeatureOperation::ComponentPattern(pattern) => Some(&pattern.layout),
        _ => None,
    }
}

pub(super) fn check_indices(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    layout: &PatternLayout,
    indices: &[&InstanceIndex],
    findings: &mut Vec<Finding>,
) {
    let seed_only = matches!(
        layout,
        PatternLayout::Linear(LinearLayout {
            seed_only: Some(Dimension::Literal(true)),
            ..
        })
    );
    let expected = coordinates(index, layout, true);
    for (at, index) in indices.iter().enumerate() {
        if indices[..at].contains(index) {
            error(
                findings,
                site.entity,
                format_args!("instance index {:?} repeats", index.0),
            );
        }
        if index.0.iter().all(|&coordinate| coordinate == 0) {
            error(
                findings,
                site.entity,
                format_args!("instance index {:?} is all zeros, the seed's", index.0),
            );
        }
        if expected.is_some_and(|expected| index.0.len() != expected) {
            error(
                findings,
                site.entity,
                format_args!(
                    "instance index {:?} has other than one coordinate per run",
                    index.0
                ),
            );
        }
        if seed_only && !index.0.contains(&0) {
            error(
                findings,
                site.entity,
                format_args!(
                    "instance index {:?} has no zero coordinate under seed_only",
                    index.0
                ),
            );
        }
    }
}

pub(super) fn check_layout(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    layout: &PatternLayout,
    component: bool,
    bound: Option<&BTreeSet<String>>,
    findings: &mut Vec<Finding>,
) {
    match layout {
        PatternLayout::Linear(linear) => check_linear(site, linear, bound, findings),
        PatternLayout::Circular(circular) => check_circular(site, circular, bound, findings),
        PatternLayout::Path {
            path,
            run,
            orientation,
        } => {
            super::options::check_path(site, "PatternLayout::Path.path", path, findings);
            check_run(site, "a path run", run, ANGULAR_RUN, true, bound, findings);
            check_orientation(site, orientation.as_ref(), true, findings);
        }
        PatternLayout::Dimensions(runs) => {
            if runs.is_empty() {
                error(
                    findings,
                    site.entity,
                    format_args!("PatternLayout::Dimensions is empty"),
                );
            }
            for run in runs {
                if run.increments.is_empty() {
                    error(
                        findings,
                        site.entity,
                        format_args!("a Dimensions run has no increment"),
                    );
                }
                site.bound(findings, |values| {
                    (values.get(&run.count)? == 0)
                        .then(|| "a Dimensions run has a count of 0".to_owned())
                });
                check_increments(site, &run.increments, bound, findings);
            }
        }
        PatternLayout::Table(table) => {
            let distinct: BTreeSet<&str> =
                table.parameters.iter().map(ParameterId::as_str).collect();
            if table.parameters.is_empty() || distinct.len() != table.parameters.len() {
                error(
                    findings,
                    site.entity,
                    format_args!("a Table's parameters are empty or repeat"),
                );
            }
            let mut rows = BTreeSet::new();
            for row in &table.rows {
                if row.values.len() != table.parameters.len() {
                    error(
                        findings,
                        site.entity,
                        format_args!(
                            "a Table row holds {} values for {} parameters",
                            row.values.len(),
                            table.parameters.len()
                        ),
                    );
                }
                if !rows.insert(row.instance) {
                    error(
                        findings,
                        site.entity,
                        format_args!("Table row instance {} repeats", row.instance),
                    );
                }
            }
        }
        PatternLayout::Composite(stages) => {
            if stages.len() < 2 {
                error(
                    findings,
                    site.entity,
                    format_args!("a Composite layout has fewer than two stages"),
                );
            }
            let mut made: Option<u64> = Some(1);
            for (at, stage) in stages.iter().enumerate() {
                if let CompositeStage::Scale(ScaleLayout {
                    count: Dimension::Literal(count),
                    ..
                }) = stage
                {
                    if at > 0
                        && made.is_some_and(|made| *count == 0 || made % u64::from(*count) != 0)
                    {
                        error(
                            findings,
                            site.entity,
                            format_args!("a Scale stage's count does not divide the instances the stages before it make"),
                        );
                    }
                }
                made = made.zip(instances(stage)).map(|(made, stage)| made * stage);
                match stage {
                    CompositeStage::Linear(linear) => check_linear(site, linear, bound, findings),
                    CompositeStage::Circular(circular) => {
                        check_circular(site, circular, bound, findings);
                    }
                    CompositeStage::Scale(scale) => check_scale(site, scale, findings),
                    CompositeStage::Mirror(_) | CompositeStage::Native(_) => {}
                }
            }
        }
        PatternLayout::Scale(scale) => check_scale(site, scale, findings),
        PatternLayout::Follow(followed) => check_follow(index, site, followed, findings),
        PatternLayout::Chain(chain) => {
            if !component {
                error(
                    findings,
                    site.entity,
                    format_args!("PatternLayout::Chain occurs only in a ComponentPattern"),
                );
            }
            super::options::check_path(site, "ChainLayout.path", &chain.path, findings);
            if let crate::design::ops::shared::ChainLinks::Connected(links) = &chain.links {
                for link in links {
                    if link.pins[0] == link.pins[1] {
                        error(
                            findings,
                            site.entity,
                            format_args!("a chain link's two pins are one axis"),
                        );
                    }
                }
            }
        }
        PatternLayout::Points { .. } | PatternLayout::Mirror(_) | PatternLayout::Native(_) => {}
    }
}

const LINEAR_RUN: RunKind = RunKind {
    fit: false,
    up_to: true,
};

const ANGULAR_RUN: RunKind = RunKind {
    fit: true,
    up_to: false,
};

const RADIAL_RUN: RunKind = RunKind {
    fit: false,
    up_to: false,
};

fn check_linear(
    site: Site<'_>,
    layout: &LinearLayout,
    bound: Option<&BTreeSet<String>>,
    findings: &mut Vec<Finding>,
) {
    if layout.seed_only.is_some() && layout.second.is_none() {
        error(
            findings,
            site.entity,
            format_args!("PatternLayout::Linear.seed_only occurs without second"),
        );
    }
    for run in std::iter::once(&layout.first).chain(&layout.second) {
        check_run(
            site,
            "a linear run",
            &run.run,
            LINEAR_RUN,
            false,
            bound,
            findings,
        );
    }
}

fn check_circular(
    site: Site<'_>,
    layout: &CircularLayout,
    bound: Option<&BTreeSet<String>>,
    findings: &mut Vec<Finding>,
) {
    check_run(
        site,
        "a circular run",
        &layout.angular,
        ANGULAR_RUN,
        false,
        bound,
        findings,
    );
    if let Some(radial) = &layout.radial {
        check_run(
            site,
            "a radial run",
            radial,
            RADIAL_RUN,
            false,
            bound,
            findings,
        );
    }
    check_orientation(site, layout.orientation.as_ref(), false, findings);
}

fn check_scale(site: Site<'_>, layout: &ScaleLayout, findings: &mut Vec<Finding>) {
    site.bound(findings, |values| {
        (values.get(&layout.count)? < 2).then(|| "a Scale layout's count is below 2".to_owned())
    });
}

/// The instances a stage makes, where its counts are literal.
fn instances(stage: &CompositeStage) -> Option<u64> {
    let run = |spacing: &Spacing<_>| literal_count(spacing).map(u64::from);
    match stage {
        CompositeStage::Linear(LinearLayout { first, second, .. }) => {
            let first = literal_count(&first.run.spacing).map(u64::from)?;
            match second {
                Some(second) => Some(first * literal_count(&second.run.spacing).map(u64::from)?),
                None => Some(first),
            }
        }
        CompositeStage::Circular(CircularLayout {
            angular, radial, ..
        }) => {
            let angular = run(&angular.spacing)?;
            match radial {
                Some(radial) => Some(angular * literal_count(&radial.spacing).map(u64::from)?),
                None => Some(angular),
            }
        }
        CompositeStage::Scale(ScaleLayout {
            count: Dimension::Literal(count),
            ..
        }) => Some(u64::from(*count)),
        CompositeStage::Scale(_) | CompositeStage::Mirror(_) | CompositeStage::Native(_) => None,
    }
}

fn check_run<Q>(
    site: Site<'_>,
    field: &str,
    run: &PatternRun<Q>,
    kind: RunKind,
    path: bool,
    bound: Option<&BTreeSet<String>>,
    findings: &mut Vec<Finding>,
) {
    check_spacing(site, field, &run.spacing, kind, findings);
    if matches!(run.sides, Some(RunSides::Against)) && !path {
        error(
            findings,
            site.entity,
            format_args!(
                "RunSides::Against occurs in {field}; it occurs only in a Path layout's run"
            ),
        );
    }
    if let Some(RunSides::Reverse(spacing)) = &run.sides {
        check_spacing(site, field, spacing, kind, findings);
    }
    if let (Some(seed), Some(count)) = (&run.seed, spacing_count(&run.spacing)) {
        site.bound(findings, |values| {
            let (seed, count) = (values.get(seed)?, values.get(count)?);
            (seed >= count).then(|| format!("{field}'s seed {seed} is not below its count {count}"))
        });
    }
    check_increments(site, &run.increments, bound, findings);
}

fn check_increments(
    site: Site<'_>,
    increments: &[crate::design::ops::shared::Increment],
    bound: Option<&BTreeSet<String>>,
    findings: &mut Vec<Finding>,
) {
    let mut seen = BTreeSet::new();
    for increment in increments {
        let parameter = increment.parameter.as_str();
        if !seen.insert(parameter) {
            error(
                findings,
                site.entity,
                format_args!("increment parameter {parameter} appears twice in one run"),
            );
        }
        if bound.is_some_and(|bound| !bound.contains(parameter)) {
            error(
                findings,
                site.entity,
                format_args!("increment parameter {parameter} is bound by no dimension of a seed feature's definition"),
            );
        }
    }
}

fn check_orientation(
    site: Site<'_>,
    orientation: Option<&InstanceOrientation>,
    path: bool,
    findings: &mut Vec<Finding>,
) {
    if let Some(InstanceOrientation::Rolled { reference_axis, .. }) = orientation {
        if !path {
            error(
                findings,
                site.entity,
                format_args!("InstanceOrientation::Rolled occurs only in a Path layout"),
            );
        }
        if *reference_axis == crate::design::construction::FrameAxis::X {
            error(
                findings,
                site.entity,
                format_args!("InstanceOrientation::Rolled's reference_axis is X; it is Y or Z"),
            );
        }
    }
}

/// `Follow` holds one pick of a pattern, which is neither the pattern
/// itself nor one that follows it.
fn check_follow(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    followed: &OrderedSelection,
    findings: &mut Vec<Finding>,
) {
    let Some(picks) = picks::named(followed) else {
        return;
    };
    if picks.len() != 1 {
        error(
            findings,
            site.entity,
            format_args!(
                "PatternLayout::Follow holds {} picks; it holds one",
                picks.len()
            ),
        );
        return;
    }
    let pick = &picks[0];
    let PickTarget::Feature(target) = &pick.target else {
        if !matches!(pick.target, PickTarget::Native { .. }) {
            error(
                findings,
                site.entity,
                format_args!("PatternLayout::Follow picks other than a pattern feature"),
            );
        }
        return;
    };
    if pick.context.is_some() {
        return;
    }
    let mut at = target.as_str();
    for _ in 0..=index.features.len() {
        if at == site.feature {
            error(
                findings,
                site.entity,
                format_args!("PatternLayout::Follow reaches the pattern itself"),
            );
            return;
        }
        let layout = match index.features.get(at).map(|f| &f.definition.operation) {
            Some(FeatureOperation::Pattern(pattern)) => &pattern.layout,
            Some(FeatureOperation::ComponentPattern(pattern)) => &pattern.layout,
            Some(
                FeatureOperation::Unresolved {
                    family: OperationFamily::Pattern | OperationFamily::ComponentPattern,
                }
                | FeatureOperation::Native { .. },
            ) => return,
            _ => {
                if at == target.as_str() {
                    error(
                        findings,
                        site.entity,
                        format_args!("PatternLayout::Follow picks {at}, which is no pattern"),
                    );
                }
                return;
            }
        };
        let PatternLayout::Follow(next) = layout else {
            return;
        };
        let Some([next]) = picks::named(next) else {
            return;
        };
        let PickTarget::Feature(next) = &next.target else {
            return;
        };
        at = next.as_str();
    }
}

pub(super) fn check_template_instance(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    instance: &TemplateInstance,
    findings: &mut Vec<Finding>,
) {
    let repeated = |roles: Vec<&str>| {
        let distinct: BTreeSet<&str> = roles.iter().copied().collect();
        distinct.len() != roles.len()
    };
    if repeated(
        instance
            .inputs
            .iter()
            .filter_map(|input| input.role.as_ref().map(NonBlankString::as_str))
            .collect(),
    ) || repeated(
        instance
            .parameters
            .iter()
            .filter_map(|parameter| parameter.role.as_ref().map(NonBlankString::as_str))
            .collect(),
    ) {
        error(
            findings,
            site.entity,
            format_args!("a TemplateInstance repeats a role within its inputs or its parameters"),
        );
    }
    for parameter in &instance.parameters {
        let scope = index
            .parameters
            .get(parameter.parameter.as_str())
            .map(|p| p.scope.as_str());
        if scope.is_none() || (site.scope.is_some() && scope != site.scope) {
            error(
                findings,
                site.entity,
                format_args!(
                    "template parameter {} is no parameter of the instance's scope",
                    parameter.parameter.as_str()
                ),
            );
        }
    }
    let listed: Vec<&str> = instance.members.iter().map(FeatureId::as_str).collect();
    check_generated_members(index, site, "template member", &listed, findings);
}

/// Every `TemplateInput` pick names an input of an instance that lists the
/// picking feature as a member.
pub(super) fn check_template_inputs(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    for feature in &index.ir.design.features {
        let Ok(wire) = serde_json::to_value(&feature.definition) else {
            continue;
        };
        for found in super::tagged(&wire, "template_input") {
            let Ok(PickTarget::TemplateInput { instance, input }) =
                serde_json::from_value::<PickTarget>(found.clone())
            else {
                continue;
            };
            let valid = match index
                .features
                .get(instance.as_str())
                .map(|f| &f.definition.operation)
            {
                Some(FeatureOperation::TemplateInstance(template)) => {
                    template.members.contains(&feature.id)
                        && usize::try_from(input).is_ok_and(|input| input < template.inputs.len())
                }
                _ => false,
            };
            if !valid {
                error(
                    findings,
                    feature.id.as_str(),
                    format_args!(
                        "TemplateInput picks input {input} of {}, which is no input of an instance that lists this feature",
                        instance.as_str()
                    ),
                );
            }
        }
    }
}

/// Part 10 (§6.5): "`target` is `None` only with `Join`, `Intersect`, or
/// `Compound`, or where the definition states `assembly`". Where it states
/// `assembly`, `target` is `None` (§6.7), which `check_assembly_feature`
/// checks.
pub(super) fn check_combine(site: Site<'_>, combine: &Combine, findings: &mut Vec<Finding>) {
    if matches!(combine.op, Some(BooleanOp::Sew)) {
        error(
            findings,
            site.entity,
            format_args!(
                "Combine.op is Sew; it is Join, Cut, Intersect, Assemble, UnionTrim, or Compound"
            ),
        );
    }
    if picks::is_empty(&combine.tools) {
        error(
            findings,
            site.entity,
            format_args!("Combine.tools is empty"),
        );
    }
    if let Some(target) = &combine.target {
        if picks::share_pick(target, &combine.tools) {
            error(
                findings,
                site.entity,
                format_args!("a pick is in both Combine.target and Combine.tools"),
            );
        }
        if matches!(combine.op, Some(BooleanOp::Compound)) {
            error(
                findings,
                site.entity,
                format_args!("a Compound combine states a target"),
            );
        }
    } else if !site.assembly
        && matches!(
            combine.op,
            Some(BooleanOp::Cut | BooleanOp::Assemble | BooleanOp::UnionTrim | BooleanOp::Sew)
        )
    {
        error(
            findings,
            site.entity,
            format_args!(
                "Combine.target is None with an op other than Join, Intersect, or Compound"
            ),
        );
    }
}

pub(super) fn check_split(site: Site<'_>, split: &Split, findings: &mut Vec<Finding>) {
    match &split.mode {
        SplitMode::ByTools {
            targets,
            tools,
            method,
            kept,
        } => {
            let allowed = |class| picks::is_body(class) || picks::is_face(class);
            match targets {
                Some(targets) => {
                    if let Some(class) = picks::refused(targets, allowed) {
                        error(
                            findings,
                            site.entity,
                            format_args!("ByTools.targets picks {class}; it picks bodies or faces"),
                        );
                    }
                }
                None if !site.assembly => error(
                    findings,
                    site.entity,
                    format_args!("ByTools.targets is None in a definition that states no assembly"),
                ),
                None => {}
            }
            let curves = match tools {
                SplitTool::Geometry {
                    tools: geometry, ..
                } => {
                    let datum = |class| matches!(class, Class::Datum | Class::Principal(_));
                    if picks::classes(geometry).into_iter().any(datum) {
                        error(
                            findings,
                            site.entity,
                            format_args!("SplitTool::Geometry picks a datum or principal entity, which is SplitTool::Plane"),
                        );
                    }
                    picks::classes(geometry)
                }
                SplitTool::Plane(_) => Vec::new(),
                SplitTool::Silhouette(_) => {
                    if method.is_some() {
                        error(
                            findings,
                            site.entity,
                            format_args!("a Silhouette split states a method"),
                        );
                    }
                    if matches!(kept, Some(Kept::Side { .. } | Kept::Pieces { .. })) {
                        error(
                            findings,
                            site.entity,
                            format_args!(
                                "a Silhouette split keeps a side or pieces; it keeps All or Native"
                            ),
                        );
                    }
                    Vec::new()
                }
            };
            let curve = |class: &Class| {
                matches!(
                    class,
                    Class::Name(
                        crate::design::names::NameKind::Curve
                            | crate::design::names::NameKind::Edge
                    ) | Class::SketchEntity
                        | Class::SpatialSketchEntity
                        | Class::Sketch
                        | Class::Unknown
                )
            };
            let sketched = |class: &Class| {
                matches!(class, Class::SketchEntity | Class::Sketch | Class::Unknown)
            };
            if let Some(SplitMethod::Project(direction)) = method {
                if !curves.iter().any(curve) {
                    error(
                        findings,
                        site.entity,
                        format_args!("SplitMethod::Project occurs where no tool is a curve"),
                    );
                }
                if matches!(**direction, ProjectionDirection::SketchNormal)
                    && !curves.iter().any(sketched)
                {
                    error(
                        findings,
                        site.entity,
                        format_args!("SketchNormal occurs where no tool curve belongs to a sketch"),
                    );
                }
            }
            if let Some(kept) = kept {
                check_kept(site, kept, findings);
            }
        }
        SplitMode::Mutual { elements, kept } => {
            let count = picks::count(elements);
            if count.is_some_and(|count| count < 2) {
                error(
                    findings,
                    site.entity,
                    format_args!("SplitMode::Mutual.elements holds fewer than two picks"),
                );
            }
            match kept {
                Some(Kept::Side { side }) => {
                    if count.is_some_and(|count| side.len() != count) {
                        error(
                            findings,
                            site.entity,
                            format_args!(
                                "Kept::Side holds {} entries for {} elements",
                                side.len(),
                                count.unwrap_or_default()
                            ),
                        );
                    }
                }
                Some(kept) => check_kept(site, kept, findings),
                None => {}
            }
        }
    }
}

fn check_kept<S>(site: Site<'_>, kept: &Kept<S>, findings: &mut Vec<Finding>) {
    let Kept::Pieces { keep, remove } = kept else {
        return;
    };
    if keep.is_none() && remove.is_none() {
        error(
            findings,
            site.entity,
            format_args!("Kept::Pieces states neither keep nor remove"),
        );
    }
    for list in [keep, remove].into_iter().flatten() {
        let Some(picks) = picks::named(list) else {
            continue;
        };
        let fragments: Vec<_> = picks
            .iter()
            .filter_map(|pick| match &pick.domain {
                Some(PickDomain::Fragment(fragment)) => Some(fragment),
                _ => None,
            })
            .collect();
        let counts: BTreeSet<u32> = fragments.iter().map(|f| f.count).collect();
        if counts.len() > 1 || fragments.iter().any(|f| f.index >= f.count) {
            error(
                findings,
                site.entity,
                format_args!("piece picks of one list do not share one fragment count, or an index is not below it"),
            );
        }
    }
}

pub(super) fn check_transform_bodies(
    site: Site<'_>,
    transform: &TransformBodies,
    findings: &mut Vec<Finding>,
) {
    let allowed = |class| picks::is_body(class) || class == Class::Component;
    if let Some(class) = picks::refused(&transform.bodies, allowed) {
        error(
            findings,
            site.entity,
            format_args!("TransformBodies.bodies picks {class}; it picks bodies or components"),
        );
    }
    check_transformation(site, &transform.transformation, findings);
    if let Some(TransformMode::Copy { count, divisions }) = &transform.mode {
        site.bound(findings, |values| {
            (values.get(count)? == 0).then(|| "Copy.count is 0; it is at least 1".to_owned())
        });
        if let Some(divisions) = divisions {
            site.bound(findings, |values| {
                (values.get(divisions)? == 0)
                    .then(|| "Copy.divisions is 0; it is at least 1".to_owned())
            });
        }
        let divided = match divisions {
            Some(Dimension::Literal(divisions)) => *divisions > 1,
            Some(_) => true,
            None => false,
        };
        let linear = transform.transformation.steps.iter().all(|step| {
            matches!(
                step,
                TransformStep::Translate { .. }
                    | TransformStep::Components { .. }
                    | TransformStep::PointToPoint { .. }
                    | TransformStep::Rotate { .. }
            )
        });
        if divided && !linear && matches!(divisions, Some(Dimension::Literal(_))) {
            error(
                findings,
                site.entity,
                format_args!("divisions above 1 occur where a step is other than Translate, Components, PointToPoint, or Rotate"),
            );
        }
    }
}

fn check_transformation(
    site: Site<'_>,
    transformation: &Transformation,
    findings: &mut Vec<Finding>,
) {
    if transformation.steps.is_empty() {
        error(
            findings,
            site.entity,
            format_args!("Transformation.steps is empty"),
        );
    }
    for step in &transformation.steps {
        let TransformStep::Constrained { mates } = step else {
            continue;
        };
        if mates.is_empty() {
            error(
                findings,
                site.entity,
                format_args!("a Constrained step has no mate"),
            );
        }
        for mate in mates {
            if !matches!(
                mate.kind,
                JointKind::Coincident { .. }
                    | JointKind::Parallel
                    | JointKind::Perpendicular
                    | JointKind::Tangent { .. }
                    | JointKind::Concentric
                    | JointKind::Distance { .. }
                    | JointKind::Angle { .. }
            ) {
                error(
                    findings,
                    site.entity,
                    format_args!("a Constrained step's mate is of a kind other than Coincident, Parallel, Perpendicular, Tangent, Concentric, Distance, or Angle"),
                );
            }
            assembly::check_connectors(site, &mate.connectors, &mate.kind, findings);
        }
    }
}

pub(super) fn check_flex(site: Site<'_>, flex: &Flex, findings: &mut Vec<Finding>) {
    if let Some(class) = picks::refused(&flex.bodies, picks::is_body) {
        error(
            findings,
            site.entity,
            format_args!("Flex.bodies picks {class}, not a body"),
        );
    }
}

fn check_faces(
    site: Site<'_>,
    field: &str,
    faces: &crate::design::selection::SetSelection,
    findings: &mut Vec<Finding>,
) {
    if let Some(class) = picks::refused(faces, picks::is_face) {
        error(
            findings,
            site.entity,
            format_args!("{field} picks {class}, not a face"),
        );
    }
}

pub(super) fn check_move_faces(site: Site<'_>, faces: &MoveFaces, findings: &mut Vec<Finding>) {
    check_faces(site, "MoveFaces.faces", &faces.faces, findings);
    check_transformation(site, &faces.transformation, findings);
}

pub(super) fn check_delete_faces(site: Site<'_>, faces: &DeleteFaces, findings: &mut Vec<Finding>) {
    check_faces(site, "DeleteFaces.faces", &faces.faces, findings);
}

pub(super) fn check_replace_faces(
    site: Site<'_>,
    faces: &ReplaceFaces,
    findings: &mut Vec<Finding>,
) {
    check_faces(site, "ReplaceFaces.faces", &faces.faces, findings);
    if picks::is_empty(&faces.replacement) {
        error(
            findings,
            site.entity,
            format_args!("ReplaceFaces.replacement is empty"),
        );
    }
    if picks::share_pick(&faces.faces, &faces.replacement) {
        error(
            findings,
            site.entity,
            format_args!("ReplaceFaces.replacement shares a pick with faces"),
        );
    }
}

pub(super) fn check_graft(site: Site<'_>, graft: &Graft, findings: &mut Vec<Finding>) {
    if picks::count(&graft.body).is_some_and(|count| count != 1)
        || picks::refused(&graft.body, picks::is_body).is_some()
    {
        error(
            findings,
            site.entity,
            format_args!("Graft.body picks other than one solid body"),
        );
    }
    let allowed = |class| picks::is_body(class) || picks::is_face(class);
    if let Some(class) = picks::refused(&graft.sheet, allowed) {
        error(
            findings,
            site.entity,
            format_args!("Graft.sheet picks {class}; it picks sheet bodies or faces"),
        );
    }
}
