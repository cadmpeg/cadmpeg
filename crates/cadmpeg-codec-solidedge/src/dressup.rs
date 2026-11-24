// SPDX-License-Identifier: Apache-2.0
//! Holes, rounds, chamfers and thin walls (`docs/formats/solidedge.md` §7),
//! stated on the feature's design operation.
//!
//! A feature whose slots do not resolve, or hold a value the specification
//! does not state, stays unresolved and adds `feature.dressup-unread`. The
//! edges a round or a chamfer selects are stated as picks with their line
//! where the topology proxy stores a straight edge (§7.6). Every other
//! selection is a topology proxy that the decoder does not read; it is stated
//! as an unread selection and adds one `feature.selection-unread` note for
//! the document. Stored lengths are metres and stored angles are radians; the
//! design states millimetres, except the thread diameters, which are stored
//! in millimetres.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::View;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::construction::{kinds, Construction, FrameValue};
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::dressup::{
    BlendControls, Bore, CenterSource, Chamfer, ChamferSet, ChamferSize, Cone, Fillet,
    FilletSection, FilletSet, FilletSize, Hole, HoleHead, HolePositions, HoleShape, ShellFeature,
    StandardSize, ThreadSpec,
};
use cadmpeg_ir::design::ops::shared::Limit;
use cadmpeg_ir::design::ops::sketch::SketchPlane;
use cadmpeg_ir::design::options::{Thickness, WallSide};
use cadmpeg_ir::design::selection::{Anchor, MemberOrder, Picks, Selection};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{InteriorAngle, Length, PositiveLength};
use cadmpeg_ir::units::UnitVector3;

use crate::construction;
use crate::edge::edge_selection;
use crate::extents::{
    dimension_value, double_slot, is, measure_slot, named, profile_selection, real_at, slot,
    DIMENSION_CLASS, KIND_AFTER_NAME_HOLE, KIND_FINITE, KIND_THROUGH_ALL, MM_PER_M, PART_CLASS,
    SIDE_AFTER_NAME_HOLE, SIDE_AGAINST, SIDE_ALONG,
};
use crate::features::{feature_id, function_span, Kind, Listed};
use crate::layout::{hole_2d, hole_prop_data, hole_thread};
use crate::loss::SolidEdgeLossCode;
use crate::psm::{Cluster, Object, TypeTable};
use crate::sketches::{profile_members, sketch_id, HOLE_2D_CLASS, PROFILE_AFTER_NAME_SHORT};

/// The class of the hole parameters.
const PROP_DATA_CLASS: &str = "CLSID_JMHolePropData";
/// The class of a stored edge or face selection.
const PROXY_CLASS: &str = "CLSID_JMTopologyProxy";

/// Bytes from the end of a function name to each hole slot (§7.2).
const HOLE_PROP_AFTER_NAME: usize = 96;
const HOLE_COUNTERBORE_DEPTH_AFTER_NAME: usize = 124;
const HOLE_SINK_ANGLE_AFTER_NAME: usize = 152;
const HOLE_DEPTH_AFTER_NAME: usize = 264;

/// Bytes from the end of a function name to the slots of a round (§7.3).
const ROUND_PROXY_AFTER_NAME: usize = 72;
const ROUND_KIND_AFTER_NAME: usize = 84;
const ROUND_RADIUS_AFTER_NAME: usize = 96;
/// Bytes between the slots of one kind in a round.
const ROUND_STRIDE: usize = 20;

/// Bytes from the end of a function name to each chamfer slot (§7.4).
const CHAMFER_TYPE_AFTER_NAME: usize = 68;
const CHAMFER_REFERENCE_AFTER_NAME: usize = 96;
const CHAMFER_EDGES_AFTER_NAME: usize = 128;
const CHAMFER_FIRST_AFTER_NAME: usize = 156;
const CHAMFER_SECOND_AFTER_NAME: usize = 184;

/// Bytes from the end of a function name to each thin-wall slot (§7.5).
const THINWALL_TYPE_AFTER_NAME: usize = 68;
const THINWALL_THICKNESS_AFTER_NAME: usize = 96;
const THINWALL_FACES_AFTER_NAME: usize = 124;
/// The thin-wall type that hollows the body inward (§7.5).
const THINWALL_INWARD: f64 = 4.0;

/// Object offset of the first UTF-16 code unit of the thread designation,
/// after its length word (§7.2).
const DESIGNATION_AT: usize = 1073;
/// Bytes of one UTF-16 code unit.
const UNIT_BYTES: usize = 2;
/// The printable ASCII code units of a thread designation.
const DESIGNATION_UNITS: std::ops::RangeInclusive<u16> = 0x20..=0x7e;

/// The hole types of `CLSID_JMHolePropData` (§7.2).
const HOLE_SIMPLE: u32 = 0;
const HOLE_COUNTERSINK: u32 = 1;
const HOLE_COUNTERBORE: u32 = 2;
const HOLE_TAPPED: u32 = 3;

/// The set kind of a constant-radius round set (§7.3).
const SET_CONSTANT_RADIUS: f64 = 1.0;

/// The chamfer types (§7.4).
const CHAMFER_EQUAL: f64 = 41.0;
const CHAMFER_DISTANCE_ANGLE: f64 = 42.0;
const CHAMFER_TWO_DISTANCES: f64 = 43.0;

/// What one feature reads as.
struct Read {
    operation: FeatureOperation,
    /// The operation states an unread selection.
    selection_unread: bool,
    /// The feature is tapped and its thread is not stated.
    thread_unread: bool,
}

fn unread<O: MemberOrder>() -> Selection<O> {
    Selection {
        at: Anchor::Input,
        picks: Picks::Unread,
        native: None,
    }
}

fn controls() -> BlendControls {
    BlendControls {
        corner_style: None,
        corners: Vec::new(),
        limits: Vec::new(),
        keep: None,
        transitions: Vec::new(),
        overflow: None,
        keep_features: None,
        round_corners: None,
    }
}

/// A positive stored length in metres, as a literal in millimetres.
fn length(metres: f64, what: &str) -> Result<Dimension<PositiveLength>, String> {
    PositiveLength::new(metres * MM_PER_M)
        .map(Dimension::Literal)
        .ok_or_else(|| format!("the {what} {metres} is not a positive length"))
}

/// A stored angle in radians that lies strictly between zero and a half turn.
fn angle(radians: f64, what: &str) -> Result<Dimension<InteriorAngle>, String> {
    InteriorAngle::new(radians)
        .map(Dimension::Literal)
        .ok_or_else(|| format!("the {what} {radians} is not between 0 and pi"))
}

/// The value of the `CLSID_JDim` a slot names; `None` where the slot holds
/// `0`.
fn dimension_slot(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    object: &Object<'_>,
    end: usize,
    after: usize,
    what: &str,
) -> Result<Option<f64>, String> {
    let id = slot(object, end, after)?;
    if id == 0 {
        return Ok(None);
    }
    let (found, class) = named(cluster, types, id, what)?;
    if class != DIMENSION_CLASS {
        return Err(format!("the {what} object is of class {class}"));
    }
    dimension_value(&found, what).map(Some)
}

/// Whether the slot names a topology proxy.
fn is_proxy(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    object: &Object<'_>,
    end: usize,
    after: usize,
) -> bool {
    slot(object, end, after).is_ok_and(|id| {
        cluster
            .object(id)
            .is_some_and(|found| types.class_name(found.form, found.flags) == Some(PROXY_CLASS))
    })
}

/// The selection of the edges the proxy a slot names stores, and whether the
/// proxy stores a straight edge.
fn edges_of<O: MemberOrder>(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    object: &Object<'_>,
    end: usize,
    after: usize,
) -> Result<(Selection<O>, bool), String> {
    let (proxy, _) = named(cluster, types, slot(object, end, after)?, "edge proxy")?;
    Ok(edge_selection(&proxy))
}

/// The thread a tapped hole's properties object stores (§7.2): the diameters
/// in millimetres and the designation, which stands as the thread's standard.
/// `None` where a value does not read.
fn thread_spec(prop: &Object<'_>) -> Option<ThreadSpec> {
    let bytes = prop.bytes;
    let diameter = |at: usize| {
        View::f64_le_at(bytes, at)
            .and_then(PositiveLength::new)
            .map(Dimension::Literal)
    };
    let minor = diameter(hole_thread::MINOR_DIAMETER)?;
    let major = diameter(hole_thread::MAJOR_DIAMETER)?;
    let units = usize::try_from(View::u32_le_at(bytes, hole_thread::DESIGNATION_LENGTH)?).ok()?;
    let size = units.checked_mul(UNIT_BYTES)?;
    if size > bytes.len().checked_sub(DESIGNATION_AT)? {
        return None;
    }
    let mut designation = String::new();
    for index in 0..units {
        let at = index.checked_mul(UNIT_BYTES)?.checked_add(DESIGNATION_AT)?;
        let unit = View::u16_le_at(bytes, at)?;
        if !DESIGNATION_UNITS.contains(&unit) {
            return None;
        }
        designation.push(char::from(u8::try_from(unit).ok()?));
    }
    Some(ThreadSpec {
        standard: Some(StandardSize {
            standard: NonBlankString::new(designation)?,
            kind: None,
            size: None,
            class: None,
        }),
        major_diameter: Some(major),
        minor_diameter: Some(minor),
        pitch_diameter: None,
        pitch: None,
        hand: None,
        length: None,
        modeled: None,
    })
}

/// `base + step * count`, checked.
fn stride(base: usize, count: usize, step: usize) -> Result<usize, String> {
    count
        .checked_mul(step)
        .and_then(|bytes| bytes.checked_add(base))
        .ok_or_else(|| "a slot offset overflows".to_owned())
}

/// The frame of the sketch `owner` owns.
fn frame_of(ir: &CadIr, owner: &FeatureId) -> Option<FrameValue> {
    let sketch = ir.design.sketches.iter().find(|s| &s.owner == owner)?;
    let SketchPlane::Placed(plane) = &sketch.plane else {
        return None;
    };
    plane.resolved.clone()
}

/// The hole positions of the profile the feature names: the points of its
/// sketch, which states one point per stored hole position (§7.2).
fn positions(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    object: &Object<'_>,
    end: usize,
) -> Result<HolePositions, String> {
    let profile = slot(object, end, PROFILE_AFTER_NAME_SHORT)?;
    let members = profile_members(cluster, types, profile)
        .ok_or_else(|| "its profile member list does not read".to_owned())?;
    let mut count = 0_usize;
    for id in members {
        let Some(member) = cluster.object(id) else {
            continue;
        };
        if types.class_name(member.form, member.flags) != Some(HOLE_2D_CLASS) {
            continue;
        }
        for at in [hole_2d::X, hole_2d::Y] {
            let value = real_at(&member, at, "hole position")?;
            if !value.is_finite() {
                return Err("a hole position is not finite".to_owned());
            }
        }
        count += 1;
    }
    if count == 0 {
        return Err("its profile holds no hole position".to_owned());
    }
    Ok(HolePositions::Points {
        points: profile_selection(&sketch_id(profile))?,
        centers: Some(BTreeSet::from([CenterSource::Points])),
    })
}

/// The drilling direction of the side value: along the profile normal for
/// side 2, against it for side 1.
fn direction(frame: &FrameValue, side: f64) -> Result<Construction<kinds::Direction>, String> {
    let normal = Vector3::from(frame.z_axis);
    let sign = if is(side, SIDE_ALONG) {
        1.0
    } else if is(side, SIDE_AGAINST) {
        -1.0
    } else {
        return Err(format!("the side value {side} is not 1 or 2"));
    };
    let resolved = UnitVector3::normalized(Vector3::new(
        sign * normal.x,
        sign * normal.y,
        sign * normal.z,
    ))
    .ok_or_else(|| "the profile normal is not a unit vector".to_owned())?;
    Ok(Construction::<kinds::Direction> {
        resolved: Some(resolved),
        ..construction::empty()
    })
}

/// The hole of a `JMHoleFunction` feature (§7.2).
fn hole(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    object: &Object<'_>,
    end: usize,
    frame: &FrameValue,
) -> Result<Read, String> {
    let positions = positions(cluster, types, object, end)?;
    let (prop, class) = named(
        cluster,
        types,
        slot(object, end, HOLE_PROP_AFTER_NAME)?,
        "hole properties",
    )?;
    if class != PROP_DATA_CLASS {
        return Err(format!("the hole properties object is of class {class}"));
    }
    let code = View::u32_le_at(prop.bytes, hole_prop_data::HOLE_TYPE)
        .ok_or_else(|| "the hole properties object is too short".to_owned())?;
    let diameter = real_at(&prop, hole_prop_data::DIAMETER, "hole diameter")?;
    let head_diameter = real_at(&prop, hole_prop_data::HEAD_DIAMETER, "head diameter")?;
    let head_depth = match dimension_slot(
        cluster,
        types,
        object,
        end,
        HOLE_COUNTERBORE_DEPTH_AFTER_NAME,
        "counterbore depth",
    )? {
        Some(depth) => depth,
        None => real_at(&prop, hole_prop_data::HEAD_DEPTH, "counterbore depth")?,
    };
    let counterbore = || -> Result<HoleHead, String> {
        Ok(HoleHead::Counterbore {
            diameter: Some(length(head_diameter, "counterbore diameter")?),
            depth: Some(length(head_depth, "counterbore depth")?),
        })
    };
    let mut thread = None;
    let mut thread_unread = false;
    let head = match code {
        HOLE_SIMPLE => HoleHead::Plain,
        HOLE_COUNTERSINK => {
            let radians = match dimension_slot(
                cluster,
                types,
                object,
                end,
                HOLE_SINK_ANGLE_AFTER_NAME,
                "countersink angle",
            )? {
                Some(radians) => radians,
                None => {
                    real_at(&prop, hole_prop_data::SINK_ANGLE, "countersink angle")?.to_radians()
                }
            };
            HoleHead::Countersink(Cone {
                diameter: Some(length(head_diameter, "countersink diameter")?),
                depth: None,
                angle: Some(angle(radians, "countersink angle")?),
            })
        }
        HOLE_COUNTERBORE => counterbore()?,
        HOLE_TAPPED => {
            thread = thread_spec(&prop);
            thread_unread = thread.is_none();
            if head_diameter > 0.0 && head_depth > 0.0 {
                counterbore()?
            } else {
                HoleHead::Plain
            }
        }
        other => return Err(format!("the hole type {other} is not 0, 1, 2 or 3")),
    };
    let kind = double_slot(
        cluster,
        types,
        object,
        end,
        KIND_AFTER_NAME_HOLE,
        "extent kind",
    )?;
    let side = double_slot(cluster, types, object, end, SIDE_AFTER_NAME_HOLE, "side")?;
    let extent = if is(kind, KIND_THROUGH_ALL) {
        Limit::ThroughAll
    } else if is(kind, KIND_FINITE) {
        let depth = match dimension_slot(
            cluster,
            types,
            object,
            end,
            HOLE_DEPTH_AFTER_NAME,
            "hole depth",
        )? {
            Some(depth) => depth,
            None => real_at(&prop, hole_prop_data::DEPTH, "hole depth")?,
        };
        if depth <= 0.0 {
            return Err(format!("the hole depth {depth} is not positive"));
        }
        Limit::Amount(Dimension::Literal(
            Length::new(depth * MM_PER_M)
                .ok_or_else(|| "the hole depth is not finite".to_owned())?,
        ))
    } else {
        return Err(format!("the extent kind {kind} is not 13 or 16"));
    };
    Ok(Read {
        operation: FeatureOperation::Hole(Box::new(Hole {
            positions,
            start: None,
            direction: Some(direction(frame, side)?),
            shape: HoleShape::Bore(Bore {
                diameter: Some(length(diameter, "hole diameter")?),
                head: Some(head),
                taper: None,
                bottom: None,
                near_chamfer: None,
                neck_chamfer: None,
                far_chamfer: None,
            }),
            extent: Some(extent),
            standard: None,
            thread,
            targets: None,
            series: Vec::new(),
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        })),
        selection_unread: false,
        thread_unread,
    })
}

/// The round of a `JMRoundFunction` feature (§7.3).
fn round(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    object: &Object<'_>,
    end: usize,
) -> Result<Read, String> {
    let mut count = 0_usize;
    while is_proxy(
        cluster,
        types,
        object,
        end,
        stride(ROUND_PROXY_AFTER_NAME, count, ROUND_STRIDE)?,
    ) {
        count = count
            .checked_add(1)
            .ok_or_else(|| "the set count overflows".to_owned())?;
    }
    if count == 0 {
        return Err("it names no edge set".to_owned());
    }
    let mut sets = Vec::new();
    let mut selection_unread = false;
    for k in 0..count {
        let (edges, stated) = edges_of(
            cluster,
            types,
            object,
            end,
            stride(ROUND_PROXY_AFTER_NAME, k, ROUND_STRIDE)?,
        )?;
        selection_unread |= !stated;
        let kind_at = stride(
            ROUND_KIND_AFTER_NAME,
            count
                .checked_add(k)
                .ok_or_else(|| "a slot offset overflows".to_owned())?,
            ROUND_STRIDE,
        )?;
        let radius_at = stride(
            ROUND_RADIUS_AFTER_NAME,
            count
                .checked_mul(2)
                .and_then(|twice| twice.checked_add(k))
                .ok_or_else(|| "a slot offset overflows".to_owned())?,
            ROUND_STRIDE,
        )?;
        let kind = double_slot(cluster, types, object, end, kind_at, "set kind")?;
        if !is(kind, SET_CONSTANT_RADIUS) {
            return Err(format!("the kind {kind} of set {k} is not 1"));
        }
        let radius = dimension_slot(cluster, types, object, end, radius_at, "radius")?
            .ok_or_else(|| format!("set {k} has no radius"))?;
        sets.push(FilletSet::Edges {
            edges,
            section: FilletSection {
                size: Some(FilletSize::Radius(length(radius, "radius")?)),
                profile: None,
                weight: None,
            },
            spine: None,
            hold: [None, None],
        });
    }
    Ok(Read {
        operation: FeatureOperation::Fillet(Box::new(Fillet {
            sets,
            controls: controls(),
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        })),
        selection_unread,
        thread_unread: false,
    })
}

/// The chamfer of a `JMChamferFunction` feature (§7.4).
fn chamfer(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    object: &Object<'_>,
    end: usize,
) -> Result<Read, String> {
    let kind = double_slot(cluster, types, object, end, CHAMFER_TYPE_AFTER_NAME, "type")?;
    if !is_proxy(cluster, types, object, end, CHAMFER_EDGES_AFTER_NAME) {
        return Err("the edge slot names no topology proxy".to_owned());
    }
    let (edges, stated) = edges_of(cluster, types, object, end, CHAMFER_EDGES_AFTER_NAME)?;
    let first = measure_slot(
        cluster,
        types,
        object,
        end,
        CHAMFER_FIRST_AFTER_NAME,
        "first setback",
    )?;
    let second =
        |what: &str| measure_slot(cluster, types, object, end, CHAMFER_SECOND_AFTER_NAME, what);
    let (size, reference) = if is(kind, CHAMFER_EQUAL) {
        (ChamferSize::Distance(length(first, "setback")?), None)
    } else if is(kind, CHAMFER_TWO_DISTANCES) {
        (
            ChamferSize::Distances {
                first: length(first, "first setback")?,
                second: length(second("second setback")?, "second setback")?,
            },
            Some(unread()),
        )
    } else if is(kind, CHAMFER_DISTANCE_ANGLE) {
        (
            ChamferSize::DistanceAngle {
                distance: length(first, "setback")?,
                angle: angle(second("angle")?, "angle")?,
            },
            Some(unread()),
        )
    } else {
        return Err(format!("the chamfer type {kind} is not 41, 42 or 43"));
    };
    let has_reference = reference.is_some();
    if has_reference && slot(object, end, CHAMFER_REFERENCE_AFTER_NAME).is_err() {
        return Err("the block has no reference slot".to_owned());
    }
    Ok(Read {
        operation: FeatureOperation::Chamfer(Box::new(Chamfer {
            sets: vec![ChamferSet {
                edges,
                size: Some(size),
                reference,
                flip: None,
                measure: None,
            }],
            controls: controls(),
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        })),
        selection_unread: !stated || has_reference,
        thread_unread: false,
    })
}

/// The thin wall of a `JMThinwallAssocFunctionNew` feature (§7.5). The open
/// faces are stored as a topology proxy that is not read.
fn thinwall(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    object: &Object<'_>,
    end: usize,
) -> Result<Read, String> {
    let kind = double_slot(
        cluster,
        types,
        object,
        end,
        THINWALL_TYPE_AFTER_NAME,
        "thin-wall type",
    )?;
    if !is(kind, THINWALL_INWARD) {
        return Err(format!("the thin-wall type {kind} is not 4"));
    }
    let thickness = dimension_slot(
        cluster,
        types,
        object,
        end,
        THINWALL_THICKNESS_AFTER_NAME,
        "wall thickness",
    )?
    .ok_or_else(|| "the thickness slot names no object".to_owned())?;
    if !is_proxy(cluster, types, object, end, THINWALL_FACES_AFTER_NAME) {
        return Err("the open-face slot names no topology proxy".to_owned());
    }
    Ok(Read {
        operation: FeatureOperation::Shell(Box::new(ShellFeature {
            bodies: None,
            removed: unread(),
            thickness: Some(Thickness::OneSide {
                value: length(thickness, "wall thickness")?,
                side: Some(WallSide::Inside),
            }),
            faces: Vec::new(),
            join: None,
            face_meeting: None,
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        })),
        selection_unread: true,
        thread_unread: false,
    })
}

/// State the operation of each hole, round, chamfer and thin-wall feature
/// that reads. A
/// feature that does not read keeps its unresolved operation and adds
/// `feature.dressup-unread`. Unread selections add one
/// `feature.selection-unread` note and tapped holes one
/// `feature.thread-unread` note, each naming the count of features.
pub(crate) fn project(
    cluster: &Cluster<'_>,
    types: &TypeTable,
    listed: &[Listed],
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) {
    let mut selections = 0_usize;
    let mut threads = 0_usize;
    for entry in listed {
        let Kind::Function(Some(function)) = &entry.kind else {
            continue;
        };
        if !matches!(
            function.as_str(),
            "JMHoleFunction"
                | "JMRoundFunction"
                | "JMChamferFunction"
                | "JMThinwallAssocFunctionNew"
        ) {
            continue;
        }
        let Some(object) = cluster.object(entry.object) else {
            continue;
        };
        if types.class_name(object.form, object.flags) != Some(PART_CLASS) {
            continue;
        }
        let Some((_, end)) = function_span(&object) else {
            continue;
        };
        let id = feature_id(entry.object);
        let read = match function.as_str() {
            "JMHoleFunction" => frame_of(ir, &id)
                .ok_or_else(|| "its profile is not a stated sketch".to_owned())
                .and_then(|frame| hole(cluster, types, &object, end, &frame)),
            "JMRoundFunction" => round(cluster, types, &object, end),
            "JMChamferFunction" => chamfer(cluster, types, &object, end),
            _ => thinwall(cluster, types, &object, end),
        };
        match read {
            Ok(read) => {
                if let Some(feature) = ir.design.features.iter_mut().find(|f| f.id == id) {
                    feature.definition.operation = read.operation;
                    selections += usize::from(read.selection_unread);
                    threads += usize::from(read.thread_unread);
                }
            }
            Err(reason) => losses.push(SolidEdgeLossCode::FeatureDressupUnread.note(format!(
                "feature object {:#x} ({function}) does not read: {reason}; the feature stays \
                 unresolved",
                entry.object
            ))),
        }
    }
    if selections > 0 {
        losses.push(SolidEdgeLossCode::FeatureSelectionUnread.note(format!(
            "{selections} feature(s) select edges or faces that are not read; each states an \
             unread selection"
        )));
    }
    if threads > 0 {
        losses.push(SolidEdgeLossCode::FeatureThreadUnread.note(format!(
            "{threads} tapped hole feature(s) store a thread that is not read; each states its \
             tap drill bore without a thread"
        )));
    }
}

#[cfg(test)]
mod tests;
