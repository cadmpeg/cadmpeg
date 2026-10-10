// SPDX-License-Identifier: Apache-2.0
//! Apply fixed-width edits to a framed ASM SAB stream.

use crate::kernel_header::RefWidth;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsSurface},
    pcurve::PcurveNurbs,
    ProceduralCurveDefinition,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};
use cadmpeg_ir::topology::{ParameterInterval, Sense};
use cadmpeg_ir::transform::Transform;

use crate::asm_header;
use crate::nurbs::reader::KnotLayout;
use crate::nurbs::reader::LEN_TO_MM;

/// Native lengths per canonical millimeter.
const LENGTH_PER_MILLIMETRE: FiniteReal = match FiniteReal::new(1.0 / LEN_TO_MM) {
    Some(scale) => scale,
    None => panic!("the native length unit must be a finite nonzero millimeter count"),
};
use crate::sab::{self, Record};
use cadmpeg_ir::geometry::curve_payloads::SpringCurvePayload;
use cadmpeg_ir::geometry::{
    CompoundComponent, IntcurveSupportContext, ProjectionTail, SurfaceCurveFamily,
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
    header_scale: Option<PositiveReal>,
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

/// Writable values for one solved NURBS parameter-curve cache, selected by the
/// carrier the caller resolved.
#[derive(Clone, Copy)]
pub enum InlinePcurveEdit<'a> {
    /// A `pcurve` wrapper, which carries the native wrapper metadata.
    PcurveWrapper {
        /// Parameter-curve geometry in the carrier's native chart.
        native_geometry: &'a PcurveNurbs,
        /// Optional native periodic flag.
        periodic: Option<bool>,
        /// Optional wrapper reversal flag.
        wrapper_reversed: Option<bool>,
        /// Optional four-flag native metadata tail.
        native_tail_flags: Option<[bool; 4]>,
        /// Optional native wrapper parameter range.
        parameter_range: Option<[f64; 2]>,
        /// Optional solved-cache fit tolerance.
        fit_tolerance: Option<f64>,
    },
    /// An `intcurve` UV cache, which has no wrapper fields.
    IntcurveCache {
        /// Parameter-curve geometry in the carrier's native chart.
        native_geometry: &'a PcurveNurbs,
        /// Optional native periodic flag.
        periodic: Option<bool>,
        /// Optional solved-cache fit tolerance.
        fit_tolerance: Option<f64>,
    },
}

impl<'a> InlinePcurveEdit<'a> {
    fn native_geometry(&self) -> &'a PcurveNurbs {
        match self {
            Self::PcurveWrapper {
                native_geometry, ..
            }
            | Self::IntcurveCache {
                native_geometry, ..
            } => native_geometry,
        }
    }

    fn periodic(&self) -> Option<bool> {
        match self {
            Self::PcurveWrapper { periodic, .. } | Self::IntcurveCache { periodic, .. } => {
                *periodic
            }
        }
    }

    fn fit_tolerance(&self) -> Option<f64> {
        match self {
            Self::PcurveWrapper { fit_tolerance, .. }
            | Self::IntcurveCache { fit_tolerance, .. } => *fit_tolerance,
        }
    }
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
        edit: impl FnOnce(&DecodeContext<'_>, &mut [u8], &Self) -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, root) = DecodeContext::read_root(
            &mut std::io::Cursor::new(&*bytes),
            &arena,
            &cadmpeg_core::decode::DecodePolicy::desktop(),
            false,
        )?;
        let edits = Self::frame(&ctx, root.window())?;
        edit(&ctx, bytes, &edits)
    }

    /// Frame the solved record partition without changing the input bytes.
    pub fn frame(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Self, CodecError> {
        let header = asm_header::parse(ctx, bytes)?
            .ok_or_else(|| CodecError::Malformed("active BREP has no SAB record stream".into()))?;
        let start = asm_header::record_stream_start_with_header(ctx, bytes, &header)?
            .ok_or_else(|| CodecError::Malformed("active BREP has no SAB record stream".into()))?;
        let limit = asm_header::solved_record_limit_with_header(ctx, bytes, &header)?
            .unwrap_or(bytes.len());
        let ref_width = header.width;
        let records = sab::frame(ctx, bytes, start, limit, ref_width, None).map_err(|failure| {
            failure.into_codec_error(ctx, |error| {
                CodecError::malformed(format_args!("cannot frame active BREP: {error}"))
            })
        })?;
        let header_scale = header.metadata.scale.unwrap_or(1.0);
        Ok(Self::from_framed(records, ref_width, header_scale))
    }

    /// Build a context from an already framed record partition.
    pub fn from_framed(records: Vec<Record>, ref_width: RefWidth, header_scale: f64) -> Self {
        Self {
            records,
            ref_width,
            header_scale: PositiveReal::new(header_scale),
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

    /// Locate a payload value token and require its exact tag.
    pub fn required_payload_field(
        &self,
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
        record: &Record,
        index: usize,
        tag: u8,
    ) -> Result<usize, CodecError> {
        Self::required_payload_field_at(ctx, bytes, record, self.ref_width, index, tag)
    }

    /// Locate a payload value token with an explicit stream integer width.
    pub fn required_payload_field_at(
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
        record: &Record,
        ref_width: RefWidth,
        index: usize,
        tag: u8,
    ) -> Result<usize, CodecError> {
        let (offset, _) =
            sab::payload_token(ctx, bytes, record, ref_width, index)?.ok_or_else(|| {
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

    /// Locate one double payload and retain its decoded value.
    pub fn required_payload_double(
        &self,
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
        record: &Record,
        index: usize,
    ) -> Result<(usize, f64), CodecError> {
        let (offset, token) = sab::payload_token(ctx, bytes, record, self.ref_width, index)?
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "{} record {} lacks payload field {index}",
                    record.head(),
                    record.index
                ))
            })?;
        let sab::Token::Double(value) = token else {
            return Err(CodecError::malformed(format_args!(
                "{} record {} payload field {index} is not tag 0x06",
                record.head(),
                record.index
            )));
        };
        Ok((offset, value))
    }

    /// Replace one tagged integer payload without changing its encoded width.
    pub fn patch_integer_field(
        &self,
        ctx: &DecodeContext<'_>,
        bytes: &mut [u8],
        record: &Record,
        index: usize,
        tag: u8,
        value: i64,
    ) -> Result<(), CodecError> {
        let offset = self.required_payload_field(ctx, bytes, record, index, tag)?;
        Self::patch_layout_integer(bytes, offset + 1, self.ref_width, value)
    }

    /// Replace one boolean sense token without changing its field position.
    pub fn patch_sense_field(
        &self,
        ctx: &DecodeContext<'_>,
        bytes: &mut [u8],
        record: &Record,
        index: usize,
        sense: Sense,
    ) -> Result<(), CodecError> {
        let (offset, _) = sab::payload_token(ctx, bytes, record, self.ref_width, index)?
            .ok_or_else(|| {
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
        ctx: &DecodeContext<'_>,
        bytes: &mut [u8],
        record: &Record,
        index: usize,
        value: bool,
    ) -> Result<(), CodecError> {
        let (offset, _) = sab::payload_token(ctx, bytes, record, self.ref_width, index)?
            .ok_or_else(|| {
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
        ctx: &DecodeContext<'_>,
        bytes: &mut [u8],
        record: &Record,
        index: usize,
        value: &str,
    ) -> Result<(), CodecError> {
        let offset = self.required_payload_field(ctx, bytes, record, index, 0x07)?;
        let encoded_length = usize::from(bytes.get(offset + 1).copied().ok_or_else(|| {
            CodecError::malformed(format_args!("{} record string is truncated", record.head()))
        })?);
        if value.len() != encoded_length
            || !ctx.is_ascii(value.as_bytes(), "ASM ASCII edit value")?
        {
            return Err(CodecError::NotImplemented(format!(
                "{} record {} string edit must retain its encoded ASCII length",
                record.head(),
                record.index
            )));
        }
        let start = offset
            .checked_add(2)
            .ok_or_else(|| CodecError::Malformed("native byte payload offset overflows".into()))?;
        let end = start
            .checked_add(value.len())
            .ok_or_else(|| CodecError::Malformed("native byte payload offset overflows".into()))?;
        let target = bytes
            .get_mut(start..end)
            .ok_or_else(|| CodecError::Malformed("native byte payload is truncated".into()))?;
        ctx.copy_into(target, value.as_bytes(), "ASM ASCII edit payload")
    }

    /// Replace one packed true-color integer without changing its carrier.
    pub fn patch_truecolor_field(
        &self,
        ctx: &DecodeContext<'_>,
        bytes: &mut [u8],
        record: &Record,
        field: usize,
        packed: u32,
    ) -> Result<(), CodecError> {
        let (offset, _) = sab::payload_token(ctx, bytes, record, self.ref_width, field)?
            .ok_or_else(|| {
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
        ctx: &DecodeContext<'_>,
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
        let (offset, _) = sab::payload_token(ctx, bytes, record, self.ref_width, field)?
            .ok_or_else(|| {
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
        let digit_count = packed.checked_ilog10().map_or(1, |digits| digits + 1);
        if u64::from(digit_count) > cadmpeg_core::decode::u64_from_index(width) {
            return Err(CodecError::NotImplemented(format!(
                "{} record {} decimal-color edit exceeds its encoded text width",
                record.head(),
                record.index
            )));
        }
        let (encoded, _storage) = ctx.format_scoped(
            format_args!("{packed:0width$}"),
            "ASM decimal color edit text",
        )?;
        let start = offset
            .checked_add(1 + length_width)
            .ok_or_else(|| CodecError::Malformed("native byte payload offset overflows".into()))?;
        let end = start
            .checked_add(encoded.len())
            .ok_or_else(|| CodecError::Malformed("native byte payload offset overflows".into()))?;
        let target = bytes
            .get_mut(start..end)
            .ok_or_else(|| CodecError::Malformed("native byte payload is truncated".into()))?;
        ctx.copy_into(target, encoded.as_bytes(), "ASM decimal color edit payload")
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
        if width == RefWidth::Four && i32::try_from(value).is_err() {
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
        ctx: &DecodeContext<'_>,
        bytes: &mut [u8],
        record_offset: usize,
        layout: &KnotLayout,
        knots: &[f64],
        int_width: RefWidth,
    ) -> Result<(), CodecError> {
        let mut run_storage = ctx.reserve_scoped(0, "ASM knot edit runs")?;
        let mut runs: Vec<(f64, usize)> = Vec::new();
        for knot in ctx.admit_iter(knots, "ASM knot edit values")? {
            if let Some((value, count)) = runs.last_mut() {
                if *value == *knot {
                    *count += 1;
                    continue;
                }
            }
            ctx.push_scoped_vec(
                &mut run_storage,
                &mut runs,
                (*knot, 1),
                "ASM knot edit runs",
            )?;
        }
        if runs.len() != layout.value_offsets.len() {
            return Err(CodecError::NotImplemented(
                "F3D NURBS curve edit changes the unique-knot count".into(),
            ));
        }
        for (ordinal, ((value, expanded_count), value_offset)) in ctx
            .admit_iter(runs, "ASM knot edit runs")?
            .zip(ctx.admit_iter(&layout.value_offsets, "ASM knot edit offsets")?)
            .enumerate()
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
        ctx: &DecodeContext<'_>,
        bytes: &mut [u8],
        record: &Record,
        definition: &ProceduralCurveDefinition,
    ) -> Result<(), CodecError> {
        match definition {
            ProceduralCurveDefinition::Helix(helix) => {
                patch_helix_definition(ctx, bytes, self.ref_width, record, helix)
            }
            ProceduralCurveDefinition::VectorOffset(definition_payload) => {
                let parameter_range = definition_payload.parameter_range();
                let offset = definition_payload.offset();
                patch_vector_offset_definition(
                    ctx,
                    bytes,
                    self.ref_width,
                    record,
                    parameter_range.endpoints(),
                    offset.get(),
                )
            }
            ProceduralCurveDefinition::Subset(definition_payload) => {
                let parameter_range = definition_payload.parameter_range();
                patch_subset_definition(
                    ctx,
                    bytes,
                    self.ref_width,
                    record,
                    parameter_range.endpoints(),
                )
            }
            ProceduralCurveDefinition::Compound(compound) => {
                let parameters = compound.parameters();
                let components = compound.components();
                patch_compound_definition(
                    ctx,
                    bytes,
                    self.ref_width,
                    record,
                    parameters,
                    components,
                )
            }
            ProceduralCurveDefinition::TwoSidedOffset(definition_payload) => {
                let context = definition_payload.context();
                let discontinuity_flag = definition_payload.discontinuity_flag();
                let offsets = definition_payload.offsets();
                patch_two_sided_offset_definition(
                    ctx,
                    bytes,
                    self.ref_width,
                    record,
                    context,
                    *discontinuity_flag,
                    offsets.get(),
                )
            }
            ProceduralCurveDefinition::SurfaceOffset(definition_payload) => {
                let context = definition_payload.context();
                let discontinuity_flag = definition_payload.discontinuity_flag();
                patch_surface_offset_definition(
                    ctx,
                    bytes,
                    self.ref_width,
                    record,
                    SurfaceOffsetFields {
                        context,
                        discontinuity_flag,
                        base_u_range: definition_payload.base_u_range(),
                        base_v_range: definition_payload.base_v_range(),
                        base_range: definition_payload.base_range(),
                        distance: definition_payload.distance(),
                        shift: definition_payload.shift(),
                        scale: definition_payload.scale(),
                    },
                )
            }
            ProceduralCurveDefinition::Spring(definition_payload) => {
                patch_spring_definition(ctx, bytes, self.ref_width, record, definition_payload)
            }
            ProceduralCurveDefinition::Projection(definition_payload) => {
                let context = definition_payload.context();
                let discontinuity_flag = definition_payload.discontinuity_flag();
                let tail = definition_payload.tail();
                patch_projection_definition(
                    ctx,
                    bytes,
                    self.ref_width,
                    record,
                    context,
                    *discontinuity_flag,
                    tail,
                )
            }
            ProceduralCurveDefinition::Intersection {
                context,
                discontinuity_flag,
                ..
            } => patch_intersection_definition(
                ctx,
                bytes,
                self.ref_width,
                record,
                context,
                *discontinuity_flag,
            ),
            ProceduralCurveDefinition::ThreeSurfaceIntersection(definition_payload) => {
                let context = definition_payload.context();
                let selector = definition_payload.selector();
                patch_three_surface_intersection_definition(
                    ctx,
                    bytes,
                    self.ref_width,
                    record,
                    context,
                    *selector,
                )
            }
            ProceduralCurveDefinition::SurfaceCurve { family, .. } => {
                patch_surface_curve_definition(ctx, bytes, self.ref_width, record, family)
            }
            ProceduralCurveDefinition::Silhouette(definition_payload) => {
                patch_silhouette_definition(ctx, bytes, self.ref_width, record, definition_payload)
            }
            ProceduralCurveDefinition::Exact { .. } => Err(CodecError::NotImplemented(
                "ASM procedural-curve definition is not writable".into(),
            )),
            ProceduralCurveDefinition::Law { .. } => Err(CodecError::NotImplemented(
                "ASM procedural-curve definition is not writable".into(),
            )),
            ProceduralCurveDefinition::TolerantIntersection { .. } => {
                Err(CodecError::NotImplemented(
                    "ASM procedural-curve definition is not writable".into(),
                ))
            }
            ProceduralCurveDefinition::Deformable(_) => Err(CodecError::NotImplemented(
                "ASM procedural-curve definition is not writable".into(),
            )),
            ProceduralCurveDefinition::Offset(_) => Err(CodecError::NotImplemented(
                "ASM procedural-curve definition is not writable".into(),
            )),
            ProceduralCurveDefinition::SpatialOffset(_) => Err(CodecError::NotImplemented(
                "ASM procedural-curve definition is not writable".into(),
            )),
            ProceduralCurveDefinition::Replica { .. } => Err(CodecError::NotImplemented(
                "ASM procedural-curve definition is not writable".into(),
            )),
            ProceduralCurveDefinition::BlendSpine { .. } => Err(CodecError::NotImplemented(
                "ASM procedural-curve definition is not writable".into(),
            )),
            ProceduralCurveDefinition::Unknown { .. } => Err(CodecError::NotImplemented(
                "ASM procedural-curve definition is not writable".into(),
            )),
        }
    }

    /// Apply an extrusion construction to its solved spline carrier.
    pub fn patch_extrusion_definition(
        &self,
        ctx: &DecodeContext<'_>,
        bytes: &mut [u8],
        record: &Record,
        parameter_interval: [f64; 2],
        direction: Vector3,
        native_position: Point3,
    ) -> Result<(), CodecError> {
        let record_bytes = record_slice(bytes, record, "extrusion")?;
        let layout =
            crate::nurbs::proc_curve::extrusion_patch_layout(ctx, record_bytes, self.ref_width)?
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
        ctx: &DecodeContext<'_>,
        bytes: &mut [u8],
        record: &Record,
        radii: [f64; 2],
    ) -> Result<(), CodecError> {
        let record_bytes = record_slice(bytes, record, "rolling-ball")?;
        let layout =
            crate::nurbs::proc_curve::rolling_ball_patch_layout(ctx, record_bytes, self.ref_width)?
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
        ctx: &DecodeContext<'_>,
        bytes: &mut [u8],
        record: &Record,
        tolerance: f64,
    ) -> Result<(), CodecError> {
        let record_bytes = record_slice(bytes, record, "procedural-surface")?;
        let layout =
            crate::nurbs::core::final_surface_patch_layout(ctx, record_bytes, self.ref_width)?
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
        ctx: &DecodeContext<'_>,
        bytes: &mut [u8],
        record: &Record,
        tolerance: f64,
    ) -> Result<(), CodecError> {
        let record_bytes = record_slice(bytes, record, "procedural-curve")?;
        let layout =
            crate::nurbs::core::final_curve_patch_layout(ctx, record_bytes, self.ref_width)?
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
        ctx: &DecodeContext<'_>,
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
        let [translation_x, translation_y, translation_z] = transform.translation().components();
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
                cadmpeg_ir::math::multiply_divide(
                    translation_x,
                    LENGTH_PER_MILLIMETRE,
                    header_scale.into(),
                )
                .ok_or_else(|| CodecError::malformed("native transform translation is non-finite"))?
                .get(),
                cadmpeg_ir::math::multiply_divide(
                    translation_y,
                    LENGTH_PER_MILLIMETRE,
                    header_scale.into(),
                )
                .ok_or_else(|| CodecError::malformed("native transform translation is non-finite"))?
                .get(),
                cadmpeg_ir::math::multiply_divide(
                    translation_z,
                    LENGTH_PER_MILLIMETRE,
                    header_scale.into(),
                )
                .ok_or_else(|| CodecError::malformed("native transform translation is non-finite"))?
                .get(),
            ],
        ];
        for (index, vector) in vectors.into_iter().enumerate() {
            let offset = self.required_payload_field(ctx, bytes, record, index, 0x14)?;
            Self::patch_vector_payload(bytes, offset + 1, vector)?;
        }
        let scale = self.required_payload_field(ctx, bytes, record, 4, 0x06)?;
        Self::patch_f64_payload(bytes, scale + 1, transform.rows()[3][3])
    }

    /// Apply one solved NURBS surface cache edit.
    pub fn patch_nurbs_surface(
        &self,
        ctx: &DecodeContext<'_>,
        bytes: &mut [u8],
        record: &Record,
        edit: NurbsSurfaceEdit<'_>,
        surface_ordinal: Option<usize>,
    ) -> Result<(), CodecError> {
        patch_nurbs_surface_record(ctx, bytes, self.ref_width, record, &edit, surface_ordinal)
    }

    /// Apply one solved NURBS curve cache edit.
    pub fn patch_nurbs_curve(
        &self,
        ctx: &DecodeContext<'_>,
        bytes: &mut [u8],
        record: &Record,
        edit: NurbsCurveEdit<'_>,
        target: CacheTarget,
    ) -> Result<(), CodecError> {
        patch_nurbs_curve_record(ctx, bytes, self.ref_width, record, &edit, target)
    }

    /// Apply the fields selected by an inline cache or reference wrapper edit.
    pub fn patch_pcurve(
        &self,
        ctx: &DecodeContext<'_>,
        bytes: &mut [u8],
        record: &Record,
        edit: PcurveEdit<'_>,
    ) -> Result<(), CodecError> {
        match edit {
            PcurveEdit::Inline(edit) => {
                patch_nurbs_pcurve_record(ctx, bytes, self.ref_width, record, &edit)
            }
            PcurveEdit::Ref { parameter_range } => {
                patch_ref_pcurve_contract(ctx, bytes, self.ref_width, record, parameter_range)
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

fn patch_helix_definition(
    ctx: &DecodeContext<'_>,
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::HelixCurveConstruction,
) -> Result<(), CodecError> {
    let angle_range = definition.angle_range();
    let center = definition.center().as_raw();
    let major = definition.major();
    let minor = definition.minor();
    let pitch = definition.pitch();
    let apex_factor = definition.apex_factor();
    let axis = definition.axis();

    let record_bytes = record_slice(bytes, record, "helix")?;
    let layout = crate::nurbs::proc_curve::helix_patch_layout(ctx, record_bytes, stream_width)?
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "procedural curve record {} lacks writable helix fields",
                record.index
            ))
        })?;
    AsmEditSet::patch_f64_payloads(
        bytes,
        record.offset,
        layout.angle_range.into_iter().zip(angle_range.get()),
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
    AsmEditSet::patch_f64_payload(bytes, apex_at, apex_factor.get())?;
    AsmEditSet::patch_vector_payload(bytes, record.offset + layout.axis, [axis.x, axis.y, axis.z])?;
    Ok(())
}

fn patch_vector_offset_definition(
    ctx: &DecodeContext<'_>,
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    parameter_range: [f64; 2],
    offset: Vector3,
) -> Result<(), CodecError> {
    let record_bytes = record_slice(bytes, record, "vector-offset")?;
    let layout =
        crate::nurbs::proc_curve::vector_offset_patch_layout(ctx, record_bytes, stream_width)?
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
    ctx: &DecodeContext<'_>,
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    parameter_range: [f64; 2],
) -> Result<(), CodecError> {
    let record_bytes = record_slice(bytes, record, "subset")?;
    let layout = crate::nurbs::proc_curve::subset_patch_layout(ctx, record_bytes, stream_width)?
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
    ctx: &DecodeContext<'_>,
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    parameters: &[FiniteReal],
    components: &[CompoundComponent<CurveId, FiniteReal>],
) -> Result<(), CodecError> {
    let record_bytes = record_slice(bytes, record, "compound")?;
    let layout = crate::nurbs::proc_curve::compound_patch_layout(ctx, record_bytes, stream_width)?
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
        ctx.admit_iter(layout.parameters, "ASM compound edit parameter offsets")?
            .chain(ctx.admit_iter(
                layout.component_parameters,
                "ASM compound edit component offsets",
            )?)
            .zip(
                ctx.admit_iter(parameters, "ASM compound edit parameters")?
                    .map(|value| value.get())
                    .chain(
                        ctx.admit_iter(components, "ASM compound edit components")?
                            .map(|item| item.parameter.get()),
                    ),
            ),
    )?;
    Ok(())
}

fn patch_two_sided_offset_definition(
    ctx: &DecodeContext<'_>,
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    context: &IntcurveSupportContext,
    discontinuity_flag: bool,
    offsets: [f64; 2],
) -> Result<(), CodecError> {
    let record_bytes = record_slice(bytes, record, "two-sided offset")?;
    let layout =
        crate::nurbs::proc_curve::two_sided_offset_patch_layout(ctx, record_bytes, stream_width)?
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
        .zip(context.parameter_range().endpoints())
    {
        AsmEditSet::patch_f64_payload(bytes, record.offset + at, value)?;
    }
    for (locations, values) in layout.discontinuities.iter().zip(context.discontinuities()) {
        for (at, value) in ctx
            .admit_iter(locations, "ASM offset edit locations")?
            .zip(ctx.admit_iter(values, "ASM offset edit values")?)
        {
            AsmEditSet::patch_f64_payload(bytes, record.offset + *at, value.get())?;
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
    base_u_range: &'a ParameterInterval,
    base_v_range: &'a ParameterInterval,
    base_range: &'a ParameterInterval,
    distance: FiniteReal,
    shift: FiniteReal,
    scale: FiniteReal,
}

struct IntcurveContextLayout {
    parameter_range: [usize; 2],
    discontinuities: [Vec<usize>; 3],
    flag: Option<(usize, bool)>,
}

fn patch_intcurve_context(
    ctx: &DecodeContext<'_>,
    bytes: &mut [u8],
    record: &Record,
    layout: IntcurveContextLayout,
    context: &IntcurveSupportContext,
    label: &str,
) -> Result<(), CodecError> {
    let IntcurveContextLayout {
        parameter_range,
        discontinuities,
        flag,
    } = layout;
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
            .zip(context.parameter_range().endpoints()),
    )?;
    for (locations, values) in discontinuities.iter().zip(context.discontinuities()) {
        for (offset, value) in ctx
            .admit_iter(locations, "ASM context edit locations")?
            .zip(ctx.admit_iter(values, "ASM context edit values")?)
        {
            let at = record.offset.checked_add(*offset).ok_or_else(|| {
                CodecError::Malformed("native double payload offset overflows".into())
            })?;
            AsmEditSet::patch_f64_payload(bytes, at, value.get())?;
        }
    }
    if let Some((offset, value)) = flag {
        AsmEditSet::patch_native_bool(bytes, record.offset + offset, value)?;
    }
    Ok(())
}

fn patch_surface_offset_definition(
    ctx: &DecodeContext<'_>,
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

    let base_u_range = base_u_range.endpoints();
    let base_v_range = base_v_range.endpoints();
    let base_range = base_range.endpoints();
    let record_bytes = record_slice(bytes, record, "surface-offset")?;
    let layout =
        crate::nurbs::proc_curve::surface_offset_patch_layout(ctx, record_bytes, stream_width)?
            .ok_or_else(|| {
                CodecError::Malformed("surface-offset construction is malformed".into())
            })?;
    patch_intcurve_context(
        ctx,
        bytes,
        record,
        IntcurveContextLayout {
            parameter_range: layout.parameter_range,
            discontinuities: layout.discontinuities,
            flag: Some((layout.discontinuity_flag, *discontinuity_flag)),
        },
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
                        distance.get() / LEN_TO_MM,
                        shift.get(),
                        scale.get(),
                    ])),
            ),
    )?;
    Ok(())
}

fn patch_spring_definition(
    ctx: &DecodeContext<'_>,
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    definition: &SpringCurvePayload,
) -> Result<(), CodecError> {
    let context = definition.support_context();
    let layout = definition.layout();
    let discontinuity_flag = match layout {
        cadmpeg_ir::geometry::SpringLayout::ContextFirst {
            discontinuity_flag, ..
        } => *discontinuity_flag,
        cadmpeg_ir::geometry::SpringLayout::CacheFirst { .. } => false,
    };
    let record_bytes = record_slice(bytes, record, "spring")?;
    let layout = crate::nurbs::proc_curve::spring_patch_layout(ctx, record_bytes, stream_width)?
        .ok_or_else(|| CodecError::Malformed("spring construction is malformed".into()))?;
    patch_intcurve_context(
        ctx,
        bytes,
        record,
        IntcurveContextLayout {
            parameter_range: layout.parameter_range,
            discontinuities: layout.discontinuities,
            flag: Some((layout.discontinuity_flag, discontinuity_flag)),
        },
        context,
        "spring",
    )?;

    AsmEditSet::patch_tagged_integer_at(
        bytes,
        record.offset + layout.direction,
        stream_width,
        *definition.direction(),
    )?;
    Ok(())
}

fn patch_projection_definition(
    ctx: &DecodeContext<'_>,
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    context: &IntcurveSupportContext,
    discontinuity_flag: bool,
    tail: &ProjectionTail<FiniteReal>,
) -> Result<(), CodecError> {
    let record_bytes = record_slice(bytes, record, "projection")?;
    let layout =
        crate::nurbs::proc_curve::projection_patch_layout(ctx, record_bytes, stream_width)?
            .ok_or_else(|| CodecError::Malformed("projection construction is malformed".into()))?;
    if layout
        .discontinuities
        .iter()
        .map(Vec::len)
        .ne(context.discontinuities().iter().map(Vec::len))
    {
        return Err(CodecError::Malformed(
            "projection context is incomplete".into(),
        ));
    }
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
            AsmEditSet::patch_native_bool(bytes, record.offset + flag_offset, *flag)?;
            AsmEditSet::patch_f64_payloads(
                bytes,
                record.offset,
                range_offsets
                    .iter()
                    .zip(parameter_range)
                    .map(|(offset, value)| (*offset, value.get())),
            )?;
            role_range.write(&mut bytes[record.offset..], *role)?;
        }
        _ => {
            return Err(CodecError::NotImplemented(
                "projection edit cannot change native tail form".into(),
            ));
        }
    }

    patch_intcurve_context(
        ctx,
        bytes,
        record,
        IntcurveContextLayout {
            parameter_range: layout.parameter_range,
            discontinuities: layout.discontinuities,
            flag: Some((layout.discontinuity_flag, discontinuity_flag)),
        },
        context,
        "projection",
    )?;
    Ok(())
}

fn patch_intersection_definition(
    ctx: &DecodeContext<'_>,
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    context: &IntcurveSupportContext,
    discontinuity_flag: bool,
) -> Result<(), CodecError> {
    let record_bytes = record_slice(bytes, record, "intersection")?;
    let layout =
        crate::nurbs::proc_curve::intersection_patch_layout(ctx, record_bytes, stream_width)?
            .ok_or_else(|| {
                CodecError::Malformed("intersection construction is malformed".into())
            })?;
    patch_intcurve_context(
        ctx,
        bytes,
        record,
        IntcurveContextLayout {
            parameter_range: layout.parameter_range,
            discontinuities: layout.discontinuities,
            flag: Some((layout.discontinuity_flag, discontinuity_flag)),
        },
        context,
        "intersection",
    )?;

    Ok(())
}

fn patch_three_surface_intersection_definition(
    ctx: &DecodeContext<'_>,
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    context: &IntcurveSupportContext,
    selector: i64,
) -> Result<(), CodecError> {
    let record_bytes = record_slice(bytes, record, "three-surface intersection")?;
    let layout =
        crate::nurbs::proc_curve::three_surface_patch_layout(ctx, record_bytes, stream_width)?
            .ok_or_else(|| {
                CodecError::Malformed("three-surface construction is malformed".into())
            })?;
    patch_intcurve_context(
        ctx,
        bytes,
        record,
        IntcurveContextLayout {
            parameter_range: layout.parameter_range,
            discontinuities: layout.discontinuities,
            flag: None,
        },
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
    ctx: &DecodeContext<'_>,
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    family: &SurfaceCurveFamily,
) -> Result<(), CodecError> {
    let context = family.context();
    let record_bytes = record_slice(bytes, record, "surface-curve")?;
    let layout = crate::nurbs::proc_curve::surface_curve_patch_layout(
        ctx,
        record_bytes,
        stream_width,
        family.kind(),
    )?
    .ok_or_else(|| CodecError::Malformed("surface-curve construction is malformed".into()))?;
    patch_intcurve_context(
        ctx,
        bytes,
        record,
        IntcurveContextLayout {
            parameter_range: layout.parameter_range,
            discontinuities: layout.discontinuities,
            flag: None,
        },
        context,
        "surface-curve",
    )?;

    Ok(())
}

fn patch_silhouette_definition(
    ctx: &DecodeContext<'_>,
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    construction: &cadmpeg_ir::geometry::curve_payloads::SilhouetteCurveConstruction,
) -> Result<(), CodecError> {
    let silhouette = construction.silhouette();
    let light_direction = *construction.light_direction();
    let draft_factor = match silhouette {
        cadmpeg_ir::geometry::SilhouetteKind::Standard {}
        | cadmpeg_ir::geometry::SilhouetteKind::Parametric {} => None,
        cadmpeg_ir::geometry::SilhouetteKind::Taper { draft_factor } => Some(draft_factor.get()),
    };
    let record_bytes = record_slice(bytes, record, "silhouette")?;
    let layout = crate::nurbs::proc_curve::silhouette_patch_layout(
        ctx,
        record_bytes,
        stream_width,
        silhouette,
    )?
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
    ctx: &DecodeContext<'_>,
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    edit: &NurbsSurfaceEdit<'_>,
    surface_ordinal: Option<usize>,
) -> Result<(), CodecError> {
    let surface = edit.surface;
    let record_bytes = record_slice(bytes, record, "NURBS surface")?;
    let layout = match surface_ordinal {
        None => crate::nurbs::core::final_surface_patch_layout(ctx, record_bytes, stream_width)?,
        Some(ordinal) => {
            crate::nurbs::core::surface_patch_layout_at(ctx, record_bytes, ordinal, stream_width)?
        }
    }
    .ok_or_else(|| {
        CodecError::malformed(format_args!(
            "spline record {} has no writable surface cache",
            record.index
        ))
    })?;
    let u_count = surface.u_count();
    let v_count = surface.v_count();
    if layout.surface.u_count() != u_count
        || layout.surface.v_count() != v_count
        || matches!(
            layout.surface.pole_grid(),
            cadmpeg_ir::geometry::nurbs::NurbsPoleGrid::Rational { .. }
        ) != matches!(
            surface.pole_grid(),
            cadmpeg_ir::geometry::nurbs::NurbsPoleGrid::Rational { .. }
        )
    {
        return Err(CodecError::NotImplemented(format!(
            "spline record {} changed NURBS cache structure",
            record.index
        )));
    }
    AsmEditSet::patch_knot_structure(
        ctx,
        bytes,
        record.offset,
        &layout.u_knots,
        surface.u_knots(),
        stream_width,
    )?;
    AsmEditSet::patch_knot_structure(
        ctx,
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
    let mut control_offsets = layout.control_value_offsets();
    match surface.pole_grid() {
        cadmpeg_ir::geometry::nurbs::NurbsPoleGrid::Polynomial { rows } => {
            for v in ctx.admit_iter(0..v_count, "ASM surface edit columns")? {
                let values = ctx
                    .admit_iter(rows, "ASM surface edit rows")?
                    .flat_map(move |row| {
                        let point = row[v];
                        [
                            point.x / LEN_TO_MM,
                            point.y / LEN_TO_MM,
                            point.z / LEN_TO_MM,
                        ]
                    });
                for (value, offset) in values.zip(control_offsets.by_ref()) {
                    AsmEditSet::patch_f64_payload(bytes, record.offset + offset, value)?;
                }
            }
        }
        cadmpeg_ir::geometry::nurbs::NurbsPoleGrid::Rational { rows } => {
            for v in ctx.admit_iter(0..v_count, "ASM surface edit columns")? {
                let values = ctx
                    .admit_iter(rows, "ASM surface edit rows")?
                    .flat_map(move |row| {
                        let pole = row[v];
                        [
                            pole.point.x / LEN_TO_MM,
                            pole.point.y / LEN_TO_MM,
                            pole.point.z / LEN_TO_MM,
                            pole.weight.get(),
                        ]
                    });
                for (value, offset) in values.zip(control_offsets.by_ref()) {
                    AsmEditSet::patch_f64_payload(bytes, record.offset + offset, value)?;
                }
            }
        }
    }
    Ok(())
}

fn patch_nurbs_curve_record(
    ctx: &DecodeContext<'_>,
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
            crate::nurbs::core::final_curve_patch_layout(ctx, record_bytes, stream_width)?
        }
        CacheTarget::First => {
            crate::nurbs::core::first_curve_patch_layout(ctx, record_bytes, stream_width)?
        }
    }
    .ok_or_else(|| {
        CodecError::malformed(format_args!(
            "spline record {} has no writable curve cache",
            record.index
        ))
    })?;
    if layout.curve.pole_count() != curve.pole_count()
        || matches!(
            layout.curve.pole_rows(),
            cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { .. }
        ) != matches!(
            curve.pole_rows(),
            cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { .. }
        )
    {
        return Err(CodecError::NotImplemented(format!(
            "spline record {} changed NURBS curve structure",
            record.index
        )));
    }
    AsmEditSet::patch_knot_structure(
        ctx,
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
    match curve.pole_rows() {
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } => {
            let values = ctx
                .admit_iter(points, "ASM curve edit control points")?
                .flat_map(|point| {
                    [
                        point.x / LEN_TO_MM,
                        point.y / LEN_TO_MM,
                        point.z / LEN_TO_MM,
                    ]
                });
            AsmEditSet::patch_f64_payloads(
                bytes,
                record.offset,
                layout.control_value_offsets().zip(values),
            )?;
        }
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => {
            let values = ctx
                .admit_iter(points, "ASM curve edit control points")?
                .flat_map(|pole| {
                    [
                        pole.point.x / LEN_TO_MM,
                        pole.point.y / LEN_TO_MM,
                        pole.point.z / LEN_TO_MM,
                        pole.weight.get(),
                    ]
                });
            AsmEditSet::patch_f64_payloads(
                bytes,
                record.offset,
                layout.control_value_offsets().zip(values),
            )?;
        }
    }

    Ok(())
}

enum PcurvePatchCarrier {
    Pcurve(std::ops::Range<usize>),
    Intcurve(std::ops::Range<usize>),
}

impl PcurvePatchCarrier {
    fn admit(
        ctx: &DecodeContext<'_>,
        bytes: &[u8],
        record: &Record,
        stream_width: RefWidth,
        edit: &InlinePcurveEdit<'_>,
    ) -> Result<Self, CodecError> {
        match (record.head(), edit) {
            ("pcurve", _) => {
                let scope =
                    sab::payload_subtype_range(ctx, bytes, record, 5, stream_width, "exp_par_cur")?
                        .ok_or_else(|| {
                            CodecError::malformed(format_args!(
                                "pcurve record {} has no exp_par_cur payload",
                                record.index
                            ))
                        })?;
                Ok(Self::Pcurve(scope))
            }
            ("intcurve", InlinePcurveEdit::IntcurveCache { .. }) => {
                let end = record.offset.checked_add(record.len).ok_or_else(|| {
                    CodecError::Malformed(
                        "NURBS pcurve record extent overflows address space".into(),
                    )
                })?;
                Ok(Self::Intcurve(record.offset..end))
            }
            _ => Err(CodecError::malformed(format_args!(
                "record {} is not a pcurve wrapper carrier",
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
    ctx: &DecodeContext<'_>,
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    edit: &InlinePcurveEdit<'_>,
) -> Result<(), CodecError> {
    let (wrapper_reversed, native_tail_flags, parameter_range) = match edit {
        InlinePcurveEdit::PcurveWrapper {
            wrapper_reversed,
            native_tail_flags,
            parameter_range,
            ..
        } => (*wrapper_reversed, *native_tail_flags, *parameter_range),
        InlinePcurveEdit::IntcurveCache { .. } => (None, None, None),
    };
    let nurbs = edit.native_geometry();
    let carrier = PcurvePatchCarrier::admit(ctx, bytes, record, stream_width, edit)?;
    let scope = carrier.scope();
    let layout = crate::nurbs::pcurve::final_pcurve_patch_layout(
        ctx,
        bytes.get(scope.clone()).ok_or_else(|| {
            CodecError::Malformed("NURBS pcurve subtype extent is truncated".into())
        })?,
        stream_width,
    )?
    .ok_or_else(|| {
        CodecError::malformed(format_args!(
            "pcurve record {} has no writable UV cache",
            record.index
        ))
    })?;
    if layout.control_count != nurbs.pole_rows().count()
        || layout.rational()
            != matches!(
                nurbs.pole_rows(),
                cadmpeg_ir::geometry::pcurve::PcurveNurbsPoles::Rational { .. }
            )
    {
        return Err(CodecError::NotImplemented(format!(
            "pcurve record {} changed UV cache structure",
            record.index
        )));
    }
    AsmEditSet::patch_knot_structure(
        ctx,
        bytes,
        scope.start,
        &layout.knots,
        nurbs.knots(),
        stream_width,
    )?;
    let at = scope.start + layout.degree_value_offset;
    AsmEditSet::patch_layout_integer(bytes, at, stream_width, i64::from(nurbs.degree()))?;
    if let Some(periodic) = edit.periodic() {
        let value = if periodic { 2i64 } else { 0i64 };
        let at = scope.start + layout.periodic_value_offset;
        AsmEditSet::patch_layout_integer(bytes, at, stream_width, value)?;
    }
    if let PcurvePatchCarrier::Pcurve(_) = &carrier {
        if let Some(reversed) = wrapper_reversed {
            let (offset, _) =
                sab::payload_token(ctx, bytes, record, stream_width, 4)?.ok_or_else(|| {
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
        // Chunk space, because `payload_token` indexes value tokens.
        let chunk_count = ctx
            .admit_iter(record.tokens.as_ref(), "ASM pcurve suffix field count")?
            .filter(|token| !token.is_payload_ident())
            .count();
        let suffix_start = chunk_count.checked_sub(6).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "pcurve record {} lacks its native metadata suffix",
                record.index
            ))
        })?;
        let mut suffix_offsets = [0; 6];
        for (ordinal, offset) in suffix_offsets.iter_mut().enumerate() {
            *offset = sab::payload_token(ctx, bytes, record, stream_width, suffix_start + ordinal)?
                .map(|(offset, _)| offset)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "pcurve record {} has an incomplete native metadata suffix",
                        record.index
                    ))
                })?;
        }
        if let Some(flags) = native_tail_flags {
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
        if let Some(range) = parameter_range {
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
    if let Some(tolerance) = edit.fit_tolerance() {
        if bytes.get(scope.start + layout.control_end()) != Some(&0x06) {
            return Err(CodecError::NotImplemented(format!(
                "pcurve record {} has no writable fit-tolerance carrier",
                record.index
            )));
        }
        let at = scope.start + layout.control_end() + 1;
        AsmEditSet::patch_f64_payload(bytes, at, tolerance)?;
    }
    match nurbs.pole_rows() {
        cadmpeg_ir::geometry::pcurve::PcurveNurbsPoles::Polynomial { points } => {
            for (point, offsets) in ctx
                .admit_iter(points, "ASM pcurve edit control points")?
                .zip(layout.control_value_offsets())
            {
                for (value, offset) in [point.u, point.v].into_iter().zip(offsets) {
                    AsmEditSet::patch_f64_payload(bytes, scope.start + offset, value)?;
                }
            }
        }
        cadmpeg_ir::geometry::pcurve::PcurveNurbsPoles::Rational { points } => {
            for (pole, offsets) in ctx
                .admit_iter(points, "ASM pcurve edit control points")?
                .zip(layout.control_value_offsets())
            {
                for (value, offset) in [pole.point.u, pole.point.v].into_iter().zip(offsets) {
                    AsmEditSet::patch_f64_payload(bytes, scope.start + offset, value)?;
                }
            }
            for (pole, offset) in ctx
                .admit_iter(points, "ASM pcurve edit weights")?
                .zip(layout.weight_value_offsets())
            {
                AsmEditSet::patch_f64_payload(bytes, scope.start + offset, pole.weight.get())?;
            }
        }
    }

    Ok(())
}

fn patch_ref_pcurve_contract(
    ctx: &DecodeContext<'_>,
    bytes: &mut [u8],
    stream_width: RefWidth,
    record: &sab::Record,
    parameter_range: Option<[f64; 2]>,
) -> Result<(), CodecError> {
    let Some(range) = parameter_range else {
        return Ok(());
    };
    for (index, value) in [5usize, 6].into_iter().zip(range) {
        let (offset, _) =
            sab::payload_token(ctx, bytes, record, stream_width, index)?.ok_or_else(|| {
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
    fn edit_framing_uses_default_subtype_depth_limit() {
        fn stream(depth: usize) -> Vec<u8> {
            let mut bytes = b"ASM BinaryFile4".to_vec();
            bytes.resize(crate::layout::asmheader_binaryfile4::LEN, 0);
            bytes.extend_from_slice(&[7, 0, 7, 0, 7, 0]);
            for _ in 0..3 {
                bytes.push(6);
                bytes.extend_from_slice(&1.0_f64.to_le_bytes());
            }
            bytes.extend_from_slice(&[0x0d, 1, b'x']);
            bytes.extend(std::iter::repeat_n(0x0f, depth));
            bytes.extend(std::iter::repeat_n(0x10, depth));
            bytes.push(0x11);
            bytes
        }

        assert_eq!(
            AsmEditSet::frame(&cadmpeg_test_support::service_decode_context(), &stream(1))
                .expect("one subtype fits the default policy")
                .records()
                .len(),
            1
        );
        assert!(matches!(
            AsmEditSet::frame(&cadmpeg_test_support::service_decode_context(), &stream(257)),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::RecursionDepth
        ));
    }

    #[test]
    fn double_field_retains_current_payload_bits_and_checks_its_actual_tag() {
        for width in [RefWidth::Four, RefWidth::Eight] {
            let mut bytes = vec![0x0d, 1, b'x', 0x06];
            bytes.extend_from_slice(&1.0_f64.to_le_bytes());
            bytes.push(0x11);
            let records = crate::test_support::sab::frame(&bytes, 0, bytes.len(), width).unwrap();
            let edits = AsmEditSet::from_framed(records.clone(), width, 1.0);
            for expected in [
                0.0_f64,
                -0.0,
                -2.5,
                f64::INFINITY,
                f64::from_bits(0x7ff8_0000_0000_0042),
            ] {
                bytes[4..12].copy_from_slice(&expected.to_le_bytes());
                let (offset, actual) = edits
                    .required_payload_double(
                        &cadmpeg_test_support::service_decode_context(),
                        &bytes,
                        &records[0],
                        0,
                    )
                    .unwrap();
                assert_eq!(offset, 3);
                assert_eq!(actual.to_bits(), expected.to_bits());
            }
            assert!(edits
                .required_payload_double(
                    &cadmpeg_test_support::service_decode_context(),
                    &bytes,
                    &records[0],
                    1
                )
                .unwrap_err()
                .to_string()
                .contains("lacks payload field 1"));
            bytes[3] = 0x17;
            assert!(edits
                .required_payload_double(
                    &cadmpeg_test_support::service_decode_context(),
                    &bytes,
                    &records[0],
                    0
                )
                .unwrap_err()
                .to_string()
                .contains("is not tag 0x06"));
            bytes[3] = 0x06;
            assert!(edits
                .required_payload_double(
                    &cadmpeg_test_support::service_decode_context(),
                    &bytes[..11],
                    &records[0],
                    0
                )
                .is_err());
        }
    }

    fn projection_fixture(width: RefWidth, early_close: bool) -> (Vec<u8>, crate::sab::Record) {
        fn integer(bytes: &mut Vec<u8>, tag: u8, value: i64, width: RefWidth) {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..width.bytes()]);
        }
        fn double(bytes: &mut Vec<u8>, value: f64) {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        fn curve(bytes: &mut Vec<u8>, width: RefWidth, dimensions: usize) {
            bytes.extend_from_slice(crate::nurbs::reader::NUBS_MARKER);
            for (tag, value) in [(0x04, 1), (0x15, 0), (0x04, 2)] {
                integer(bytes, tag, value, width);
            }
            for knot in [0.0, 1.0] {
                double(bytes, knot);
                integer(bytes, 0x04, 1, width);
            }
            for pole in [0.0, 1.0] {
                for _ in 0..dimensions {
                    double(bytes, pole);
                }
            }
        }
        let mut bytes = b"\x0f\x0d\x0cproj_int_cur".to_vec();
        for _ in 0..2 {
            bytes.extend_from_slice(b"\x0d\x05plane");
            for (tag, values) in [
                (0x13, [0.0_f64; 3]),
                (0x14, [0.0, 0.0, 1.0]),
                (0x14, [1.0, 0.0, 0.0]),
            ] {
                bytes.push(tag);
                for value in values {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
            bytes.push(0x0b);
        }
        curve(&mut bytes, width, 2);
        curve(&mut bytes, width, 2);
        double(&mut bytes, -2.0);
        double(&mut bytes, 3.0);
        for values in [vec![0.25], vec![], vec![0.5, 0.75]] {
            integer(
                &mut bytes,
                0x04,
                i64::try_from(values.len()).expect("test value fits"),
                width,
            );
            for value in values {
                double(&mut bytes, value);
            }
        }
        bytes.push(0x0a);
        curve(&mut bytes, width, 3);
        bytes.push(0x0b);
        if !early_close {
            double(&mut bytes, -1.0);
            double(&mut bytes, 1.0);
            bytes.extend_from_slice(b"\x07\x05surf1");
        }
        bytes.push(0x10);
        let record = crate::test_support::sab::record(
            0,
            "intcurve".into(),
            Vec::new().into(),
            0,
            bytes.len(),
        );
        (bytes, record)
    }

    #[test]
    fn projection_tail_form_rejection_preserves_all_bytes() {
        use cadmpeg_ir::geometry::{
            IntcurveSupportContext, IntcurveSupportSide, ProjectionRole, ProjectionTail,
        };

        for width in [RefWidth::Four, RefWidth::Eight] {
            for early_close in [false, true] {
                let (mut bytes, record) = projection_fixture(width, early_close);
                let context = IntcurveSupportContext::try_new(
                    std::array::from_fn(|_| IntcurveSupportSide {
                        surface: None,
                        pcurve: None,
                    }),
                    [4.0, 5.0],
                    [vec![0.1], vec![], vec![0.2, 0.3]],
                )
                .unwrap();
                let tail = if early_close {
                    ProjectionTail::Ranged {
                        flag: true,
                        parameter_range: [0.0, 2.0]
                            .map(|value| cadmpeg_ir::scalar::FiniteReal::new(value).unwrap()),
                        role: ProjectionRole::Surf2,
                    }
                } else {
                    ProjectionTail::EarlyClose { flag: true }
                };
                let before = bytes.clone();
                let error = super::patch_projection_definition(
                    &cadmpeg_test_support::service_decode_context(),
                    &mut bytes,
                    width,
                    &record,
                    &context,
                    false,
                    &tail,
                )
                .unwrap_err();
                assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
                assert_eq!(bytes, before);
            }
        }
    }

    #[test]
    fn incomplete_projection_context_preserves_ranged_tail_bytes() {
        use cadmpeg_ir::geometry::{
            IntcurveSupportContext, IntcurveSupportSide, ProjectionRole, ProjectionTail,
        };
        for width in [RefWidth::Four, RefWidth::Eight] {
            let (mut bytes, record) = projection_fixture(width, false);
            let context = IntcurveSupportContext::try_new(
                std::array::from_fn(|_| IntcurveSupportSide {
                    surface: None,
                    pcurve: None,
                }),
                [4.0, 5.0],
                std::array::from_fn(|_| Vec::new()),
            )
            .unwrap();
            let tail = ProjectionTail::Ranged {
                flag: true,
                parameter_range: [7.0, 8.0]
                    .map(|value| cadmpeg_ir::scalar::FiniteReal::new(value).unwrap()),
                role: ProjectionRole::Surf2,
            };
            let before = bytes.clone();
            let error = super::patch_projection_definition(
                &cadmpeg_test_support::service_decode_context(),
                &mut bytes,
                width,
                &record,
                &context,
                false,
                &tail,
            )
            .unwrap_err();
            assert!(
                matches!(error, cadmpeg_core::CodecError::Malformed(ref message) if message == "projection context is incomplete")
            );
            assert_eq!(bytes, before);
        }
    }

    #[test]
    fn ascii_field_patch_rejects_a_truncated_payload() {
        let original = b"\x0d\x01x\x07\x05surf1\x11";
        let records =
            crate::test_support::sab::frame(original, 0, original.len(), RefWidth::Eight).unwrap();
        let edits = AsmEditSet::from_framed(records.clone(), RefWidth::Eight, 1.0);
        let mut bytes = original[..7].to_vec();
        let before = bytes.clone();
        let error = edits
            .patch_ascii_field(
                &cadmpeg_test_support::service_decode_context(),
                &mut bytes,
                &records[0],
                0,
                "surf2",
            )
            .unwrap_err();
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
        assert_eq!(bytes, before);
    }

    #[test]
    fn ascii_edit_payload_refuses_before_writing() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        let original = b"\x0d\x01x\x07\x05surf1\x11";
        let records =
            crate::test_support::sab::frame(original, 0, original.len(), RefWidth::Eight).unwrap();
        let edits = AsmEditSet::from_framed(records.clone(), RefWidth::Eight, 1.0);
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "ASM ASCII edit payload",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                let mut bytes = original.to_vec();
                let result = edits.patch_ascii_field(&ctx, &mut bytes, &records[0], 0, "surf2");
                assert_eq!(bytes, original);
                result
            },
        );
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("resource refusal")
        };
        assert_eq!(limit.operation, "ASM ASCII edit payload");
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
            let records =
                crate::test_support::sab::frame(&original, 0, original.len(), width).unwrap();
            let edits = AsmEditSet::from_framed(records.clone(), width, 1.0);
            let mut bytes = original[..4].to_vec();
            let before = bytes.clone();
            let error = edits
                .patch_truecolor_field(
                    &cadmpeg_test_support::service_decode_context(),
                    &mut bytes,
                    &records[0],
                    0,
                    1,
                )
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
        let records =
            crate::test_support::sab::frame(&bytes, 0, bytes.len(), RefWidth::Eight).unwrap();
        let edits = AsmEditSet::from_framed(records.clone(), RefWidth::Eight, 1.0);
        edits
            .patch_ascii_field(
                &cadmpeg_test_support::service_decode_context(),
                &mut bytes,
                &records[0],
                0,
                "surf2",
            )
            .unwrap();
        assert_eq!(bytes, b"\x0d\x01x\x07\x05surf2\x11");
        let before = bytes.clone();
        assert!(edits
            .patch_ascii_field(
                &cadmpeg_test_support::service_decode_context(),
                &mut bytes,
                &records[0],
                0,
                "longer"
            )
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
            let records = crate::test_support::sab::frame(&bytes, 0, bytes.len(), width).unwrap();
            let edits = AsmEditSet::from_framed(records.clone(), width, 1.0);
            edits
                .patch_truecolor_field(
                    &cadmpeg_test_support::service_decode_context(),
                    &mut bytes,
                    &records[0],
                    0,
                    u32::MAX,
                )
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
    fn decimal_color_edit_preserves_width_at_unsigned_digit_boundaries() {
        for (packed, width, expected) in [
            (0, 10, Some("0000000000")),
            (9, 1, Some("9")),
            (10, 1, None),
            (10, 2, Some("10")),
            (u32::MAX, 10, Some("4294967295")),
            (u32::MAX, 9, None),
        ] {
            let mut bytes = vec![0x0d, 1, b'x', 0x07, u8::try_from(width).unwrap()];
            bytes.extend(std::iter::repeat_n(b'0', width));
            bytes.push(0x11);
            let records =
                crate::test_support::sab::frame(&bytes, 0, bytes.len(), RefWidth::Eight).unwrap();
            let edits = AsmEditSet::from_framed(records.clone(), RefWidth::Eight, 1.0);
            let before = bytes.clone();
            let result = edits.patch_decimal_rgb_field(
                &cadmpeg_test_support::service_decode_context(),
                &mut bytes,
                &records[0],
                0,
                packed,
            );
            if let Some(expected) = expected {
                result.unwrap();
                assert_eq!(&bytes[5..5 + width], expected.as_bytes());
                assert_eq!(&bytes[..5], &before[..5]);
                assert_eq!(bytes.last(), Some(&0x11));
            } else {
                assert!(matches!(
                    result,
                    Err(cadmpeg_core::CodecError::NotImplemented(_))
                ));
                assert_eq!(bytes, before);
            }
        }
    }

    #[test]
    fn surface_edit_writes_every_column_in_native_order() {
        use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
        use cadmpeg_ir::math::Point3;
        fn integer(bytes: &mut Vec<u8>, tag: u8, value: i64, width: RefWidth) {
            bytes.push(tag);
            bytes.extend_from_slice(&value.to_le_bytes()[..width.bytes()]);
        }
        fn double(bytes: &mut Vec<u8>, value: f64) {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for width in [RefWidth::Four, RefWidth::Eight] {
            for rational in [false, true] {
                let mut bytes = b"\x0d\x06spline".to_vec();
                bytes.extend_from_slice(if rational {
                    b"\x0d\x05nurbs"
                } else {
                    b"\x0d\x04nubs"
                });
                for _ in 0..2 {
                    integer(&mut bytes, 0x04, 1, width);
                }
                for _ in 0..4 {
                    integer(&mut bytes, 0x15, 0, width);
                }
                for _ in 0..2 {
                    integer(&mut bytes, 0x04, 2, width);
                }
                for _ in 0..2 {
                    for knot in [0.0, 1.0] {
                        double(&mut bytes, knot);
                        integer(&mut bytes, 0x04, 1, width);
                    }
                }
                for point in [
                    [0.0, 0.0, 0.0],
                    [1.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    [1.0, 1.0, 0.0],
                ] {
                    for value in point {
                        double(&mut bytes, value);
                    }
                    if rational {
                        double(&mut bytes, 1.0);
                    }
                }
                bytes.push(0x11);
                let records =
                    crate::test_support::sab::frame(&bytes, 0, bytes.len(), width).unwrap();
                let edits = AsmEditSet::from_framed(records.clone(), width, 1.0);
                let ctx = cadmpeg_test_support::service_decode_context();
                let rows = [
                    [[5.0, 11.0, 17.0], [7.0, 13.0, 19.0]],
                    [[23.0, 29.0, 31.0], [37.0, 41.0, 43.0]],
                ]
                .map(|row| {
                    row.map(|[x, y, z]| {
                        Point3::new(
                            x * crate::nurbs::reader::LEN_TO_MM,
                            y * crate::nurbs::reader::LEN_TO_MM,
                            z * crate::nurbs::reader::LEN_TO_MM,
                        )
                    })
                    .to_vec()
                })
                .to_vec();
                let weights = rational.then(|| vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
                let poles = NurbsPoleGrid::from_lanes(&ctx, rows, weights)
                    .unwrap()
                    .unwrap();
                let surface = NurbsSurface::new(
                    &ctx,
                    NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
                    NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
                    poles,
                    false,
                )
                .unwrap()
                .unwrap();
                edits
                    .patch_nurbs_surface(
                        &ctx,
                        &mut bytes,
                        &records[0],
                        super::NurbsSurfaceEdit {
                            surface: &surface,
                            periodic: None,
                        },
                        None,
                    )
                    .unwrap();
                let decoded = crate::nurbs::core::final_surface_patch_layout(&ctx, &bytes, width)
                    .unwrap()
                    .unwrap();
                assert_eq!(decoded.surface.pole_grid(), surface.pole_grid());
            }
        }
    }

    #[test]
    fn native_bool_patch_rejects_a_truncated_record_without_writing() {
        let original = b"\x0d\x08intcurve\x0b\x11";
        let records =
            crate::test_support::sab::frame(original, 0, original.len(), RefWidth::Eight).unwrap();
        let offset = AsmEditSet::required_payload_field_at(
            &cadmpeg_test_support::service_decode_context(),
            original,
            &records[0],
            RefWidth::Eight,
            0,
            0x0b,
        )
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
        let records =
            crate::test_support::sab::frame(original, 0, original.len(), RefWidth::Eight).unwrap();
        let mut truncated = original[..original.len() - 1].to_vec();
        let before = truncated.clone();
        let error = super::patch_subset_definition(
            &cadmpeg_test_support::service_decode_context(),
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
    fn intcurve_uv_cache_admits_only_the_intcurve_cache_edit() {
        use cadmpeg_ir::geometry::pcurve::PcurveNurbs;
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
        let records =
            crate::test_support::sab::frame(&original, 0, original.len(), RefWidth::Eight).unwrap();
        let geometry = PcurveNurbs::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)],
            None,
            false,
        )
        .expect("fixture pcurve construction admission")
        .unwrap();
        let base = super::InlinePcurveEdit::IntcurveCache {
            native_geometry: &geometry,
            periodic: None,
            fit_tolerance: None,
        };
        let edits = AsmEditSet::from_framed(records.clone(), RefWidth::Eight, 1.0);
        edits
            .patch_pcurve(
                &cadmpeg_test_support::service_decode_context(),
                &mut original.clone(),
                &records[0],
                super::PcurveEdit::Inline(base),
            )
            .unwrap();
        for (wrapper_reversed, native_tail_flags, parameter_range) in [
            (Some(true), None, None),
            (None, Some([true; 4]), None),
            (None, None, Some([2.0, 3.0])),
        ] {
            let edit = super::InlinePcurveEdit::PcurveWrapper {
                native_geometry: &geometry,
                periodic: None,
                wrapper_reversed,
                native_tail_flags,
                parameter_range,
                fit_tolerance: None,
            };
            let mut bytes = original.clone();
            assert!(matches!(
                edits.patch_pcurve(
                    &cadmpeg_test_support::service_decode_context(),
                    &mut bytes,
                    &records[0],
                    super::PcurveEdit::Inline(edit)
                ),
                Err(cadmpeg_core::CodecError::Malformed(_))
            ));
            assert_eq!(bytes, original);
        }
    }

    #[test]
    fn transform_patch_preserves_translation_at_large_header_scale() {
        let mut bytes = vec![0x0d, 9];
        bytes.extend_from_slice(b"transform");
        for _ in 0..4 {
            bytes.push(0x14);
            bytes.extend_from_slice(&[0; 24]);
        }
        bytes.push(0x06);
        bytes.extend_from_slice(&1.0f64.to_le_bytes());
        bytes.push(0x11);
        let records =
            crate::test_support::sab::frame(&bytes, 0, bytes.len(), RefWidth::Eight).unwrap();
        let edits = AsmEditSet::from_framed(records.clone(), RefWidth::Eight, 1e308);
        let transform = cadmpeg_ir::transform::Transform::affine([
            [1.0, 0.0, 0.0, 1e308],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap();
        edits
            .patch_transform(
                &cadmpeg_test_support::service_decode_context(),
                &mut bytes,
                &records[0],
                transform,
            )
            .unwrap();
        let offset = edits
            .required_payload_field(
                &cadmpeg_test_support::service_decode_context(),
                &bytes,
                &records[0],
                3,
                0x14,
            )
            .unwrap();
        let native = cadmpeg_core::decode::View::f64_le_at(&bytes, offset + 1).unwrap();
        assert!((native - 0.1).abs() <= f64::EPSILON);
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
        let records =
            crate::test_support::sab::frame(&original, 0, original.len(), RefWidth::Eight).unwrap();
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut bytes = original.clone();
            let edits = AsmEditSet::from_framed(records.clone(), RefWidth::Eight, scale);
            assert!(matches!(
                edits.patch_transform(
                    &cadmpeg_test_support::service_decode_context(),
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
    #[test]
    fn knot_edit_traversal_refuses_before_writing() {
        use super::{CodecError, DecodeContext, KnotLayout};
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            "ASM knot edit values",
            |cap| {
                let arena = cadmpeg_core::decode::DecodeArena::new();
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                let mut bytes = [0; 40];
                let result = AsmEditSet::patch_knot_structure(
                    &ctx,
                    &mut bytes,
                    0,
                    &KnotLayout {
                        value_offsets: vec![0, 20],
                    },
                    &[0.0, 0.0, 1.0, 1.0],
                    RefWidth::Four,
                );
                assert_eq!(bytes, [0; 40]);
                result
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("resource refusal")
        };
        assert_eq!(limit.operation, "ASM knot edit values");
    }
}
