// SPDX-License-Identifier: Apache-2.0
//! Patterns and mirrors (`docs/formats/solidedge.md` §8), stated on the
//! feature's design operation.
//!
//! A `CLSID_JMGroup` feature whose parameters do not resolve, or hold a value
//! the specification does not state, stays unresolved and adds
//! `feature.pattern-unread`. A pattern states its instances as the pattern's
//! result only. The rotation sense of a circular pattern is not established:
//! the layout states a positive step about the profile normal, and a pattern
//! that does not fill a full turn adds `feature.pattern-sense-unread`. Stored
//! lengths are metres and stored angles are radians; the design states
//! millimetres.

use std::collections::BTreeMap;

use cadmpeg_core::decode::View;
use cadmpeg_ir::design::construction::{kinds, AxisValue, Construction, FrameValue, PlaneValue};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::body::{Pattern, PatternInstances, PatternSeeds};
use cadmpeg_ir::design::ops::shared::{
    CircularLayout, LinearLayout, LinearRun, MirrorLayout, PatternLayout, PatternRun, RunSides,
    Spacing,
};
use cadmpeg_ir::design::selection::{
    Anchor, Members, OrderedSelection, PickTarget, Picked, Picks, Selection,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::units::UnitVector3;

use crate::construction;
use crate::extents::{model_point, named, real_at, slot, MM_PER_M, PART_CLASS};
use crate::features::{feature_id, function_span, Kind, Listed};
use crate::layout::{arc_2d, line_2d, mirror_group, pattern_group, reference_plane};
use crate::loss::SolidEdgeLossCode;
use crate::psm::{Cluster, Object, TypeTable};
use crate::sketches::{profile_frame, profile_members, PROFILE_AFTER_NAME_SHORT};

/// The function of the part that stores a pattern's profile (§8.1).
const FAST_PATTERN_FUNCTION: &str = "JMFastPatternFunction";
/// The class of the pattern parameters.
const PATTERN_2D_CLASS: &str = "CLSID_JPattern2d";
/// The classes of the sketch geometry a pattern references.
const LINE_CLASS: &str = "CLSID_JLine2d";
const ARC_CLASS: &str = "CLSID_JArc2d";
/// The class of a mirror plane.
const PLANE_CLASS: &str = "ReferencePlaneObject";

/// The class GUID `{0145EEC0-1602-11CE-B7EE-08003601E53B}` in stored byte
/// order, after whose last occurrence in a pattern object the parameters
/// stand (§8.1).
const PATTERN_GUID: [u8; 16] = [
    0xc0, 0xee, 0x45, 0x01, 0x02, 0x16, 0xce, 0x11, 0xb7, 0xee, 0x08, 0x00, 0x36, 0x01, 0xe5, 0x3b,
];
/// Bytes from the end of the GUID to each pattern parameter (§8.1).
const KIND_AFTER_GUID: usize = 14;
const FIRST_COUNT_AFTER_GUID: usize = 26;
const SECOND_COUNT_AFTER_GUID: usize = 30;
const FIRST_STEP_AFTER_GUID: usize = 34;
const SECOND_STEP_AFTER_GUID: usize = 42;
/// The pattern kinds (§8.1).
const KIND_RECTANGULAR: u32 = 1;
const KIND_CIRCULAR: u32 = 2;

/// The first two words of a reference triple (§8.1).
const REFERENCE_TAG: u32 = 0x10;
const REFERENCE_MARK: u32 = 0x8000_0000;
/// Bytes of one word, and of one reference triple.
const WORD: usize = 4;
const TRIPLE: usize = 12;
/// Bytes of one `f64`.
const REAL: usize = 8;

/// A stored plane normal has unit length within this distance.
const EPS_NORMAL_UNIT: f64 = 1e-6;
/// A pattern fills a full turn when its extent is within this many radians of
/// one.
const EPS_FULL_TURN: f64 = 1e-9;

/// What one group reads as.
struct Read {
    operation: FeatureOperation,
    /// The pattern turns about its axis in a sense the document does not
    /// establish and fills less than a full turn.
    sense_unread: bool,
}

fn word(object: &Object<'_>, at: usize, what: &str) -> Result<u32, String> {
    View::u32_le_at(object.bytes, at)
        .ok_or_else(|| format!("the group object is too short for the {what}"))
}

/// The listed feature `id` names.
fn seed_of(listed: &[Listed], id: u32) -> Result<u32, String> {
    if listed
        .iter()
        .any(|entry| entry.object == id && entry.kind != Kind::Folder)
    {
        Ok(id)
    } else {
        Err(format!("the seed {id:#x} is not a listed feature"))
    }
}

fn seeds(ids: &[u32]) -> Result<PatternSeeds, String> {
    let picks = ids
        .iter()
        .map(|&id| Picked {
            target: PickTarget::FeatureResult(feature_id(id)),
            context: None,
            native: None,
            domain: None,
            witness: None,
            resolved: None,
        })
        .collect();
    let features: OrderedSelection = Selection {
        at: Anchor::Input,
        picks: Picks::Named(
            Members::new(picks).map_err(|_| "the seed selection is invalid".to_owned())?,
        ),
        native: None,
    };
    Ok(PatternSeeds::Features {
        features,
        method: None,
    })
}

fn pattern(seeds: PatternSeeds, layout: PatternLayout) -> FeatureOperation {
    FeatureOperation::Pattern(Box::new(Pattern {
        seeds,
        layout,
        instances: PatternInstances::Implicit(Vec::new()),
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }))
}

fn finite_point(at: Vector3) -> Result<FinitePoint3, String> {
    FinitePoint3::new(Point3::new(at.x, at.y, at.z))
        .ok_or_else(|| "a stored point is not finite".to_owned())
}

/// The reference triples of a pattern object that name a curve of `class`, as
/// the curve objects in stored order.
fn referenced<'a>(
    cluster: &Cluster<'a>,
    types: &TypeTable,
    object: &Object<'_>,
    class: &str,
) -> Vec<Object<'a>> {
    let bytes = object.bytes;
    let mut found = Vec::new();
    let mut at = 0_usize;
    while at < bytes.len() {
        let word_at = |offset: usize| {
            at.checked_add(offset)
                .and_then(|next| View::u32_le_at(bytes, next))
        };
        let triple = (word_at(0), word_at(WORD)) == (Some(REFERENCE_TAG), Some(REFERENCE_MARK));
        let id = word_at(2 * WORD).filter(|_| triple);
        if let Some(curve) = id.and_then(|id| cluster.object(id)) {
            if types.class_name(curve.form, curve.flags) == Some(class) {
                found.push(curve);
            }
        }
        let step = if id.is_some() { TRIPLE } else { 1 };
        let Some(next) = at.checked_add(step) else {
            break;
        };
        at = next;
    }
    found
}

/// The unit model-space direction from the profile point `from` to `to`.
fn run_direction(
    frame: &FrameValue,
    from: [f64; 2],
    to: [f64; 2],
) -> Result<Construction<kinds::Direction>, String> {
    let start = model_point(frame, from[0] * MM_PER_M, from[1] * MM_PER_M);
    let end = model_point(frame, to[0] * MM_PER_M, to[1] * MM_PER_M);
    let resolved = UnitVector3::normalized(Vector3::new(
        end.x - start.x,
        end.y - start.y,
        end.z - start.z,
    ))
    .ok_or_else(|| "a pattern line has no direction".to_owned())?;
    Ok(Construction::<kinds::Direction> {
        resolved: Some(resolved),
        ..construction::empty()
    })
}

/// The profile coordinates stored at `at` and `at` + 8.
fn pair(object: &Object<'_>, at: usize, what: &str) -> Result<[f64; 2], String> {
    let second = at
        .checked_add(REAL)
        .ok_or_else(|| "an offset overflows".to_owned())?;
    Ok([real_at(object, at, what)?, real_at(object, second, what)?])
}

fn count(value: u32, what: &str) -> Result<Dimension<u32>, String> {
    if value == 0 {
        return Err(format!("the {what} count is zero"));
    }
    Ok(Dimension::Literal(value))
}

fn run<Q>(count: Dimension<u32>, pitch: Dimension<Q>) -> PatternRun<Q> {
    PatternRun {
        spacing: Spacing::Pitch { count, pitch },
        sides: Some(RunSides::One),
        seed: None,
        increments: Vec::new(),
    }
}

fn linear_run(
    frame: &FrameValue,
    line: &Object<'_>,
    count: Dimension<u32>,
    metres: f64,
    what: &str,
) -> Result<LinearRun, String> {
    let from = pair(line, line_2d::X0, "pattern line")?;
    let to = pair(line, line_2d::X1, "pattern line")?;
    let pitch = metres * MM_PER_M;
    if !(pitch.is_finite() && pitch > 0.0) {
        return Err(format!("the {what} spacing {metres} is not positive"));
    }
    Ok(LinearRun {
        direction: run_direction(frame, from, to)?,
        run: run(
            count,
            Dimension::Literal(
                Length::new(pitch).ok_or_else(|| format!("the {what} spacing is not finite"))?,
            ),
        ),
    })
}

/// The pattern of a group whose `JMFastPatternFunction` part is `part` (§8.1).
fn read_pattern(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    listed: &[Listed],
    group: &Object<'_>,
    part: &Object<'_>,
    end: usize,
) -> Result<Read, String> {
    let seed = seed_of(listed, word(group, pattern_group::SEED, "seed")?)?;
    let profile = slot(part, end, PROFILE_AFTER_NAME_SHORT)?;
    let frame = profile_frame(cluster, types, profile)
        .ok_or_else(|| "its profile frame does not read".to_owned())?;
    let members = profile_members(cluster, types, profile)
        .ok_or_else(|| "its profile member list does not read".to_owned())?;
    let mut parameters = members.into_iter().filter_map(|id| {
        cluster
            .object(id)
            .filter(|member| types.class_name(member.form, member.flags) == Some(PATTERN_2D_CLASS))
    });
    let (Some(object), None) = (parameters.next(), parameters.next()) else {
        return Err("its profile has no single pattern-2d member".to_owned());
    };
    let seeds = seeds(&[seed])?;
    let base = parameter_base(&object)?;
    let SketchLayout {
        layout,
        sense_unread,
    } = sketch_layout(cluster, types, &frame, &object, base)?;
    Ok(Read {
        operation: pattern(seeds, layout),
        sense_unread,
    })
}

/// The layout a pattern-2d object states, with whether the pattern turns in a
/// sense the document does not establish.
pub(crate) struct SketchLayout {
    pub(crate) layout: PatternLayout,
    /// The pattern fills less than a full turn about its axis.
    pub(crate) sense_unread: bool,
}

/// The offset `E` of the parameters of a part pattern's pattern-2d object: the
/// offset after the last occurrence of the parameter GUID (§8.1).
fn parameter_base(object: &Object<'_>) -> Result<usize, String> {
    object
        .bytes
        .windows(PATTERN_GUID.len())
        .rposition(|window| window == PATTERN_GUID)
        .and_then(|at| at.checked_add(PATTERN_GUID.len()))
        .ok_or_else(|| "the pattern object holds no parameter GUID".to_owned())
}

/// The layout of the pattern-2d object `object` in the profile frame `frame`,
/// whose parameters stand at the offset `base` (§8.1).
pub(crate) fn sketch_layout(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    frame: &FrameValue,
    object: &Object<'_>,
    base: usize,
) -> Result<SketchLayout, String> {
    let after = |offset: usize| {
        base.checked_add(offset)
            .ok_or_else(|| "an offset overflows".to_owned())
    };
    let kind = word(object, after(KIND_AFTER_GUID)?, "pattern kind")?;
    let first_count = word(object, after(FIRST_COUNT_AFTER_GUID)?, "first count")?;
    let second_count = word(object, after(SECOND_COUNT_AFTER_GUID)?, "second count")?;
    let first_step = real_at(object, after(FIRST_STEP_AFTER_GUID)?, "first spacing")?;
    let second_step = real_at(object, after(SECOND_STEP_AFTER_GUID)?, "second spacing")?;
    match kind {
        KIND_RECTANGULAR => {
            let lines = referenced(cluster, types, object, LINE_CLASS);
            let [first, second, ..] = lines.as_slice() else {
                return Err("a rectangular pattern references fewer than two lines".to_owned());
            };
            let layout = PatternLayout::Linear(LinearLayout {
                first: linear_run(
                    frame,
                    first,
                    count(first_count, "first")?,
                    first_step,
                    "first",
                )?,
                second: Some(linear_run(
                    frame,
                    second,
                    count(second_count, "second")?,
                    second_step,
                    "second",
                )?),
                seed_only: Some(Dimension::Literal(false)),
            });
            Ok(SketchLayout {
                layout,
                sense_unread: false,
            })
        }
        KIND_CIRCULAR => {
            let arcs = referenced(cluster, types, object, ARC_CLASS);
            let Some(arc) = arcs.first() else {
                return Err("a circular pattern references no arc".to_owned());
            };
            let [u, v] = pair(arc, arc_2d::CENTER_X, "pattern arc")?;
            let centre = model_point(frame, u * MM_PER_M, v * MM_PER_M);
            let axis = AxisValue {
                origin: finite_point(centre)?,
                direction: frame.z_axis,
            };
            let instances = count(first_count, "first")?;
            if first_step == 0.0 {
                return Err("the angle between instances is zero".to_owned());
            }
            let step = Angle::new(first_step)
                .ok_or_else(|| "the angle between instances is not finite".to_owned())?;
            let extent = f64::from(first_count) * first_step.abs();
            let layout = PatternLayout::Circular(CircularLayout {
                axis: Construction::<kinds::Axis> {
                    resolved: Some(axis),
                    ..construction::empty()
                },
                angular: run(instances, Dimension::Literal(step)),
                radial: None,
                orientation: None,
            });
            Ok(SketchLayout {
                layout,
                sense_unread: extent < std::f64::consts::TAU - EPS_FULL_TURN,
            })
        }
        other => Err(format!("the pattern kind {other} is not 1 or 2")),
    }
}

/// The plane of a `ReferencePlaneObject` as a construction, in millimetres.
fn mirror_plane(object: &Object<'_>) -> Result<Construction<kinds::Plane>, String> {
    let at = |base: usize| -> Result<Vector3, String> {
        let get = |index: usize| {
            index
                .checked_mul(REAL)
                .and_then(|bytes| base.checked_add(bytes))
                .ok_or_else(|| "an offset overflows".to_owned())
                .and_then(|offset| real_at(object, offset, "mirror plane"))
        };
        Ok(Vector3::new(get(0)?, get(1)?, get(2)?))
    };
    let point = at(reference_plane::POINT)?;
    let normal = at(reference_plane::NORMAL)?;
    if (normal.norm() - 1.0).abs() > EPS_NORMAL_UNIT {
        return Err("the mirror plane normal is not a unit vector".to_owned());
    }
    let basis = if normal.x.abs() <= normal.y.abs() && normal.x.abs() <= normal.z.abs() {
        Vector3::new(1.0, 0.0, 0.0)
    } else if normal.y.abs() <= normal.z.abs() {
        Vector3::new(0.0, 1.0, 0.0)
    } else {
        Vector3::new(0.0, 0.0, 1.0)
    };
    Ok(Construction::<kinds::Plane> {
        resolved: Some(PlaneValue {
            origin: finite_point(Vector3::new(
                point.x * MM_PER_M,
                point.y * MM_PER_M,
                point.z * MM_PER_M,
            ))?,
            normal: UnitVector3::normalized(normal)
                .ok_or_else(|| "the mirror plane normal is not a unit vector".to_owned())?,
            x_axis: UnitVector3::normalized(normal.cross(basis))
                .ok_or_else(|| "the mirror plane frame is degenerate".to_owned())?,
        }),
        ..construction::empty()
    })
}

/// The mirror of a group (§8.2).
fn read_mirror(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    listed: &[Listed],
    group: &Object<'_>,
) -> Result<Read, String> {
    let seed_count = word(group, mirror_group::SEED_COUNT, "seed count")?;
    if seed_count != 1 {
        return Err(format!("the mirror has {seed_count} seeds, not one"));
    }
    let seed = seed_of(listed, word(group, mirror_group::FIRST_SEED, "seed")?)?;
    let (plane, class) = named(
        cluster,
        types,
        word(group, mirror_group::PLANE, "mirror plane")?,
        "mirror plane",
    )?;
    if class != PLANE_CLASS {
        return Err(format!("the mirror plane object is of class {class}"));
    }
    let layout = PatternLayout::Mirror(MirrorLayout {
        plane: mirror_plane(&plane)?,
        stitch_tolerance: None,
    });
    Ok(Read {
        operation: pattern(seeds(&[seed])?, layout),
        sense_unread: false,
    })
}

/// The `JMFastPatternFunction` part the group names, with the offset after
/// its function name. `None` where the group names no such part.
fn fast_pattern_part<'a>(
    cluster: &Cluster<'a>,
    types: &TypeTable,
    group: &Object<'_>,
) -> Option<(Object<'a>, usize)> {
    let id = View::u32_le_at(group.bytes, pattern_group::FAST_PATTERN_PART)?;
    let part = cluster.object(id)?;
    if types.class_name(part.form, part.flags) != Some(PART_CLASS) {
        return None;
    }
    let (function, end) = function_span(&part)?;
    (function == FAST_PATTERN_FUNCTION).then_some((part, end))
}

/// State the operation of each pattern and mirror group that reads. A group
/// that does not read keeps its unresolved operation and adds
/// `feature.pattern-unread`; a circular pattern that fills less than a full
/// turn adds `feature.pattern-sense-unread`.
pub(crate) fn project(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    listed: &[Listed],
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) {
    for entry in listed.iter().filter(|entry| entry.kind == Kind::Group) {
        let Some(group) = cluster.object(entry.object) else {
            continue;
        };
        let (read, what) = match fast_pattern_part(cluster, types, &group) {
            Some((part, end)) => (
                read_pattern(cluster, types, listed, &group, &part, end),
                "pattern",
            ),
            None => (read_mirror(cluster, types, listed, &group), "mirror"),
        };
        match read {
            Ok(read) => {
                let id = feature_id(entry.object);
                if let Some(feature) = ir.design.features.iter_mut().find(|f| f.id == id) {
                    feature.definition.operation = read.operation;
                }
                if read.sense_unread {
                    losses.push(SolidEdgeLossCode::FeaturePatternSenseUnread.note(format!(
                        "the circular pattern feature object {:#x} fills less than a full turn; \
                         its rotation sense about the profile normal is not established, and the \
                         layout states a positive step",
                        entry.object
                    )));
                }
            }
            Err(reason) => losses.push(SolidEdgeLossCode::FeaturePatternUnread.note(format!(
                "the {what} feature object {:#x} does not read: {reason}; the feature stays \
                 unresolved",
                entry.object
            ))),
        }
    }
}

#[cfg(test)]
mod tests;
