// SPDX-License-Identifier: Apache-2.0
//! The `CadSketch` schema (spec §18.2): points, curves, and constraints of a
//! planar sketch, in the sketch prim's local units.

use crate::schema::{boolean, integer, integers, reals, token, vector, Issues};
use crate::stage::{Prim, Stage};

/// The constraint types (spec §18.2).
pub(crate) const CONSTRAINT_TYPES: &[&str] = &[
    "CadCoincidentConstraint",
    "CadHorizontalConstraint",
    "CadVerticalConstraint",
    "CadParallelConstraint",
    "CadPerpendicularConstraint",
    "CadFixedConstraint",
    "CadPointOnCurveConstraint",
    "CadSymmetricConstraint",
    "CadEqualConstraint",
    "CadTangentConstraint",
    "CadRadiusConstraint",
    "CadDistanceConstraint",
    "CadAngleConstraint",
];

/// Whether a prim type belongs to a sketch.
pub(crate) fn is_sketch_type(type_name: Option<&str>) -> bool {
    matches!(
        type_name,
        Some(
            "CadSketch"
                | "CadSketchPoint"
                | "CadSketchLine"
                | "CadSketchArc"
                | "CadSketchCircle"
                | "CadSketchSpline"
        )
    ) || type_name.is_some_and(|name| CONSTRAINT_TYPES.contains(&name))
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PointData {
    pub(crate) path: String,
    pub(crate) position: [f64; 2],
    pub(crate) construction: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CurveKind {
    Line {
        start: String,
        end: String,
    },
    Arc {
        start: String,
        end: String,
        center: String,
        radius: f64,
        clockwise: bool,
    },
    Circle {
        center: String,
        radius: f64,
    },
    Spline {
        degree: u32,
        /// The full knot vector, each knot repeated by its multiplicity.
        knots: Vec<f64>,
        poles: Vec<[f64; 2]>,
        closed: bool,
        fit_rule: Option<String>,
        /// The control and fit point prims, which belong to the spline.
        owned: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CurveData {
    pub(crate) path: String,
    pub(crate) kind: CurveKind,
    pub(crate) construction: bool,
    /// The datum role, when it is not `none`.
    pub(crate) role: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ConstraintData {
    pub(crate) path: String,
    /// The type name without the `Cad` prefix and `Constraint` suffix.
    pub(crate) kind: String,
    pub(crate) constrained: Vec<String>,
    pub(crate) reference: Vec<String>,
    pub(crate) value: Option<f64>,
    pub(crate) driving: bool,
    /// The `CadParameter` prim that the dimension states.
    pub(crate) parameter: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct SketchData {
    pub(crate) points: Vec<PointData>,
    pub(crate) curves: Vec<CurveData>,
    pub(crate) constraints: Vec<ConstraintData>,
    pub(crate) tolerance: Option<f64>,
    /// Prims whose data is malformed, with the reason.
    pub(crate) rejected: Vec<String>,
}

fn target(prim: &Prim<'_>, name: &str) -> Option<String> {
    match prim.relationship_targets(name).as_slice() {
        [target] => Some(target.clone()),
        _ => None,
    }
}

fn scalar(prim: &Prim<'_>, name: &str, issues: &mut Issues) -> Option<f64> {
    vector(prim, name, 1, issues).and_then(|value| value.first().copied())
}

fn construction(prim: &Prim<'_>, kind: &str, issues: &mut Issues) -> bool {
    boolean(prim, &format!("sketch:{kind}:construction"), issues).unwrap_or(false)
}

fn curve(prim: &Prim<'_>, type_name: &str, issues: &mut Issues) -> Result<CurveKind, String> {
    let need =
        |value: Option<String>, name: &str| value.ok_or(format!("`{name}` has no one target"));
    Ok(match type_name {
        "CadSketchLine" => CurveKind::Line {
            start: need(
                target(prim, "sketch:line:startPoint"),
                "sketch:line:startPoint",
            )?,
            end: need(target(prim, "sketch:line:endPoint"), "sketch:line:endPoint")?,
        },
        "CadSketchArc" => CurveKind::Arc {
            start: need(
                target(prim, "sketch:arc:startPoint"),
                "sketch:arc:startPoint",
            )?,
            end: need(target(prim, "sketch:arc:endPoint"), "sketch:arc:endPoint")?,
            center: need(
                target(prim, "sketch:arc:centerPoint"),
                "sketch:arc:centerPoint",
            )?,
            radius: scalar(prim, "sketch:arc:radius", issues)
                .filter(|radius| radius.is_finite() && *radius > 0.0)
                .ok_or("`sketch:arc:radius` is not positive")?,
            clockwise: match token(prim, "sketch:arc:sweep", issues).as_deref() {
                Some("ccw") => false,
                Some("cw") => true,
                _ => return Err("`sketch:arc:sweep` is not `ccw` or `cw`".to_owned()),
            },
        },
        "CadSketchCircle" => CurveKind::Circle {
            center: need(
                target(prim, "sketch:circle:centerPoint"),
                "sketch:circle:centerPoint",
            )?,
            radius: scalar(prim, "sketch:circle:radius", issues)
                .filter(|radius| radius.is_finite() && *radius > 0.0)
                .ok_or("`sketch:circle:radius` is not positive")?,
        },
        _ => {
            let degree = integer(prim, "sketch:spline:degree", issues)
                .and_then(|degree| u32::try_from(degree).ok())
                .filter(|degree| *degree >= 1)
                .ok_or("`sketch:spline:degree` is not positive")?;
            let poles: Vec<[f64; 2]> = reals(prim, "sketch:spline:poles", 2, issues)
                .unwrap_or_default()
                .chunks_exact(2)
                .map(|pair| [pair[0], pair[1]])
                .collect();
            let distinct = reals(prim, "sketch:spline:knots", 1, issues).unwrap_or_default();
            let multiplicities =
                integers(prim, "sketch:spline:multiplicities", issues).unwrap_or_default();
            if distinct.len() != multiplicities.len() {
                return Err("the knots and their multiplicities differ in count".to_owned());
            }
            let closed = token(prim, "sketch:spline:form", issues).as_deref() == Some("closed");
            let mut counts = Vec::new();
            for count in &multiplicities {
                counts.push(usize::try_from(*count).map_err(|_| "a multiplicity is negative")?);
            }
            let knots = if closed {
                periodic_knots(&distinct, &counts, degree, poles.len())?
            } else {
                clamped_knots(&distinct, &counts, degree, poles.len())?
            };
            let poles = if closed {
                // The unwrapped periodic form repeats the first `degree`
                // poles after the last.
                let wrap = usize::try_from(degree).map_err(|_| "the degree overflows")?;
                poles
                    .iter()
                    .chain(poles.iter().take(wrap))
                    .copied()
                    .collect()
            } else {
                poles
            };
            CurveKind::Spline {
                degree,
                knots,
                poles,
                closed,
                fit_rule: token(prim, "sketch:spline:fitRule", issues)
                    .filter(|rule| rule != "none"),
                owned: prim
                    .relationship_targets("sketch:spline:controlPoints")
                    .into_iter()
                    .chain(prim.relationship_targets("sketch:spline:fitPoints"))
                    .collect(),
            }
        }
    })
}

/// The full knot vector of a clamped spline: each knot repeated by its
/// multiplicity, one knot per pole plus the order.
fn clamped_knots(
    distinct: &[f64],
    counts: &[usize],
    degree: u32,
    poles: usize,
) -> Result<Vec<f64>, String> {
    let expected = usize::try_from(degree)
        .ok()
        .and_then(|degree| poles.checked_add(degree + 1))
        .ok_or("the knot count overflows")?;
    let total = counts
        .iter()
        .try_fold(0_usize, |total, count| total.checked_add(*count))
        .ok_or("the knot count overflows")?;
    if total != expected {
        return Err(format!(
            "the multiplicities sum to {total}, not the pole count plus the order, {expected}"
        ));
    }
    let mut knots = Vec::new();
    for (knot, count) in distinct.iter().zip(counts) {
        knots.extend(std::iter::repeat_n(*knot, *count));
    }
    Ok(knots)
}

/// The unwrapped knot vector of a periodic spline (spec §18.2): the flat
/// sequence of every knot but the last, by multiplicity, extended by the
/// period, and placed so that the span that starts at the last copy of the
/// first knot reads the first `degree + 1` poles.
fn periodic_knots(
    distinct: &[f64],
    counts: &[usize],
    degree: u32,
    poles: usize,
) -> Result<Vec<f64>, String> {
    let (Some(first), Some(last), Some(first_count)) =
        (distinct.first(), distinct.last(), counts.first())
    else {
        return Err("a closed spline has no knots".to_owned());
    };
    let period = last - first;
    let interior = counts
        .split_last()
        .map(|(_, rest)| rest)
        .unwrap_or_default();
    let total = interior
        .iter()
        .try_fold(0_usize, |total, count| total.checked_add(*count))
        .ok_or("the knot count overflows")?;
    if total != poles || poles == 0 || *first_count == 0 || period.is_nan() || period <= 0.0 {
        return Err(format!(
            "the multiplicities before the last sum to {total}, not the pole count {poles}"
        ));
    }
    let mut flat = Vec::new();
    for (knot, count) in distinct.iter().zip(interior) {
        flat.extend(std::iter::repeat_n(*knot, *count));
    }
    let degree = usize::try_from(degree).map_err(|_| "the degree overflows")?;
    let length = poles
        .checked_add(2 * degree + 1)
        .ok_or("the knot count overflows")?;
    // Knot `index` is flat knot `index − degree + first_count − 1`, wrapped
    // by the period; `base` whole periods keep the index non-negative.
    let base = degree / poles + 1;
    let mut knots = Vec::new();
    for index in 0..length {
        let shifted = index + poles * base + first_count - 1 - degree;
        let wraps = cadmpeg_core::convert::f64_from_index(shifted / poles)
            .ok_or("the knot count overflows")?
            - cadmpeg_core::convert::f64_from_index(base).ok_or("the knot count overflows")?;
        knots.push(flat[shifted % poles] + period * wraps);
    }
    Ok(knots)
}

/// Reads the sketch at `prim` and the prims it owns.
pub(crate) fn read(stage: &Stage<'_>, prim: &Prim<'_>, issues: &mut Issues) -> SketchData {
    let mut data = SketchData {
        tolerance: scalar(prim, "sketch:tolerance", issues),
        ..SketchData::default()
    };
    let mut queue: Vec<usize> = prim.children.clone();
    while let Some(index) = queue.pop() {
        let Some(child) = stage.prims.get(index) else {
            continue;
        };
        let Some(type_name) = child.type_name() else {
            queue.extend(child.children.iter().copied());
            continue;
        };
        match type_name {
            "CadSketchPoint" => match vector(child, "sketch:point:position", 2, issues) {
                Some(position) if position.iter().all(|value| value.is_finite()) => {
                    data.points.push(PointData {
                        path: child.path.clone(),
                        position: [position[0], position[1]],
                        construction: construction(child, "point", issues),
                    });
                }
                _ => data.rejected.push(format!(
                    "{}: `sketch:point:position` is not finite",
                    child.path
                )),
            },
            "CadSketchLine" | "CadSketchArc" | "CadSketchCircle" | "CadSketchSpline" => {
                let kind_name = type_name
                    .trim_start_matches("CadSketch")
                    .to_ascii_lowercase();
                match curve(child, type_name, issues) {
                    Ok(kind) => data.curves.push(CurveData {
                        path: child.path.clone(),
                        kind,
                        construction: construction(child, &kind_name, issues),
                        role: token(child, "sketch:construction:role", issues)
                            .filter(|role| role != "none"),
                    }),
                    Err(reason) => data.rejected.push(format!("{}: {reason}", child.path)),
                }
            }
            name if CONSTRAINT_TYPES.contains(&name) => {
                let kind = name
                    .trim_start_matches("Cad")
                    .trim_end_matches("Constraint")
                    .to_owned();
                data.constraints.push(ConstraintData {
                    path: child.path.clone(),
                    kind,
                    constrained: child.relationship_targets("sketch:constraint:constrained"),
                    reference: child.relationship_targets("sketch:constraint:reference"),
                    value: scalar(child, "sketch:constraint:value", issues),
                    driving: boolean(child, "sketch:constraint:driving", issues).unwrap_or(true),
                    parameter: child
                        .relationship_targets("sketch:constraint:parameter")
                        .first()
                        .cloned(),
                });
            }
            _ => queue.extend(child.children.iter().copied()),
        }
    }
    data.points
        .sort_by(|left, right| left.path.cmp(&right.path));
    data.curves
        .sort_by(|left, right| left.path.cmp(&right.path));
    data.constraints
        .sort_by(|left, right| left.path.cmp(&right.path));
    data
}
