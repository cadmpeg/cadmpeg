// SPDX-License-Identifier: Apache-2.0
//! Revolve controls and parameter dimensions.

use cadmpeg_core::decode::{bounded_len, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    AngularTermination, BooleanOp, PlanarProfileRef, RevolutionAxis, RevolveConstruction,
    RevolveExtent, SketchProfileRegion,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::native::bytes::NativeBytes;
use cadmpeg_ir::scalar::Angle;
use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchEntityId, SketchGeometryDefinition};
use serde::{Deserialize, Serialize};

use super::{
    PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcFeatureReferenceFamily, AUXILIARY_ENUM_TYPE, BOOLEAN_TYPE, BOUNDARY_PATCH_TYPE,
    EXTENT_TYPE, EXTENT_TYPE_12_FIRST_MAJOR, FEATURE_DIMENSIONS_TYPE, OBJECT_COLLECTION_TYPE,
    PART_OPERATION_TYPE, REVOLVE_EXTENT_FULL, WORK_AXIS_TYPE,
};
use crate::layout::pm_dc_feature_dimension_tail as dimension_tail;
use crate::loss::InventorLossCode;
use crate::pmdc::{type_id_string, Cursor, PmDcListMetadata, PmDcReference};
use crate::record_identity::{Located, RecordPayload};

pub(super) const CONTROL_TYPE: [u8; 16] = [
    0xc2, 0x6a, 0xa0, 0xc7, 0x28, 0x41, 0x23, 0x12, 0xab, 0xf0, 0x72, 0x8f, 0x9d, 0x59, 0x8b, 0xc9,
];
pub(crate) const DIMENSION_TYPE: [u8; 16] = [
    0xb1, 0x37, 0x77, 0x79, 0xd2, 0x11, 0x67, 0x94, 0xa0, 0x00, 0x49, 0x80, 0x36, 0x03, 0xc8, 0xc9,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DimensionPayload {
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) item_count: u32,
    pub(crate) metadata: Option<PmDcListMetadata>,
    pub(crate) items: NativeBytes<Vec<u8>>,
    pub(crate) annotation: NativeBytes<[u8; 5]>,
    pub(crate) parameter: PmDcReference,
}

impl Default for DimensionPayload {
    fn default() -> Self {
        Self {
            header_value: 0,
            header_id: 0,
            item_count: 0,
            metadata: None,
            items: NativeBytes::new(Vec::new()),
            annotation: NativeBytes::new([0; 5]),
            parameter: PmDcReference::from_packed(0),
        }
    }
}

pub(crate) type Dimension = Located<DimensionPayload>;

impl RecordPayload for DimensionPayload {
    const KIND: &'static str = "feature-dimension";
}

pub(super) fn parse_dimension(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
) -> Result<DimensionPayload, CodecError> {
    let mut cursor = Cursor::new(source);
    let header_value = cursor.u32("feature dimension header value")?;
    let header_id = cursor.u16("feature dimension header id")?;
    if cursor.u32("feature dimension list marker")? != 0x3000_0002 {
        return Err(CodecError::malformed(
            "Inventor feature dimension list marker is invalid",
        ));
    }
    let count = cursor.u32("feature dimension item count")?;
    let metadata = if count == 0 {
        None
    } else {
        Some(PmDcListMetadata::U32(
            cursor.u32_array("feature dimension list metadata")?,
        ))
    };
    let mut source = cursor.into_view();
    let data_len = source
        .remaining()
        .checked_sub(dimension_tail::LEN)
        .ok_or_else(|| CodecError::truncated(source.location(), "feature dimension tail"))?;
    if (count == 0 && data_len != 0) || bounded_len(u64::from(count), 1, data_len).is_none() {
        return Err(CodecError::malformed(
            "Inventor feature dimension count disagrees with its annotation data",
        ));
    }
    let mut items = ctx.alloc_filled(data_len, 0u8, "retain Inventor dimension annotation data")?;
    items.copy_from_slice(crate::reader::take(
        &mut source,
        data_len,
        "feature dimension annotation data",
    )?);
    let mut cursor = Cursor::new(source);
    let mut annotation = [0; 5];
    for byte in &mut annotation {
        *byte = cursor.u8("feature dimension annotation byte")?;
    }
    let parameter = cursor.reference("feature dimension parameter")?;
    cursor.finish("feature dimension")?;
    Ok(DimensionPayload {
        header_value,
        header_id,
        item_count: count,
        metadata,
        items: NativeBytes::new(items),
        annotation: NativeBytes::new(annotation),
        parameter,
    })
}

pub(crate) struct Operands {
    pub(crate) axis: RevolutionAxis,
    pub(crate) extent: RevolveExtent,
    pub(crate) op: BooleanOp,
    pub(crate) unresolved_control: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeReason {
    Slots,
    Slot(usize),
    Operation,
    Angle,
    Dimension,
    Axis,
    Extent(u16),
    Direction(u16),
    ProfileOrResult,
    ProfileCrossesAxis,
}

impl NativeReason {
    pub(crate) fn note(self) -> cadmpeg_ir::report::loss::LossNote {
        let code = match self {
            Self::Extent(_) => InventorLossCode::FeatureRevolveExtentNative,
            Self::Direction(_) => InventorLossCode::FeatureRevolveDirectionNative,
            _ => InventorLossCode::FeatureRevolveOperandsNative,
        };
        let reason = match self {
            Self::Slots => "property vector is not the 20-slot form".to_owned(),
            Self::Slot(slot) => format!("slot {slot} differs from the admitted control pattern"),
            Self::Operation => "part operation is unresolved".to_owned(),
            Self::Angle => "angular parameter is unresolved or outside the extent range".to_owned(),
            Self::Dimension => {
                "feature dimension does not resolve to the angle parameter".to_owned()
            }
            Self::Axis => "work-axis controls or nonzero span are unresolved".to_owned(),
            Self::Extent(value) => format!("extent value {value} has unresolved angular operands"),
            Self::Direction(value) => format!("fixed-angle direction value {value} is unresolved"),
            Self::ProfileOrResult => "profile or result-body selection is unresolved".to_owned(),
            Self::ProfileCrossesAxis => {
                "the profile has points on both sides of the revolve axis".to_owned()
            }
        };
        code.note(format!("Retained Inventor revolve natively: {reason}."))
    }
}

pub(super) fn indexed_operands(
    source: &PmDcFeature,
    index: &super::ProjectionIndex<'_>,
) -> Result<Operands, NativeReason> {
    operands(
        source,
        |slot| super::slot_property(source, slot, index),
        |slot| super::angle_parameter(source, slot, index),
        |ordinal| {
            index
                .dimensions
                .get(&(source.identity.segment_token.as_str(), ordinal))?
                .as_ref()
                .copied()
        },
    )
}

pub(crate) fn operands<'a>(
    source: &PmDcFeature,
    property: impl Fn(usize) -> Option<&'a PmDcFeatureProperty>,
    angle: impl Fn(usize) -> Option<Angle>,
    dimension: impl Fn(u32) -> Option<&'a Dimension>,
) -> Result<Operands, NativeReason> {
    let slots = source.properties.references();
    if slots.len() != 20 {
        return Err(NativeReason::Slots);
    }
    for slot in [6, 8, 13, 14, 15, 18, 19] {
        if slots[slot].index() != 0 {
            return Err(NativeReason::Slot(slot));
        }
    }
    if slots[11].index() != 0 && slots[11].index() != slots[1].index() {
        return Err(NativeReason::Slot(11));
    }
    for (slot, type_id) in [
        (0, PART_OPERATION_TYPE),
        (1, BOUNDARY_PATCH_TYPE),
        (2, WORK_AXIS_TYPE),
        (3, EXTENT_TYPE),
        (5, CONTROL_TYPE),
        (7, FEATURE_DIMENSIONS_TYPE),
        (9, AUXILIARY_ENUM_TYPE),
        (10, BOOLEAN_TYPE),
        (12, BOOLEAN_TYPE),
        (16, BOOLEAN_TYPE),
        (17, OBJECT_COLLECTION_TYPE),
    ] {
        if property(slot)
            .ok_or(NativeReason::Slot(slot))?
            .identity
            .type_id
            .as_str()
            != type_id_string(type_id)
        {
            return Err(NativeReason::Slot(slot));
        }
    }
    let enumeration = |slot, family, expected_type| match property(slot)?.kind {
        PmDcFeaturePropertyKind::Enumeration {
            family: actual,
            type_value,
            value,
        } if actual == family && type_value == expected_type => Some(value),
        _ => None,
    };
    let op = match enumeration(0, PmDcFeatureEnumFamily::PartOperation, 5)
        .ok_or(NativeReason::Operation)?
    {
        1 => BooleanOp::NewBody,
        2 => BooleanOp::Cut,
        3 => BooleanOp::Join,
        4 => BooleanOp::Intersect,
        _ => return Err(NativeReason::Operation),
    };
    if enumeration(9, PmDcFeatureEnumFamily::Auxiliary, 4).ok_or(NativeReason::Slot(9))? != 0 {
        return Err(NativeReason::Slot(9));
    }
    let control =
        enumeration(5, PmDcFeatureEnumFamily::Auxiliary, 3).ok_or(NativeReason::Slot(5))?;
    if !matches!(control, 0 | 1) {
        return Err(NativeReason::Slot(5));
    }
    for (slot, expected) in [(10, true), (12, false)] {
        if !matches!(&property(slot).ok_or(NativeReason::Slot(slot))?.kind, PmDcFeaturePropertyKind::Boolean { value, .. } if *value == expected)
        {
            return Err(NativeReason::Slot(slot));
        }
    }
    let PmDcFeaturePropertyKind::Boolean {
        value: unresolved_control,
        ..
    } = property(16).ok_or(NativeReason::Slot(16))?.kind
    else {
        return Err(NativeReason::Slot(16));
    };
    if !matches!(&property(1).ok_or(NativeReason::Slot(1))?.kind, PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::BoundaryPatch, items,
    } if !items.references().is_empty())
    {
        return Err(NativeReason::Slot(1));
    }
    let angle_value = angle(4).ok_or(NativeReason::Angle)?;
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::FeatureDimensions,
        items,
    } = &property(7).ok_or(NativeReason::Dimension)?.kind
    else {
        return Err(NativeReason::Dimension);
    };
    let [item] = items.references() else {
        return Err(NativeReason::Dimension);
    };
    let dimension = dimension(item.record_ordinal().ok_or(NativeReason::Dimension)?)
        .ok_or(NativeReason::Dimension)?;
    if dimension.parameter.index() != slots[4].index() {
        return Err(NativeReason::Dimension);
    }
    let PmDcFeaturePropertyKind::WorkAxis {
        entity_flags: 0,
        values,
        references,
        marker: false,
        origin,
        span,
        state: 0 | 1,
    } = &property(2).ok_or(NativeReason::Axis)?.kind
    else {
        return Err(NativeReason::Axis);
    };
    if values.len() != if source.save_version_major > 21 { 3 } else { 2 }
        || values.iter().any(|value| *value != 0)
        || !references.references().is_empty()
    {
        return Err(NativeReason::Axis);
    }
    let origin = origin.map(cadmpeg_ir::scalar::FiniteReal::get);
    let span = span.map(cadmpeg_ir::scalar::FiniteReal::get);
    let direction_vector =
        cadmpeg_ir::units::UnitVector3::normalized(Vector3::new(span[0], span[1], span[2]))
            .ok_or(NativeReason::Axis)?;
    let axis = RevolutionAxis {
        origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(
            origin[0] * 10.0,
            origin[1] * 10.0,
            origin[2] * 10.0,
        ))
        .ok_or(NativeReason::Axis)?,
        direction: cadmpeg_ir::features::FeatureDirection3::from(direction_vector),
        reference: None,
    };
    let extent_type = if source.save_version_major >= EXTENT_TYPE_12_FIRST_MAJOR {
        12
    } else {
        11
    };
    let extent_value =
        enumeration(3, PmDcFeatureEnumFamily::Extent, extent_type).ok_or(NativeReason::Slot(3))?;
    let travel = match extent_value {
        REVOLVE_EXTENT_FULL => std::f64::consts::TAU,
        1 if control == 0 => angle_value.get(),
        1 => return Err(NativeReason::Direction(control)),
        other => return Err(NativeReason::Extent(other)),
    };
    let angle = cadmpeg_ir::scalar::PositiveAngle::new(travel).ok_or(NativeReason::Angle)?;
    Ok(Operands {
        axis,
        extent: RevolveExtent::OneSided {
            termination: AngularTermination::Angle { angle },
        },
        op,
        unresolved_control,
    })
}

/// Distance from the axis, in sketch units, at or below which a profile
/// point lies on the axis.
pub(crate) const EPS_AXIS_SIDE: f64 = 1.0e-7;
/// Largest component of the axis direction along the sketch normal.
const EPS_AXIS_DIRECTION_NORMAL: f64 = 1.0e-9;
/// Largest distance of the axis origin from the sketch plane, in sketch units.
const EPS_AXIS_PLANE_OFFSET: f64 = 1.0e-6;
/// Samples along one profile arc.
const ARC_SAMPLES: u32 = 32;

/// A revolve axis in the coordinates of its profile sketch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SketchAxis {
    origin: [f64; 2],
    direction: [f64; 2],
}

impl SketchAxis {
    /// The axis in the sketch plane, or `None` when the axis does not lie in
    /// that plane or the sketch is unplaced.
    pub(crate) fn in_sketch(sketch: &Sketch, axis: &RevolutionAxis) -> Option<Self> {
        let (frame_origin, normal, u_axis) = sketch.resolved_placement()?;
        let (frame_origin, normal, u) = (frame_origin.get(), normal.get(), u_axis.get());
        let normal = unit([normal.x, normal.y, normal.z])?;
        let u = unit([u.x, u.y, u.z])?;
        let v = [
            normal[1] * u[2] - normal[2] * u[1],
            normal[2] * u[0] - normal[0] * u[2],
            normal[0] * u[1] - normal[1] * u[0],
        ];
        let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let direction = axis.direction.get();
        let direction = unit([direction.x, direction.y, direction.z])?;
        let origin = axis.origin.get();
        let offset = [
            origin.x - frame_origin.x,
            origin.y - frame_origin.y,
            origin.z - frame_origin.z,
        ];
        if dot(direction, normal).abs() > EPS_AXIS_DIRECTION_NORMAL
            || dot(offset, normal).abs() > EPS_AXIS_PLANE_OFFSET
        {
            return None;
        }
        Some(Self {
            origin: [dot(offset, u), dot(offset, v)],
            direction: [dot(direction, u), dot(direction, v)],
        })
    }

    /// Whether the points lie strictly on both sides of the axis. Points on
    /// the axis, within [`EPS_AXIS_SIDE`], belong to neither side.
    pub(crate) fn crossed_by(&self, points: &[[f64; 2]]) -> bool {
        let (mut negative, mut positive) = (false, false);
        for point in points {
            let side = self.direction[0] * (point[1] - self.origin[1])
                - self.direction[1] * (point[0] - self.origin[0]);
            negative |= side < -EPS_AXIS_SIDE;
            positive |= side > EPS_AXIS_SIDE;
        }
        negative && positive
    }
}

fn unit(vector: [f64; 3]) -> Option<[f64; 3]> {
    let norm = vector.iter().map(|c| c * c).sum::<f64>().sqrt();
    (norm.is_finite() && norm > 0.0).then(|| vector.map(|c| c / norm))
}

/// Whether the revolve profile has points on both sides of its axis. `None`
/// when the profile form, a profile curve, or the axis placement is outside
/// what this predicate evaluates: whole-loop regions of lines, arcs, and
/// circles about an axis in the sketch plane.
pub(crate) fn profile_crosses_axis<'a>(
    profile: &PlanarProfileRef,
    axis: &RevolutionAxis,
    sketch: impl Fn(&cadmpeg_ir::sketches::SketchId) -> Option<&'a Sketch>,
    entity: impl Fn(&SketchEntityId) -> Option<&'a SketchEntity>,
) -> Option<bool> {
    let PlanarProfileRef::SketchRegions {
        sketch: id,
        regions,
    } = profile
    else {
        return None;
    };
    let sketch = sketch(id)?;
    let axis = SketchAxis::in_sketch(sketch, axis)?;
    let mut points = Vec::new();
    for region in regions.as_slice() {
        let SketchProfileRegion::Loops { loops } = region else {
            return None;
        };
        for index in std::iter::once(loops.outer()).chain(loops.holes().iter().copied()) {
            let chain = sketch
                .profiles
                .as_slice()
                .get(cadmpeg_core::decode::index_from_u32(index))?;
            for entity_use in chain {
                let geometry = entity(&entity_use.entity)?;
                curve_points(&geometry.geometry.definition().to_raw(), &mut points)?;
            }
        }
    }
    Some(axis.crossed_by(&points))
}

fn curve_points(geometry: &SketchGeometryDefinition, points: &mut Vec<[f64; 2]>) -> Option<()> {
    match geometry {
        SketchGeometryDefinition::Line { start, end } => {
            points.extend([[start.u, start.v], [end.u, end.v]]);
        }
        SketchGeometryDefinition::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let start = start_angle.get();
            let mut sweep = end_angle.get() - start;
            if sweep <= 0.0 {
                sweep += std::f64::consts::TAU;
            }
            for step in 0..=ARC_SAMPLES {
                let angle = start + sweep * f64::from(step) / f64::from(ARC_SAMPLES);
                points.push([
                    center.u + radius.get() * angle.cos(),
                    center.v + radius.get() * angle.sin(),
                ]);
            }
        }
        SketchGeometryDefinition::Circle { center, radius } => {
            for (du, dv) in [(1.0, 0.0), (0.0, 1.0), (-1.0, 0.0), (0.0, -1.0)] {
                points.push([center.u + du * radius.get(), center.v + dv * radius.get()]);
            }
        }
        _ => return None,
    }
    Some(())
}

pub(crate) fn matches(
    expected: &Operands,
    construction: &RevolveConstruction,
    op: BooleanOp,
) -> bool {
    matches!(construction, RevolveConstruction::Resolved {
        axis, extent, solid: Some(true), face_maker: None, fuse_order: None,
        allow_multi_profile_faces: None, ..
    } if *axis == expected.axis && *extent == expected.extent && op == expected.op)
}

#[cfg(test)]
mod tests;
