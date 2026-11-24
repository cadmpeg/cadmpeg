// SPDX-License-Identifier: Apache-2.0
//! Sketches (`docs/formats/solidedge.md` §5): the profile a feature consumes,
//! stated as one planar design sketch on the stored frame with its lines,
//! circles, arcs and hole positions. Constraints and dimensions are not read.
//!
//! A profile that does not read is a loss and the rest of the document still
//! projects. Stored lengths are metres; the design states millimetres.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::construction::{kinds, Construction, FrameValue};
use cadmpeg_ir::design::evaluation::EvaluationStatus;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::names::CodecName;
use cadmpeg_ir::design::ops::sketch::{
    EntityDefinition, Planar, PlanarCurve, Sketch, SketchEntity, SketchPlane, SketchSolve,
};
use cadmpeg_ir::design::options::{NativeOption, NativeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{Angle, PositiveLength};
use cadmpeg_ir::sketches::{SketchEntityId, SketchId};
use cadmpeg_ir::units::{FinitePoint2, UnitVector3};

use crate::construction;
use crate::extents::profile_selection;
use crate::features::{feature_id, function_span, Kind, Listed};
use crate::layout::{arc_2d, circle_2d, hole_2d, line_2d, profile, profile_2d};
use crate::loss::SolidEdgeLossCode;
use crate::psm::{Cluster, Object, TypeTable};

/// The class of a feature stored as a part function.
const PART_CLASS: &str = "CLSID_JMPart";
/// The class of a sketch profile.
const PROFILE_CLASS: &str = "CLSID_JProfile";
/// The class of the member list of a profile.
const PROFILE_2D_CLASS: &str = "CLSID_JProfile2d";
/// The member classes that are curves, and the class that joins curves.
const LINE_CLASS: &str = "CLSID_JLine2d";
const CIRCLE_CLASS: &str = "CLSID_JCircle2d";
const ARC_CLASS: &str = "CLSID_JArc2d";
const KEYPOINT_CLASS: &str = "CLSID_JKeyPoint2d";
/// The class of a hole position of a profile (§7.2), stated as a sketch point.
pub(crate) const HOLE_2D_CLASS: &str = "CLSID_JHole2d";

/// Bytes from the end of a function name to the profile identifier of the
/// functions that consume a profile of a sketch (§5.3).
const PROFILE_AFTER_NAME: usize = 72;
/// The same offset for a hole, a flange and a fast pattern.
pub(crate) const PROFILE_AFTER_NAME_SHORT: usize = 68;

/// Millimetres per metre.
const MM_PER_M: f64 = 1000.0;
/// A stored frame axis has unit length within this distance.
const EPS_AXIS_UNIT: f64 = 1e-6;
/// The stored axes of a frame have a dot product within this bound.
const EPS_AXIS_ORTHO: f64 = 1e-6;
/// Bytes of one `f64`.
const REAL: usize = 8;
/// A member identifier that names no object (§5.1).
const NO_MEMBER: u32 = 0xFFFF_FFFF;
/// Bytes of one member identifier.
const WORD: usize = 4;

/// The offset after a function name at which the profile identifier of
/// `function` is stored, or `None` for a function that stores no profile.
fn profile_offset(function: &str) -> Option<usize> {
    match function {
        "JMProtrusionFunction"
        | "JMCutoutFunction"
        | "JMRevolvedProtrusionFunction"
        | "JMMultiSweepProtrusionFunction"
        | "JMLoftedProtrusionFunction"
        | "JMBaseTabFunction" => Some(PROFILE_AFTER_NAME),
        "JMHoleFunction" | "JMFlangeFunction" | "JMFastPatternFunction" => {
            Some(PROFILE_AFTER_NAME_SHORT)
        }
        _ => None,
    }
}

pub(crate) fn sketch_id(profile: u32) -> SketchId {
    SketchId::compose(
        &cadmpeg_ir::identity_namespace!("solidedge", "model", "sketch"),
        profile,
    )
}

fn entity_id(member: u32) -> SketchEntityId {
    SketchEntityId::compose(
        &cadmpeg_ir::identity_namespace!("solidedge", "model", "sketch_entity"),
        member,
    )
}

/// Three consecutive `f64` at `at`.
fn vector(bytes: &[u8], at: usize) -> Option<Vector3> {
    Some(Vector3::new(
        View::f64_le_at(bytes, at)?,
        View::f64_le_at(bytes, at.checked_add(REAL)?)?,
        View::f64_le_at(bytes, at.checked_add(REAL.checked_mul(2)?)?)?,
    ))
}

/// The frame a profile object states, in millimetres. `None` where a value is
/// not finite, an axis is not a unit vector, or the axes are not orthogonal.
fn frame(bytes: &[u8]) -> Option<FrameValue> {
    let origin = vector(bytes, profile::ORIGIN)?;
    let normal = vector(bytes, profile::NORMAL)?;
    let x_axis = vector(bytes, profile::X_AXIS)?;
    let unit = |axis: Vector3| (axis.norm() - 1.0).abs() <= EPS_AXIS_UNIT;
    if !unit(normal) || !unit(x_axis) || normal.dot(x_axis).abs() > EPS_AXIS_ORTHO {
        return None;
    }
    let z_axis = UnitVector3::normalized(normal)?;
    let x_axis = UnitVector3::normalized(x_axis)?;
    let y_axis = UnitVector3::normalized(Vector3::from(z_axis).cross(Vector3::from(x_axis)))?;
    Some(FrameValue {
        origin: FinitePoint3::new(Point3::new(
            origin.x * MM_PER_M,
            origin.y * MM_PER_M,
            origin.z * MM_PER_M,
        ))?,
        x_axis,
        y_axis,
        z_axis,
    })
}

/// A profile point in millimetres.
fn point(bytes: &[u8], at: usize) -> Option<FinitePoint2> {
    FinitePoint2::new(Point2 {
        u: View::f64_le_at(bytes, at)? * MM_PER_M,
        v: View::f64_le_at(bytes, at.checked_add(REAL)?)? * MM_PER_M,
    })
}

fn line(bytes: &[u8]) -> Option<PlanarCurve> {
    Some(PlanarCurve::Line {
        start: point(bytes, line_2d::X0)?,
        end: point(bytes, line_2d::X1)?,
    })
}

fn circle(bytes: &[u8]) -> Option<PlanarCurve> {
    Some(PlanarCurve::Circle {
        center: point(bytes, circle_2d::CENTER_X)?,
        radius: PositiveLength::new(View::f64_le_at(bytes, circle_2d::RADIUS)? * MM_PER_M)?,
    })
}

fn hole_position(bytes: &[u8]) -> Option<PlanarCurve> {
    Some(PlanarCurve::Point(point(bytes, hole_2d::X)?))
}

fn arc(bytes: &[u8]) -> Option<PlanarCurve> {
    Some(PlanarCurve::Arc {
        center: point(bytes, arc_2d::CENTER_X)?,
        radius: PositiveLength::new(View::f64_le_at(bytes, arc_2d::RADIUS)? * MM_PER_M)?,
        start: Angle::new(View::f64_le_at(bytes, arc_2d::START_ANGLE)?)?,
        end: Angle::new(View::f64_le_at(bytes, arc_2d::END_ANGLE)?)?,
    })
}

/// What a member of a profile states.
enum Member {
    /// A curve.
    Curve(PlanarCurve),
    /// A keypoint, which states no coordinate.
    Keypoint,
    /// A constraint, a dimension or another item that is not read.
    Other,
    /// A curve member whose values do not read, or a member the cluster does
    /// not hold.
    Unread,
}

fn member(cluster: &Cluster<'_>, types: &TypeTable, id: u32) -> Member {
    let Some(object) = cluster.object(id) else {
        return Member::Unread;
    };
    let Some(class) = types.class_name(object.form, object.flags) else {
        return Member::Unread;
    };
    let read = match class {
        LINE_CLASS => line(object.bytes),
        CIRCLE_CLASS => circle(object.bytes),
        ARC_CLASS => arc(object.bytes),
        HOLE_2D_CLASS => hole_position(object.bytes),
        KEYPOINT_CLASS => return Member::Keypoint,
        _ => return Member::Other,
    };
    read.map_or(Member::Unread, Member::Curve)
}

/// A profile read from the cluster.
struct Profile {
    frame: FrameValue,
    /// The curves with the identifier of their member.
    curves: Vec<(u32, PlanarCurve)>,
    /// Members that are constraints, dimensions or other items.
    unstated: usize,
    /// Members that name a curve whose values do not read.
    unread: usize,
}

/// Read profile `id`. The error is the reason it does not read.
fn read_profile(cluster: &Cluster<'_>, types: &TypeTable, id: u32) -> Result<Profile, String> {
    let object = cluster
        .object(id)
        .ok_or_else(|| "the cluster holds no such object".to_owned())?;
    match types.class_name(object.form, object.flags) {
        Some(PROFILE_CLASS) => {}
        Some(class) => return Err(format!("the object is of class {class}, not a profile")),
        None => return Err("the object has no class".to_owned()),
    }
    let frame = frame(object.bytes).ok_or_else(|| "its frame does not read".to_owned())?;
    let members_id = View::u32_le_at(object.bytes, profile::PROFILE_2D)
        .ok_or_else(|| "the profile object is too short".to_owned())?;
    if id.checked_add(1) != Some(members_id) {
        return Err("its member list is not the next object".to_owned());
    }
    let list = cluster
        .object(members_id)
        .filter(|list| types.class_name(list.form, list.flags) == Some(PROFILE_2D_CLASS))
        .ok_or_else(|| "its member list is not a profile-2d object".to_owned())?;
    let ids = member_ids(&list).ok_or_else(|| "its member list does not read".to_owned())?;
    let mut profile = Profile {
        frame,
        curves: Vec::new(),
        unstated: 0,
        unread: 0,
    };
    for member_id in ids {
        if member_id == NO_MEMBER {
            continue;
        }
        match member(cluster, types, member_id) {
            Member::Curve(curve) => profile.curves.push((member_id, curve)),
            Member::Keypoint => {}
            Member::Other => profile.unstated += 1,
            Member::Unread => profile.unread += 1,
        }
    }
    Ok(profile)
}

/// The member identifiers of a profile-2d object.
fn member_ids(list: &Object<'_>) -> Option<Vec<u32>> {
    let bytes = list.bytes;
    let count = usize::try_from(View::u32_le_at(bytes, profile_2d::MEMBER_COUNT)?).ok()?;
    if count > bytes.len().checked_sub(profile_2d::FIRST_MEMBER)? / WORD {
        return None;
    }
    let mut ids = Vec::new();
    for index in 0..count {
        let at = profile_2d::FIRST_MEMBER.checked_add(index.checked_mul(WORD)?)?;
        ids.push(View::u32_le_at(bytes, at)?);
    }
    Some(ids)
}

/// The member identifiers of profile `id`, in stored order. `None` where the
/// profile or its member list does not read.
pub(crate) fn profile_members(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    id: u32,
) -> Option<Vec<u32>> {
    let object = cluster.object(id)?;
    if types.class_name(object.form, object.flags) != Some(PROFILE_CLASS) {
        return None;
    }
    let members_id = View::u32_le_at(object.bytes, profile::PROFILE_2D)?;
    let list = cluster
        .object(members_id)
        .filter(|list| types.class_name(list.form, list.flags) == Some(PROFILE_2D_CLASS))?;
    member_ids(&list)
}

/// The profile identifier a feature object stores. `None` where the object's
/// function stores no profile; the inner `None` where the stored block is too
/// short to hold it.
fn profile_word(object: &Object<'_>) -> Option<Result<u32, ()>> {
    let (function, end) = function_span(object)?;
    let after = profile_offset(&function)?;
    Some(
        end.checked_add(after)
            .and_then(|at| View::u32_le_at(object.bytes, at))
            .ok_or(()),
    )
}

/// State the profile of each listed feature as a design sketch owned by that
/// feature. A profile that does not read adds `sketch.unread`; constraints
/// and dimensions add one `sketch.relations-unread` note naming their count.
pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    cluster: &Cluster<'_>,
    types: &TypeTable,
    listed: &[Listed],
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut claimed = BTreeSet::new();
    let mut sketches = Vec::new();
    let mut entities = Vec::new();
    let mut unstated = 0_usize;
    for entry in listed {
        let Kind::Function(Some(_)) = &entry.kind else {
            continue;
        };
        let Some(object) = cluster.object(entry.object) else {
            continue;
        };
        if types.class_name(object.form, object.flags) != Some(PART_CLASS) {
            continue;
        }
        let Some(stored) = profile_word(&object) else {
            continue;
        };
        let Ok(profile_id) = stored else {
            losses.push(SolidEdgeLossCode::SketchUnread.note(format!(
                "feature object {:#x} stores no profile identifier; no sketch is stated",
                entry.object
            )));
            continue;
        };
        if !claimed.insert(profile_id) {
            losses.push(SolidEdgeLossCode::SketchUnread.note(format!(
                "profile object {profile_id:#x} is named by more than one feature; the sketch is \
                 stated once"
            )));
            continue;
        }
        let profile = match read_profile(cluster, types, profile_id) {
            Ok(profile) => profile,
            Err(reason) => {
                losses.push(SolidEdgeLossCode::SketchUnread.note(format!(
                    "profile object {profile_id:#x} of feature object {:#x} does not read: \
                     {reason}; no sketch is stated",
                    entry.object
                )));
                continue;
            }
        };
        if profile.unread > 0 {
            losses.push(SolidEdgeLossCode::SketchUnread.note(format!(
                "profile object {profile_id:#x} has {} curve member(s) that do not read; they \
                 are not stated",
                profile.unread
            )));
        }
        unstated = unstated
            .checked_add(profile.unstated)
            .ok_or_else(|| CodecError::malformed("sketch member count overflows"))?;
        ctx.charge_entities(1, "project Solid Edge sketch")?;
        let id = sketch_id(profile_id);
        sketches.push(Sketch {
            id: id.clone(),
            owner: feature_id(entry.object),
            name: None,
            plane: SketchPlane::Placed(Box::new(Construction::<kinds::Frame> {
                resolved: Some(profile.frame),
                ..construction::empty()
            })),
            solve: SketchSolve {
                state: None,
                status: EvaluationStatus::Current,
            },
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
            native_ref: None,
        });
        for (member_id, curve) in profile.curves {
            ctx.charge_entities(1, "project Solid Edge sketch entity")?;
            entities.push(SketchEntity::<Planar> {
                id: entity_id(member_id),
                sketch: id.clone(),
                definition: EntityDefinition::Free(curve),
                roles: Vec::new(),
                construction: None,
                centerline: None,
                native: None,
                native_options: BTreeMap::new(),
                source_settings: BTreeMap::new(),
                native_ref: None,
            });
        }
    }
    if unstated > 0 {
        losses.push(SolidEdgeLossCode::SketchRelationsUnread.note(format!(
            "{unstated} profile member(s) are constraints, dimensions or other items that are \
             not read; the sketches state their curves only"
        )));
    }
    sketches.sort_by(|a, b| a.id.cmp(&b.id));
    entities.sort_by(|a, b| a.id.cmp(&b.id));
    ir.design.sketches.extend(sketches);
    ir.design.sketch_entities.extend(entities);
    Ok(())
}

/// Name the sketches of each feature that no reader typed. A design sketch
/// is owned by a feature that holds or names it, so a feature that stays
/// `Unresolved` or `Native` states its sketches as whole-sketch selections in
/// the native options `profile`, `profile_2`, and so on, and its function as
/// the native kind. A native feature that already names its sketch is left.
pub(crate) fn name_untyped_profiles(listed: &[Listed], ir: &mut CadIr) {
    let design = &mut ir.design;
    for entry in listed {
        let Kind::Function(Some(function)) = &entry.kind else {
            continue;
        };
        let id = feature_id(entry.object);
        let sketches: Vec<SketchId> = design
            .sketches
            .iter()
            .filter(|sketch| sketch.owner == id)
            .map(|sketch| sketch.id.clone())
            .collect();
        if sketches.is_empty() {
            continue;
        }
        let Some(feature) = design.features.iter_mut().find(|f| f.id == id) else {
            continue;
        };
        let mut options = match &feature.definition.operation {
            FeatureOperation::Unresolved { .. } => NativeOptions::new(),
            FeatureOperation::Native { native_options, .. }
                if !native_options
                    .keys()
                    .any(|key| key.as_str().ends_with(":profile")) =>
            {
                native_options.clone()
            }
            _ => continue,
        };
        for (index, sketch) in sketches.iter().enumerate() {
            let name = if index == 0 {
                "profile".to_owned()
            } else {
                format!("profile_{}", index + 1)
            };
            let (Some(key), Ok(selection)) = (
                CodecName::new(&cadmpeg_ir::codec_id!("solidedge"), &name),
                profile_selection(sketch),
            ) else {
                continue;
            };
            options.insert(key, NativeOption::OrderedSelection(selection));
        }
        let Some(kind) = CodecName::new(&cadmpeg_ir::codec_id!("solidedge"), function) else {
            continue;
        };
        feature.definition.operation = FeatureOperation::Native {
            kind,
            native_options: options,
            source_settings: NativeOptions::new(),
        };
    }
}

#[cfg(test)]
mod tests;
