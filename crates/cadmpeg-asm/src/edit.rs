// SPDX-License-Identifier: Apache-2.0
//! Apply fixed-width edits to a framed ASM SAB stream.

use crate::kernel_header::RefWidth;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{NurbsCurve, NurbsSurface, PcurveGeometry, ProceduralCurveDefinition};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::transform::Transform;

use crate::asm_header;
use crate::nurbs::reader::KnotLayout;
use crate::nurbs::reader::LEN_TO_MM;
use crate::sab::{self, Record};
use cadmpeg_ir::geometry::{
    CompoundComponent, IntcurveSupportContext, ProjectionTail, SilhouetteKind, SpringLayout,
    SurfaceCurveFamily,
};
use cadmpeg_ir::ids::CurveId;

/// Framing and fixed-width write context for one ASM record stream.
///
/// The context owns the framed record table and the stream-wide integer width.
/// Callers retain format-specific edit selection and pass each selected record
/// to the typed field writers.
pub struct AsmEditSet {
    records: Vec<Record>,
    ref_width: RefWidth,
    header_scale: Option<f64>,
}

/// Writable values for one solved NURBS surface cache.
#[derive(Clone, Copy)]
pub struct NurbsSurfaceEdit<'a> {
    /// Neutral surface geometry.
    pub surface: &'a NurbsSurface,
    /// Optional native periodic flags in U/V order.
    pub periodic: Option<[bool; 2]>,
}

/// The solved NURBS curve cache selected for patching.
#[derive(Clone, Copy)]
pub enum CacheTarget {
    /// The final curve cache.
    Final,
    /// The first curve cache.
    First,
}

/// Writable values for one solved NURBS curve cache.
#[derive(Clone, Copy)]
pub struct NurbsCurveEdit<'a> {
    /// Neutral curve geometry.
    pub curve: &'a NurbsCurve,
    /// Optional native periodic flag.
    pub periodic: Option<bool>,
}

/// Writable values for one solved NURBS parameter-curve cache.
#[derive(Clone, Copy)]
pub struct InlinePcurveEdit<'a> {
    /// Parameter-curve geometry in the carrier's native chart.
    pub native_geometry: &'a PcurveGeometry,
    /// Optional native periodic flag.
    pub periodic: Option<bool>,
    /// Optional wrapper reversal flag.
    pub wrapper_reversed: Option<bool>,
    /// Optional four-flag native metadata tail.
    pub native_tail_flags: Option<[bool; 4]>,
    /// Optional native wrapper parameter range.
    pub parameter_range: Option<[f64; 2]>,
    /// Optional solved-cache fit tolerance.
    pub fit_tolerance: Option<f64>,
}

/// Writable fields selected by the native pcurve form.
#[derive(Clone, Copy)]
pub enum PcurveEdit<'a> {
    /// Geometry and optional metadata of an inline cache.
    Inline(InlinePcurveEdit<'a>),
    /// The parameter interval stored by a reference wrapper.
    Ref {
        /// The replacement interval, when edited.
        parameter_range: Option<[f64; 2]>,
    },
}

impl AsmEditSet {
    /// Frame the solved record partition and apply edits through one context.
    pub fn apply<T>(
        bytes: &mut [u8],
        edit: impl FnOnce(&mut [u8], &Self) -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        let edits = Self::frame(bytes)?;
        edit(bytes, &edits)
    }

    /// Frame the solved record partition without changing the input bytes.
    pub fn frame(bytes: &[u8]) -> Result<Self, CodecError> {
        let header = asm_header::parse(bytes)
            .ok_or_else(|| CodecError::Malformed("active BREP has no SAB record stream".into()))?;
        let start = asm_header::record_stream_start_with_header(bytes, &header)
            .ok_or_else(|| CodecError::Malformed("active BREP has no SAB record stream".into()))?;
        let limit =
            asm_header::solved_record_limit_with_header(bytes, &header).unwrap_or(bytes.len());
        let ref_width = header.width;
        let records = sab::frame(bytes, start, limit, ref_width).map_err(|error| {
            CodecError::malformed(format_args!("cannot frame active BREP: {error}"))
        })?;
        let header_scale = header.metadata.scale.unwrap_or(1.0);
        Ok(Self::from_framed(records, ref_width, header_scale))
    }

    /// Build a context from an already framed record partition.
    pub fn from_framed(records: Vec<Record>, ref_width: RefWidth, header_scale: f64) -> Self {
        Self {
            records,
            ref_width,
            header_scale: Some(header_scale).filter(|scale| scale.is_finite() && *scale > 0.0),
        }
    }

    /// The framed solved records in record-table order.
    pub fn records(&self) -> &[Record] {
        &self.records
    }

    /// Find one solved record by its record-table index.
    pub fn record(&self, index: usize) -> Option<&Record> {
        self.records.iter().find(|record| record.index == index)
    }

    /// Integer and reference payload width for this SAB stream.
    pub const fn ref_width(&self) -> RefWidth {
        self.ref_width
    }

    /// Positive finite ASM length scale, absent when the native scale is invalid.
    pub const fn header_scale(&self) -> Option<f64> {
        self.header_scale
    }

    /// Locate a payload value token and require its exact tag.
    pub fn required_payload_field(
        &self,
        bytes: &[u8],
        record: &Record,
        index: usize,
        tag: u8,
    ) -> Result<usize, CodecError> {
        Self::required_payload_field_at(bytes, record, self.ref_width, index, tag)
    }

    /// Locate a payload value token with an explicit stream integer width.
    pub fn required_payload_field_at(
        bytes: &[u8],
        record: &Record,
        ref_width: RefWidth,
        index: usize,
        tag: u8,
    ) -> Result<usize, CodecError> {
        let offset =
            sab::payload_token_offset(bytes, record, ref_width, index).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "{} record {} lacks payload field {index}",
                    record.head(),
                    record.index
                ))
            })?;
        if bytes.get(offset) != Some(&tag) {
            return Err(CodecError::malformed(format_args!(
                "{} record {} payload field {index} is not tag {tag:#04x}",
                record.head(),
                record.index
            )));
        }
        Ok(offset)
    }

    /// Replace one tagged integer payload without changing its encoded width.
    pub fn patch_integer_field(
        &self,
        bytes: &mut [u8],
        record: &Record,
        index: usize,
        tag: u8,
        value: i64,
    ) -> Result<(), CodecError> {
        let offset = self.required_payload_field(bytes, record, index, tag)?;
        Self::patch_layout_integer(bytes, offset + 1, self.ref_width, value)
    }

    /// Replace one boolean sense token without changing its field position.
    pub fn patch_sense_field(
        &self,
        bytes: &mut [u8],
        record: &Record,
        index: usize,
        sense: Sense,
    ) -> Result<(), CodecError> {
        let offset =
            sab::payload_token_offset(bytes, record, self.ref_width, index).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "{} record {} lacks payload field {index}",
                    record.head(),
                    record.index
                ))
            })?;
        if !matches!(bytes.get(offset), Some(0x0a | 0x0b)) {
            return Err(CodecError::malformed(format_args!(
                "{} record {} payload field {index} is not a sense token",
                record.head(),
                record.index
            )));
        }
        Self::patch_native_bool(bytes, offset, sense == Sense::Reversed)
    }

    /// Replace one boolean token without changing its field position.
    pub fn patch_boolean_field(
        &self,
        bytes: &mut [u8],
        record: &Record,
        index: usize,
        value: bool,
    ) -> Result<(), CodecError> {
        let offset =
            sab::payload_token_offset(bytes, record, self.ref_width, index).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "{} record {} lacks boolean field {index}",
                    record.head(),
                    record.index
                ))
            })?;
        Self::patch_boolean_at(bytes, offset, value).map_err(|_| {
            CodecError::malformed(format_args!(
                "{} record {} payload field {index} is not a boolean token",
                record.head(),
                record.index
            ))
        })
    }

    /// Replace one boolean carrier at an absolute byte offset.
    pub fn patch_boolean_at(
        bytes: &mut [u8],
        offset: usize,
        value: bool,
    ) -> Result<(), CodecError> {
        if !matches!(bytes.get(offset), Some(0x0a | 0x0b)) {
            return Err(CodecError::Malformed(
                "ASM boolean token carrier is missing".into(),
            ));
        }
        Self::patch_native_bool(bytes, offset, value)
    }

    /// Replace one fixed-width ASCII token payload.
    pub fn patch_ascii_field(
        &self,
        bytes: &mut [u8],
        record: &Record,
        index: usize,
        value: &str,
    ) -> Result<(), CodecError> {
        let offset = self.required_payload_field(bytes, record, index, 0x07)?;
        let encoded_length = bytes.get(offset + 1).copied().ok_or_else(|| {
            CodecError::malformed(format_args!("{} record string is truncated", record.head()))
        })? as usize;
        if value.len() != encoded_length || !value.is_ascii() {
            return Err(CodecError::NotImplemented(format!(
                "{} record {} string edit must retain its encoded ASCII length",
                record.head(),
                record.index
            )));
        }
        Self::patch_bytes_at(bytes, offset, 2, value.as_bytes())
    }

    /// Replace one packed true-color integer without changing its carrier.
    pub fn patch_truecolor_field(
        &self,
        bytes: &mut [u8],
        record: &Record,
        field: usize,
        packed: u32,
    ) -> Result<(), CodecError> {
        let offset =
            sab::payload_token_offset(bytes, record, self.ref_width, field).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "{} record {} lacks packed truecolor field {field}",
                    record.head(),
                    record.index
                ))
            })?;
        match (bytes.get(offset).copied(), self.ref_width) {
            (Some(0x17), _) => {
                Self::patch_bytes_at(bytes, offset, 1, &i64::from(packed).to_le_bytes())?;
            }
            (Some(0x04), RefWidth::Four) => {
                Self::patch_bytes_at(bytes, offset, 1, &packed.to_le_bytes())?;
            }
            (Some(0x04), RefWidth::Eight) => {
                Self::patch_bytes_at(bytes, offset, 1, &i64::from(packed).to_le_bytes())?;
            }
            _ => {
                return Err(CodecError::malformed(format_args!(
                    "{} record {} truecolor field {field} is not an integer",
                    record.head(),
                    record.index
                )));
            }
        }
        Ok(())
    }

    /// Replace one decimal RGB string without changing its encoded width.
    pub fn patch_decimal_rgb_field(
        &self,
        bytes: &mut [u8],
        record: &Record,
        field: usize,
        packed: u32,
    ) -> Result<(), CodecError> {
        let Some(sab::Token::Str(current)) = record.chunk(field) else {
            return Err(CodecError::malformed(format_args!(
                "{} record {} decimal-color field {field} is not text",
                record.head(),
                record.index
            )));
        };
        let offset =
            sab::payload_token_offset(bytes, record, self.ref_width, field).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "{} record {} lacks decimal-color field {field}",
                    record.head(),
                    record.index
                ))
            })?;
        let length_width = match bytes.get(offset).copied() {
            Some(0x07) => 1,
            Some(0x08) => 2,
            Some(0x09 | 0x12) => self.ref_width.bytes(),
            _ => {
                return Err(CodecError::malformed(format_args!(
                    "{} record {} decimal-color field {field} has an invalid text tag",
                    record.head(),
                    record.index
                )));
            }
        };
        let width = current.len();
        let value = packed.to_string();
        if value.len() > width {
            return Err(CodecError::NotImplemented(format!(
                "{} record {} decimal-color edit exceeds its encoded text width",
                record.head(),
                record.index
            )));
        }
        let encoded = format!("{packed:0width$}");
        Self::patch_bytes_at(bytes, offset, 1 + length_width, encoded.as_bytes())
    }

    fn patch_bytes_at(
        bytes: &mut [u8],
        offset: usize,
        skip: usize,
        payload: &[u8],
    ) -> Result<(), CodecError> {
        let offset = offset
            .checked_add(skip)
            .ok_or_else(|| CodecError::Malformed("native byte payload offset overflows".into()))?;
        let end = offset
            .checked_add(payload.len())
            .ok_or_else(|| CodecError::Malformed("native byte payload offset overflows".into()))?;
        let target = bytes
            .get_mut(offset..end)
            .ok_or_else(|| CodecError::Malformed("native byte payload is truncated".into()))?;
        target.copy_from_slice(payload);
        Ok(())
    }

    /// Replace one fixed-width little-endian integer payload.
    pub fn patch_layout_integer(
        bytes: &mut [u8],
        offset: usize,
        width: RefWidth,
        value: i64,
    ) -> Result<(), CodecError> {
        if width == RefWidth::Four && i64::from(value as i32) != value {
            return Err(CodecError::NotImplemented(
                "F3D NURBS integer edit exceeds BinaryFile4 range".into(),
            ));
        }
        let end = offset
            .checked_add(width.bytes())
            .ok_or_else(|| CodecError::Malformed("ASM integer payload offset overflows".into()))?;
        let target = bytes.get_mut(offset..end).ok_or_else(|| {
            CodecError::Malformed("F3D NURBS integer payload is truncated".into())
        })?;
        target.copy_from_slice(&value.to_le_bytes()[..width.bytes()]);
        Ok(())
    }

    /// Replace an integer payload whose carrier tag is at `tag_offset`.
    pub fn patch_tagged_integer_at(
        bytes: &mut [u8],
        tag_offset: usize,
        width: RefWidth,
        value: i64,
    ) -> Result<(), CodecError> {
        if !matches!(bytes.get(tag_offset), Some(0x04 | 0x0c | 0x15)) {
            return Err(CodecError::Malformed(
                "F3D tagged integer carrier is missing".into(),
            ));
        }
        Self::patch_layout_integer(bytes, tag_offset + 1, width, value)
    }

    /// Replace one 8-byte integer field in a fixed-stride tagged record.
    pub fn patch_tagged_i64(
        bytes: &mut [u8],
        record_offset: u64,
        ordinal: usize,
        expected_tag: u8,
        value: i64,
    ) -> Result<(), CodecError> {
        let tag = usize::try_from(record_offset)
            .ok()
            .and_then(|offset| {
                ordinal
                    .checked_mul(9)
                    .and_then(|step| offset.checked_add(step))
            })
            .ok_or_else(|| {
                CodecError::Malformed("ASM record offset exceeds address space".into())
            })?;
        if bytes.get(tag) != Some(&expected_tag) {
            return Err(CodecError::malformed(format_args!(
                "ASM field {ordinal} at byte {tag} has the wrong token tag"
            )));
        }
        let end = tag
            .checked_add(9)
            .ok_or_else(|| CodecError::Malformed("ASM tagged integer offset overflows".into()))?;
        bytes
            .get_mut(tag + 1..end)
            .ok_or_else(|| CodecError::Malformed("ASM tagged integer is truncated".into()))?
            .copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    /// Replace one little-endian `f64` payload.
    pub fn patch_f64_payload(
        bytes: &mut [u8],
        offset: usize,
        value: f64,
    ) -> Result<(), CodecError> {
        let end = offset.checked_add(8).ok_or_else(|| {
            CodecError::Malformed("native double payload offset overflows".into())
        })?;
        bytes
            .get_mut(offset..end)
            .ok_or_else(|| CodecError::Malformed("native double payload is truncated".into()))?
            .copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    /// Replace one native boolean byte at a checked offset.
    pub fn patch_native_bool(
        bytes: &mut [u8],
        offset: usize,
        value: bool,
    ) -> Result<(), CodecError> {
        let target = bytes
            .get_mut(offset)
            .ok_or_else(|| CodecError::Malformed("native boolean payload is truncated".into()))?;
        *target = native_bool(value);
        Ok(())
    }

    /// Replace little-endian `f64` payloads relative to one record offset.
    pub fn patch_f64_payloads(
        bytes: &mut [u8],
        record_offset: usize,
        patches: impl IntoIterator<Item = (usize, f64)>,
    ) -> Result<(), CodecError> {
        for (offset, value) in patches {
            let at = record_offset.checked_add(offset).ok_or_else(|| {
                CodecError::Malformed("native double payload offset overflows".into())
            })?;
            Self::patch_f64_payload(bytes, at, value)?;
        }
        Ok(())
    }

    /// Replace three consecutive little-endian `f64` payloads.
    pub fn patch_vector_payload(
        bytes: &mut [u8],
        offset: usize,
        components: [f64; 3],
    ) -> Result<(), CodecError> {
        for (component, value) in components.into_iter().enumerate() {
            let component_offset = component
                .checked_mul(8)
                .and_then(|step| offset.checked_add(step))
                .ok_or_else(|| {
                    CodecError::Malformed("native vector payload offset overflows".into())
                })?;
            Self::patch_f64_payload(bytes, component_offset, value)?;
        }
        Ok(())
    }

    /// Replace knot values and multiplicities without changing cardinality.
    pub fn patch_knot_structure(
        bytes: &mut [u8],
        record_offset: usize,
        layout: &KnotLayout,
        knots: &[f64],
        int_width: RefWidth,
    ) -> Result<(), CodecError> {
        let mut runs: Vec<(f64, usize)> = Vec::new();
        for knot in knots {
            if let Some((value, count)) = runs.last_mut() {
                if *value == *knot {
                    *count += 1;
                    continue;
                }
            }
            runs.push((*knot, 1));
        }
        if runs.len() != layout.value_offsets.len() {
            return Err(CodecError::NotImplemented(
                "F3D NURBS curve edit changes the unique-knot count".into(),
            ));
        }
        for (ordinal, ((value, expanded_count), value_offset)) in
            runs.into_iter().zip(&layout.value_offsets).enumerate()
        {
            let endpoint_extra =
                usize::from(ordinal == 0 || ordinal + 1 == layout.value_offsets.len());
            let stored = expanded_count
                .checked_sub(endpoint_extra)
                .filter(|count| *count > 0)
                .ok_or_else(|| {
                    CodecError::NotImplemented(
                        "F3D NURBS curve knot multiplicity is not writable".into(),
                    )
                })?;
            let stored = i64::try_from(stored).map_err(|_| {
                CodecError::Malformed("F3D NURBS curve knot multiplicity exceeds i64".into())
            })?;
            let value_at = record_offset.checked_add(*value_offset).ok_or_else(|| {
                CodecError::Malformed("ASM knot value offset exceeds address space".into())
            })?;
            Self::patch_f64_payload(bytes, value_at, value)?;
            let multiplicity_at = value_at.checked_add(9).ok_or_else(|| {
                CodecError::Malformed("ASM knot multiplicity offset exceeds address space".into())
            })?;
            Self::patch_layout_integer(bytes, multiplicity_at, int_width, stored)?;
        }
        Ok(())
    }

    /// Apply one writable procedural-curve definition to its SAB carrier.
    pub fn patch_procedural_curve_definition(
        &self,
        bytes: &mut [u8],
        record: &Record,
        definition: &ProceduralCurveDefinition,
    ) -> Result<(), CodecError> {
        match definition {
            ProceduralCurveDefinition::Helix(helix) => {
                patch_helix_definition(bytes, self.ref_width, record, helix)
            }
            ProceduralCurveDefinition::VectorOffset {
                parameter_range,
                offset,
                ..
            } => patch_vector_offset_definition(
                bytes,
                self.ref_width,
                record,
                *parameter_range,
                *offset,
            ),
            ProceduralCurveDefinition::Subset {
                parameter_range, ..
            } => patch_subset_definition(bytes, self.ref_width, record, *parameter_range),
            ProceduralCurveDefinition::Compound(compound) => {
                let (parameters, components) = compound.parts();
                patch_compound_definition(bytes, self.ref_width, record, parameters, components)
            }
            ProceduralCurveDefinition::TwoSidedOffset {
                context,
                discontinuity_flag,
                offsets,
                ..
            } => patch_two_sided_offset_definition(
                bytes,
                self.ref_width,
                record,
                context,
                *discontinuity_flag,
                *offsets,
            ),
            ProceduralCurveDefinition::SurfaceOffset {
                context,
                discontinuity_flag,
                base_u_range,
                base_v_range,
                base_range,
                distance,
                shift,
                scale,
                ..
            } => patch_surface_offset_definition(
                bytes,
                self.ref_width,
                record,
                SurfaceOffsetFields {
                    context,
                    discontinuity_flag,
                    base_u_range,
                    base_v_range,
                    base_range,
                    distance,
                    shift,
                    scale,
                },
            ),
            ProceduralCurveDefinition::Spring {
                layout, direction, ..
            } => patch_spring_definition(bytes, self.ref_width, record, layout, *direction),
            ProceduralCurveDefinition::Projection {
                context,
                discontinuity_flag,
                tail,
                ..
            } => patch_projection_definition(
                bytes,
                self.ref_width,
                record,
                context,
                *discontinuity_flag,
                tail,
            ),
            ProceduralCurveDefinition::Intersection {
                context,
                discontinuity_flag,
                ..
            } => patch_intersection_definition(
                bytes,
                self.ref_width,
                record,
                context,
                *discontinuity_flag,
            ),
            ProceduralCurveDefinition::ThreeSurfaceIntersection {
                context, selector, ..
            } => patch_three_surface_intersection_definition(
                bytes,
                self.ref_width,
                record,
                context,
                *selector,
            ),
            ProceduralCurveDefinition::SurfaceCurve { family, .. } => {
                patch_surface_curve_definition(bytes, self.ref_width, record, family)
            }
            ProceduralCurveDefinition::Silhouette {
                silhouette,
                light_direction,
                ..
            } => patch_silhouette_definition(
                bytes,
                self.ref_width,
                record,
                silhouette,
                *light_direction,
            ),
            ProceduralCurveDefinition::Exact
            | ProceduralCurveDefinition::Law { .. }
            | ProceduralCurveDefinition::TolerantIntersection { .. }
            | ProceduralCurveDefinition::Deformable { .. }
            | ProceduralCurveDefinition::Offset { .. }
            | ProceduralCurveDefinition::SpatialOffset { .. }
            | ProceduralCurveDefinition::Replica { .. }
            | ProceduralCurveDefinition::BlendSpine { .. }
            | ProceduralCurveDefinition::Unknown { .. } => Err(CodecError::NotImplemented(
                "ASM procedural-curve definition is not writable".into(),
            )),
        }
    }

    /// Apply an extrusion construction to its solved spline carrier.
    pub fn patch_extrusion_definition(
        &self,
        bytes: &mut [u8],
        record: &Record,
        parameter_interval: [f64; 2],
        direction: Vector3,
        native_position: Point3,
    ) -> Result<(), CodecError> {
        let record_bytes = record_slice(bytes, record, "extrusion")?;
        let layout = crate::nurbs::proc_curve::extrusion_patch_layout(record_bytes, self.ref_width)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "spline record {} lacks writable extrusion fields",
                    record.index
                ))
            })?;
        Self::patch_f64_payloads(
            bytes,
            record.offset,
            layout
                .parameter_interval
                .into_iter()
                .zip(parameter_interval),
        )?;
        for (base, values) in [
            (
                layout.direction,
                [
                    direction.x / LEN_TO_MM,
                    direction.y / LEN_TO_MM,
                    direction.z / LEN_TO_MM,
                ],
            ),
            (
                layout.native_position,
                [
                    native_position.x / LEN_TO_MM,
                    native_position.y / LEN_TO_MM,
                    native_position.z / LEN_TO_MM,
                ],
            ),
        ] {
            Self::patch_vector_payload(bytes, record.offset + base, values)?;
        }
        Ok(())
    }

    /// Apply the two rolling-ball radii to their solved spline carrier.
    pub fn patch_blend_radii(
        &self,
        bytes: &mut [u8],
        record: &Record,
        radii: [f64; 2],
    ) -> Result<(), CodecError> {
        let record_bytes = record_slice(bytes, record, "rolling-ball")?;
        let layout =
            crate::nurbs::proc_curve::rolling_ball_patch_layout(record_bytes, self.ref_width)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "spline record {} lacks a writable rolling-ball radius pair",
                        record.index
                    ))
                })?;
        Self::patch_f64_payloads(
            bytes,
            record.offset,
            layout
                .radii
                .into_iter()
                .zip(radii)
                .map(|(offset, radius)| (offset, radius / LEN_TO_MM)),
        )
    }

    /// Apply the solved procedural-surface fit tolerance.
    pub fn patch_procedural_surface_fit(
        &self,
        bytes: &mut [u8],
        record: &Record,
        tolerance: f64,
    ) -> Result<(), CodecError> {
        let record_bytes = record_slice(bytes, record, "procedural-surface")?;
        let layout = crate::nurbs::core::final_surface_patch_layout(record_bytes, self.ref_width)
            .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "spline record {} has no solved surface cache",
                record.index
            ))
        })?;
        if record_bytes.get(layout.end()) != Some(&0x06) {
            return Err(CodecError::NotImplemented(format!(
                "spline record {} has no writable fit-tolerance carrier",
                record.index
            )));
        }
        Self::patch_f64_payload(
            bytes,
            record.offset + layout.end() + 1,
            tolerance / LEN_TO_MM,
        )
    }

    /// Apply the solved procedural-curve fit tolerance.
    pub fn patch_procedural_curve_fit(
        &self,
        bytes: &mut [u8],
        record: &Record,
        tolerance: f64,
    ) -> Result<(), CodecError> {
        let record_bytes = record_slice(bytes, record, "procedural-curve")?;
        let layout = crate::nurbs::core::final_curve_patch_layout(record_bytes, self.ref_width)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "intcurve record {} has no solved curve cache",
                    record.index
                ))
            })?;
        if record_bytes.get(layout.end()) != Some(&0x06) {
            return Err(CodecError::NotImplemented(format!(
                "intcurve record {} has no writable fit-tolerance carrier",
                record.index
            )));
        }
        Self::patch_f64_payload(
            bytes,
            record.offset + layout.end() + 1,
            tolerance / LEN_TO_MM,
        )
    }

    /// Apply a homogeneous transform to one ASM transform record.
    pub fn patch_transform(
        &self,
        bytes: &mut [u8],
        record: &Record,
        transform: Transform,
    ) -> Result<(), CodecError> {
        let header_scale = self.header_scale.ok_or_else(|| {
            CodecError::malformed(format_args!(
                "transform record {} requires a positive finite header scale",
                record.index
            ))
        })?;
        let vectors = [
            [
                transform.rows()[0][0],
                transform.rows()[1][0],
                transform.rows()[2][0],
            ],
            [
                transform.rows()[0][1],
                transform.rows()[1][1],
                transform.rows()[2][1],
            ],
            [
                transform.rows()[0][2],
                transform.rows()[1][2],
                transform.rows()[2][2],
            ],
            [
                transform.rows()[0][3] / (header_scale * LEN_TO_MM),
                transform.rows()[1][3] / (header_scale * LEN_TO_MM),
                transform.rows()[2][3] / (header_scale * LEN_TO_MM),
            ],
        ];
        for (index, vector) in vectors.into_iter().enumerate() {
            let offset = self.required_payload_field(bytes, record, index, 0x14)?;
            Self::patch_vector_payload(bytes, offset + 1, vector)?;
        }
        let scale = self.required_payload_field(bytes, record, 4, 0x06)?;
        Self::patch_f64_payload(bytes, scale + 1, transform.rows()[3][3])
    }

    /// Apply one solved NURBS surface cache edit.
    pub fn patch_nurbs_surface(
        &self,
        bytes: &mut [u8],
        record: &Record,
        edit: NurbsSurfaceEdit<'_>,
        surface_ordinal: Option<usize>,
    ) -> Result<(), CodecError> {
        patch_nurbs_surface_record(bytes, self.ref_width, record, &edit, surface_ordinal)
    }

    /// Apply one solved NURBS curve cache edit.
    pub fn patch_nurbs_curve(
        &self,
        bytes: &mut [u8],
        record: &Record,
        edit: NurbsCurveEdit<'_>,
        target: CacheTarget,
    ) -> Result<(), CodecError> {
        patch_nurbs_curve_record(bytes, self.ref_width, record, &edit, target)
    }

    /// Apply the fields selected by an inline cache or reference wrapper edit.
    pub fn patch_pcurve(
        &self,
        bytes: &mut [u8],
        record: &Record,
        edit: PcurveEdit<'_>,
    ) -> Result<(), CodecError> {
        match edit {
            PcurveEdit::Inline(edit) => {
                patch_nurbs_pcurve_record(bytes, self.ref_width, record, &edit)
            }
            PcurveEdit::Ref { parameter_range } => {
                patch_ref_pcurve_contract(bytes, self.ref_width, record, parameter_range)
            }
        }
    }
}

fn record_slice<'a>(bytes: &'a [u8], record: &Record, label: &str) -> Result<&'a [u8], CodecError> {
    let end = record.offset.checked_add(record.len).ok_or_else(|| {
        CodecError::malformed(format_args!(
            "{label} record extent overflows address space"
        ))
    })?;
    bytes
        .get(record.offset..end)
        .ok_or_else(|| CodecError::malformed(format_args!("{label} record is truncated")))
}

const fn native_bool(value: bool) -> u8 {
    if value {
        0x0a
    } else {
        0x0b
    }
}

fn finite_vector(vector: Vector3) -> bool {
    vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
}

fn patch_helix_definition(
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::HelixCurveConstruction,
) -> Result<(), CodecError> {
    let (angle_range, center, major, minor, pitch, apex_factor, axis) = definition.parts();

    let record_bytes = record_slice(bytes, record, "helix")?;
    let layout = crate::nurbs::proc_curve::helix_patch_layout(record_bytes, stream_width)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "procedural curve record {} lacks writable helix fields",
                record.index
            ))
        })?;
    AsmEditSet::patch_f64_payloads(
        bytes,
        record.offset,
        layout.angle_range.into_iter().zip(*angle_range),
    )?;
    for (offset, value) in layout.frame_vectors.into_iter().zip([
        [
            center.x / LEN_TO_MM,
            center.y / LEN_TO_MM,
            center.z / LEN_TO_MM,
        ],
        [
            major.x / LEN_TO_MM,
            major.y / LEN_TO_MM,
            major.z / LEN_TO_MM,
        ],
        [
            minor.x / LEN_TO_MM,
            minor.y / LEN_TO_MM,
            minor.z / LEN_TO_MM,
        ],
        [
            pitch.x / LEN_TO_MM,
            pitch.y / LEN_TO_MM,
            pitch.z / LEN_TO_MM,
        ],
    ]) {
        AsmEditSet::patch_vector_payload(bytes, record.offset + offset, value)?;
    }
    let apex_at = record.offset + layout.apex_factor;
    AsmEditSet::patch_f64_payload(bytes, apex_at, *apex_factor)?;
    AsmEditSet::patch_vector_payload(bytes, record.offset + layout.axis, [axis.x, axis.y, axis.z])?;
    Ok(())
}

fn patch_vector_offset_definition(
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    parameter_range: [f64; 2],
    offset: Vector3,
) -> Result<(), CodecError> {
    let record_bytes = record_slice(bytes, record, "vector-offset")?;
    let layout = crate::nurbs::proc_curve::vector_offset_patch_layout(record_bytes, stream_width)
        .ok_or_else(|| {
        CodecError::malformed(format_args!(
            "vector-offset record {} lacks writable construction fields",
            record.index
        ))
    })?;
    AsmEditSet::patch_f64_payloads(
        bytes,
        record.offset,
        layout.parameter_range.into_iter().zip(parameter_range),
    )?;
    AsmEditSet::patch_vector_payload(
        bytes,
        record.offset + layout.offset,
        [
            offset.x / LEN_TO_MM,
            offset.y / LEN_TO_MM,
            offset.z / LEN_TO_MM,
        ],
    )?;
    Ok(())
}

fn patch_subset_definition(
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    parameter_range: [f64; 2],
) -> Result<(), CodecError> {
    let record_bytes = record_slice(bytes, record, "subset")?;
    let layout = crate::nurbs::proc_curve::subset_patch_layout(record_bytes, stream_width)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "subset record {} lacks writable construction fields",
                record.index
            ))
        })?;
    AsmEditSet::patch_f64_payloads(
        bytes,
        record.offset,
        layout.parameter_range.into_iter().zip(parameter_range),
    )?;
    Ok(())
}

fn patch_compound_definition(
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    parameters: &[f64],
    components: &[CompoundComponent<CurveId>],
) -> Result<(), CodecError> {
    let record_bytes = record_slice(bytes, record, "compound")?;
    let layout = crate::nurbs::proc_curve::compound_patch_layout(record_bytes, stream_width)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "compound record {} lacks writable parameter arrays",
                record.index
            ))
        })?;
    if layout.parameters.len() != parameters.len()
        || layout.component_parameters.len() != components.len()
    {
        return Err(CodecError::NotImplemented(
            "compound edit changes native parameter cardinality".into(),
        ));
    }
    AsmEditSet::patch_f64_payloads(
        bytes,
        record.offset,
        layout
            .parameters
            .into_iter()
            .chain(layout.component_parameters)
            .zip(
                parameters
                    .iter()
                    .copied()
                    .chain(components.iter().map(|item| item.parameter)),
            ),
    )?;
    Ok(())
}

fn patch_two_sided_offset_definition(
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    context: &IntcurveSupportContext,
    discontinuity_flag: bool,
    offsets: [f64; 2],
) -> Result<(), CodecError> {
    let record_bytes = record_slice(bytes, record, "two-sided offset")?;
    let layout =
        crate::nurbs::proc_curve::two_sided_offset_patch_layout(record_bytes, stream_width)
            .filter(|layout| {
                layout
                    .discontinuities
                    .iter()
                    .map(Vec::len)
                    .eq(context.discontinuities().iter().map(Vec::len))
            })
            .ok_or_else(|| CodecError::Malformed("two-sided offset layout is malformed".into()))?;
    for (at, value) in layout
        .parameter_range
        .into_iter()
        .zip(context.parameter_range())
    {
        AsmEditSet::patch_f64_payload(bytes, record.offset + at, value)?;
    }
    for (locations, values) in layout.discontinuities.iter().zip(context.discontinuities()) {
        for (at, value) in locations.iter().zip(values) {
            AsmEditSet::patch_f64_payload(bytes, record.offset + *at, *value)?;
        }
    }
    AsmEditSet::patch_native_bool(
        bytes,
        record.offset + layout.discontinuity_flag,
        discontinuity_flag,
    )?;
    for (at, value) in layout.offsets.into_iter().zip(offsets) {
        AsmEditSet::patch_f64_payload(bytes, record.offset + at, value / LEN_TO_MM)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct SurfaceOffsetFields<'a> {
    context: &'a IntcurveSupportContext,
    discontinuity_flag: &'a bool,
    base_u_range: &'a [f64; 2],
    base_v_range: &'a [f64; 2],
    base_range: &'a [f64; 2],
    distance: &'a f64,
    shift: &'a f64,
    scale: &'a f64,
}

fn patch_intcurve_context(
    bytes: &mut [u8],
    record: &Record,
    parameter_range: [usize; 2],
    discontinuities: [Vec<usize>; 3],
    flag: Option<(usize, bool)>,
    context: &IntcurveSupportContext,
    label: &str,
) -> Result<(), CodecError> {
    if discontinuities
        .iter()
        .map(Vec::len)
        .ne(context.discontinuities().iter().map(Vec::len))
    {
        return Err(CodecError::malformed(format_args!(
            "{label} context is incomplete"
        )));
    }
    AsmEditSet::patch_f64_payloads(
        bytes,
        record.offset,
        parameter_range
            .into_iter()
            .chain(discontinuities.into_iter().flatten())
            .zip(
                context
                    .parameter_range()
                    .into_iter()
                    .chain(context.discontinuities().iter().flatten().copied()),
            ),
    )?;
    if let Some((offset, value)) = flag {
        AsmEditSet::patch_native_bool(bytes, record.offset + offset, value)?;
    }
    Ok(())
}

fn patch_surface_offset_definition(
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    fields: SurfaceOffsetFields<'_>,
) -> Result<(), CodecError> {
    let SurfaceOffsetFields {
        context,
        discontinuity_flag,
        base_u_range,
        base_v_range,
        base_range,
        distance,
        shift,
        scale,
    } = fields;

    if !distance.is_finite() || !shift.is_finite() || !scale.is_finite() {
        return Err(CodecError::Malformed(
            "surface-offset scalars must be finite".into(),
        ));
    }
    if [base_u_range, base_v_range, base_range]
        .into_iter()
        .flatten()
        .any(|value| !value.is_finite())
    {
        return Err(CodecError::Malformed(
            "surface-offset ranges must be finite".into(),
        ));
    }
    let record_bytes = record_slice(bytes, record, "surface-offset")?;
    let layout = crate::nurbs::proc_curve::surface_offset_patch_layout(record_bytes, stream_width)
        .ok_or_else(|| CodecError::Malformed("surface-offset construction is malformed".into()))?;
    patch_intcurve_context(
        bytes,
        record,
        layout.parameter_range,
        layout.discontinuities,
        Some((layout.discontinuity_flag, *discontinuity_flag)),
        context,
        "surface-offset",
    )?;
    AsmEditSet::patch_f64_payloads(
        bytes,
        record.offset,
        layout
            .base_u_range
            .into_iter()
            .chain(layout.base_v_range)
            .chain(layout.base_range)
            .chain([layout.distance, layout.shift, layout.scale])
            .zip(
                base_u_range
                    .iter()
                    .copied()
                    .chain(base_v_range.iter().copied())
                    .chain(base_range.iter().copied().chain([
                        distance / LEN_TO_MM,
                        *shift,
                        *scale,
                    ])),
            ),
    )?;
    Ok(())
}

fn patch_spring_definition(
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    layout: &SpringLayout,
    direction: i64,
) -> Result<(), CodecError> {
    let context = layout.support_context().map_err(CodecError::malformed)?;
    let discontinuity_flag = match layout {
        cadmpeg_ir::geometry::SpringLayout::ContextFirst {
            discontinuity_flag, ..
        } => *discontinuity_flag,
        cadmpeg_ir::geometry::SpringLayout::CacheFirst { .. } => false,
    };
    let record_bytes = record_slice(bytes, record, "spring")?;
    let layout = crate::nurbs::proc_curve::spring_patch_layout(record_bytes, stream_width)
        .ok_or_else(|| CodecError::Malformed("spring construction is malformed".into()))?;
    patch_intcurve_context(
        bytes,
        record,
        layout.parameter_range,
        layout.discontinuities,
        Some((layout.discontinuity_flag, discontinuity_flag)),
        &context,
        "spring",
    )?;

    AsmEditSet::patch_tagged_integer_at(
        bytes,
        record.offset + layout.direction,
        stream_width,
        direction,
    )?;
    Ok(())
}

fn patch_projection_definition(
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    context: &IntcurveSupportContext,
    discontinuity_flag: bool,
    tail: &ProjectionTail,
) -> Result<(), CodecError> {
    let record_bytes = record_slice(bytes, record, "projection")?;
    let layout = crate::nurbs::proc_curve::projection_patch_layout(record_bytes, stream_width)
        .ok_or_else(|| CodecError::Malformed("projection construction is malformed".into()))?;
    patch_intcurve_context(
        bytes,
        record,
        layout.parameter_range,
        layout.discontinuities,
        Some((layout.discontinuity_flag, discontinuity_flag)),
        context,
        "projection",
    )?;
    match (&layout.tail, tail) {
        (
            crate::nurbs::proc_curve::ProjectionTailPatchLayout::EarlyClose { flag: offset },
            cadmpeg_ir::geometry::ProjectionTail::EarlyClose { flag },
        ) => AsmEditSet::patch_native_bool(bytes, record.offset + offset, *flag)?,
        (
            crate::nurbs::proc_curve::ProjectionTailPatchLayout::Ranged {
                flag: flag_offset,
                parameter_range: range_offsets,
                role: role_range,
            },
            cadmpeg_ir::geometry::ProjectionTail::Ranged {
                flag,
                parameter_range,
                role,
            },
        ) => {
            if !parameter_range.iter().copied().all(f64::is_finite) {
                return Err(CodecError::Malformed(
                    "projection tail range must be finite".into(),
                ));
            }
            AsmEditSet::patch_native_bool(bytes, record.offset + flag_offset, *flag)?;
            AsmEditSet::patch_f64_payloads(
                bytes,
                record.offset,
                range_offsets
                    .iter()
                    .zip(parameter_range)
                    .map(|(offset, value)| (*offset, *value)),
            )?;
            role_range.write(&mut bytes[record.offset..], *role)?;
        }
        _ => {
            return Err(CodecError::NotImplemented(
                "projection edit cannot change native tail form".into(),
            ));
        }
    }

    Ok(())
}

fn patch_intersection_definition(
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    context: &IntcurveSupportContext,
    discontinuity_flag: bool,
) -> Result<(), CodecError> {
    let record_bytes = record_slice(bytes, record, "intersection")?;
    let layout = crate::nurbs::proc_curve::intersection_patch_layout(record_bytes, stream_width)
        .ok_or_else(|| CodecError::Malformed("intersection construction is malformed".into()))?;
    patch_intcurve_context(
        bytes,
        record,
        layout.parameter_range,
        layout.discontinuities,
        Some((layout.discontinuity_flag, discontinuity_flag)),
        context,
        "intersection",
    )?;

    Ok(())
}

fn patch_three_surface_intersection_definition(
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    context: &IntcurveSupportContext,
    selector: i64,
) -> Result<(), CodecError> {
    let record_bytes = record_slice(bytes, record, "three-surface intersection")?;
    let layout = crate::nurbs::proc_curve::three_surface_patch_layout(record_bytes, stream_width)
        .ok_or_else(|| {
        CodecError::Malformed("three-surface construction is malformed".into())
    })?;
    patch_intcurve_context(
        bytes,
        record,
        layout.parameter_range,
        layout.discontinuities,
        None,
        context,
        "three-surface intersection",
    )?;

    AsmEditSet::patch_tagged_integer_at(
        bytes,
        record.offset + layout.selector,
        stream_width,
        selector,
    )?;
    Ok(())
}

fn patch_surface_curve_definition(
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    family: &SurfaceCurveFamily,
) -> Result<(), CodecError> {
    let context = family.context();
    let record_bytes = record_slice(bytes, record, "surface-curve")?;
    let layout = crate::nurbs::proc_curve::surface_curve_patch_layout(
        record_bytes,
        stream_width,
        family.kind(),
    )
    .ok_or_else(|| CodecError::Malformed("surface-curve construction is malformed".into()))?;
    patch_intcurve_context(
        bytes,
        record,
        layout.parameter_range,
        layout.discontinuities,
        None,
        context,
        "surface-curve",
    )?;

    Ok(())
}

fn patch_silhouette_definition(
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    silhouette: &SilhouetteKind,
    light_direction: Vector3,
) -> Result<(), CodecError> {
    if !finite_vector(light_direction) {
        return Err(CodecError::Malformed(
            "silhouette light direction must be finite".into(),
        ));
    }
    let draft_factor = match silhouette {
        cadmpeg_ir::geometry::SilhouetteKind::Standard
        | cadmpeg_ir::geometry::SilhouetteKind::Parametric => None,
        cadmpeg_ir::geometry::SilhouetteKind::Taper { draft_factor } => {
            if !draft_factor.is_finite() {
                return Err(CodecError::Malformed(
                    "silhouette draft factor must be finite".into(),
                ));
            }
            Some(*draft_factor)
        }
    };
    let record_bytes = record_slice(bytes, record, "silhouette")?;
    let layout =
        crate::nurbs::proc_curve::silhouette_patch_layout(record_bytes, stream_width, silhouette)
            .ok_or_else(|| CodecError::Malformed("silhouette construction is malformed".into()))?;
    AsmEditSet::patch_vector_payload(
        bytes,
        record.offset + layout.light_direction,
        [light_direction.x, light_direction.y, light_direction.z],
    )?;
    if let Some(draft_factor) = draft_factor {
        let draft_offset = layout
            .draft_factor
            .ok_or_else(|| CodecError::Malformed("silhouette draft factor is missing".into()))?;
        let draft_offset = record.offset + draft_offset;
        AsmEditSet::patch_f64_payload(bytes, draft_offset, draft_factor)?;
    }
    Ok(())
}

fn patch_nurbs_surface_record(
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    edit: &NurbsSurfaceEdit<'_>,
    surface_ordinal: Option<usize>,
) -> Result<(), CodecError> {
    let surface = edit.surface;
    let record_bytes = record_slice(bytes, record, "NURBS surface")?;
    let layout = surface_ordinal
        .map_or_else(
            || crate::nurbs::core::final_surface_patch_layout(record_bytes, stream_width),
            |ordinal| {
                crate::nurbs::core::surface_patch_layout_at(record_bytes, ordinal, stream_width)
            },
        )
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "spline record {} has no writable surface cache",
                record.index
            ))
        })?;
    let u_count = usize::try_from(surface.u_count())
        .map_err(|_| CodecError::Malformed("NURBS u pole count exceeds address space".into()))?;
    let v_count = usize::try_from(surface.v_count())
        .map_err(|_| CodecError::Malformed("NURBS v pole count exceeds address space".into()))?;
    if usize::try_from(layout.surface.u_count()).ok() != Some(u_count)
        || usize::try_from(layout.surface.v_count()).ok() != Some(v_count)
        || layout.surface.weights().is_some() != surface.weights().is_some()
    {
        return Err(CodecError::NotImplemented(format!(
            "spline record {} changed NURBS cache structure",
            record.index
        )));
    }
    AsmEditSet::patch_knot_structure(
        bytes,
        record.offset,
        &layout.u_knots,
        surface.u_knots(),
        stream_width,
    )?;
    AsmEditSet::patch_knot_structure(
        bytes,
        record.offset,
        &layout.v_knots,
        surface.v_knots(),
        stream_width,
    )?;
    for (offset, degree) in layout
        .degree_value_offsets
        .into_iter()
        .zip([surface.u_degree(), surface.v_degree()])
    {
        let at = record.offset + offset;
        AsmEditSet::patch_layout_integer(bytes, at, stream_width, i64::from(degree))?;
    }
    if let Some(periodic) = edit.periodic {
        for (offset, periodic) in layout.periodic_value_offsets.into_iter().zip(periodic) {
            let at = record.offset + offset;
            let value = if periodic { 2i64 } else { 0i64 };
            AsmEditSet::patch_layout_integer(bytes, at, stream_width, value)?;
        }
    }
    let components = if layout.surface.weights().is_some() {
        4
    } else {
        3
    };
    let weights = surface.weights();
    let values = (0..v_count).flat_map(|v| {
        (0..u_count).flat_map(move |u| {
            let ir_index = u * v_count + v;
            let point = surface.control_points()[ir_index];
            [
                point.x / LEN_TO_MM,
                point.y / LEN_TO_MM,
                point.z / LEN_TO_MM,
                weights.map_or(0.0, |weights| weights[ir_index]),
            ]
            .into_iter()
            .take(components)
        })
    });
    for (offset, value) in layout.control_value_offsets().zip(values) {
        AsmEditSet::patch_f64_payload(bytes, record.offset + offset, value)?;
    }
    Ok(())
}

fn patch_nurbs_curve_record(
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    edit: &NurbsCurveEdit<'_>,
    target: CacheTarget,
) -> Result<(), CodecError> {
    let curve = edit.curve;
    let record_bytes = record_slice(bytes, record, "NURBS curve")?;
    let layout = match target {
        CacheTarget::Final => {
            crate::nurbs::core::final_curve_patch_layout(record_bytes, stream_width)
        }
        CacheTarget::First => {
            crate::nurbs::core::first_curve_patch_layout(record_bytes, stream_width)
        }
    }
    .ok_or_else(|| {
        CodecError::malformed(format_args!(
            "spline record {} has no writable curve cache",
            record.index
        ))
    })?;
    if layout.curve.control_points().len() != curve.control_points().len()
        || layout.curve.weights().is_some() != curve.weights().is_some()
    {
        return Err(CodecError::NotImplemented(format!(
            "spline record {} changed NURBS curve structure",
            record.index
        )));
    }
    AsmEditSet::patch_knot_structure(
        bytes,
        record.offset,
        &layout.knots,
        curve.knots(),
        stream_width,
    )?;
    let degree_at = record.offset + layout.degree_value_offset;
    AsmEditSet::patch_layout_integer(bytes, degree_at, stream_width, i64::from(curve.degree()))?;
    if let Some(periodic) = edit.periodic {
        let periodic = if periodic { 2i64 } else { 0i64 };
        let periodic_at = record.offset + layout.periodic_value_offset;
        AsmEditSet::patch_layout_integer(bytes, periodic_at, stream_width, periodic)?;
    }
    let components = if layout.curve.weights().is_some() {
        4
    } else {
        3
    };
    let weights = curve.weights();
    let values = curve
        .control_points()
        .iter()
        .enumerate()
        .flat_map(|(index, point)| {
            [
                point.x / LEN_TO_MM,
                point.y / LEN_TO_MM,
                point.z / LEN_TO_MM,
                weights.map_or(0.0, |weights| weights[index]),
            ]
            .into_iter()
            .take(components)
        });
    for (offset, value) in layout.control_value_offsets().zip(values) {
        AsmEditSet::patch_f64_payload(bytes, record.offset + offset, value)?;
    }
    Ok(())
}

enum PcurvePatchCarrier {
    Pcurve(std::ops::Range<usize>),
    Intcurve(std::ops::Range<usize>),
}

impl PcurvePatchCarrier {
    fn admit(
        bytes: &[u8],
        record: &Record,
        stream_width: RefWidth,
        edit: &InlinePcurveEdit<'_>,
    ) -> Result<Self, CodecError> {
        match record.head() {
            "pcurve" => {
                let scope =
                    sab::payload_subtype_range(bytes, record, 5, stream_width, "exp_par_cur")
                        .ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "pcurve record {} has no exp_par_cur payload",
                                record.index
                            ))
                        })?;
                Ok(Self::Pcurve(scope))
            }
            "intcurve" => match (
                edit.wrapper_reversed,
                edit.native_tail_flags,
                edit.parameter_range,
            ) {
                (None, None, None) => {
                    let end = record.offset.checked_add(record.len).ok_or_else(|| {
                        CodecError::Malformed(
                            "NURBS pcurve record extent overflows address space".into(),
                        )
                    })?;
                    Ok(Self::Intcurve(record.offset..end))
                }
                _ => Err(CodecError::NotImplemented(
                    "intcurve UV caches have no pcurve wrapper fields".into(),
                )),
            },
            _ => Err(CodecError::malformed(format_args!(
                "record {} is not a pcurve carrier",
                record.index
            ))),
        }
    }

    fn scope(&self) -> &std::ops::Range<usize> {
        match self {
            Self::Pcurve(scope) | Self::Intcurve(scope) => scope,
        }
    }
}

fn patch_nurbs_pcurve_record(
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    edit: &InlinePcurveEdit<'_>,
) -> Result<(), CodecError> {
    let geometry = edit.native_geometry;
    let PcurveGeometry::Nurbs { nurbs } = geometry else {
        return Err(CodecError::NotImplemented(format!(
            "pcurve record {} is not a writable NURBS cache",
            record.index
        )));
    };
    let carrier = PcurvePatchCarrier::admit(bytes, record, stream_width, edit)?;
    let scope = carrier.scope();
    let layout = crate::nurbs::pcurve::final_pcurve_patch_layout(
        bytes.get(scope.clone()).ok_or_else(|| {
            CodecError::Malformed("NURBS pcurve subtype extent is truncated".into())
        })?,
        stream_width,
    )
    .ok_or_else(|| {
        CodecError::malformed(format_args!(
            "pcurve record {} has no writable UV cache",
            record.index
        ))
    })?;
    if layout.control_count != nurbs.control_points().len()
        || layout.rational() != nurbs.weights().is_some()
    {
        return Err(CodecError::NotImplemented(format!(
            "pcurve record {} changed UV cache structure",
            record.index
        )));
    }
    AsmEditSet::patch_knot_structure(
        bytes,
        scope.start,
        &layout.knots,
        nurbs.knots(),
        stream_width,
    )?;
    let at = scope.start + layout.degree_value_offset;
    AsmEditSet::patch_layout_integer(bytes, at, stream_width, i64::from(nurbs.degree()))?;
    if let Some(periodic) = edit.periodic {
        let value = if periodic { 2i64 } else { 0i64 };
        let at = scope.start + layout.periodic_value_offset;
        AsmEditSet::patch_layout_integer(bytes, at, stream_width, value)?;
    }
    if let PcurvePatchCarrier::Pcurve(_) = &carrier {
        if let Some(reversed) = edit.wrapper_reversed {
            let offset =
                sab::payload_token_offset(bytes, record, stream_width, 4).ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "pcurve record {} lacks wrapper-reversal carrier",
                        record.index
                    ))
                })?;
            if !matches!(bytes.get(offset), Some(0x0a | 0x0b)) {
                return Err(CodecError::malformed(format_args!(
                    "pcurve record {} has a non-boolean wrapper-reversal carrier",
                    record.index
                )));
            }
            AsmEditSet::patch_boolean_at(bytes, offset, reversed)?;
        }
        if bytes.get(scope.end) != Some(&0x10) {
            return Err(CodecError::malformed(format_args!(
                "pcurve record {} lacks the exp_par_cur close",
                record.index
            )));
        }
        // Chunk space, because `payload_token_offset` indexes value tokens.
        let suffix_start = record.chunk_len().checked_sub(6).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "pcurve record {} lacks its native metadata suffix",
                record.index
            ))
        })?;
        let suffix_offsets = (suffix_start..record.chunk_len())
            .map(|index| {
                sab::payload_token_offset(bytes, record, stream_width, index).ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "pcurve record {} has an incomplete native metadata suffix",
                        record.index
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(flags) = edit.native_tail_flags {
            for (offset, flag) in suffix_offsets[..4].iter().zip(flags) {
                if !matches!(bytes.get(*offset), Some(0x0a | 0x0b)) {
                    return Err(CodecError::malformed(format_args!(
                        "pcurve record {} has an incomplete native boolean tail",
                        record.index
                    )));
                }
                AsmEditSet::patch_boolean_at(bytes, *offset, flag)?;
            }
        } else {
            for offset in &suffix_offsets[..4] {
                if !matches!(bytes.get(*offset), Some(0x0a | 0x0b)) {
                    return Err(CodecError::malformed(format_args!(
                        "pcurve record {} has an incomplete native boolean tail",
                        record.index
                    )));
                }
            }
        }
        if let Some(range) = edit.parameter_range {
            for (offset, value) in suffix_offsets[4..].iter().zip(range) {
                if bytes.get(*offset) != Some(&0x06) {
                    return Err(CodecError::malformed(format_args!(
                        "pcurve record {} has an incomplete parameter range",
                        record.index
                    )));
                }
                AsmEditSet::patch_f64_payload(bytes, *offset + 1, value)?;
            }
        }
    }
    if let Some(tolerance) = edit.fit_tolerance {
        if bytes.get(scope.start + layout.control_end()) != Some(&0x06) {
            return Err(CodecError::NotImplemented(format!(
                "pcurve record {} has no writable fit-tolerance carrier",
                record.index
            )));
        }
        let at = scope.start + layout.control_end() + 1;
        AsmEditSet::patch_f64_payload(bytes, at, tolerance)?;
    }
    for (point, offsets) in nurbs
        .control_points()
        .iter()
        .zip(layout.control_value_offsets())
    {
        for (value, offset) in [point.u, point.v].into_iter().zip(offsets) {
            let at = scope.start + offset;
            AsmEditSet::patch_f64_payload(bytes, at, value)?;
        }
    }
    if let Some(weights) = nurbs.weights() {
        for (weight, offset) in weights.iter().zip(layout.weight_value_offsets()) {
            let at = scope.start + offset;
            AsmEditSet::patch_f64_payload(bytes, at, *weight)?;
        }
    }
    Ok(())
}

fn patch_ref_pcurve_contract(
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    parameter_range: Option<[f64; 2]>,
) -> Result<(), CodecError> {
    let Some(range) = parameter_range else {
        return Ok(());
    };
    for (index, value) in [5usize, 6].into_iter().zip(range) {
        let offset =
            sab::payload_token_offset(bytes, record, stream_width, index).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "ref-form pcurve record {} lacks parameter-range field {index}",
                    record.index
                ))
            })?;
        if bytes.get(offset) != Some(&0x06) {
            return Err(CodecError::malformed(format_args!(
                "ref-form pcurve record {} parameter-range field {index} is not a double",
                record.index
            )));
        }
        AsmEditSet::patch_f64_payload(bytes, offset + 1, value)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::AsmEditSet;
    use crate::kernel_header::RefWidth;

    #[test]
    fn ascii_field_patch_rejects_a_truncated_payload() {
        let original = b"\x0d\x01x\x07\x05surf1\x11";
        let records = crate::sab::frame(original, 0, original.len(), RefWidth::Eight).unwrap();
        let edits = AsmEditSet::from_framed(records.clone(), RefWidth::Eight, 1.0);
        let mut bytes = original[..7].to_vec();
        let before = bytes.clone();
        let error = edits
            .patch_ascii_field(&mut bytes, &records[0], 0, "surf2")
            .unwrap_err();
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
        assert_eq!(bytes, before);
    }

    #[test]
    fn truecolor_field_patch_rejects_truncated_payloads() {
        for (tag, width, payload_width) in [
            (0x17, RefWidth::Eight, 8),
            (0x04, RefWidth::Four, 4),
            (0x04, RefWidth::Eight, 8),
        ] {
            let mut original = vec![0x0d, 1, b'x', tag];
            original.extend_from_slice(&[0; 8][..payload_width]);
            original.push(0x11);
            let records = crate::sab::frame(&original, 0, original.len(), width).unwrap();
            let edits = AsmEditSet::from_framed(records.clone(), width, 1.0);
            let mut bytes = original[..4].to_vec();
            let before = bytes.clone();
            let error = edits
                .patch_truecolor_field(&mut bytes, &records[0], 0, 1)
                .unwrap_err();
            assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
            assert_eq!(bytes, before);
        }
    }

    #[test]
    fn byte_payload_patch_rejects_truncation_and_overflow_without_writing() {
        for (offset, skip) in [(2, 0), (usize::MAX, 0), (usize::MAX, 1)] {
            let mut bytes = [0x11; 5];
            let before = bytes;
            let error = AsmEditSet::patch_bytes_at(&mut bytes, offset, skip, b"surf2").unwrap_err();
            assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
            assert_eq!(bytes, before);
        }
    }

    #[test]
    fn ascii_field_patch_preserves_the_token_length_and_surrounding_bytes() {
        let mut bytes = b"\x0d\x01x\x07\x05surf1\x11".to_vec();
        let records = crate::sab::frame(&bytes, 0, bytes.len(), RefWidth::Eight).unwrap();
        let edits = AsmEditSet::from_framed(records.clone(), RefWidth::Eight, 1.0);
        edits
            .patch_ascii_field(&mut bytes, &records[0], 0, "surf2")
            .unwrap();
        assert_eq!(bytes, b"\x0d\x01x\x07\x05surf2\x11");
        let before = bytes.clone();
        assert!(edits
            .patch_ascii_field(&mut bytes, &records[0], 0, "longer")
            .is_err());
        assert_eq!(bytes, before);
    }

    #[test]
    fn truecolor_field_patch_preserves_all_unsigned_bits_and_carrier_widths() {
        for (tag, width, payload_width) in [
            (0x04, RefWidth::Four, 4),
            (0x04, RefWidth::Eight, 8),
            (0x17, RefWidth::Four, 8),
            (0x17, RefWidth::Eight, 8),
        ] {
            let mut bytes = vec![0x0d, 1, b'x', tag];
            bytes.extend_from_slice(&[0; 8][..payload_width]);
            bytes.push(0x11);
            let records = crate::sab::frame(&bytes, 0, bytes.len(), width).unwrap();
            let edits = AsmEditSet::from_framed(records.clone(), width, 1.0);
            edits
                .patch_truecolor_field(&mut bytes, &records[0], 0, u32::MAX)
                .unwrap();
            assert_eq!(&bytes[..4], &[0x0d, 1, b'x', tag]);
            assert_eq!(
                &bytes[4..4 + payload_width],
                &u64::from(u32::MAX).to_le_bytes()[..payload_width]
            );
            assert_eq!(bytes.last(), Some(&0x11));
        }
    }

    #[test]
    fn native_bool_patch_rejects_a_truncated_record_without_writing() {
        let original = b"\x0d\x08intcurve\x0b\x11";
        let records = crate::sab::frame(original, 0, original.len(), RefWidth::Eight).unwrap();
        let offset =
            AsmEditSet::required_payload_field_at(original, &records[0], RefWidth::Eight, 0, 0x0b)
                .unwrap();
        let mut truncated = original[..offset].to_vec();
        let before = truncated.clone();
        let error = AsmEditSet::patch_native_bool(&mut truncated, offset, true).unwrap_err();
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
        assert!(error.to_string().contains("truncated"));
        assert_eq!(truncated, before);
    }

    #[test]
    fn procedural_writer_returns_malformed_for_a_truncated_framed_record() {
        let original = b"\x0d\x08intcurve\x11";
        let records = crate::sab::frame(original, 0, original.len(), RefWidth::Eight).unwrap();
        let mut truncated = original[..original.len() - 1].to_vec();
        let before = truncated.clone();
        let error = super::patch_subset_definition(
            &mut truncated,
            RefWidth::Eight,
            &records[0],
            [0.0, 1.0],
        )
        .unwrap_err();
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
        assert!(error.to_string().contains("truncated"));
        assert_eq!(truncated, before);
    }

    #[test]
    fn intcurve_uv_cache_rejects_pcurve_wrapper_edits() {
        use cadmpeg_ir::geometry::{PcurveGeometry, PcurveNurbs};
        use cadmpeg_ir::math::Point2;
        let mut original = vec![0x0d, 8];
        original.extend_from_slice(b"intcurve");
        original.extend_from_slice(crate::nurbs::reader::NUBS_MARKER);
        for (tag, value) in [(0x04, 1i64), (0x15, 0), (0x04, 2)] {
            original.push(tag);
            original.extend_from_slice(&value.to_le_bytes());
        }
        for knot in [0.0f64, 1.0] {
            original.push(0x06);
            original.extend_from_slice(&knot.to_le_bytes());
            original.push(0x04);
            original.extend_from_slice(&1i64.to_le_bytes());
        }
        for component in [0.0f64, 0.0, 1.0, 1.0] {
            original.push(0x06);
            original.extend_from_slice(&component.to_le_bytes());
        }
        original.push(0x11);
        let records = crate::sab::frame(&original, 0, original.len(), RefWidth::Eight).unwrap();
        let geometry = PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)],
                None,
                false,
            )
            .unwrap(),
        };
        let base = super::InlinePcurveEdit {
            native_geometry: &geometry,
            periodic: None,
            wrapper_reversed: None,
            native_tail_flags: None,
            parameter_range: None,
            fit_tolerance: None,
        };
        let edits = AsmEditSet::from_framed(records.clone(), RefWidth::Eight, 1.0);
        edits
            .patch_pcurve(
                &mut original.clone(),
                &records[0],
                super::PcurveEdit::Inline(base),
            )
            .unwrap();
        for edit in [
            super::InlinePcurveEdit {
                wrapper_reversed: Some(true),
                ..base
            },
            super::InlinePcurveEdit {
                native_tail_flags: Some([true; 4]),
                ..base
            },
            super::InlinePcurveEdit {
                parameter_range: Some([2.0, 3.0]),
                ..base
            },
        ] {
            let mut bytes = original.clone();
            assert!(matches!(
                edits.patch_pcurve(&mut bytes, &records[0], super::PcurveEdit::Inline(edit)),
                Err(cadmpeg_core::CodecError::NotImplemented(_))
            ));
            assert_eq!(bytes, original);
        }
    }

    #[test]
    fn transform_rejects_nonpositive_and_nonfinite_header_scales() {
        let mut original = vec![0x0d, 9];
        original.extend_from_slice(b"transform");
        for _ in 0..4 {
            original.push(0x14);
            original.extend_from_slice(&[0; 24]);
        }
        original.push(0x06);
        original.extend_from_slice(&1.0f64.to_le_bytes());
        original.push(0x11);
        let records = crate::sab::frame(&original, 0, original.len(), RefWidth::Eight).unwrap();
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut bytes = original.clone();
            let edits = AsmEditSet::from_framed(records.clone(), RefWidth::Eight, scale);
            assert!(matches!(
                edits.patch_transform(
                    &mut bytes,
                    &records[0],
                    cadmpeg_ir::transform::Transform::identity()
                ),
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
            assert_eq!(bytes, original);
        }
    }

    #[test]
    fn tagged_i64_replaces_only_the_selected_payload() {
        let mut bytes = vec![0; 27];
        bytes[9] = 0x0c;
        bytes[18] = 0x04;

        AsmEditSet::patch_tagged_i64(&mut bytes, 0, 1, 0x0c, -7).expect("tagged reference");
        AsmEditSet::patch_tagged_i64(&mut bytes, 0, 2, 0x04, 42).expect("tagged integer");

        assert_eq!(&bytes[10..18], &(-7i64).to_le_bytes());
        assert_eq!(&bytes[19..27], &42i64.to_le_bytes());
        assert_eq!(bytes[9], 0x0c);
        assert_eq!(bytes[18], 0x04);
    }

    #[test]
    fn vector_payload_is_three_consecutive_doubles() {
        let mut bytes = [0u8; 24];
        AsmEditSet::patch_vector_payload(&mut bytes, 0, [1.5, -2.0, 3.25]).expect("vector payload");

        assert_eq!(&bytes[0..8], &1.5f64.to_le_bytes());
        assert_eq!(&bytes[8..16], &(-2.0f64).to_le_bytes());
        assert_eq!(&bytes[16..24], &3.25f64.to_le_bytes());
    }

    #[test]
    fn layout_integer_rejects_binary_file4_overflow() {
        let mut bytes = [0u8; 4];
        let error = AsmEditSet::patch_layout_integer(
            &mut bytes,
            0,
            RefWidth::Four,
            i64::from(i32::MAX) + 1,
        )
        .expect_err("overflow must fail");

        assert!(error.to_string().contains("BinaryFile4 range"));
    }
}
