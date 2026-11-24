// SPDX-License-Identifier: Apache-2.0
//! The shared option types (§6): native keys, profiles and paths, outputs
//! and Booleans, spans and limits, and wall thicknesses.

use serde_json::Value;

use super::picks::{self, Class};
use super::{error, Site};
use crate::design::container::{Container, OrderedKind};
use crate::design::names::NameKind;
use crate::design::ops::shared::{Limit, Span};
use crate::design::options::{
    BodyForm, Boolean, BooleanOp, LimitOffset, PathRef, ProfileRef, SheetOutput, SolidOutput,
    Solidify, SymmetricMeasure, Targets, Thickness,
};
use crate::report::check::Finding;
use crate::scalar::{Angle, Length};

use super::super::records::{DesignRecords, MemberKey};

/// No key is in both `native_options` and `source_settings` of one record.
pub(super) fn check_native_keys(wire: &Value, findings: &mut Vec<Finding>) {
    let mut pending = vec![(wire, "design")];
    while let Some((value, owner)) = pending.pop() {
        match value {
            Value::Object(map) => {
                let owner = map.get("id").and_then(Value::as_str).unwrap_or(owner);
                let keys = |field: &str| {
                    map.get(field)
                        .and_then(Value::as_object)
                        .map(|options| options.keys().map(String::as_str).collect::<Vec<_>>())
                        .unwrap_or_default()
                };
                let native = keys("native_options");
                let settings = keys("source_settings");
                for key in native.iter().filter(|key| settings.contains(key)) {
                    error(
                        findings,
                        owner,
                        format_args!(
                            "key {key:?} is in both native_options and source_settings of one record"
                        ),
                    );
                }
                pending.extend(map.values().map(|value| (value, owner)));
            }
            Value::Array(items) => pending.extend(items.iter().map(|value| (value, owner))),
            _ => {}
        }
    }
}

/// A profile holds at least one pick or is `Unread`, and its picks target
/// what a profile can be made of.
pub(super) fn check_profile(
    site: Site<'_>,
    field: &str,
    profile: &ProfileRef,
    findings: &mut Vec<Finding>,
) {
    if picks::is_empty(profile) {
        error(
            findings,
            site.entity,
            format_args!("{field} holds no pick; a profile holds at least one or is Unread"),
        );
    }
    let allowed = |class: Class| {
        matches!(
            class,
            Class::Sketch
                | Class::SketchRegion
                | Class::SketchEntity
                | Class::SpatialSketchEntity
                | Class::FeatureResult
                | Class::Name(
                    NameKind::Face | NameKind::Edge | NameKind::Curve | NameKind::Surface
                )
        )
    };
    if let Some(class) = picks::refused(profile, allowed) {
        error(
            findings,
            site.entity,
            format_args!("{field} picks {class}, which no profile is made of"),
        );
    }
}

/// A path holds at least one pick or is `Unread`, and its picks target
/// curves.
pub(super) fn check_path(site: Site<'_>, field: &str, path: &PathRef, findings: &mut Vec<Finding>) {
    if picks::is_empty(path) {
        error(
            findings,
            site.entity,
            format_args!("{field} holds no pick; a path holds at least one or is Unread"),
        );
    }
    let allowed = |class: Class| {
        matches!(
            class,
            Class::Sketch
                | Class::SketchEntity
                | Class::SpatialSketchEntity
                | Class::FeatureResult
                | Class::Name(NameKind::Edge | NameKind::Curve)
        )
    };
    if let Some(class) = picks::refused(path, allowed) {
        error(
            findings,
            site.entity,
            format_args!("{field} picks {class}, which no path is made of"),
        );
    }
}

/// Where a `Boolean` occurs.
#[derive(Clone, Copy)]
pub(super) enum BooleanAt<'a> {
    /// In a `SolidOutput`, with its body form.
    Solid(Option<&'a BodyForm>),
    /// In a `SheetOutput`, with its solidification.
    Sheet(Option<&'a Solidify>),
    /// A feature's own `boolean` or `targets`, outside an output.
    Own,
}

pub(super) fn check_solid_output(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    output: &SolidOutput,
    findings: &mut Vec<Finding>,
) {
    if let Some(boolean) = &output.boolean {
        check_boolean(
            index,
            site,
            boolean,
            BooleanAt::Solid(output.body.as_ref()),
            findings,
        );
    }
    if let Some(BodyForm::Thin(wall)) = &output.body {
        check_thickness(site, &wall.thickness, findings);
    }
}

pub(super) fn check_sheet_output(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    output: &SheetOutput,
    findings: &mut Vec<Finding>,
) {
    if let Some(boolean) = &output.boolean {
        check_boolean(
            index,
            site,
            boolean,
            BooleanAt::Sheet(output.solid.as_ref()),
            findings,
        );
    }
}

/// `Assemble`, `Compound`, and `UnionTrim` occur only in `Combine`; `Sew`
/// only in a sheet output; `Selected` targets pick bodies; a member of a
/// `Body` container names no targets.
pub(super) fn check_boolean(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    boolean: &Boolean,
    at: BooleanAt<'_>,
    findings: &mut Vec<Finding>,
) {
    let Boolean::Apply { op, targets } = boolean else {
        return;
    };
    match op {
        BooleanOp::Assemble | BooleanOp::Compound | BooleanOp::UnionTrim => error(
            findings,
            site.entity,
            format_args!("Boolean op {} occurs only as a Combine's op", op_name(op)),
        ),
        BooleanOp::Sew => {
            let sheet = match at {
                BooleanAt::Solid(body) => matches!(body, None | Some(BodyForm::Sheet)),
                BooleanAt::Sheet(solid) => matches!(solid, None | Some(Solidify::Sheet)),
                BooleanAt::Own => false,
            };
            if !sheet {
                error(
                    findings,
                    site.entity,
                    format_args!("Sew occurs only in an output whose result is a sheet"),
                );
            }
        }
        _ => {}
    }
    if let Some(targets) = targets {
        check_targets(index, site, targets, findings);
    }
}

pub(super) fn check_targets(
    index: &DesignRecords<'_>,
    site: Site<'_>,
    targets: &Targets,
    findings: &mut Vec<Finding>,
) {
    if let Targets::Selected(selection) = targets {
        if let Some(class) = picks::refused(selection, picks::is_body) {
            error(
                findings,
                site.entity,
                format_args!("Targets::Selected picks {class}; its picks resolve to bodies"),
            );
        }
    }
    if in_body_container(index, site.feature) {
        error(
            findings,
            site.entity,
            format_args!("a member of a Body container states targets; its target is its body"),
        );
    }
}

fn in_body_container(index: &DesignRecords<'_>, feature: &str) -> bool {
    let Some(parent) = index.parents.get(&MemberKey::Feature(feature)) else {
        return false;
    };
    matches!(
        index.containers.get(parent),
        Some(Container::Ordered {
            kind: OrderedKind::Body,
            ..
        })
    )
}

const fn op_name(op: &BooleanOp) -> &'static str {
    match op {
        BooleanOp::Join => "Join",
        BooleanOp::Cut => "Cut",
        BooleanOp::Intersect => "Intersect",
        BooleanOp::Assemble => "Assemble",
        BooleanOp::Sew => "Sew",
        BooleanOp::Compound => "Compound",
        BooleanOp::UnionTrim => "UnionTrim",
        BooleanOp::Native(_) => "Native",
    }
}

/// A length span: its limits, and a nonzero symmetric amount.
pub(super) fn check_length_span(
    site: Site<'_>,
    field: &str,
    span: &Span<Length>,
    findings: &mut Vec<Finding>,
) {
    match span {
        Span::Between { start, end } => {
            check_between(site, field, start, end, findings);
            check_limit(site, field, start, false, findings);
            check_limit(site, field, end, false, findings);
        }
        Span::Symmetric { amount, .. } => site.bound(findings, |values| {
            (values.get(amount)?.get() == 0.0)
                .then(|| format!("{field} is a symmetric span of zero"))
        }),
    }
}

/// An angular span: its limits, and a symmetric amount that is not zero and
/// at most π per side or 2π in total.
pub(super) fn check_angle_span(
    site: Site<'_>,
    field: &str,
    span: &Span<Angle>,
    findings: &mut Vec<Finding>,
) {
    match span {
        Span::Between { start, end } => {
            check_between(site, field, start, end, findings);
            check_limit(site, field, start, true, findings);
            check_limit(site, field, end, true, findings);
        }
        Span::Symmetric { amount, measure } => {
            let most = match measure {
                Some(SymmetricMeasure::PerSide) => std::f64::consts::PI,
                Some(SymmetricMeasure::Total) | None => std::f64::consts::TAU,
            };
            site.bound(findings, |values| {
                let value = values.get(amount)?.get();
                (value == 0.0 || value.abs() > most).then(|| {
                    format!("{field} is a symmetric angular span of {value}; an amount is nonzero and at most π per side or 2π in total")
                })
            });
        }
    }
}

fn check_between<Q>(
    site: Site<'_>,
    field: &str,
    start: &Limit<Q>,
    end: &Limit<Q>,
    findings: &mut Vec<Finding>,
) {
    if matches!((start, end), (Limit::AtOrigin, Limit::AtOrigin)) {
        error(
            findings,
            site.entity,
            format_args!("{field} is a Span::Between with AtOrigin at both start and end"),
        );
    }
}

/// `ToGeometry` targets faces, surfaces, or bodies, and `far` only bodies;
/// `LimitOffset::Angle` occurs only in an angular limit and `Translated`
/// only in a length limit.
pub(super) fn check_limit<Q>(
    site: Site<'_>,
    field: &str,
    limit: &Limit<Q>,
    angular: bool,
    findings: &mut Vec<Finding>,
) {
    let Limit::ToGeometry {
        target,
        offset,
        far,
        ..
    } = limit
    else {
        return;
    };
    let allowed = |class: Class| {
        matches!(
            class,
            Class::Name(NameKind::Face | NameKind::Surface | NameKind::Body) | Class::FeatureResult
        )
    };
    if let Some(class) = picks::refused(target, allowed) {
        error(
            findings,
            site.entity,
            format_args!(
                "{field} ToGeometry.target picks {class}; it picks faces, surfaces, or bodies"
            ),
        );
    }
    if far.is_some() && picks::refused(target, picks::is_body).is_some() {
        error(
            findings,
            site.entity,
            format_args!("{field} ToGeometry.far occurs only where target picks bodies"),
        );
    }
    match offset {
        Some(LimitOffset::Angle(_)) if !angular => error(
            findings,
            site.entity,
            format_args!("{field} LimitOffset::Angle occurs only in an angular limit"),
        ),
        Some(LimitOffset::Translated(_)) if angular => error(
            findings,
            site.entity,
            format_args!("{field} LimitOffset::Translated occurs only in a length limit"),
        ),
        _ => {}
    }
}

/// Two `Offsets` sum to a positive wall.
pub(super) fn check_thickness(site: Site<'_>, thickness: &Thickness, findings: &mut Vec<Finding>) {
    if let Thickness::Offsets { outside, inside } = thickness {
        site.bound(findings, |values| {
            (values.get(outside)?.get() + values.get(inside)?.get() <= 0.0)
                .then(|| "Thickness::Offsets has outside + inside not above zero".to_owned())
        });
    }
}
