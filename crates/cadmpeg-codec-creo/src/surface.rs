// SPDX-License-Identifier: Apache-2.0
//! Surface namespace rows and prototype parameters.
//!
//! A [`SurfaceRow`] identifies a surface family and its feature, orientation,
//! boundary, and namespace links. A named prototype locates its adjacent first
//! positional instance.

pub(crate) mod arrays;
pub(crate) mod cylinder_frame_readers;
pub(crate) mod unique_rows;

use cadmpeg_core::decode::{bounded_len, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{NonNegativeLength, PositiveAngle, PositiveLength};
use cadmpeg_ir::units::{OrthonormalFrame3, UnitVector3};

use crate::layout::type24_first_coordinate_bounded_round as type24_round;
use crate::layout::type24_segmented_first_coordinate_bounded_round as type24_seg;
use crate::psb::{self, compact_int};
use crate::scalar;
use crate::vecmath::local_system_lanes;
use std::collections::BTreeMap;

const EPS_CYLINDER_GEOMETRY_RELATIVE: f64 = 1.0e-9;
const EPS_CYLINDER_GEOMETRY_MIN: f64 = 1.0e-12;
const EPS_PLANE_FRAME_NONZERO: f64 = 1.0e-6;
const EPS_PLANE_FRAME_ZERO: f64 = 1.0e-9;
const EPS_PLANE_FRAME_SCALE: f64 = 1.0e-9;
const EPS_PLANE_FRAME_ORTHOGONAL: f64 = 1.0e-9;
const EPS_SURFACE_AGREEMENT: f64 = 1.0e-9;
const EPS_SURFACE_NONZERO: f64 = 1.0e-12;
const EPS_FRAME_AGREEMENT: f64 = 1.0e-10;
const EPS_TORUS_FRAME_COLUMN_DOT: f64 = 1.0e-10;
const EPS_AXIS_COMPONENT_NONZERO: f64 = 1.0e-9;
const EPS_AXIS_ALIGNMENT: f64 = 1.0e-9;
const EPS_SUPPORT_ORTHOGONALITY: f64 = 1.0e-9;

/// Surface family encoded by an `srf_array` row's `geom_type` byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceKind {
    /// `geom_type = 0x22`.
    Plane,
    /// `geom_type = 0x24`.
    Cylinder,
    /// `geom_type = 0x25`.
    Cone,
    /// `geom_type = 0x26`: a torus when the prototype's `radius1` is
    /// nonzero, a sphere when `radius1 = 0` ([spec §3.3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#33-torus-and-sphere-representation)).
    TorusOrSphere,
    /// `geom_type = 0x28`.
    Spline,
    /// `geom_type = 0x29`: fillet surface family.
    Fillet,
    /// Linear-extrusion family, with its encoding variant.
    Extrusion(ExtrusionVariant),
}

impl cadmpeg_core::decode::cost::DecodeCost for SurfaceKind {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        match self {
            Self::Extrusion(variant) => cadmpeg_core::decode::cost::DecodeCost::decode_cost(
                &(1_u8, variant),
                ctx,
                operation,
            ),
            Self::Plane
            | Self::Cylinder
            | Self::Cone
            | Self::TorusOrSphere
            | Self::Spline
            | Self::Fillet => Ok(1),
        }
    }
}

impl SurfaceKind {
    /// Compare surface families without the extrusion encoding variant.
    pub(crate) fn same_family(self, other: Self) -> bool {
        std::mem::discriminant(&self) == std::mem::discriminant(&other)
    }

    pub(crate) fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x22 => Some(Self::Plane),
            0x24 => Some(Self::Cylinder),
            0x25 => Some(Self::Cone),
            0x26 => Some(Self::TorusOrSphere),
            0x28 => Some(Self::Spline),
            0x29 => Some(Self::Fillet),
            0x2a => Some(Self::Extrusion(ExtrusionVariant::Linear)),
            0x2c => Some(Self::Extrusion(ExtrusionVariant::TabulatedCylinder)),
            _ => None,
        }
    }

    pub(crate) const fn canonical_type_byte(self) -> u8 {
        match self {
            Self::Plane => 0x22,
            Self::Cylinder => 0x24,
            Self::Cone => 0x25,
            Self::TorusOrSphere => 0x26,
            Self::Spline => 0x28,
            Self::Fillet => 0x29,
            Self::Extrusion(ExtrusionVariant::Linear) => 0x2a,
            Self::Extrusion(ExtrusionVariant::TabulatedCylinder) => 0x2c,
        }
    }
}

/// Encoding variant of an extrusion surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExtrusionVariant {
    /// `geom_type = 0x2a`.
    Linear,
    /// `geom_type = 0x2c`.
    TabulatedCylinder,
}

impl cadmpeg_core::decode::cost::DecodeCost for ExtrusionVariant {
    const FIXED_BYTES: Option<u64> = Some(1);
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(1)
    }
}

/// Admitted surface-row boundary codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoundaryType {
    Code00,
    Code01,
    Code06,
    Code08,
    CodeF6,
}

impl cadmpeg_core::decode::cost::DecodeCost for BoundaryType {
    const FIXED_BYTES: Option<u64> = Some(1);
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(1)
    }
}

impl BoundaryType {
    pub(crate) fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::Code00),
            0x01 => Some(Self::Code01),
            0x06 => Some(Self::Code06),
            0x08 => Some(Self::Code08),
            0xf6 => Some(Self::CodeF6),
            _ => None,
        }
    }

    pub(crate) const fn code(self) -> u8 {
        match self {
            Self::Code00 => 0x00,
            Self::Code01 => 0x01,
            Self::Code06 => 0x06,
            Self::Code08 => 0x08,
            Self::CodeF6 => 0xf6,
        }
    }
}

/// One `srf_array` row whose fixed prefix passed the row grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SurfaceRow {
    /// The row's `geom_id`: the surface's identifier in the `srf_array`
    /// namespace, referenced by curve `F0`/`F1` face fields and by
    /// `next_surface` links.
    pub(crate) id: u32,
    /// The row's surface family, from `geom_type`.
    pub(crate) kind: SurfaceKind,
    /// The `feat_id` compact integer: the feature that generated this
    /// surface, joining `AllFeatur`/`MdlStatus` feature rows.
    pub(crate) feature_id: u32,
    /// `true` when the row's orientation byte is `0xf6` (reversed), `false`
    /// when it is `0x01` (as-stored orientation).
    pub(crate) reversed: bool,
    /// The row's `boundary_type` byte: one of `0x00`, `0x01`, `0x06`, `0x08`,
    /// or `0xf6`.
    pub(crate) boundary_type: BoundaryType,
    /// The `next_geom_ptr` compact integer: the identifier of the next
    /// `srf_array` row in this namespace's link chain.
    pub(crate) next_surface: u32,
    /// Byte offset of the row's `geom_id` field in the original stream.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for SurfaceRow {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                (&self.id, &self.kind, &self.feature_id, &self.reversed),
                (&self.boundary_type, &self.next_surface, &self.offset),
            ),
            ctx,
            operation,
        )
    }
}

/// Surface rows of one namespace, indexed by the identifiers that occur once.
pub(crate) type SurfaceRows = unique_rows::UniqueIdRows<SurfaceRow>;

/// Return the surface row for `id` only when the namespace contains one match.
pub(crate) fn unique_surface_row(rows: &SurfaceRows, id: u32) -> Option<&SurfaceRow> {
    rows.unique(id)
}

/// Named `srf_prim_ptr(<kind>)` prototype family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SurfacePrototypeFamily {
    /// Plane prototype.
    Plane,
    /// Cylinder prototype.
    Cylinder,
    /// Cone prototype.
    Cone,
    /// Torus or sphere prototype.
    Torus(TorusLabel),
    /// Spline-surface prototype.
    Spline(SplineLabel),
    /// Fillet-surface prototype.
    Fillet(FilletLabel),
    /// Surface-of-extrusion prototype.
    Extrusion(ExtrusionLabel),
    /// Structurally valid family name outside the defined set.
    Other(String),
}

/// Exact torus-family label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TorusLabel {
    Torus,
    Sphere,
}

/// Exact spline-family label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SplineLabel {
    Spline,
    Splsrf,
}

/// Exact fillet-family label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FilletLabel {
    Fillet,
    FilletSrf,
}

/// Exact extrusion-family label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExtrusionLabel {
    SurfaceOfExtrusion,
    Extrusion,
    TabulatedCylinder,
    RuledSurface,
}

impl SurfacePrototypeFamily {
    fn from_known_name(name: &str) -> Option<Self> {
        match name {
            "plane" => Some(Self::Plane),
            "cylinder" => Some(Self::Cylinder),
            "cone" => Some(Self::Cone),
            "torus" => Some(Self::Torus(TorusLabel::Torus)),
            "sphere" => Some(Self::Torus(TorusLabel::Sphere)),
            "spline" => Some(Self::Spline(SplineLabel::Spline)),
            "splsrf" => Some(Self::Spline(SplineLabel::Splsrf)),
            "fillet" => Some(Self::Fillet(FilletLabel::Fillet)),
            "fillet_srf" => Some(Self::Fillet(FilletLabel::FilletSrf)),
            "surface_of_extrusion" => Some(Self::Extrusion(ExtrusionLabel::SurfaceOfExtrusion)),
            "extrusion" => Some(Self::Extrusion(ExtrusionLabel::Extrusion)),
            "tab_cyl" => Some(Self::Extrusion(ExtrusionLabel::TabulatedCylinder)),
            "ruled_srf" => Some(Self::Extrusion(ExtrusionLabel::RuledSurface)),
            _ => None,
        }
    }

    /// Exact family name inside `srf_prim_ptr(<family>)`.
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Plane => "plane",
            Self::Cylinder => "cylinder",
            Self::Cone => "cone",
            Self::Torus(TorusLabel::Torus) => "torus",
            Self::Torus(TorusLabel::Sphere) => "sphere",
            Self::Spline(SplineLabel::Spline) => "spline",
            Self::Spline(SplineLabel::Splsrf) => "splsrf",
            Self::Fillet(FilletLabel::Fillet) => "fillet",
            Self::Fillet(FilletLabel::FilletSrf) => "fillet_srf",
            Self::Extrusion(ExtrusionLabel::SurfaceOfExtrusion) => "surface_of_extrusion",
            Self::Extrusion(ExtrusionLabel::Extrusion) => "extrusion",
            Self::Extrusion(ExtrusionLabel::TabulatedCylinder) => "tab_cyl",
            Self::Extrusion(ExtrusionLabel::RuledSurface) => "ruled_srf",
            Self::Other(name) => name,
        }
    }
}

/// Typed wrapper carried by a named surface-prototype parameter.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SurfaceNamedValue {
    /// Present named field with an empty body.
    Empty,
    /// One compact integer.
    CompactInt(u32),
    /// Count-bounded compact-integer array.
    CompactIntArray(Vec<u32>),
    /// Count consecutive entity IDs beginning at one stored reference.
    ContiguousEntityReferences(Vec<u32>),
    /// Dimensioned `f9` scalar body.
    ScalarArray(arrays::DimensionedScalars),
    /// Counted `f8` scalar body.
    CountedScalarArray(arrays::CountedScalars),
    /// One or more consecutive scalar tokens.
    ScalarSequence(Vec<f64>),
    /// Exact bytes of a wrapper that is not structurally defined.
    Opaque(Vec<u8>),
}

impl SurfaceNamedValue {
    fn copy_retained(&self, ctx: &DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(match self {
            Self::Empty => Self::Empty,
            Self::CompactInt(value) => Self::CompactInt(*value),
            Self::CompactIntArray(values) => Self::CompactIntArray(
                ctx.collect_retained_vec(values.iter().copied(), "creo retained surface integers")?,
            ),
            Self::ContiguousEntityReferences(values) => {
                Self::ContiguousEntityReferences(ctx.collect_retained_vec(
                    values.iter().copied(),
                    "creo retained surface references",
                )?)
            }
            Self::ScalarArray(values) => Self::ScalarArray(values.copy_retained(ctx)?),
            Self::CountedScalarArray(values) => {
                Self::CountedScalarArray(values.copy_retained(ctx)?)
            }
            Self::ScalarSequence(values) => Self::ScalarSequence(ctx.collect_retained_vec(
                values.iter().copied(),
                "creo retained surface scalar sequence",
            )?),
            Self::Opaque(bytes) => {
                Self::Opaque(ctx.copy_retained(bytes, "creo retained opaque surface bytes")?)
            }
        })
    }
}

/// One selected named parameter inside a surface prototype.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SurfaceNamedParameter {
    /// Named-record field name.
    pub(crate) name: String,
    /// Typed interpretation of the field body.
    pub(crate) value: SurfaceNamedValue,
    /// Exact field body bytes.
    pub(crate) body: Vec<u8>,
    /// Byte offset of the named-record header.
    pub(crate) offset: usize,
    /// Byte offset of the first value byte.
    pub(crate) value_offset: usize,
}

/// Bounded `srf_prim_ptr(<kind>)` prototype and its named parameters.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SurfacePrototypeRecord {
    /// Surface family named by the prototype label.
    pub(crate) family: SurfacePrototypeFamily,
    /// Selected named parameters in byte order.
    parameters: Vec<SurfaceNamedParameter>,
    fields: [PrototypeField; PROTOTYPE_PARAMETER_NAMES.len()],
    /// Byte offset of the prototype label.
    pub(crate) offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrototypeField {
    Missing,
    One(usize),
    Many,
}

impl SurfacePrototypeRecord {
    pub(crate) fn parameters(&self) -> &[SurfaceNamedParameter] {
        &self.parameters
    }

    pub(crate) fn relocate_parameters(
        &mut self,
        ctx: &DecodeContext<'_>,
        base: usize,
    ) -> Result<(), CodecError> {
        for parameter in ctx.admit_iter(&mut self.parameters, "creo record child relocation traversal")? {
            parameter.offset += base;
            parameter.value_offset += base;
        }
        Ok(())
    }

    pub(crate) fn new(
        ctx: &DecodeContext<'_>,
        family: SurfacePrototypeFamily,
        parameters: Vec<SurfaceNamedParameter>,
        offset: usize,
    ) -> Result<Self, CodecError> {
        let mut fields = [PrototypeField::Missing; PROTOTYPE_PARAMETER_NAMES.len()];
        for (position, parameter) in ctx
            .admit_iter(&parameters, "creo prototype parameter index")?
            .enumerate()
        {
            if let Some(slot) = PROTOTYPE_PARAMETER_NAMES
                .iter()
                .position(|name| *name == parameter.name)
            {
                fields[slot] = match fields[slot] {
                    PrototypeField::Missing => PrototypeField::One(position),
                    PrototypeField::One(_) | PrototypeField::Many => PrototypeField::Many,
                };
            }
        }
        Ok(Self {
            family,
            parameters,
            fields,
            offset,
        })
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        family: SurfacePrototypeFamily,
        parameters: Vec<SurfaceNamedParameter>,
        offset: usize,
    ) -> Self {
        crate::decode::with_test_decode_ctx(|ctx| Self::new(ctx, family, parameters, offset))
            .expect("prototype fixture index")
    }

    /// Return the unique selected parameter with `name`.
    pub(crate) fn field(&self, name: &str) -> Option<&SurfaceNamedParameter> {
        let slot = PROTOTYPE_PARAMETER_NAMES
            .iter()
            .position(|candidate| *candidate == name)?;
        match self.fields[slot] {
            PrototypeField::One(position) => self.parameters.get(position),
            PrototypeField::Missing | PrototypeField::Many => None,
        }
    }

    /// Return the chart-origin vector carried by a `tab_cyl` local system.
    pub(crate) fn tabulated_cylinder_chart_origin(&self) -> Option<[f64; 3]> {
        if self.family != SurfacePrototypeFamily::Extrusion(ExtrusionLabel::TabulatedCylinder) {
            return None;
        }
        let SurfaceNamedValue::ScalarArray(array) = &self.field("local_sys")?.value else {
            return None;
        };
        if array.dimensions() != 4 || array.count() != 3 {
            return None;
        }
        let values = array.values();
        if values.iter().all(|value| value.is_some_and(f64::is_finite)) {
            return Some([values[9]?, values[10]?, values[11]?]);
        }
        let compact_chart = values[..7]
            .iter()
            .all(|value| value.is_some_and(|value| value == 0.0))
            && values[7..10]
                .iter()
                .all(|value| value.is_some_and(f64::is_finite))
            && values[10..12].iter().all(Option::is_none);
        compact_chart.then_some([values[7]?, values[8]?, values[9]?])
    }

    /// Return the four contiguous control-point IDs in a `tab_cyl` prototype.
    pub(crate) fn tabulated_cylinder_control_point_ids(&self) -> Option<[u32; 4]> {
        if self.family != SurfacePrototypeFamily::Extrusion(ExtrusionLabel::TabulatedCylinder) {
            return None;
        }
        let SurfaceNamedValue::ContiguousEntityReferences(entity_ids) =
            &self.field("c_pnts")?.value
        else {
            return None;
        };
        entity_ids.as_slice().try_into().ok()
    }
}

/// Structural boundary that terminates a positional surface parameter body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceBodyBoundary {
    /// `e3` compound-close byte.
    CompoundClose,
    /// Start of the next validated positional surface row.
    NextRow,
    /// Start of the next named record.
    NamedRecord,
    /// End of the containing section.
    SectionEnd,
}

/// Bounded analytic parameter body from one positional `srf_array` row.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SurfaceParameterRecord {
    /// Owning `srf_array` geometry identifier.
    pub(crate) surface_id: u32,
    /// Exact bytes after `next_geom_ptr` and before the structural boundary.
    pub(crate) body: Vec<u8>,
    /// Decoded scalar tokens with byte spans relative to `body`.
    pub(crate) scalar_tokens: Vec<SurfaceParameterScalar>,
    /// Exact byte spans not owned by a recognized scalar token.
    pub(crate) opaque_spans: Vec<SurfaceParameterOpaqueSpan>,
    /// Maximal contiguous scalar-token frames in byte order.
    pub(crate) scalar_frames: Vec<SurfaceParameterScalarFrame>,
    /// Row kind and its decoded carrier, when available.
    pub(crate) carrier: SurfaceParameterCarrier,
    /// Structural form that bounded the body.
    pub(crate) boundary: SurfaceBodyBoundary,
    /// Byte offset of the positional surface row in the original stream.
    pub(crate) offset: usize,
    /// Byte offset of the first parameter-body byte in the original stream.
    pub(crate) body_offset: usize,
}

/// Decoded carrier or the declared kind of an unresolved parameter body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SurfaceParameterCarrier {
    Unresolved(SurfaceKind),
    Resolved(InlineSurfaceCarrier),
}

#[derive(Debug, Clone, Copy)]
struct SplineReplayShape {
    tangent_conditions: [u32; 2],
    u_count: usize,
    v_count: usize,
    point_count: usize,
    u_derivative_count: usize,
    v_derivative_count: usize,
}

fn complete_spline_vector_count(prototype: &SurfacePrototypeRecord, name: &str) -> Option<usize> {
    let SurfaceNamedValue::ScalarArray(array) = &prototype.field(name)?.value else {
        return None;
    };
    (array.count() == 3).then_some(())?;
    let dimensions = usize::try_from(array.dimensions()).ok()?;
    array.is_complete().then_some(dimensions)
}

fn complete_spline_parameter_count(
    prototype: &SurfacePrototypeRecord,
    name: &str,
) -> Option<usize> {
    let SurfaceNamedValue::CountedScalarArray(array) = &prototype.field(name)?.value else {
        return None;
    };
    let count = usize::try_from(array.count()).ok()?;
    array.is_strictly_increasing().then_some(count)
}

fn spline_replay_shape(prototype: &SurfacePrototypeRecord) -> Option<SplineReplayShape> {
    matches!(prototype.family, SurfacePrototypeFamily::Spline(_)).then_some(())?;
    let SurfaceNamedValue::CompactIntArray(tangent_conditions) =
        &prototype.field("tan_cond")?.value
    else {
        return None;
    };
    let tangent_conditions: [u32; 2] = tangent_conditions.as_slice().try_into().ok()?;
    let u_count = complete_spline_parameter_count(prototype, "u_params")?;
    let v_count = complete_spline_parameter_count(prototype, "v_params")?;
    let point_count = u_count.checked_mul(v_count)?;
    let u_derivative_count = v_count.checked_mul(2)?;
    let v_derivative_count = u_count.checked_mul(2)?;
    (u_count >= 2
        && v_count >= 2
        && complete_spline_vector_count(prototype, "i_points")? == point_count
        && complete_spline_vector_count(prototype, "end_u_tangts")? == u_derivative_count
        && complete_spline_vector_count(prototype, "end_v_tangts")? == v_derivative_count
        && complete_spline_vector_count(prototype, "end_uv_deriv")? == 4)
        .then_some(SplineReplayShape {
            tangent_conditions,
            u_count,
            v_count,
            point_count,
            u_derivative_count,
            v_derivative_count,
        })
}

fn associated_spline_replay_prototype(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    rows: &[SurfaceRow],
    row: &SurfaceRow,
) -> Result<Option<SurfacePrototypeRecord>, CodecError> {
    if row.kind != SurfaceKind::Spline {
        return Ok(None);
    }
    let mut scratch = ctx.reserve_scoped(0, "creo spline prototype association scratch")?;
    let complete_bounds = scratch.with_storage(|| complete_surface_array_bounds(ctx, payload))?;
    let Some(&(frame_start, frame_end)) = crate::decode::uniqueness::exactly_one_by(
        ctx,
        &complete_bounds,
        |(start, end)| Ok(row.offset >= *start && row.offset < *end),
        "creo spline replay array lookup",
    )?
    else {
        return Ok(None);
    };
    let cache = scalar::ScalarCache::from_section_checked(ctx, payload)?;
    let frames = scratch.with_storage(|| named_prototype_frames(ctx, payload, &cache))?;
    let Some(prototype) = crate::decode::uniqueness::exactly_one_by(
        ctx,
        &frames,
        |prototype| {
            Ok(
                matches!(prototype.family, SurfacePrototypeFamily::Spline(_))
                    && prototype.offset >= frame_start
                    && prototype.offset < frame_end,
            )
        },
        "creo spline replay prototype lookup",
    )?
    else {
        return Ok(None);
    };
    if row.offset <= prototype.offset {
        return Ok(None);
    }
    let mut previous: Option<&SurfaceRow> = None;
    let mut first: Option<&SurfaceRow> = None;
    for candidate in ctx.admit_iter(rows, "creo spline replay owner rows")? {
        if candidate.offset < frame_start || candidate.offset >= frame_end {
            continue;
        }
        if candidate.offset < prototype.offset
            && previous.is_none_or(|previous| candidate.offset >= previous.offset)
        {
            previous = Some(candidate);
        }
        if candidate.offset > prototype.offset
            && candidate.kind == SurfaceKind::Spline
            && first.is_none_or(|first| candidate.offset < first.offset)
        {
            first = Some(candidate);
        }
    }
    let first = if previous.is_some_and(|previous| previous.kind == SurfaceKind::Spline) {
        previous
    } else {
        first
    };
    let Some(first) = first else {
        return Ok(None);
    };
    if first.feature_id != row.feature_id || first.offset == row.offset {
        return Ok(None);
    }
    Ok(Some(decode_named_prototype_frame(
        ctx,
        payload,
        prototype,
        &cache,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )?))
}

/// Return the unique spline prototype that owns a later positional replay.
pub(crate) fn positional_spline_replay_prototype(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    rows: &[SurfaceRow],
    row: &SurfaceRow,
) -> Result<Option<SurfacePrototypeRecord>, CodecError> {
    associated_spline_replay_prototype(ctx, payload, rows, row)
}

fn take_spline_scalars(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    cursor: &mut usize,
    count: usize,
    name: &str,
    cache: &scalar::ScalarCache,
) -> Result<Option<Vec<f64>>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(remaining) = body.len().checked_sub(*cursor) else {
        return Ok(None);
    };
    if count > remaining {
        return Ok(None);
    }
    let mut values = Vec::new();
    let mut slots = 0..count;
    while !slots.is_empty() {
        if *cursor >= body.len() {
            return Ok(None);
        }
        let Some(_) = ctx.next_charged(&mut slots, "creo spline replay scalar dispatch")? else {
            break;
        };
        let Some((value, next)) = named_spline_scalar_slot(
            &SurfacePrototypeFamily::Spline(SplineLabel::Spline),
            name,
            body,
            *cursor,
            cache,
        ) else {
            return Ok(None);
        };
        let Some(value) = value else {
            return Ok(None);
        };
        if next <= *cursor {
            return Ok(None);
        }
        ctx.reserve_vec(&mut values, 1, "creo spline replay scalar values")?;
        values.push(value);
        *cursor = next;
    }
    Ok(Some(values))
}

fn take_spline_vectors(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    cursor: &mut usize,
    count: usize,
    name: &str,
    cache: &scalar::ScalarCache,
) -> Result<Option<Vec<[f64; 3]>>, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "creo spline replay vector candidate")?;
    if !count.is_multiple_of(3) || body.len().checked_sub(*cursor).is_none_or(|remaining| count > remaining) {
        return Ok(None);
    }
    let mut vectors = Vec::new();
    let mut slots = 0..count;
    while !slots.is_empty() {
        let mut vector = [0.0; 3];
        for coordinate in &mut vector {
            if *cursor >= body.len() { return Ok(None); }
            let Some(_) = ctx.next_charged(&mut slots, "creo spline replay scalar dispatch")? else { return Ok(None); };
            let Some((Some(value), next)) = named_spline_scalar_slot(
                &SurfacePrototypeFamily::Spline(SplineLabel::Spline), name, body, *cursor, cache,
            ) else { return Ok(None); };
            if next <= *cursor { return Ok(None); }
            *coordinate = value;
            *cursor = next;
        }
        storage.with_storage(|| ctx.reserve_vec(&mut vectors, 1, "creo spline replay vectors"))?;
        vectors.push(vector);
    }
    Ok(Some(storage.commit_value(vectors)?))
}

fn take_spline_mixed_derivatives(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    cursor: &mut usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<[[f64; 3]; 4]>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(remaining) = body.len().checked_sub(*cursor) else {
        return Ok(None);
    };
    if remaining < 12 {
        return Ok(None);
    }
    let mut derivatives = [[0.0; 3]; 4];
    for derivative in &mut derivatives {
        for slot in derivative {
            let Some((Some(value), next)) = named_spline_scalar_slot(
                &SurfacePrototypeFamily::Spline(SplineLabel::Spline),
                "end_uv_deriv",
                body,
                *cursor,
                cache,
            ) else {
                return Ok(None);
            };
            if next <= *cursor {
                return Ok(None);
            }
            *slot = value;
            *cursor = next;
        }
    }
    Ok(Some(derivatives))
}


fn parse_positional_spline_replay(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    prototype: &SurfacePrototypeRecord,
    cache: &scalar::ScalarCache,
) -> Result<Option<(crate::interpolation_grid::InterpolationGrid, usize)>, CodecError> {
    let Some(shape) = spline_replay_shape(prototype) else {
        return Ok(None);
    };
    let Some(envelope_close) = surface_body_compound_close(ctx, SurfaceKind::Spline, body, cache)?
    else {
        return Ok(None);
    };
    let Some(mut cursor) = envelope_close.checked_add(1) else {
        return Ok(None);
    };

    let mut replay_tangent_conditions = [0; 2];
    for condition in &mut replay_tangent_conditions {
        let (value, next) = compact_int(body, cursor);
        if next <= cursor {
            return Ok(None);
        }
        *condition = value;
        cursor = next;
    }
    if replay_tangent_conditions != shape.tangent_conditions {
        return Ok(None);
    }

    let Some(point_count) = shape.point_count.checked_mul(3) else {
        return Ok(None);
    };
    let Some(points) = take_spline_vectors(ctx, body, &mut cursor, point_count, "i_points", cache)?
    else {
        return Ok(None);
    };
    let Some(u_count) = shape.u_derivative_count.checked_mul(3) else {
        return Ok(None);
    };
    let Some(u_derivatives) =
        take_spline_vectors(ctx, body, &mut cursor, u_count, "end_u_tangts", cache)?
    else {
        return Ok(None);
    };
    let Some(v_count) = shape.v_derivative_count.checked_mul(3) else {
        return Ok(None);
    };
    let Some(v_derivatives) =
        take_spline_vectors(ctx, body, &mut cursor, v_count, "end_v_tangts", cache)?
    else {
        return Ok(None);
    };
    let Some(mixed_derivatives) =
        take_spline_mixed_derivatives(ctx, body, &mut cursor, cache)?
    else {
        return Ok(None);
    };
    let Some(u_parameters) =
        take_spline_scalars(ctx, body, &mut cursor, shape.u_count, "u_params", cache)?
    else {
        return Ok(None);
    };
    let Some(v_parameters) =
        take_spline_scalars(ctx, body, &mut cursor, shape.v_count, "v_params", cache)?
    else {
        return Ok(None);
    };
    Ok(crate::interpolation_grid::InterpolationGrid::try_new(
        ctx,
        points,
        u_parameters,
        v_parameters,
        u_derivatives,
        v_derivatives,
        mixed_derivatives,
    )?
    .map(|grid| (grid, cursor)))
}

/// Return the final structural close of a complete positional spline replay.
fn positional_spline_replay_body_end(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    rows: &[SurfaceRow],
    row: &SurfaceRow,
    body_start: usize,
    body_limit: usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<usize>, CodecError> {
    let mut prototype_scope = ctx.reserve_scoped(0, "creo spline boundary prototype scratch")?;
    let Some(prototype) = prototype_scope
        .with_storage(|| associated_spline_replay_prototype(ctx, payload, rows, row))?
    else {
        return Ok(None);
    };
    let Some(body) = payload.get(body_start..body_limit) else {
        return Ok(None);
    };
    let mut replay_scope = ctx.reserve_scoped(0, "creo spline replay boundary scratch")?;
    let Some((_, consumed)) = replay_scope
        .with_storage(|| parse_positional_spline_replay(ctx, body, &prototype, cache))?
    else {
        return Ok(None);
    };
    Ok((body.get(consumed) == Some(&psb::token::COMPOUND_CLOSE))
        .then(|| body_start.checked_add(consumed))
        .flatten())
}

/// Decode a positional spline replay body after its final close was removed.
pub(crate) fn decode_positional_spline_replay(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    prototype: &SurfacePrototypeRecord,
    cache: &scalar::ScalarCache,
) -> Result<Option<crate::interpolation_grid::InterpolationGrid>, CodecError> {
    let mut replay_scope = ctx.reserve_scoped(0, "creo positional spline replay scratch")?;
    let Some((replay, consumed)) = replay_scope
        .with_storage(|| parse_positional_spline_replay(ctx, body, prototype, cache))?
    else {
        return Ok(None);
    };
    if consumed != body.len() {
        return Ok(None);
    }
    Ok(Some(replay_scope.commit_value(replay)?))
}

/// One complete contour-chain entry following a positional `srf_array` row's
/// envelope and local-system bodies.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SurfaceContourRecord {
    /// Owning `srf_array` geometry identifier.
    pub(crate) surface_id: u32,
    /// Zero-based position in the owning contour chain.
    pub(crate) chain_index: usize,
    /// Two-byte compact reference carried by the contour header.
    pub(crate) curve_header_id: u32,
    /// Stored contour traversal byte.
    pub(crate) trv: u8,
    /// Four ordered parameter-space envelope slots. `None` retains a
    /// structurally framed unresolved slot.
    pub(crate) parameter_envelope: [Option<f64>; 4],
    /// Optional canonical reference following this contour's close marker.
    pub(crate) separator_reference: Option<u32>,
    /// Exact contour bytes from `curve_header_id` through its `e3` or `e1`
    /// close marker.
    pub(crate) body: Vec<u8>,
    /// Byte offset of the contour header in the containing stream.
    pub(crate) offset: usize,
    /// Byte offset of the first parameter-space envelope scalar.
    pub(crate) envelope_offset: usize,
    /// Byte offset of the owning positional surface row.
    pub(crate) surface_row_offset: usize,
}

/// Return the positional parameter record for `surface_id` only when exactly
/// one exists.
pub(crate) fn unique_surface_parameter(
    records: &SurfaceParameters,
    surface_id: u32,
) -> Option<&SurfaceParameterRecord> {
    records.unique(surface_id)
}

/// Positional parameter records of one namespace, indexed by the surface
/// identifiers that occur once.
pub(crate) type SurfaceParameters = unique_rows::UniqueIdRows<SurfaceParameterRecord>;

/// Six-slot model-space envelope frame following a tabulated-cylinder marker.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TabulatedCylinderFrame {
    values: cadmpeg_ir::units::FiniteVector<6>,
    prefixes: [u8; 6],
}

impl TabulatedCylinderFrame {
    /// Admits six finite frame coordinates with their scalar prefixes.
    pub(crate) fn new(values: [f64; 6], prefixes: [u8; 6]) -> Option<Self> {
        Some(Self {
            values: cadmpeg_ir::units::FiniteVector::new(values)?,
            prefixes,
        })
    }

    /// Ordered frame coordinates.
    pub(crate) fn values(&self) -> cadmpeg_ir::units::FiniteVector<6> {
        self.values
    }

    /// Scalar-lane prefix bytes in coordinate order.
    pub(crate) fn prefixes(&self) -> [u8; 6] {
        self.prefixes
    }
}

/// Model-space origin with an orthonormal direction pair.
///
/// The positional and legacy analytic carriers hold one of these and add their own scalars.
/// The origin is the origin of a plane, a point on the axis of a cylinder, the apex of a cone
/// and the center of a torus or sphere. The pair is the IR [`OrthonormalFrame3`] and is
/// admitted by its measurement, so an analytic carrier takes the held origin and frame
/// without a second admission.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PositionalFrame {
    origin: FinitePoint3,
    frame: OrthonormalFrame3,
}

impl PositionalFrame {
    /// Admits a finite origin and a unit-length orthogonal direction pair.
    pub(crate) fn new(origin: [f64; 3], axis: [f64; 3], ref_direction: [f64; 3]) -> Option<Self> {
        Some(Self {
            origin: FinitePoint3::new(Point3::from(origin))?,
            frame: OrthonormalFrame3::new(Vector3::from(axis), Vector3::from(ref_direction))?,
        })
    }

    /// Returns the same frame with the axis directed the opposite way.
    pub(crate) fn with_reversed_axis(self) -> Self {
        let mut frame = self.frame;
        frame.reverse_axis();
        Self { frame, ..self }
    }

    /// Returns the origin.
    pub(crate) fn origin(&self) -> [f64; 3] {
        let origin = self.origin.get();
        [origin.x, origin.y, origin.z]
    }
    /// Returns the axis.
    pub(crate) fn axis(&self) -> [f64; 3] {
        (*self.frame.axis().as_raw()).into()
    }
    /// Returns the ref direction.
    pub(crate) fn ref_direction(&self) -> [f64; 3] {
        (*self.frame.reference().as_raw()).into()
    }

    /// Returns the admitted origin.
    pub(crate) fn finite_origin(&self) -> FinitePoint3 {
        self.origin
    }
    /// Returns the admitted direction pair, the axis first.
    pub(crate) fn orthonormal_frame(&self) -> OrthonormalFrame3 {
        self.frame
    }
}

/// Complete model-space carrier and optional axial extent from a positional cylinder row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PositionalCylinderFrame {
    frame: PositionalFrame,
    /// Cylinder radius.
    radius: PositiveLength,
    /// Distance between the axial ends.
    ///
    /// The type carries the sign and the finiteness of a present extent, so a reader converting
    /// one to an IR length runs a total conversion and states no refusal arm.
    ///
    /// `None` states a file condition and not a decode gap: the body either carries no axial
    /// extent field, or carries an axial span that its own nonzero tolerance refuses. A reader
    /// that needs an extent takes one from the surrounding feature or states no geometry.
    length: Option<PositiveLength>,
}

impl PositionalCylinderFrame {
    /// Admits a finite frame with valid directions and dimensions.
    pub(crate) fn new(
        origin: [f64; 3],
        axis: [f64; 3],
        ref_direction: [f64; 3],
        radius: f64,
        length: Option<f64>,
    ) -> Option<Self> {
        let length = match length {
            Some(length) => Some(PositiveLength::new(length)?),
            None => None,
        };
        let radius = PositiveLength::new(radius)?;
        Self::with_admitted_dimensions(origin, axis, ref_direction, radius, length)
    }
    /// Admits a finite frame with valid directions around an admitted radius and extent.
    pub(crate) fn with_admitted_dimensions(
        origin: [f64; 3],
        axis: [f64; 3],
        ref_direction: [f64; 3],
        radius: PositiveLength,
        length: Option<PositiveLength>,
    ) -> Option<Self> {
        Some(Self {
            frame: PositionalFrame::new(origin, axis, ref_direction)?,
            radius,
            length,
        })
    }
    /// Returns the frame.
    pub(crate) fn frame(&self) -> &PositionalFrame {
        &self.frame
    }
    /// Returns the radius.
    pub(crate) fn radius(&self) -> PositiveLength {
        self.radius
    }
    /// Returns the length.
    pub(crate) fn length(&self) -> Option<PositiveLength> {
        self.length
    }
}

/// Complete model-space carrier decoded from a positional cone row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PositionalConeFrame {
    frame: PositionalFrame,
    half_angle: ApexConeHalfAngle,
}

impl PositionalConeFrame {
    /// Admits a finite apex and valid directions around an admitted half angle.
    pub(crate) fn new(
        apex: [f64; 3],
        axis: [f64; 3],
        ref_direction: [f64; 3],
        half_angle: ApexConeHalfAngle,
    ) -> Option<Self> {
        Some(Self {
            frame: PositionalFrame::new(apex, axis, ref_direction)?,
            half_angle,
        })
    }
    /// Returns the frame, whose origin is the apex.
    pub(crate) fn frame(&self) -> &PositionalFrame {
        &self.frame
    }
    /// Returns the half angle.
    pub(crate) fn half_angle(&self) -> ApexConeHalfAngle {
        self.half_angle
    }
}

/// Complete model-space carrier decoded from a positional torus row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PositionalTorusFrame {
    frame: PositionalFrame,
    /// Major radius; zero selects the sphere form.
    major_radius: NonNegativeLength,
    /// Minor radius.
    minor_radius: PositiveLength,
}

impl PositionalTorusFrame {
    /// Admits a finite frame with valid directions and dimensions.
    pub(crate) fn new(
        center: [f64; 3],
        axis: [f64; 3],
        ref_direction: [f64; 3],
        major_radius: f64,
        minor_radius: f64,
    ) -> Option<Self> {
        Some(Self {
            frame: PositionalFrame::new(center, axis, ref_direction)?,
            major_radius: NonNegativeLength::new(major_radius)?,
            minor_radius: PositiveLength::new(minor_radius)?,
        })
    }
    /// Returns the frame, whose origin is the center.
    pub(crate) fn frame(&self) -> &PositionalFrame {
        &self.frame
    }
    /// Returns the major radius.
    pub(crate) fn major_radius(&self) -> NonNegativeLength {
        self.major_radius
    }
    /// Returns the minor radius.
    pub(crate) fn minor_radius(&self) -> PositiveLength {
        self.minor_radius
    }
}

/// Six finite outline coordinates in a positional torus-or-sphere body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TorusOutlineFrame {
    /// Ordered outline coordinates.
    pub(crate) values: [f64; 6],
    /// Compact selector following the outline marker.
    pub(crate) selector: u32,
    /// Byte offset of the outline marker relative to the parameter body.
    pub(crate) offset: usize,
}

/// Five finite endpoint coordinates in an untagged type-26 body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Type26FiveCoordinateEnvelope {
    /// Final five coordinates after the leading body-local scalar.
    pub(crate) values: [f64; 5],
    /// Byte offset of the first retained coordinate relative to the body.
    pub(crate) offset: usize,
}

/// Four finite coordinates separated by a body-local control payload in a type-26 body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Type26SplitCoordinateEnvelope {
    /// Two coordinates before and two coordinates after the control payload.
    pub(crate) values: [f64; 4],
    /// Byte offset of the first coordinate relative to the body.
    pub(crate) offset: usize,
}

/// Finite nonnegative major and positive minor radius overrides in a positional
/// torus-or-sphere body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TorusRadiusOverrides {
    /// Major torus radius, or zero for a sphere.
    pub(crate) radius1: f64,
    /// Minor torus radius, or sphere radius.
    pub(crate) radius2: f64,
    /// Whether the first stored scalar is `radius2` or `radius1 + radius2`.
    pub(crate) radius2_encoding: TorusRadius2Encoding,
    /// Byte offset of the `18 0d` radius trailer marker.
    pub(crate) offset: usize,
}

/// Interpretation of the first radial scalar in a tagged type-26 trailer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TorusRadius2Encoding {
    /// The scalar stores `radius2` directly.
    Direct,
    /// The scalar stores the outer ring radius `radius1 + radius2`.
    OuterRingDifference,
}

/// Terminal half-angle override in a positional cone body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ConeHalfAngleOverride {
    /// Cone half angle.
    pub(crate) radians: ApexConeHalfAngle,
    /// Byte offset of the positive-DICT token relative to the parameter body.
    pub(crate) offset: usize,
}

#[derive(Debug, Clone, Copy)]
struct ConeHalfAngleLayout {
    value: ApexConeHalfAngle,
    start: usize,
    end: usize,
}

#[derive(Debug, Clone, Copy)]
struct TorusRadiusOverrideLayout {
    overrides: TorusRadiusOverrides,
    radius2_start: usize,
    radius2_end: usize,
    radius1_start: usize,
}

/// Diameter and model-space extent endpoints from a bounded type-24 round body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Type24RoundEnvelope {
    /// Positive difference between the two stored diameter endpoints.
    pub(crate) diameter: f64,
    /// Opposite model-space extent corners.
    pub(crate) extent_endpoints: [[f64; 3]; 2],
}

/// Finite axial parameters and model-space endpoint samples from a generated
/// type-24 round edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Type24RoundEdgeEnvelope {
    /// The two stored edge parameters in source order.
    pub(crate) parameter_interval: [f64; 2],
    /// The two model-space edge endpoints in source order.
    pub(crate) vertices: [[f64; 3]; 2],
    /// Optional generated-entity reference following the endpoint samples.
    pub(crate) generated_entity_reference: Option<u32>,
}

fn torus_outline_values(
    body: &[u8],
    slots: &[SurfaceParameterScalar],
    marker: usize,
    after_selector: usize,
    selector: u32,
) -> Option<TorusOutlineFrame> {
    let selected: &[SurfaceParameterScalar; 6] = slots.try_into().ok()?;
    let mut cursor = after_selector;
    let mut values = [0.0; 6];
    for (index, slot) in selected.iter().enumerate() {
        if slot.offset != cursor {
            return None;
        }
        cursor = cursor.checked_add(slot.raw.len())?;
        values[index] = slot.value?;
    }
    (cursor == body.len() && values.iter().all(|value| value.is_finite())).then_some(
        TorusOutlineFrame {
            values,
            selector,
            offset: marker,
        },
    )
}

#[cfg(test)]
impl SurfaceParameterRecord {
    pub(crate) fn type26_replayed_minor_radius(&self, prototype_minor_radius: f64) -> Option<f64> {
        crate::decode::with_test_decode_ctx(|ctx| {
            self.type26_replayed_minor_radius_checked(ctx, prototype_minor_radius)
        })
        .expect("torus replay fixture admission")
    }
    pub(crate) fn torus_radius_overrides(&self) -> Option<TorusRadiusOverrides> {
        crate::decode::with_test_decode_ctx(|ctx| self.torus_radius_overrides_checked(ctx))
            .expect("torus fixture admission")
    }
    pub(crate) fn torus_outline_frame(&self) -> Option<TorusOutlineFrame> {
        crate::decode::with_test_decode_ctx(|ctx| self.torus_outline_frame_checked(ctx))
            .expect("torus fixture admission")
    }
}

fn perpendicular_round_edge_radius(envelope: Type24RoundEdgeEnvelope) -> Option<f64> {
    let delta = std::array::from_fn::<_, 3, _>(|axis| {
        envelope.vertices[1][axis] - envelope.vertices[0][axis]
    });
    let scale = envelope
        .vertices
        .iter()
        .flatten()
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let mut pairs = (0..3)
        .flat_map(|first| ((first + 1)..3).map(move |second| (first, second)))
        .filter(|(first, second)| {
            delta[*first].abs() > EPS_CYLINDER_GEOMETRY_MIN * scale
                && (delta[*first].abs() - delta[*second].abs()).abs()
                    <= EPS_CYLINDER_GEOMETRY_RELATIVE * scale
        });
    let (first, second) = pairs.next()?;
    pairs.next().is_none().then_some(())?;
    Some(f64::midpoint(delta[first].abs(), delta[second].abs()))
}

/// One contiguous positional scalar frame with no intervening bytes.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct SurfaceParameterScalarFrame {
    /// Byte offset relative to the start of the parameter body.
    pub(crate) offset: usize,
    /// Ordered scalar tokens occupying the frame.
    pub(crate) slots: Vec<SurfaceParameterScalar>,
}

/// One maximal unframed span inside a positional surface parameter body.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SurfaceParameterOpaqueSpan {
    /// Exact source bytes in the span.
    pub(crate) raw: Vec<u8>,
    /// Byte offset relative to the start of the parameter body.
    pub(crate) offset: usize,
}

impl serde::Serialize for SurfaceParameterOpaqueSpan {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut wire = serializer.serialize_struct("SurfaceParameterOpaqueSpan", 3)?;
        wire.serialize_field("raw", &self.raw)?;
        wire.serialize_field("offset", &self.offset)?;
        wire.serialize_field("length", &self.raw.len())?;
        wire.end()
    }
}

/// One scalar token located within a positional surface parameter body.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SurfaceParameterScalar {
    /// Raw decoded scalar value, which can be nonfinite, or `None` for a
    /// structurally framed token whose numeric mapping is not defined.
    pub(crate) value: Option<f64>,
    /// Exact source bytes occupied by the token.
    pub(crate) raw: Vec<u8>,
    /// Byte offset relative to the start of the parameter body.
    pub(crate) offset: usize,
}

fn six_finite_scalar_values(slots: &[SurfaceParameterScalar]) -> Option<[f64; 6]> {
    let [first_x, first_y, first_z, second_x, second_y, second_z] = slots else {
        return None;
    };
    let values = [
        first_x.value?,
        first_y.value?,
        first_z.value?,
        second_x.value?,
        second_y.value?,
        second_z.value?,
    ];
    values
        .iter()
        .all(|value| value.is_finite())
        .then_some(values)
}

impl serde::Serialize for SurfaceParameterScalar {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut wire = serializer.serialize_struct("SurfaceParameterScalar", 4)?;
        wire.serialize_field("value", &self.value)?;
        wire.serialize_field("raw", &self.raw)?;
        wire.serialize_field("offset", &self.offset)?;
        wire.serialize_field("length", &self.raw.len())?;
        wire.end()
    }
}

/// Complete finite positional construction for a line-generated extrusion
/// surface, with nonparallel sweep and directrix directions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LineExtrusionFrame {
    /// Stored model-space sweep direction.
    pub(crate) direction: [f64; 3],
    /// Two model-space points defining the straight directrix.
    pub(crate) directrix: [[f64; 3]; 2],
}

impl LineExtrusionFrame {
    fn is_valid(&self) -> bool {
        let normalize = |vector: [f64; 3]| {
            if !vector.into_iter().all(f64::is_finite) {
                return None;
            }
            let magnitude = vector[0].hypot(vector[1]).hypot(vector[2]);
            (magnitude.is_finite() && magnitude > 0.0)
                .then(|| vector.map(|value| value / magnitude))
        };
        let directrix =
            std::array::from_fn(|axis| self.directrix[1][axis] - self.directrix[0][axis]);
        let Some(direction) = normalize(self.direction) else {
            return false;
        };
        let Some(directrix) = normalize(directrix) else {
            return false;
        };
        let normal = [
            directrix[1] * direction[2] - directrix[2] * direction[1],
            directrix[2] * direction[0] - directrix[0] * direction[2],
            directrix[0] * direction[1] - directrix[1] * direction[0],
        ];
        normal.into_iter().any(|value| value != 0.0)
    }
}

/// One positional cubic B-spline replay bound to a following tabulated-
/// cylinder surface row.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TabulatedCylinderCurveReplay {
    /// Exact bytes from the replay curve identifier through the terminal
    /// `f6 e3` trailer.
    pub(crate) body: Vec<u8>,
    /// Owning `geom_type = 2c` surface identifier.
    pub(crate) surface_id: u32,
    /// Replayed curve identifier.
    pub(crate) curve_id: u32,
    /// Raw curve-family discriminator.
    pub(crate) curve_type: u8,
    /// Stored curve flip byte.
    pub(crate) flip: u8,
    /// Stored tangent-condition byte.
    pub(crate) tangent_condition: u8,
    /// B-spline degree.
    pub(crate) degree: u8,
    /// Exact count-budgeted parameter body.
    pub(crate) parameter_body: Vec<u8>,
    /// Four contiguous control-point entity identifiers.
    pub(crate) control_point_ids: [u32; 4],
    /// Reference following the control-point-array header.
    pub(crate) successor_reference: u32,
    /// Four individually bounded packed control-point bodies.
    pub(crate) control_point_bodies: [Vec<u8>; 4],
    /// Two-coordinate control points when both scalar tokens consume their
    /// complete packed bodies.
    pub(crate) control_points: [Option<[f64; 2]>; 4],
    /// Reference in the terminal control-point trailer.
    pub(crate) terminal_reference: u32,
    /// Byte offset of the replay curve identifier.
    pub(crate) offset: usize,
    /// Byte offset of the owning surface row.
    pub(crate) surface_row_offset: usize,
}

impl SurfaceParameterRecord {
    /// Declared surface family, retained even when no carrier is decoded.
    fn kind(&self) -> SurfaceKind {
        match self.carrier {
            SurfaceParameterCarrier::Unresolved(kind) => kind,
            SurfaceParameterCarrier::Resolved(carrier) => match carrier {
                InlineSurfaceCarrier::Cylinder { .. } | InlineSurfaceCarrier::CylinderBounds(_) => {
                    SurfaceKind::Cylinder
                }
                InlineSurfaceCarrier::Cone(_) => SurfaceKind::Cone,
                InlineSurfaceCarrier::Torus(_) => SurfaceKind::TorusOrSphere,
                InlineSurfaceCarrier::Tabulated { variant, .. } => SurfaceKind::Extrusion(variant),
            },
        }
    }

    /// Context-independent scalar values in body order.
    #[cfg(test)]
    pub(crate) fn scalar_values(&self) -> Vec<f64> {
        self.scalar_tokens
            .iter()
            .filter_map(|token| token.value)
            .collect()
    }

    pub(crate) fn positional_cylinder_frame(&self) -> Option<PositionalCylinderFrame> {
        match self.carrier {
            SurfaceParameterCarrier::Resolved(InlineSurfaceCarrier::Cylinder { frame, .. }) => {
                Some(frame)
            }
            _ => None,
        }
    }

    pub(crate) fn split_cylinder_outline_bounds(&self) -> Option<[[f64; 2]; 2]> {
        match self.carrier {
            SurfaceParameterCarrier::Resolved(InlineSurfaceCarrier::Cylinder {
                split_bounds,
                ..
            }) => split_bounds,
            SurfaceParameterCarrier::Resolved(InlineSurfaceCarrier::CylinderBounds(bounds)) => {
                Some(bounds)
            }
            _ => None,
        }
    }

    pub(crate) fn positional_cone_frame(&self) -> Option<PositionalConeFrame> {
        match self.carrier {
            SurfaceParameterCarrier::Resolved(InlineSurfaceCarrier::Cone(frame)) => Some(frame),
            _ => None,
        }
    }

    pub(crate) fn positional_torus_frame(&self) -> Option<PositionalTorusFrame> {
        match self.carrier {
            SurfaceParameterCarrier::Resolved(InlineSurfaceCarrier::Torus(frame)) => Some(frame),
            _ => None,
        }
    }

    pub(crate) fn tabulated_cylinder_frame(&self) -> Option<TabulatedCylinderFrame> {
        match self.carrier {
            SurfaceParameterCarrier::Resolved(InlineSurfaceCarrier::Tabulated {
                frame, ..
            }) => Some(frame),
            _ => None,
        }
    }

    /// Whether the bounded body has the inline non-plane envelope delimiter.
    ///
    /// The terminal local-system close is excluded from `body`; the single
    /// remaining compound close therefore identifies the envelope close.
    pub(crate) fn has_inline_non_plane_envelope_checked(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<bool, CodecError> {
        let mut close = None;
        let mut selector = false;
        let mut bytes = self.body.iter().enumerate();
        while bytes.len() != 0 {
let Some((offset, &byte)) =
            ctx.next_charged(&mut bytes, "creo inline envelope delimiter search")?
        else { break; };
            if byte == psb::token::COMPOUND_CLOSE {
                if close.is_some() {
                    return Ok(false);
                }
                close = Some(offset);
            } else if close.is_none() && byte == 0x12 {
                selector = true;
            }
        }
        Ok(selector && close.is_some_and(|offset| offset + 1 < self.body.len()))
    }

    /// Whether the bounded body ends with a complete inline non-plane local
    /// system and its family suffix.
    pub(crate) fn has_inline_non_plane_local_system_suffix(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<bool, CodecError> {
        if let Some(refusal) = ctx.resource_refusal() {
            return Err(refusal.into());
        }
        let kind = self.kind();
        if !matches!(
            kind,
            SurfaceKind::Cylinder | SurfaceKind::Cone | SurfaceKind::TorusOrSphere
        ) {
            return Ok(false);
        }
        let cache = scalar::ScalarCache::default();
        for local_start in inline_local_starts(ctx, &self.body) {
            let Some(local) = self.body.get(local_start?..) else {
                continue;
            };
            for prefix in scalar::decode_inline_non_plane_local_system_prefix(ctx, local, &cache)? {
                for frame in inline_resolved_frames(ctx, local, prefix, &cache)? {
                    if decode_inline_surface_suffix_at(kind, local, frame.cursor, &cache)
                        .is_some_and(|(_, end)| end == local.len())
                    {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    /// Decode the terminal positive-DICT half-angle of a positional cone body.
    #[must_use]
    pub(crate) fn cone_half_angle_override(&self) -> Option<ConeHalfAngleOverride> {
        let kind = self.kind();
        if kind != SurfaceKind::Cone {
            return None;
        }
        let layout = terminal_cone_half_angle_layout(&self.body)?;
        Some(ConeHalfAngleOverride {
            radians: layout.value,
            offset: layout.start,
        })
    }

    /// Decode the tagged radius trailer of a positional torus-or-sphere body.
    pub(crate) fn torus_radius_overrides_checked(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<TorusRadiusOverrides>, CodecError> {
        if self.kind() != SurfaceKind::TorusOrSphere {
            return Ok(None);
        }
        Ok(torus_radius_override_layout(ctx, &self.body)?.map(|layout| layout.overrides))
    }

    /// Decode the terminal replay of the prototype minor radius.
    pub(crate) fn type26_replayed_minor_radius_checked(
        &self,
        ctx: &DecodeContext<'_>,
        prototype_minor_radius: f64,
    ) -> Result<Option<f64>, CodecError> {
        if self.kind() != SurfaceKind::TorusOrSphere
            || self.torus_radius_overrides_checked(ctx)?.is_some()
            || !prototype_minor_radius.is_finite()
            || prototype_minor_radius <= 0.0
        {
            return Ok(None);
        }
        let Some(slot) = self
            .terminal_scalar_frame()
            .and_then(|frame| frame.slots.last())
        else {
            return Ok(None);
        };
        Ok(slot.value.filter(|value| {
            slot.offset.checked_add(slot.raw.len()) == Some(self.body.len())
                && value.to_bits() == prototype_minor_radius.to_bits()
        }))
    }

    /// Decode the terminal outline frame of a positional torus-or-sphere body.
    pub(crate) fn torus_outline_frame_checked(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<TorusOutlineFrame>, CodecError> {
        if self.kind() != SurfaceKind::TorusOrSphere {
            return Ok(None);
        }
        let mut offsets = 0..self.body.len();
        let Some(marker) = ctx.find_map(
            &mut offsets,
            |offset| Ok(torus_outline_marker(&self.body, offset)),
            "creo torus outline marker search",
        )?
        else {
            return Ok(None);
        };
        if ctx
            .find_map(
                &mut offsets,
                |offset| Ok(torus_outline_marker(&self.body, offset)),
                "creo torus outline marker search",
            )?
            .is_some()
        {
            return Ok(None);
        }
        let (marker, after_selector, selector) = marker;
        let start = ctx.partition_point(
            &self.scalar_tokens,
            |slot| Ok(slot.offset < after_selector),
            "creo torus outline scalar lookup",
        )?;
        Ok(torus_outline_values(
            &self.body,
            &self.scalar_tokens[start..],
            marker,
            after_selector,
            selector,
        ))
    }

    /// Decode the bounded untagged five-coordinate type-26 envelope.
    #[must_use]
    pub(crate) fn type26_five_coordinate_envelope(&self) -> Option<Type26FiveCoordinateEnvelope> {
        let kind = self.kind();
        (kind == SurfaceKind::TorusOrSphere).then_some(())?;
        if self.body.ends_with(&[0xf7, 0x1c]) {
            let frame_end = self.body.len().checked_sub(2)?;
            if let Some(frame) = self.scalar_frames.last() {
                let end = frame
                    .slots
                    .last()
                    .and_then(|slot| slot.offset.checked_add(slot.raw.len()));
                if let (
                    Ok([coordinate0, coordinate1, coordinate2, coordinate3, coordinate4]),
                    true,
                ) = (
                    <&[SurfaceParameterScalar; 5]>::try_from(frame.slots.as_slice()),
                    end == Some(frame_end),
                ) {
                    let values = [
                        coordinate0.value?,
                        coordinate1.value?,
                        coordinate2.value?,
                        coordinate3.value?,
                        coordinate4.value?,
                    ];
                    return values.iter().all(|value| value.is_finite()).then_some(
                        Type26FiveCoordinateEnvelope {
                            values,
                            offset: frame.offset,
                        },
                    );
                }
            }
            if let [.., first, second] = self.scalar_frames.as_slice() {
                let first_end = first
                    .slots
                    .last()
                    .and_then(|slot| slot.offset.checked_add(slot.raw.len()))?;
                let second_end = second
                    .slots
                    .last()
                    .and_then(|slot| slot.offset.checked_add(slot.raw.len()))?;
                if let (
                    Some([coordinate0, coordinate1, coordinate2]),
                    Ok([coordinate3, coordinate4]),
                    true,
                ) = (
                    first.slots.last_chunk::<3>(),
                    <&[SurfaceParameterScalar; 2]>::try_from(second.slots.as_slice()),
                    first_end < second.offset && second_end == frame_end,
                ) {
                    let values = [
                        coordinate0.value?,
                        coordinate1.value?,
                        coordinate2.value?,
                        coordinate3.value?,
                        coordinate4.value?,
                    ];
                    return values.iter().all(|value| value.is_finite()).then_some(
                        Type26FiveCoordinateEnvelope {
                            values,
                            offset: coordinate0.offset,
                        },
                    );
                }
            }
        }
        let (frame_offset, leading_count, frame_end) =
            if self.body.starts_with(&[0x18, 0x18, 0x01, 0x11]) && self.body.ends_with(&[0x18]) {
                (4, 1, self.body.len() - 1)
            } else if self.body.get(8..19)
                == Some(&[
                    0x18, 0x94, 0x3f, 0x02, 0x70, 0x16, 0xbe, 0xfc, 0x00, 0x12, 0x20,
                ])
                && self.body.get(44) == Some(&0x21)
            {
                (19, 0, 44)
            } else {
                return None;
            };
        let mut frames = self
            .scalar_frames
            .iter()
            .take(frame_offset + 1)
            .filter(|frame| frame.offset == frame_offset);
        let frame = frames.next()?;
        frames.next().is_none().then_some(())?;
        let slots = frame.slots.get(leading_count..)?;
        let [a1, a2, b0, b1, b2] = slots else {
            return None;
        };
        let mut cursor = frame.offset;
        for slot in &frame.slots {
            (slot.offset == cursor).then_some(())?;
            cursor = cursor.checked_add(slot.raw.len())?;
        }
        (cursor == frame_end).then_some(())?;
        let values = [a1.value?, a2.value?, b0.value?, b1.value?, b2.value?];
        values
            .iter()
            .all(|value| value.is_finite())
            .then_some(Type26FiveCoordinateEnvelope {
                values,
                offset: a1.offset,
            })
    }

    /// Decode the type-26 envelope whose final coordinate pair follows a
    /// six-byte body-local control payload.
    #[must_use]
    pub(crate) fn type26_split_coordinate_envelope(&self) -> Option<Type26SplitCoordinateEnvelope> {
        let kind = self.kind();
        (kind == SurfaceKind::TorusOrSphere
            && self.body.get(8..19)
                == Some(&[
                    0x18, 0x94, 0x3f, 0x02, 0x70, 0x16, 0xbe, 0xfc, 0x00, 0x12, 0x20,
                ])
            && self.body.get(30) == Some(&0x3a)
            && self.body.len() == 48)
            .then_some(())?;
        let decode_at = |offset| {
            let (value, end) = scalar::decode(&self.body, offset)?;
            value.is_finite().then_some((value, end))
        };
        let (a1, first_end) = decode_at(19)?;
        let (a2, second_end) = decode_at(first_end)?;
        (second_end == 30).then_some(())?;
        let (b1, third_end) = decode_at(37)?;
        let (b2, fourth_end) = decode_at(third_end)?;
        (third_end == 45 && fourth_end == self.body.len()).then_some(())?;
        Some(Type26SplitCoordinateEnvelope {
            values: [a1, a2, b1, b2],
            offset: 19,
        })
    }

    /// Decode the rolling radius repeated by a bounded type-24 round envelope.
    #[must_use]
    fn type24_round_radius(&self) -> Option<f64> {
        let kind = self.kind();
        (kind == SurfaceKind::Cylinder).then_some(())?;
        self.type24_scalar_frame_round_layout()
            .or_else(|| self.type24_split_coordinate_round_layout())
            .map(|layout| 0.5 * layout.diameter)
            .or_else(|| {
                (self.is_type24_first_coordinate_round_body()
                    || self.is_type24_segmented_first_coordinate_round_body())
                .then_some(())?;
                self.positional_cylinder_frame()
                    .map(|frame| frame.radius.get())
            })
            .or_else(|| {
                self.type24_held_coordinate_round_frame()
                    .map(|frame| frame.radius.get())
            })
            .or_else(|| self.type24_terminal_round_radius())
    }

    /// Decode a type-24 radius in a class-913 generated-round context.
    #[must_use]
    pub(crate) fn type24_generated_round_radius(&self) -> Option<f64> {
        let kind = self.kind();
        (kind == SurfaceKind::Cylinder).then_some(())?;
        let axial_candidates = self.type24_axial_interval_corner_candidates();
        if let Some(first) = axial_candidates
            .as_ref()
            .and_then(|candidates| candidates.first())
        {
            return Some(first.radius.get());
        }
        if let Some(envelope) = self.type24_round_edge_envelope() {
            return perpendicular_round_edge_radius(envelope);
        }
        self.type24_round_radius()
    }

    fn type24_terminal_round_radius(&self) -> Option<f64> {
        let terminal_end = self
            .body
            .strip_suffix(&[0xf7, 0x17])
            .map_or(self.body.len(), <[u8]>::len);
        let terminal = self.scalar_tokens.last()?;
        (terminal.offset.checked_add(terminal.raw.len())? == terminal_end).then_some(())?;
        (terminal.raw.len() == 7
            && terminal
                .raw
                .first()
                .is_some_and(|prefix| matches!(prefix, 0x53..=0xa3)))
        .then_some(())?;
        let radius = terminal.value?;
        (radius.is_finite() && radius > 0.0).then_some(radius)
    }

    /// Decode the diameter and extent envelope of a scalar-frame type-24 row.
    #[must_use]
    pub(crate) fn type24_scalar_frame_round_envelope(&self) -> Option<Type24RoundEnvelope> {
        let kind = self.kind();
        (kind == SurfaceKind::Cylinder).then_some(())?;
        self.type24_scalar_frame_round_layout()
    }

    /// Decode the final two three-coordinate corners of a type-24 patch.
    pub(crate) fn type24_terminal_corner_envelope_checked(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<[[f64; 3]; 2]>, CodecError> {
        if self.kind() != SurfaceKind::Cylinder
            || self.boundary != SurfaceBodyBoundary::CompoundClose
        {
            return Ok(None);
        }
        let Some(terminal) = self.scalar_frames.last() else {
            return Ok(None);
        };
        let mut cursor = terminal.offset;
        let mut slots = terminal.slots.iter();
        while slots.len() != 0 {
let Some(slot) =
            ctx.next_charged(&mut slots, "creo terminal corner frame traversal")?
        else { break; };
            if slot.offset != cursor {
                return Ok(None);
            }
            cursor += slot.raw.len();
        }
        if self.scalar_frame_end_is_owned(cursor).is_none()
            && self.body.get(cursor..) != Some(&[0xf7, 0x17][..])
        {
            return Ok(None);
        }
        let corners = terminal
            .slots
            .len()
            .checked_sub(6)
            .and_then(|start| terminal.slots.get(start..));
        Ok(corners
            .and_then(six_finite_scalar_values)
            .and_then(|values| Some([*values.first_chunk::<3>()?, *values.last_chunk::<3>()?])))
    }

    #[cfg(test)]
    pub(crate) fn type24_terminal_corner_envelope(&self) -> Option<[[f64; 3]; 2]> {
        crate::decode::with_test_decode_ctx(|ctx| self.type24_terminal_corner_envelope_checked(ctx))
            .expect("terminal corner fixture")
    }

    /// Decode a source-bound selector-corner interval cylinder.
    #[must_use]
    pub(crate) fn selector_corner_interval_cylinder_frame(
        &self,
    ) -> Option<PositionalCylinderFrame> {
        let kind = self.kind();
        (kind == SurfaceKind::Cylinder).then_some(())?;
        let frame = cylinder_frame_readers::decode_selector_corner_interval_cylinder_frame(
            &self.body,
            &scalar::ScalarCache::default(),
        )?;
        self.positional_cylinder_frame()
            .is_some_and(|stored| {
                cylinder_frame_readers::positional_cylinder_frames_agree(stored, frame)
            })
            .then_some(frame)
    }

    /// Decode every cylinder placement allowed by a type-24 axial-interval
    /// corner envelope whose control shell does not select a radial quadrant.
    #[must_use]
    pub(crate) fn type24_axial_interval_corner_candidates(
        &self,
    ) -> Option<[PositionalCylinderFrame; 4]> {
        let kind = self.kind();
        if kind != SurfaceKind::Cylinder {
            return None;
        }
        cylinder_frame_readers::decode_type24_axial_interval_corner_candidates(
            &self.body,
            &scalar::ScalarCache::default(),
        )
    }

    /// Decode the positional round-edge envelope carried by a type-24 row.
    ///
    /// The leading control shell and the separator are structural. They must
    /// be consumed before the first-coordinate scalar lane is entered; a
    /// generic scalar walk would mistake separator payload bytes for vertex
    /// coordinates. A compound close may follow the optional generated-entity
    /// reference when the row continues with another bounded body.
    #[must_use]
    pub(crate) fn type24_round_edge_envelope(&self) -> Option<Type24RoundEdgeEnvelope> {
        let kind = self.kind();
        (kind == SurfaceKind::Cylinder).then_some(())?;
        let cache = scalar::ScalarCache::default();
        let start = type24_round_edge_shell_end(&self.body, &cache)?;
        let (first_parameter, mut cursor) =
            scalar::decode_round_edge_coordinate(&self.body, start, &cache)?;
        cursor = type24_round_edge_separator_end(&self.body, cursor, &cache)?;
        let (second_parameter, next) =
            scalar::decode_round_edge_coordinate(&self.body, cursor, &cache)?;
        cursor = next;
        let mut values = [0.0; 6];
        for value in &mut values {
            let (decoded, next) = scalar::decode_round_edge_coordinate(&self.body, cursor, &cache)?;
            *value = decoded;
            cursor = next;
        }
        let generated_entity_reference = if self.body.get(cursor) == Some(&psb::token::ENTITY_REF) {
            let (reference, end) = psb::reference_id(&self.body, cursor + 1).ok()?;
            cursor = end;
            Some(reference)
        } else {
            None
        };
        let valid_end =
            cursor == self.body.len() || self.body.get(cursor) == Some(&psb::token::COMPOUND_CLOSE);
        valid_end.then_some(())?;
        let [first_x, first_y, first_z, second_x, second_y, second_z] = values;
        let values = [
            first_parameter,
            second_parameter,
            first_x,
            first_y,
            first_z,
            second_x,
            second_y,
            second_z,
        ];
        values.iter().all(|value| value.is_finite()).then_some(())?;
        Some(Type24RoundEdgeEnvelope {
            parameter_interval: [first_parameter, second_parameter],
            vertices: [[first_x, first_y, first_z], [second_x, second_y, second_z]],
            generated_entity_reference,
        })
    }

    fn type24_round_frame(&self, cache: &scalar::ScalarCache) -> Option<PositionalCylinderFrame> {
        let kind = self.kind();
        (kind == SurfaceKind::Cylinder).then_some(())?;
        self.repeated_diameter_type24_round_frame(cache)
            .or_else(|| self.type24_held_coordinate_round_frame())
            .or_else(|| {
                match (
                    self.type24_single_diameter_round_frame(),
                    self.type24_square_radial_round_frame(),
                ) {
                    (Some(frame), None) | (None, Some(frame)) => Some(frame),
                    (Some(_), Some(_)) | (None, None) => None,
                }
            })
    }

    /// The maximal scalar-token frame ending at the body boundary.
    #[must_use]
    pub(crate) fn terminal_scalar_frame(&self) -> Option<&SurfaceParameterScalarFrame> {
        terminal_scalar_frame_of(&self.body, &self.scalar_frames)
    }

    fn terminal_scalar_frame_has_owned_end(
        &self,
        terminal: &SurfaceParameterScalarFrame,
    ) -> Option<()> {
        let terminal_end = terminal
            .slots
            .iter()
            .try_fold(terminal.offset, |cursor, slot| {
                (slot.offset == cursor).then(|| cursor + slot.raw.len())
            })?;
        self.scalar_frame_end_is_owned(terminal_end)
    }

    fn scalar_frame_end_is_owned(&self, terminal_end: usize) -> Option<()> {
        if terminal_end == self.body.len() {
            return Some(());
        }
        let suffix = self.body.get(terminal_end..)?;
        if matches!(suffix, [0x00 | 0x10 | 0x18]) {
            return Some(());
        }
        (suffix.first() == Some(&psb::token::ENTITY_REF)).then_some(())?;
        let (_, reference_end) = psb::reference_id(&self.body, terminal_end + 1).ok()?;
        (reference_end == self.body.len()).then_some(())
    }

    fn type24_single_diameter_round_frame(&self) -> Option<PositionalCylinderFrame> {
        (self.boundary == SurfaceBodyBoundary::CompoundClose).then_some(())?;
        let terminal = self.scalar_frames.last()?;
        (terminal.slots.len() == 8).then_some(())?;
        self.terminal_scalar_frame_has_owned_end(terminal)?;
        let diameter = terminal.slots[1].value?;
        let corners = &terminal.slots[2..];
        let values = six_finite_scalar_values(corners)?;
        let first = *values.first_chunk::<3>()?;
        let second = *values.last_chunk::<3>()?;
        let spans = std::array::from_fn::<_, 3, _>(|axis| second[axis] - first[axis]);
        let scale = values
            .iter()
            .chain([&diameter])
            .map(|value| value.abs())
            .fold(1.0, f64::max);
        (diameter.is_finite() && diameter > EPS_SURFACE_NONZERO * scale).then_some(())?;
        let mut radial_axes = (0..3)
            .filter(|axis| (spans[*axis].abs() - diameter).abs() <= EPS_SURFACE_AGREEMENT * scale);
        let radial_axis = radial_axes.next()?;
        radial_axes.next().is_none().then_some(())?;
        let mut axis_delta = spans;
        axis_delta[radial_axis] = 0.0;
        let length = axis_delta
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt();
        (length > EPS_SURFACE_NONZERO * scale).then_some(())?;
        let mut origin = first;
        origin[radial_axis] = f64::midpoint(first[radial_axis], second[radial_axis]);
        let axis = axis_delta.map(|value| value / length);
        let mut ref_direction = [0.0; 3];
        ref_direction[radial_axis] = spans[radial_axis].signum();
        PositionalCylinderFrame::new(origin, axis, ref_direction, 0.5 * diameter, Some(length))
    }

    fn type24_square_radial_round_frame(&self) -> Option<PositionalCylinderFrame> {
        (self.boundary == SurfaceBodyBoundary::CompoundClose).then_some(())?;
        let terminal = self.scalar_frames.last()?;
        ((6..=9).contains(&terminal.slots.len())).then_some(())?;
        let repeated_diameter_shell = if terminal.slots.len() == 7 {
            match self.scalar_frames.as_slice() {
                [leading, _] if (1..=3).contains(&leading.slots.len()) => {
                    let leading_end = leading
                        .slots
                        .iter()
                        .try_fold(leading.offset, |cursor, slot| {
                            (slot.offset == cursor).then(|| cursor + slot.raw.len())
                        })?;
                    let control_length = terminal.offset.checked_sub(leading_end)?;
                    matches!(
                        (leading.slots.len(), leading.offset, control_length),
                        (1, 1, 1 | 3) | (1, 3, 1) | (2, 0 | 1, 1) | (3, 0, 1)
                    )
                }
                _ => false,
            }
        } else {
            false
        };
        self.terminal_scalar_frame_has_owned_end(terminal)?;
        let corners = &terminal.slots[terminal.slots.len() - 6..];
        let values = six_finite_scalar_values(corners)?;
        let first = *values.first_chunk::<3>()?;
        let second = *values.last_chunk::<3>()?;
        let spans = std::array::from_fn::<_, 3, _>(|axis| second[axis] - first[axis]);
        let scale = values.iter().map(|value| value.abs()).fold(1.0, f64::max);
        let mut equal_pairs = [(0, 1), (0, 2), (1, 2)]
            .into_iter()
            .filter(|(first, second)| {
                (spans[*first].abs() - spans[*second].abs()).abs() <= EPS_SURFACE_AGREEMENT * scale
            });
        let (first_radial, second_radial) = equal_pairs.next()?;
        equal_pairs.next().is_none().then_some(())?;
        let axis_index = 3usize.checked_sub(first_radial + second_radial)?;
        let diameter = f64::midpoint(spans[first_radial].abs(), spans[second_radial].abs());
        let length = spans[axis_index].abs();
        (diameter > EPS_SURFACE_NONZERO * scale).then_some(())?;
        let bounded = length > EPS_SURFACE_NONZERO * scale;
        (!repeated_diameter_shell || !bounded).then_some(())?;
        let mut origin = first;
        origin[first_radial] = f64::midpoint(first[first_radial], second[first_radial]);
        origin[second_radial] = f64::midpoint(first[second_radial], second[second_radial]);
        let mut axis = [0.0; 3];
        axis[axis_index] = if bounded {
            spans[axis_index].signum()
        } else {
            1.0
        };
        let mut ref_direction = [0.0; 3];
        ref_direction[first_radial] = spans[first_radial].signum();
        PositionalCylinderFrame::new(
            origin,
            axis,
            ref_direction,
            0.5 * diameter,
            bounded.then_some(length),
        )
    }

    fn repeated_diameter_type24_round_frame(
        &self,
        cache: &scalar::ScalarCache,
    ) -> Option<PositionalCylinderFrame> {
        let layout = self.type24_round_layout(cache)?;
        let spans = std::array::from_fn::<_, 3, _>(|index| {
            layout.extent_endpoints[1][index] - layout.extent_endpoints[0][index]
        });
        let scale = layout
            .extent_endpoints
            .iter()
            .flatten()
            .chain([layout.diameter].iter())
            .map(|value| value.abs())
            .fold(1.0, f64::max);
        let mut radial_indices = spans.iter().enumerate().filter_map(|(index, span)| {
            ((span.abs() - layout.diameter).abs() <= EPS_SURFACE_AGREEMENT * scale).then_some(index)
        });
        let radial_index = radial_indices.next()?;
        radial_indices.next().is_none().then_some(())?;
        let mut axis_vector = spans;
        axis_vector[radial_index] = 0.0;
        let length = axis_vector
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt();
        (length.is_finite() && length > EPS_SURFACE_NONZERO * scale).then_some(())?;
        let mut origin = layout.extent_endpoints[0];
        origin[radial_index] = f64::midpoint(
            layout.extent_endpoints[0][radial_index],
            layout.extent_endpoints[1][radial_index],
        );
        let mut ref_direction = [0.0; 3];
        ref_direction[radial_index] = spans[radial_index].signum();
        PositionalCylinderFrame::new(
            origin,
            axis_vector.map(|value| value / length),
            ref_direction,
            0.5 * layout.diameter,
            Some(length),
        )
    }

    fn type24_held_coordinate_round_frame(&self) -> Option<PositionalCylinderFrame> {
        let contiguous_end = |frame: &SurfaceParameterScalarFrame| {
            frame.slots.iter().try_fold(frame.offset, |cursor, slot| {
                (slot.offset == cursor).then(|| cursor + slot.raw.len())
            })
        };
        let [leading, controls, terminal] = self.scalar_frames.as_slice() else {
            return None;
        };
        let [leading_zero, _] = leading.slots.as_slice() else {
            return None;
        };
        (leading.offset == 0
            && leading_zero.value == Some(0.0)
            && contiguous_end(leading) == Some(9)
            && self.body.get(9..11) == Some(&[0x78, 0xac])
            && controls.offset == 11)
            .then_some(())?;
        let values = match (controls.slots.as_slice(), terminal.slots.as_slice()) {
            ([_, _], [axial_start, radial_start, held, axial_end, radial_end]) => {
                (contiguous_end(controls) == Some(25)
                    && self.body.get(25..27) == Some(&[0x24, 0x00])
                    && terminal.offset == 27
                    && contiguous_end(terminal) == Some(self.body.len()))
                .then_some(())?;
                [
                    axial_start.value?,
                    radial_start.value?,
                    held.value?,
                    axial_end.value?,
                    radial_end.value?,
                ]
            }
            ([control], [auxiliary, axial_start, radial_start, held, axial_end, radial_end]) => {
                let controls_end = contiguous_end(controls)?;
                let terminal_end = contiguous_end(terminal)?;
                (control.value?.is_finite()
                    && controls_end.checked_add(2) == Some(terminal.offset)
                    && auxiliary.value?.is_finite()
                    && (terminal_end == self.body.len()
                        || self.body.get(terminal_end..) == Some(&[0xf7, 0x18])))
                .then_some(())?;
                [
                    axial_start.value?,
                    radial_start.value?,
                    held.value?,
                    axial_end.value?,
                    radial_end.value?,
                ]
            }
            _ => return None,
        };
        let [axial_start, radial_start, held, axial_end, radial_end] = values;
        let axial_span = axial_end - axial_start;
        let radial_span = radial_end - radial_start;
        let scale = [axial_start, radial_start, held, axial_end, radial_end]
            .into_iter()
            .map(f64::abs)
            .fold(1.0, f64::max);
        (held.is_finite()
            && axial_span.is_finite()
            && radial_span.is_finite()
            && axial_span.abs() > EPS_SURFACE_NONZERO * scale
            && radial_span.abs() > EPS_SURFACE_NONZERO * scale)
            .then_some(PositionalCylinderFrame::new(
                [axial_start, f64::midpoint(radial_start, radial_end), held],
                [axial_span.signum(), 0.0, 0.0],
                [0.0, radial_span.signum(), 0.0],
                0.5 * radial_span.abs(),
                Some(axial_span.abs()),
            )?)
    }

    fn type24_round_layout(&self, cache: &scalar::ScalarCache) -> Option<Type24RoundEnvelope> {
        self.type24_scalar_frame_round_layout()
            .or_else(|| self.type24_first_coordinate_round_layout(cache))
            .or_else(|| self.type24_split_coordinate_round_layout())
            .or_else(|| self.type24_segmented_first_coordinate_round_layout(cache))
    }

    fn type24_split_coordinate_round_layout(&self) -> Option<Type24RoundEnvelope> {
        let contiguous_end = |frame: &SurfaceParameterScalarFrame| {
            frame.slots.iter().try_fold(frame.offset, |cursor, slot| {
                (slot.offset == cursor).then(|| cursor + slot.raw.len())
            })
        };
        let [leading, middle, terminal] = self.scalar_frames.as_slice() else {
            return None;
        };
        let [zero, first_diameter] = leading.slots.as_slice() else {
            return None;
        };
        let [second_diameter, a0, a1] = middle.slots.as_slice() else {
            return None;
        };
        let [b0, b1, b2] = terminal.slots.as_slice() else {
            return None;
        };
        let middle_end = contiguous_end(middle)?;
        (leading.offset == 0
            && zero.value == Some(0.0)
            && contiguous_end(leading) == Some(9)
            && self.body.get(9) == Some(&0x12)
            && middle.offset == 10
            && self.body.get(middle_end..terminal.offset) == Some(&[0x34, 0xf0, 0x00])
            && contiguous_end(terminal) == Some(self.body.len()))
        .then_some(())?;
        let diameter_endpoints = [first_diameter.value?, second_diameter.value?];
        let extent_endpoints = [
            [a0.value?, a1.value?, 0.0],
            [b0.value?, b1.value?, b2.value?],
        ];
        let diameter = (diameter_endpoints[1] - diameter_endpoints[0]).abs();
        let scale = diameter_endpoints
            .iter()
            .chain(extent_endpoints.iter().flatten())
            .map(|value| value.abs())
            .fold(1.0, f64::max);
        (diameter > EPS_SURFACE_NONZERO * scale
            && extent_endpoints[0]
                .iter()
                .zip(extent_endpoints[1])
                .any(|(first, second)| {
                    ((second - first).abs() - diameter).abs() <= EPS_SURFACE_AGREEMENT * scale
                }))
        .then_some(Type24RoundEnvelope {
            diameter,
            extent_endpoints,
        })
    }

    fn type24_first_coordinate_round_layout(
        &self,
        cache: &scalar::ScalarCache,
    ) -> Option<Type24RoundEnvelope> {
        self.is_type24_first_coordinate_round_body().then_some(())?;
        let decode_at = |offset| {
            let (value, end) =
                scalar::decode_tabulated_cylinder_first_coordinate(&self.body, offset, cache)?;
            value.is_finite().then_some((value, end))
        };
        let (first_diameter, first_end) = decode_at(type24_round::FIRST_DIAMETER_ENDPOINT)?;
        (first_end == type24_round::SEPARATOR).then_some(())?;
        let (second_diameter, mut cursor) = decode_at(type24_round::SECOND_DIAMETER_ENDPOINT)?;
        (cursor == type24_round::EXTENT_SCALARS).then_some(())?;
        let mut coordinates = [0.0; 6];
        for coordinate in coordinates.iter_mut().take(5) {
            let (value, next) = decode_at(cursor)?;
            *coordinate = value;
            cursor = next;
        }
        (cursor == type24_round::TERMINAL).then_some(())?;
        let [a0, a1, a2, b0, b1, b2] = coordinates;
        let diameter = (second_diameter - first_diameter).abs();
        let scale = [first_diameter, second_diameter]
            .into_iter()
            .chain(coordinates.iter().copied())
            .map(f64::abs)
            .fold(1.0, f64::max);
        (diameter > EPS_SURFACE_NONZERO * scale).then_some(Type24RoundEnvelope {
            diameter,
            extent_endpoints: [[a0, a1, a2], [b0, b1, b2]],
        })
    }

    fn type24_segmented_first_coordinate_round_layout(
        &self,
        cache: &scalar::ScalarCache,
    ) -> Option<Type24RoundEnvelope> {
        self.is_type24_segmented_first_coordinate_round_body()
            .then_some(())?;
        let decode_at = |offset| {
            let (value, end) =
                scalar::decode_tabulated_cylinder_first_coordinate(&self.body, offset, cache)?;
            value.is_finite().then_some((value, end))
        };
        let (first_diameter, first_end) = decode_at(type24_seg::FIRST_DIAMETER_ENDPOINT)?;
        (first_end == type24_seg::LITERAL_RUN).then_some(())?;
        let (second_diameter, mut cursor) = decode_at(type24_seg::SECOND_DIAMETER_ENDPOINT)?;
        (cursor == type24_seg::EXTENT_COORDINATES).then_some(())?;
        let mut coordinates = [0.0; 6];
        for coordinate in &mut coordinates {
            let (value, next) = decode_at(cursor)?;
            *coordinate = value;
            cursor = next;
        }
        (cursor == type24_seg::TRAILER).then_some(())?;
        let [a0, a1, a2, b0, b1, b2] = coordinates;
        let diameter = (second_diameter - first_diameter).abs();
        let scale = [first_diameter, second_diameter]
            .into_iter()
            .chain(coordinates.iter().copied())
            .map(f64::abs)
            .fold(1.0, f64::max);
        (diameter > EPS_SURFACE_NONZERO * scale).then_some(Type24RoundEnvelope {
            diameter,
            extent_endpoints: [[a0, a1, a2], [b0, b1, b2]],
        })
    }

    fn is_type24_first_coordinate_round_body(&self) -> bool {
        self.body.len() == type24_round::LEN
            && self.body.get(..2) == Some(&[0x4c, 0xb7])
            && self.body.get(type24_round::SEPARATOR) == Some(&0x12)
            && self.body.get(type24_round::TERMINAL) == Some(&0x18)
    }

    fn is_type24_segmented_first_coordinate_round_body(&self) -> bool {
        self.body.len() == type24_seg::LEN
            && self.body.get(type24_seg::OPENER) == Some(&0x18)
            && self
                .body
                .get(type24_seg::LITERAL_RUN..type24_seg::SECOND_DIAMETER_ENDPOINT)
                == Some(&[0x70, 0xbf, 0xe3, 0x4f, 0x05, 0x11, 0x10])
            && self.body.get(type24_seg::TRAILER..type24_seg::LEN) == Some(&[0xf7, 0x19])
    }

    fn type24_scalar_frame_round_layout(&self) -> Option<Type24RoundEnvelope> {
        let frame_reaches_body_end = |end: usize| {
            if end == self.body.len() {
                return true;
            }
            self.body.get(end) == Some(&psb::token::ENTITY_REF)
                && psb::reference_id(&self.body, end + 1)
                    .is_ok_and(|(_, reference_end)| reference_end == self.body.len())
        };
        let contiguous_slots_end = |frame: &SurfaceParameterScalarFrame| {
            let mut cursor = frame.offset;
            for slot in &frame.slots {
                (slot.offset == cursor).then_some(())?;
                cursor = cursor.checked_add(slot.raw.len())?;
                slot.value?.is_finite().then_some(())?;
            }
            Some(cursor)
        };
        let (diameter_endpoints, extent_endpoints) = match self.scalar_frames.as_slice() {
            [frame]
                if matches!(
                    self.body.get(..frame.offset),
                    Some([0x15] | [0x00, 0x15, 0x1c])
                ) =>
            {
                let [first, _, second, a0, a1, a2, b0, b1, b2] = frame.slots.as_slice() else {
                    return None;
                };
                let end = contiguous_slots_end(frame)?;
                frame_reaches_body_end(end).then_some(())?;
                (
                    [first.value?, second.value?],
                    [
                        [a0.value?, a1.value?, a2.value?],
                        [b0.value?, b1.value?, b2.value?],
                    ],
                )
            }
            [leading, trailing] if leading.slots.len() == 1 => {
                let [first] = leading.slots.as_slice() else {
                    return None;
                };
                let [second, a0, a1, a2, b0, b1, b2] = trailing.slots.as_slice() else {
                    return None;
                };
                let leading_end = contiguous_slots_end(leading)?;
                let trailing_end = contiguous_slots_end(trailing)?;
                let controls_match = (leading.offset == 1
                    && matches!(self.body.first(), Some(0x11..=0x14))
                    && trailing.offset == leading_end + 1
                    && matches!(self.body.get(leading_end), Some(0x11..=0x14)))
                    || (leading.offset == 1
                        && self.body.first() == Some(&0x14)
                        && self.body.get(leading_end..trailing.offset)
                            == Some(&[0x00, 0x13, 0x1a]))
                    || (leading.offset == 1
                        && self.body.first() == Some(&0x12)
                        && self.body.get(leading_end..trailing.offset)
                            == Some(&[0x00, 0x11, 0x13]))
                    || (leading.offset == 3
                        && self.body.get(..3) == Some(&[0x00, 0x11, 0x13])
                        && self.body.get(leading_end..trailing.offset) == Some(&[0x14]))
                    || (leading.offset == 5
                        && self.body.get(..2) == Some(&[0xeb, 0xba])
                        && self.body.get(leading_end..trailing.offset) == Some(&[0x12]));
                (controls_match && frame_reaches_body_end(trailing_end)).then_some(())?;
                (
                    [first.value?, second.value?],
                    [
                        [a0.value?, a1.value?, a2.value?],
                        [b0.value?, b1.value?, b2.value?],
                    ],
                )
            }
            [leading, trailing] => {
                let [_, first] = leading.slots.as_slice() else {
                    return None;
                };
                let [second, a0, a1, a2, b0, b1, b2] = trailing.slots.as_slice() else {
                    return None;
                };
                let leading_end = contiguous_slots_end(leading)?;
                let trailing_end = contiguous_slots_end(trailing)?;
                ((leading.offset == 0
                    || (leading.offset == 1 && matches!(self.body.first(), Some(0x19 | 0x32))))
                    && self.body.get(leading_end..trailing.offset) == Some(&[0x12])
                    && frame_reaches_body_end(trailing_end))
                .then_some(())?;
                (
                    [first.value?, second.value?],
                    [
                        [a0.value?, a1.value?, a2.value?],
                        [b0.value?, b1.value?, b2.value?],
                    ],
                )
            }
            _ => return None,
        };
        let diameter = (diameter_endpoints[1] - diameter_endpoints[0]).abs();
        let scale = diameter_endpoints
            .iter()
            .chain(extent_endpoints.iter().flatten())
            .map(|value| value.abs())
            .fold(1.0, f64::max);
        (diameter > EPS_SURFACE_NONZERO * scale).then_some(())?;
        extent_endpoints[0]
            .iter()
            .zip(extent_endpoints[1])
            .any(|(first, second)| {
                ((second - first).abs() - diameter).abs() <= EPS_SURFACE_AGREEMENT * scale
            })
            .then_some(Type24RoundEnvelope {
                diameter,
                extent_endpoints,
            })
    }

    /// Decode the common model-space sweep-direction prefix of a positional
    /// `surface_of_extrusion` body.
    #[must_use]
    pub(crate) fn extrusion_direction(&self) -> Option<[f64; 3]> {
        let kind = self.kind();
        if kind != SurfaceKind::Extrusion(ExtrusionVariant::TabulatedCylinder) {
            return None;
        }
        let direction = self.scalar_frames.first()?;
        let [x, y, z] = direction.slots.as_slice() else {
            return None;
        };
        let direction_end = direction.offset
            + direction
                .slots
                .iter()
                .map(|slot| slot.raw.len())
                .sum::<usize>();
        let separator = self.opaque_spans.first()?;
        if separator.offset != direction_end || !separator.raw.starts_with(&[0x00, 0x0c, 0x9a]) {
            return None;
        }
        let direction = [x.value?, y.value?, z.value?];
        direction
            .iter()
            .all(|value| value.is_finite())
            .then_some(direction)
    }

    /// Decode the positional body used by an unbound straight
    /// `surface_of_extrusion` instance.
    ///
    /// Generic scalar framing covers the original direction/directrix form.
    /// The specialized tabulated-cylinder frame covers the lane-specific
    /// coordinate form; callers must exclude rows owned by a cubic replay
    /// before using this result.
    #[must_use]
    pub(crate) fn line_extrusion_frame(&self) -> Option<LineExtrusionFrame> {
        if self.boundary != SurfaceBodyBoundary::CompoundClose {
            return None;
        }
        let direction_values = self.extrusion_direction()?;
        if let [direction, directrix] = self.scalar_frames.as_slice() {
            let [start_x, start_y, start_z, end_x, end_y, end_z] = directrix.slots.as_slice()
            else {
                return None;
            };
            let directrix_points = [
                [start_x.value?, start_y.value?, start_z.value?],
                [end_x.value?, end_y.value?, end_z.value?],
            ];
            directrix_points
                .as_flattened()
                .iter()
                .all(|value| value.is_finite())
                .then_some(())?;
            let first_gap = self.opaque_spans.first()?;
            if first_gap.offset
                != direction.offset
                    + direction
                        .slots
                        .iter()
                        .map(|slot| slot.raw.len())
                        .sum::<usize>()
                || first_gap.raw != [0x00, 0x0c, 0x9a]
                || directrix.offset != first_gap.offset + first_gap.raw.len()
            {
                return None;
            }
            if self.opaque_spans.len() > 2 {
                return None;
            }
            if let Some(reference) = self.opaque_spans.get(1) {
                let directrix_end = directrix.offset
                    + directrix
                        .slots
                        .iter()
                        .map(|slot| slot.raw.len())
                        .sum::<usize>();
                if reference.offset != directrix_end || reference.raw.first() != Some(&0xf7) {
                    return None;
                }
                let (_, end) = psb::reference_id(&reference.raw, 1).ok()?;
                if end != reference.raw.len() {
                    return None;
                }
            }
            return Some(LineExtrusionFrame {
                direction: direction_values,
                directrix: directrix_points,
            })
            .filter(LineExtrusionFrame::is_valid);
        }

        let frame = self.tabulated_cylinder_frame()?;
        let [start_x, start_y, start_z, end_x, end_y, end_z] = frame.values().get();
        Some(LineExtrusionFrame {
            direction: direction_values,
            directrix: [[start_x, start_y, start_z], [end_x, end_y, end_z]],
        })
        .filter(LineExtrusionFrame::is_valid)
    }
}

fn type24_round_edge_shell_end(body: &[u8], cache: &scalar::ScalarCache) -> Option<usize> {
    match body {
        [0x1b | 0x34, _, 0x00, ..] => Some(3),
        [0x5a, 0xb2, ..] => Some(2),
        [0x39, 0x19 | 0x29, 0x00, ..] => Some(3),
        [0x18, ..] => Some(1),
        [0xeb..=0xed, 0xba, _, _, _, ..] => Some(5),
        [0x32, _, _, _, _, _, _, _, ..] => Some(8),
        [0x19, ..] => scalar::decode_round_edge_coordinate(body, 1, cache).map(|(_, end)| end),
        _ => None,
    }
}

fn type24_round_edge_separator_end(
    body: &[u8],
    offset: usize,
    cache: &scalar::ScalarCache,
) -> Option<usize> {
    if matches!(body.get(offset), Some(0x90 | 0x91)) {
        let (_, end) = scalar::decode_round_edge_coordinate(body, offset, cache)?;
        return (end == offset + 7 && body.get(end) == Some(&0x2d)).then_some(end);
    }
    if matches!(body.get(offset), Some(0x11..=0x14)) {
        return Some(offset + 1);
    }
    if matches!(body.get(offset), Some(0x00 | 0x34)) {
        return body.get(offset + 1..offset + 3).map(|_| offset + 3);
    }
    None
}

/// Structural classification of a plane-row local-system chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalSystemClassification {
    /// Compact byte-characterised local-system form.
    Simple,
    /// Structurally bounded chunk outside the compact form.
    Unclassified,
}

/// Inherited twelve-slot support frame following a plane-row envelope.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlaneLocalSystem {
    /// Owning plane surface identifier.
    pub(crate) surface_id: u32,
    /// Exact bytes between the envelope close and local-system close.
    pub(crate) body: Vec<u8>,
    /// Twelve inherited `f9 04 03` scalar slots; unresolved slots remain `None`.
    pub(crate) slots: [Option<f64>; 12],
    /// Decoded support-frame layout, when the scalar carrier is complete.
    pub(crate) layout: Option<scalar::PlaneSupportFrameLayout>,
    /// Compact versus raw-preserved chunk classification.
    pub(crate) classification: LocalSystemClassification,
    /// Byte offset of the plane row in the original stream.
    pub(crate) row_offset: usize,
    /// Byte offset of the local-system chunk in the original stream.
    pub(crate) offset: usize,
}

impl PlaneLocalSystem {
    pub(crate) fn complete_slots(&self) -> Option<[f64; 12]> {
        let mut slots = [0.0; 12];
        for (value, slot) in slots.iter_mut().zip(self.slots) {
            *value = slot?;
        }
        Some(slots)
    }

    pub(crate) fn frame(&self) -> PlaneFrame {
        match self.layout {
            Some(scalar::PlaneSupportFrameLayout::DirectNormalTriples) => {
                plane_direct_frame(&self.slots)
            }
            Some(scalar::PlaneSupportFrameLayout::MatrixColumns) => plane_matrix_frame(&self.slots),
            _ => plane_frame(&self.slots),
        }
    }
}

/// Return whether a retained plane frame agrees with the strict matrix form.
///
/// The matrix form carries an explicit zero rank column. Its frame is
/// authoritative over envelope coordinates that merely hold one coordinate
/// equal across two stored corners; treating that bound as a second plane
/// equation would reject valid oblique planes.
pub(crate) fn uses_matrix_column_frame(frame: &PlaneLocalSystem) -> bool {
    let matrix = plane_matrix_frame(&frame.slots);
    let frame = frame.frame();
    let Some(matrix_u_axis) = matrix.u_axis else {
        return false;
    };
    let Some(matrix_normal) = matrix.normal else {
        return false;
    };
    let directions_agree = |left: UnitVector3, right: UnitVector3| {
        let left: [f64; 3] = Vector3::from(left).into();
        let right: [f64; 3] = Vector3::from(right).into();
        left.into_iter().zip(right).all(|(left, right)| {
            (left - right).abs() <= EPS_PLANE_FRAME_SCALE * left.abs().max(right.abs()).max(1.0)
        })
    };
    frame.origin == matrix.origin
        && frame
            .u_axis
            .is_some_and(|u_axis| directions_agree(u_axis, matrix_u_axis))
        && frame
            .normal
            .is_some_and(|normal| directions_agree(normal, matrix_normal))
}

/// Plane-specific positional envelope layout.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PlaneEnvelope {
    /// Four 2D bound values followed by two 3D corner triples.
    Standard {
        /// Two parameter-space bound pairs.
        bounds_2d: [[Option<f64>; 2]; 2],
        /// Two model-space corner triples.
        corners_3d: [[Option<f64>; 3]; 2],
    },
    /// `0x0e` variant with three prefix values and two 3D corner triples.
    Compact {
        /// Three envelope-prefix values.
        prefix: [Option<f64>; 3],
        /// Two model-space corner triples.
        corners_3d: [[Option<f64>; 3]; 2],
    },
}

/// Decoded positional envelope for one plane row.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlaneEnvelopeRecord {
    /// Owning plane surface identifier.
    pub(crate) surface_id: u32,
    /// Exact envelope bytes, including a compact-variant marker.
    pub(crate) body: Vec<u8>,
    /// Plane-specific envelope layout.
    pub(crate) envelope: PlaneEnvelope,
    /// Per-coordinate equality of the two stored model-space corners. `None`
    /// means equality cannot be decided from the scalar token pair.
    pub(crate) corner_coordinate_equal: [Option<bool>; 3],
    /// Exact token bytes for each declared envelope scalar slot.
    pub(crate) scalar_tokens: Vec<Vec<u8>>,
    /// Byte offset of the plane row in the original stream.
    pub(crate) row_offset: usize,
    /// Byte offset of the envelope body in the original stream.
    pub(crate) offset: usize,
}

/// Axis-aligned model-space plane with one held coordinate.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OutlinePlane {
    /// Owning `srf_array` surface identifier.
    pub(crate) surface_id: u32,
    /// Model-space plane origin with only the held coordinate populated.
    pub(crate) origin: [f64; 3],
    /// Unit model-space normal of the held coordinate.
    pub(crate) normal: UnitVector3,
    /// Unit in-plane direction for carrier constructions.
    /// The outline does not define the surface parameter chart.
    pub(crate) u_axis: UnitVector3,
    /// Byte offset of the outline body.
    pub(crate) offset: usize,
}

impl cadmpeg_core::decode::cost::DecodeCost for OutlinePlane {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(
                &self.surface_id,
                &self.origin,
                self.normal(),
                self.u_axis(),
                &self.offset,
            ),
            ctx,
            operation,
        )
    }
}

impl OutlinePlane {
    pub(crate) fn normal(&self) -> [f64; 3] {
        Vector3::from(self.normal).into()
    }

    pub(crate) fn u_axis(&self) -> [f64; 3] {
        Vector3::from(self.u_axis).into()
    }
}

/// Return the outline plane for `surface_id` only when exactly one exists.

/// Derive axis-aligned plane equations from complete, non-degenerate outline
/// corner pairs. Ambiguous pairs with zero or multiple held axes are withheld.
fn outline_planes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    envelopes: &[PlaneEnvelopeRecord],
) -> Result<Vec<OutlinePlane>, cadmpeg_core::CodecError> {
    let mut result = Vec::new();
    for record in ctx.admit_iter(envelopes, "creo outline envelope traversal")? {
        let corners = match &record.envelope {
            PlaneEnvelope::Standard { corners_3d, .. }
            | PlaneEnvelope::Compact { corners_3d, .. } => corners_3d,
        };
        let mut held = (0..3).filter(|axis| record.corner_coordinate_equal[*axis] == Some(true));
        let Some(axis) = held.next() else {
            continue;
        };
        if held.next().is_some()
            || record
                .corner_coordinate_equal
                .iter()
                .enumerate()
                .any(|(candidate, equal)| candidate != axis && *equal != Some(false))
        {
            continue;
        }
        let Some(coordinate) = corners[0][axis] else {
            continue;
        };
        let mut origin = [0.0; 3];
        origin[axis] = coordinate;
        let normal = [
            UnitVector3::X_AXIS,
            UnitVector3::Y_AXIS,
            UnitVector3::Z_AXIS,
        ][axis];
        let u_axis = if axis == 0 {
            UnitVector3::Y_AXIS
        } else {
            UnitVector3::X_AXIS
        };
        ctx.reserve_vec(&mut result, 1, "creo held-coordinate outline planes")?;
        result.push(OutlinePlane {
            surface_id: record.surface_id,
            origin,
            normal,
            u_axis,
            offset: record.offset,
        });
    }
    ctx.stable_sort_by(
        result.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo outline planes result ordering",
    )?;
    Ok(result)
}

/// Derive axis-aligned plane equations from complete positional corner frames
/// owned by uniquely identified plane rows.
fn auxiliary_plane_frame(
    record: &SurfaceParameterRecord,
) -> Option<(usize, &[SurfaceParameterScalar])> {
    let [leading, terminal] = record.scalar_frames.as_slice() else {
        return None;
    };
    let [leading_slot] = leading.slots.as_slice() else {
        return None;
    };
    let [_, corners @ ..] = terminal.slots.as_slice() else {
        return None;
    };
    if corners.len() != 6 {
        return None;
    }
    let leading_end = leading_slot.offset.checked_add(leading_slot.raw.len())?;
    let terminal_end = terminal
        .slots
        .iter()
        .try_fold(terminal.offset, |cursor, slot| {
            (slot.offset == cursor).then(|| cursor + slot.raw.len())
        })?;
    (leading.offset == 3
        && leading_end == 10
        && terminal.offset == 18
        && terminal_end == record.body.len()
        && record.opaque_spans.len() == 2
        && record.opaque_spans[0].offset == 0
        && record.opaque_spans[0].raw.len() == 3
        && record.opaque_spans[1].offset == 10
        && record.opaque_spans[1].raw.len() == 8)
        .then(|| (corners[0].offset, corners))
}
fn suffixed_auxiliary_plane_frame(
    record: &SurfaceParameterRecord,
) -> Option<(usize, &[SurfaceParameterScalar])> {
    let frame_end = record.body.len().checked_sub(2)?;
    let terminal = record.scalar_frames.last()?;
    ((7..=10).contains(&terminal.slots.len())
        && terminal
            .slots
            .last()
            .is_some_and(|slot| slot.offset.checked_add(slot.raw.len()) == Some(frame_end)))
    .then_some(())?;
    record.body.ends_with(&[0xf7, 0x0c]).then_some(())?;
    let corners = &terminal.slots[terminal.slots.len() - 6..];
    Some((corners[0].offset, corners))
}
fn terminal_plane_corner_frame(
    record: &SurfaceParameterRecord,
) -> Option<(usize, &[SurfaceParameterScalar])> {
    let frame_end = record.body.len().checked_sub(2)?;
    record.body.ends_with(&[0xf7, 0x1f]).then_some(())?;
    let [terminal] = record.scalar_frames.as_slice() else {
        return None;
    };
    ((6..=10).contains(&terminal.slots.len())
        && terminal
            .slots
            .last()
            .is_some_and(|slot| slot.offset.checked_add(slot.raw.len()) == Some(frame_end)))
    .then_some(())?;
    let corners = &terminal.slots[terminal.slots.len() - 6..];
    Some((corners[0].offset, corners))
}
fn split_terminal_plane_corner_frame(
    record: &SurfaceParameterRecord,
) -> Option<(usize, &[SurfaceParameterScalar])> {
    let frame_end = record.body.len().checked_sub(2)?;
    record.body.ends_with(&[0xf7, 0x1f]).then_some(())?;
    let [leading, terminal] = record.scalar_frames.as_slice() else {
        return None;
    };
    ((1..=2).contains(&leading.slots.len()) && terminal.slots.len() == 8).then_some(())?;
    let leading_end = leading
        .slots
        .iter()
        .try_fold(leading.offset, |cursor, slot| {
            (slot.offset == cursor).then(|| cursor + slot.raw.len())
        })?;
    let terminal_end = terminal
        .slots
        .iter()
        .try_fold(terminal.offset, |cursor, slot| {
            (slot.offset == cursor).then(|| cursor + slot.raw.len())
        })?;
    let [prefix, controls, trailer] = record.opaque_spans.as_slice() else {
        return None;
    };
    (prefix.offset == 0
        && prefix.raw.len() == leading.offset
        && controls.offset == leading_end
        && controls.raw.len() == terminal.offset.checked_sub(leading_end)?
        && trailer.offset == frame_end
        && trailer.raw.len() == 2
        && terminal_end == frame_end)
        .then_some(())?;
    let corners = &terminal.slots[2..];
    Some((corners[0].offset, corners))
}

pub(crate) fn positional_frame_planes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    parameters: &crate::surface::SurfaceParameters,
    rows: &crate::surface::SurfaceRows,
) -> Result<Vec<OutlinePlane>, cadmpeg_core::CodecError> {
    let mut result = Vec::new();
    for record in ctx.admit_iter(&**parameters, "creo positional frame plane parameters")? {
        if record.boundary != SurfaceBodyBoundary::CompoundClose
            || unique_surface_parameter(parameters, record.surface_id)
                .is_none_or(|unique| !std::ptr::eq(unique, record))
            || unique_surface_row(rows, record.surface_id)
                .is_none_or(|row| row.kind != SurfaceKind::Plane)
        {
            continue;
        }
        let marked_frames = ctx
            .admit_iter(&record.scalar_frames, "creo positional plane scalar frames")?
            .filter_map(|frame| {
                (frame.slots.len() == 6
                    && frame.offset >= 3
                    && record.body.get(frame.offset - 3..frame.offset) == Some(&[0x00, 0x0c, 0x9a]))
                .then_some((frame.offset, frame.slots.as_slice()))
            });
        let auxiliary_frame = auxiliary_plane_frame(record);
        let suffixed_auxiliary_frame = suffixed_auxiliary_plane_frame(record);
        let terminal_corner_frame = terminal_plane_corner_frame(record);
        let split_terminal_corner_frame = split_terminal_plane_corner_frame(record);
        let mut selected: Option<OutlinePlane> = None;
        let mut conflicting = false;
        for (offset, slots) in marked_frames
            .chain(auxiliary_frame)
            .chain(suffixed_auxiliary_frame)
            .chain(terminal_corner_frame)
            .chain(split_terminal_corner_frame)
        {
            let Some(values) = six_finite_scalar_values(slots) else {
                continue;
            };
            let scale = values.iter().map(|value| value.abs()).fold(1.0, f64::max);
            let equal = std::array::from_fn::<_, 3, _>(|axis| {
                (values[axis] - values[axis + 3]).abs() <= EPS_SURFACE_AGREEMENT * scale
            });
            let mut held = equal
                .iter()
                .enumerate()
                .filter_map(|(axis, equal)| equal.then_some(axis));
            let Some(axis) = held.next() else {
                continue;
            };
            if held.next().is_some() {
                continue;
            }
            let mut origin = [0.0; 3];
            origin[axis] = values[axis];
            let normal = [
                UnitVector3::X_AXIS,
                UnitVector3::Y_AXIS,
                UnitVector3::Z_AXIS,
            ][axis];
            let u_axis = if axis == 0 {
                UnitVector3::Y_AXIS
            } else {
                UnitVector3::X_AXIS
            };
            let candidate = OutlinePlane {
                surface_id: record.surface_id,
                origin,
                normal,
                u_axis,
                offset: record.body_offset + offset,
            };
            if let Some(previous) = &selected {
                conflicting |= previous.origin != candidate.origin
                    || previous.normal != candidate.normal
                    || previous.u_axis != candidate.u_axis;
            } else {
                selected = Some(candidate);
            }
        }
        if !conflicting {
            if let Some(candidate) = selected {
                ctx.reserve_vec(&mut result, 1, "creo positional frame planes")?;
                result.push(candidate);
            }
        }
    }
    ctx.stable_sort_by(
        result.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo positional frame planes result ordering",
    )?;
    Ok(result)
}

/// Place axis-aligned plane outlines whose support frame selects one proven
/// held coordinate even when other outline-coordinate relations are unresolved.
#[cfg(test)]
pub(crate) fn frame_bound_outline_plane_checked(
    ctx: &DecodeContext<'_>,
    record: &PlaneEnvelopeRecord,
    frames: &[PlaneLocalSystem],
) -> Result<Option<OutlinePlane>, CodecError> {
    let mut source = frames.iter();
    let mut selected = None;
    while source.len() != 0 {
let Some(frame) = ctx.next_charged(&mut source, "creo outline support frame lookup")? else { break; };
        if frame.surface_id != record.surface_id {
            continue;
        }
        let frame = frame.frame();
        let (Some(normal), Some(u_axis)) = (frame.normal, frame.u_axis) else {
            continue;
        };
        if let Some((previous_normal, previous_u_axis)) = selected {
            if !outline_directions_agree(previous_normal, normal)
                || !outline_directions_agree(previous_u_axis, u_axis)
            {
                return Ok(None);
            }
        } else {
            selected = Some((normal, u_axis));
        }
    }
    let Some((normal, u_axis)) = selected else {
        return Ok(None);
    };
    Ok(frame_bound_outline_plane_with_directions(
        record, normal, u_axis,
    ))
}

fn outline_directions_agree(first: UnitVector3, second: UnitVector3) -> bool {
    let first: [f64; 3] = Vector3::from(first).into();
    let second: [f64; 3] = Vector3::from(second).into();
    first.iter().zip(second).all(|(first, second)| {
        (first - second).abs() <= EPS_FRAME_AGREEMENT * first.abs().max(second.abs()).max(1.0)
    })
}

fn frame_bound_outline_plane_with_directions(
    record: &PlaneEnvelopeRecord,
    normal: UnitVector3,
    u_axis: UnitVector3,
) -> Option<OutlinePlane> {
    let normal_components: [f64; 3] = Vector3::from(normal).into();
    let mut axes = normal_components
        .iter()
        .enumerate()
        .filter_map(|(axis, value)| (value.abs() > EPS_AXIS_COMPONENT_NONZERO).then_some(axis));
    let axis = axes.next()?;
    if axes.next().is_some() {
        return None;
    }
    let shortened_held_coordinate = record.scalar_tokens.len() == 10
        && record.scalar_tokens[..8]
            .iter()
            .all(|token| !token.is_empty())
        && record.scalar_tokens[8..].iter().all(Vec::is_empty)
        && !record.scalar_tokens[4 + axis].is_empty()
        && record.scalar_tokens[4 + axis] == record.scalar_tokens[7];
    if record.corner_coordinate_equal[axis] != Some(true) && !shortened_held_coordinate {
        return None;
    }
    let corners = match &record.envelope {
        PlaneEnvelope::Standard { corners_3d, .. } | PlaneEnvelope::Compact { corners_3d, .. } => {
            corners_3d
        }
    };
    let coordinate = corners[0][axis]?;
    let mut origin = [0.0; 3];
    origin[axis] = coordinate;
    Some(OutlinePlane {
        surface_id: record.surface_id,
        origin,
        normal,
        u_axis,
        offset: record.offset,
    })
}

#[cfg(test)]
pub(crate) fn frame_bound_outline_plane(
    record: &PlaneEnvelopeRecord,
    frames: &[PlaneLocalSystem],
) -> Option<OutlinePlane> {
    crate::decode::with_test_decode_ctx(|ctx| {
        frame_bound_outline_plane_checked(ctx, record, frames)
    })
    .expect("outline fixture admission")
}

/// Derive outline plane equations and retain complete support-frame directions
/// for carrier constructions when available.
pub(crate) fn placed_outline_planes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    envelopes: &[PlaneEnvelopeRecord],
    frames: &[PlaneLocalSystem],
) -> Result<Vec<OutlinePlane>, cadmpeg_core::CodecError> {
    if envelopes.is_empty() {
        return Ok(Vec::new());
    }
    let mut scratch = ctx.reserve_scoped(0, "creo placed outline scratch")?;
    let mut directions = std::collections::HashMap::new();
    let mut matrix_frame_ids = std::collections::HashSet::new();
    for frame in ctx.admit_iter(frames, "creo outline support frame indexing")? {
        if uses_matrix_column_frame(frame) {
            scratch.with_storage(|| {
                ctx.insert_hash_set(
                    &mut matrix_frame_ids,
                    frame.surface_id,
                    "creo matrix frame ID nodes",
                )
            })?;
        }
        let geometry = frame.frame();
        let (Some(normal), Some(u_axis)) = (geometry.normal, geometry.u_axis) else {
            continue;
        };
        match scratch.with_storage(|| {
            ctx.entry_hash_map(
                &mut directions,
                frame.surface_id,
                "creo outline support direction nodes",
            )
        })? {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(Some((normal, u_axis)));
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                if entry
                    .get()
                    .is_none_or(|(previous_normal, previous_u_axis)| {
                        !outline_directions_agree(previous_normal, normal)
                            || !outline_directions_agree(previous_u_axis, u_axis)
                    })
                {
                    entry.insert(None);
                }
            }
        }
    }
    let mut frame_bound = Vec::new();
    let mut frame_bound_ids = std::collections::HashSet::new();
    for record in ctx.admit_iter(envelopes, "creo outline envelope traversal")? {
        let Some(&(normal, u_axis)) = directions.get(&record.surface_id).and_then(Option::as_ref)
        else {
            continue;
        };
        if let Some(plane) = frame_bound_outline_plane_with_directions(record, normal, u_axis) {
            scratch.with_storage(|| {
                ctx.reserve_vec(&mut frame_bound, 1, "creo frame-bound outline planes")
            })?;
            scratch.with_storage(|| {
                ctx.insert_hash_set(
                    &mut frame_bound_ids,
                    record.surface_id,
                    "creo frame-bound outline ID nodes",
                )
            })?;
            frame_bound.push(plane);
        }
    }
    let mut result = outline_planes(ctx, envelopes)?;
    ctx.retain_vec(
        &mut result,
        |plane| {
            Ok(!frame_bound_ids.contains(&plane.surface_id)
                && !matrix_frame_ids.contains(&plane.surface_id))
        },
        "creo placed outline retain",
    )?;
    for plane in ctx.admit_iter(frame_bound, "creo frame-bound outline projection")? {
        ctx.reserve_vec(&mut result, 1, "creo placed outline planes")?;
        result.push(plane);
    }
    ctx.stable_sort_by(
        result.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo placed outline planes result ordering",
    )?;
    Ok(result)
}

const BOUNDARY_TYPES: &[BoundaryType] = &[
    BoundaryType::Code00,
    BoundaryType::Code01,
    BoundaryType::Code06,
    BoundaryType::Code08,
    BoundaryType::CodeF6,
];

#[derive(Debug, Clone, Copy)]
struct SurfaceArrayFrame {
    start: usize,
    end: usize,
    count: usize,
}

fn surface_array_frames<'a, 'ctx>(
    ctx: &'a DecodeContext<'ctx>,
    payload: &'a [u8],
) -> impl Iterator<Item = Result<SurfaceArrayFrame, CodecError>> + use<'a, 'ctx> {
    let mut search = 0;
    let mut finished = false;
    std::iter::from_fn(move || {
        if finished {
            return None;
        }
        let frame = next_surface_array_frame(ctx, payload, &mut search);
        if !matches!(frame, Ok(Some(_))) {
            finished = true;
        }
        frame.transpose()
    })
}

fn next_surface_array_frame(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    search: &mut usize,
) -> Result<Option<SurfaceArrayFrame>, CodecError> {
    const LABEL: &[u8] = b"srf_array\0";
    loop {
        let Some(label) = ctx.find_map(
            payload
                .get(*search..)
                .unwrap_or_default()
                .windows(LABEL.len())
                .enumerate(),
            |(offset, bytes)| Ok((bytes == LABEL).then_some(*search + offset)),
            "find Creo surface array",
        )?
        else {
            return Ok(None);
        };
        let start = label + LABEL.len();
        *search = start;
        if payload.get(start) != Some(&psb::token::ARRAY_OPEN) {
            continue;
        }
        let (count, after_count) = compact_int(payload, start + 1);
        if after_count == start + 1 {
            continue;
        }
        // The declared count is a slot extent every reader compares against a
        // `usize` length, so a count this target cannot address states no
        // frame.
        let Ok(count) = usize::try_from(count) else {
            continue;
        };
        const TERMINATORS: [&[u8]; 3] = [b"crv_array\0", b"lo_array\0", b"qlt_array\0"];
        let end = ctx.find_map(
            start..payload.len(),
            |offset| Ok((payload[offset..].starts_with(LABEL)
                || (offset >= after_count && TERMINATORS.iter().any(|label| payload[offset..].starts_with(label))))
                .then_some(offset)),
            "find Creo surface array boundary",
        )?.unwrap_or(payload.len());
        *search = end;
        return Ok(Some(SurfaceArrayFrame {
            start: after_count,
            end,
            count,
        }));
    }
}

fn rows_in_frame<'a>(
    ctx: &DecodeContext<'_>,
    rows: &'a [SurfaceRow],
    frame: SurfaceArrayFrame,
    operation: &'static str,
) -> Result<&'a [SurfaceRow], CodecError> {
    let start = ctx.partition_point(rows, |row| Ok(row.offset < frame.start), operation)?;
    let end = ctx.partition_point(&rows[start..], |row| Ok(row.offset < frame.end), operation)?;
    Ok(&rows[start..start + end])
}

/// Discover positional rows from every `srf_array` namespace in `payload`.
/// The scan anchors on the surface-kind byte and validates both adjacent
/// compact-integer fields and the orientation/boundary discriminators. This
/// retains only byte-backed rows; a link target never inherits a kind. A
/// counted frame can contain unmaterialized slots, so validated rows are
/// retained even when their count does not equal the frame count. Consumers
/// that require a complete frame use [`counted_row_bounds`] or
/// [`complete_surface_array_bounds`].
pub(crate) fn rows(ctx: &DecodeContext<'_>, payload: &[u8]) -> Result<Vec<SurfaceRow>, CodecError> {
    rows_with_boundaries(ctx, payload, BOUNDARY_TYPES)
}

/// Discover rows and their containing frame bounds from complete counted
/// `srf_array` frames.
pub(crate) fn counted_row_bounds(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<(SurfaceRow, usize)>, CodecError> {
    let mut frames = surface_array_frames(ctx, payload);
    let Some(first) = frames.next().transpose()? else {
        return Ok(Vec::new());
    };
    let mut scratch = ctx.reserve_scoped(0, "creo counted row scratch")?;
    let candidates = scratch.with_storage(|| rows(ctx, payload))?;
    let mut result = Vec::new();
    for frame in std::iter::once(Ok(first)).chain(frames) {
        let frame = frame?;
        let selected = rows_in_frame(ctx, &candidates, frame, "creo counted surface row count")?;
        if selected.len() == frame.count {
            for row in ctx.admit_iter(selected, "creo counted surface row projection")? {
                ctx.reserve_vec(&mut result, 1, "creo counted surface row bounds")?;
                result.push((row.clone(), frame.end));
            }
        }
    }
    ctx.stable_sort_by(
        result.as_mut_slice(),
        |value| &value.0.offset,
        Ord::cmp,
        "creo counted row bounds result ordering",
    )?;
    Ok(result)
}

/// Return the byte bounds of complete counted `srf_array` frames.
///
/// A named prototype and its positional rows share one frame. Incomplete
/// frames are excluded so a prototype cannot join to a row in a neighboring
/// frame through section-wide adjacency.
pub(crate) fn complete_surface_array_bounds(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<(usize, usize)>, CodecError> {
    let mut frames = surface_array_frames(ctx, payload);
    let Some(first) = frames.next().transpose()? else {
        return Ok(Vec::new());
    };
    let mut scratch = ctx.reserve_scoped(0, "creo complete row scratch")?;
    let rows = scratch.with_storage(|| rows(ctx, payload))?;
    let mut bounds = Vec::new();
    for frame in std::iter::once(Ok(first)).chain(frames) {
        let frame = frame?;
        if frame.count != 0
            && rows_in_frame(ctx, &rows, frame, "creo complete surface row count")?.len()
                == frame.count
        {
            ctx.reserve_vec(&mut bounds, 1, "creo complete surface array bounds")?;
            bounds.push((frame.start, frame.end));
        }
    }
    Ok(bounds)
}

/// Discover rows from a DEPDB `Sld_Xsections` surface namespace.
/// Named prototype rows use boundary type `00`; positional replays use `06`.
pub(crate) fn cross_section_rows(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<SurfaceRow>, CodecError> {
    rows_with_boundaries(ctx, payload, &[BoundaryType::Code00, BoundaryType::Code06])
}

fn rows_with_boundaries(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    boundary_types: &[BoundaryType],
) -> Result<Vec<SurfaceRow>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo surface row scratch")?;
    let candidates =
        scratch.with_storage(|| row_candidates_with_boundaries(ctx, payload, boundary_types))?;
    scratch.commit_value(candidates)
}

const NAMED_ROW_FIELDS: [&[u8]; 6] = [
    b"geom_id\0", b"geom_type\0", b"feat_id\0", b"next_geom_ptr\0",
    b"orient\0", b"boundary_type\0",
];

/// Find each first named-row field and the next namespace in one source walk.
fn named_row_fields(
    ctx: &DecodeContext<'_>, payload: &[u8], start: usize,
) -> Result<([Option<usize>; 6], usize), CodecError> {
    let mut fields = [None; 6];
    let mut offsets = start..payload.len();
    while !offsets.is_empty() {
        let Some(offset) = ctx.next_charged(&mut offsets, "find Creo surface marker")? else { break; };
        let tail = &payload[offset..];
        if tail.starts_with(b"srf_array\0") { return Ok((fields, offset)); }
        for (field, label) in fields.iter_mut().zip(NAMED_ROW_FIELDS) {
            if field.is_none() && tail.starts_with(label) { *field = Some(offset); }
        }
    }
    Ok((fields, payload.len()))
}

fn row_candidates_with_boundaries(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    boundary_types: &[BoundaryType],
) -> Result<Vec<SurfaceRow>, CodecError> {
    let mut result_scope = ctx.reserve_scoped(0, "creo retained surface rows")?;
    let mut result = Vec::new();
    let mut namespace_start = 0;
    while let Some(array) = ctx.find_map(
        payload
            .get(namespace_start..)
            .unwrap_or_default()
            .windows(b"srf_array\0".len())
            .enumerate(),
        |(offset, bytes)| Ok((bytes == b"srf_array\0").then_some(namespace_start + offset)),
        "find Creo surface marker",
    )? {
        let start = array + b"srf_array\0".len();
        let (fields, end) = named_row_fields(ctx, payload, start)?;
        namespace_start = end;
        let value = |field: usize| {
            let at = fields[field]?;
            let value_start = at + NAMED_ROW_FIELDS[field].len();
            let (value, after) = compact_int(payload, value_start);
            (after > value_start).then_some((value, at))
        };
        let typed_kind = fields[1]
            .and_then(|at| payload.get(at + NAMED_ROW_FIELDS[1].len()))
            .and_then(|byte| SurfaceKind::from_byte(*byte));
        if let (Some((id, id_offset)), Some(kind), Some((feature_id, _)), Some((next_surface, _))) = (
            value(0), typed_kind, value(2), value(3),
        ) {
            let Some(orientation) = fields[4]
                .and_then(|at| payload.get(at + NAMED_ROW_FIELDS[4].len()))
                .copied().filter(|byte| matches!(byte, 0x01 | 0xf6)) else { continue; };
            let Some(boundary_type) = fields[5]
                .and_then(|at| payload.get(at + NAMED_ROW_FIELDS[5].len()))
                .copied().and_then(BoundaryType::from_byte) else { continue; };
            result_scope.with_storage(|| ctx.reserve_vec(&mut result, 1, "creo surface rows"))?;
            result.push(SurfaceRow {
                id,
                kind,
                feature_id,
                reversed: orientation == 0xf6,
                boundary_type,
                next_surface,
                offset: id_offset,
            });
        }
    }
    for type_offset in ctx.admit_iter(1..payload.len(), "creo positional surface row discovery")? {
        let Some(kind) = SurfaceKind::from_byte(payload[type_offset]) else {
            continue;
        };
        let Some((id, id_start)) = id_ending_at(payload, type_offset) else {
            continue;
        };
        let mut pos = type_offset + 1;
        let (feature_id, next) = compact_int(payload, pos);
        if next == pos || next > payload.len() {
            continue;
        }
        pos = next;
        let Some(&orientation) = payload.get(pos) else {
            continue;
        };
        if !matches!(orientation, 0x01 | 0xf6) {
            continue;
        }
        let Some(&boundary_type) = payload.get(pos + 1) else {
            continue;
        };
        let Some(boundary_type) = BoundaryType::from_byte(boundary_type) else {
            continue;
        };
        pos += 2;
        let (next_surface, end) = compact_int(payload, pos);
        if end == pos {
            continue;
        }
        result_scope.with_storage(|| ctx.reserve_vec(&mut result, 1, "creo surface rows"))?;
        result.push(SurfaceRow {
            id,
            kind,
            feature_id,
            reversed: orientation == 0xf6,
            boundary_type,
            next_surface,
            offset: id_start,
        });
    }
    ctx.stable_sort_by(
        result.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo rows with boundaries result ordering",
    )?;
    ctx.dedup_by_key(
        &mut result,
        |row| Ok(row.offset),
        "creo rows with boundaries result deduplication",
    )?;
    let mut scratch = ctx.reserve_scoped(0, "creo surface row scratch")?;
    let mut id_counts = std::collections::HashMap::<u32, usize>::new();
    for row in ctx.admit_iter(&result, "creo surface row identities")? {
        *scratch
            .with_storage(|| {
                ctx.entry_hash_map(&mut id_counts, row.id, "creo surface row ID nodes")
            })?
            .or_insert(0) += 1;
    }
    ctx.retain_vec(
        &mut result,
        |row| Ok(id_counts.get(&row.id) == Some(&1)),
        "creo unique surface row retain",
    )?;
    let mut prototype_parameter_spans = Vec::new();
    let cache = scalar::ScalarCache::from_section_checked(ctx, payload)?;
    let prototype_frames = scratch.with_storage(|| named_prototype_frames(ctx, payload, &cache))?;
    for frame in ctx.admit_iter(prototype_frames, "creo prototype row frames")? {
        for parameter in ctx.admit_iter(frame.parameters, "creo prototype row parameters")? {
            scratch.with_storage(|| {
                ctx.reserve_vec(
                    &mut prototype_parameter_spans,
                    1,
                    "creo prototype parameter spans",
                )
            })?;
            prototype_parameter_spans.push((parameter.value_offset, parameter.value_end));
        }
    }
    ctx.retain_vec(
        &mut result,
        |row| {
            Ok(!ctx.any_by(
                &prototype_parameter_spans,
                |(start, end)| Ok(row.offset >= *start && row.offset < *end),
                "creo prototype row span lookup",
            )?)
        },
        "creo prototype surface row retain",
    )?;
    ctx.retain_vec(
        &mut result,
        |row| Ok(boundary_types.contains(&row.boundary_type)),
        "creo boundary surface row retain",
    )?;
    let mut frames = surface_array_frames(ctx, payload);
    if let Some(first) = frames.next().transpose()? {
        let mut frame_scope = ctx.reserve_scoped(0, "creo retained surface rows")?;
        let mut framed = Vec::new();
        let mut saw_framed_candidate = false;
        for frame in std::iter::once(Ok(first)).chain(frames) {
            let frame = frame?;
            let selected = rows_in_frame(ctx, &result, frame, "creo framed surface row count")?;
            let selected_count = selected.len();
            saw_framed_candidate |= selected_count != 0;
            // The count is the frame's slot extent. Releases can leave slots
            // without a materialized fixed-prefix header; retaining the
            // validated headers preserves the rows that are present without
            // treating an incomplete frame as complete. More headers than
            // slots is structurally invalid, so withhold that frame. Exact-
            // count callers are kept separate in `counted_row_bounds` and
            // `complete_surface_array_bounds`.
            if selected_count <= frame.count {
                for row in ctx.admit_iter(selected, "creo framed surface row projection")? {
                    frame_scope.with_storage(|| ctx.reserve_vec(&mut framed, 1, "creo framed surface rows"))?;
                    framed.push(row.clone());
                }
            }
        }
        if saw_framed_candidate {
            drop(result);
            drop(result_scope);
            return frame_scope.commit_value(framed);
        }
    }
    result_scope.commit_value(result)
}

const PROTOTYPE_PARAMETER_NAMES: &[&str] = &[
    "local_sys",
    "radius",
    "radius1",
    "radius2",
    "half_angle",
    "i_pnts",
    "i_points",
    "c_pnts",
    "tangts",
    "end_tangts",
    "end_u_tangts",
    "end_v_tangts",
    "end_uv_deriv",
    "u_params",
    "v_params",
    "params",
    "ctr_spline",
    "tan_spline",
    "par_v_0",
    "par_v_1",
    "offset_type",
    "parent_feats",
    "frst_cntr_ptr",
    "frst_cntr_crv_hdr_ptr",
    "trv",
    "envlp",
    "outline",
    "next_cntr_ptr",
    "srf_flip_dat",
    "id",
    "type",
    "flip",
    "tan_cond",
    "degree",
    "dum_array",
    "data_dbls",
    "data_type",
];

fn prototype_parameter_allowed(family: &SurfacePrototypeFamily, name: &str) -> bool {
    PROTOTYPE_PARAMETER_NAMES.contains(&name)
        && !(matches!(family, SurfacePrototypeFamily::Torus(_))
            && matches!(name, "i_pnts" | "i_points" | "c_pnts"))
}

/// The decoded value of one named prototype field, or the field's bytes when
/// no form states it.
///
/// A bounded scalar body that the decoder refuses states why. The reason names
/// the record, the field, the declared slot count and the slot and byte the
/// refusal stands at, and it reaches `refusals` so the refusal names its
/// instance instead of leaving only an opaque body behind.
fn named_surface_value(
    ctx: &DecodeContext<'_>,
    family: &SurfacePrototypeFamily,
    name: &str,
    body: &[u8],
    cache: &scalar::ScalarCache,
    record: &dyn std::fmt::Display,
    refusals: &mut crate::lane_refusal::LaneRefusals,
) -> Result<SurfaceNamedValue, CodecError> {
    let mut refusal = ScalarBodyRefusal::default();
    let mut grid = None;
    if body.first() == Some(&psb::token::SCALAR_BODY) {
        let (dimensions, dimensions_end) = compact_int(body, 1);
        let (count, values_start) = compact_int(body, dimensions_end);
        let slot_count = usize::try_from(dimensions).ok().and_then(|dimensions| {
            usize::try_from(count)
                .ok()
                .and_then(|count| dimensions.checked_mul(count))
        });
        if slot_count.is_some_and(|slot_count| {
            crate::scalar::admitted_scalar_body(body, dimensions_end, values_start, slot_count)
                .is_some()
        }) {
            grid = arrays::DimensionedScalars::extent(dimensions, count);
        }
    }
    let mut scratch = ctx.reserve_scoped(0, "creo named parameter scratch")?;
    if let Some(value) = scratch.with_storage(|| {
        parsed_named_surface_value(ctx, family, name, body, cache, &mut refusal, grid).transpose()
    })? {
        return value.copy_retained(ctx);
    }
    if let Some(reason) = refusal.reason() {
        refusals.note_checked(ctx, record, &format_args!("named field `{name}` {reason}"));
    }
    Ok(SurfaceNamedValue::Opaque(ctx.copy_retained(
        body,
        "creo opaque surface parameter bytes",
    )?))
}

fn parsed_named_surface_value(
    ctx: &DecodeContext<'_>,
    family: &SurfacePrototypeFamily,
    name: &str,
    body: &[u8],
    cache: &scalar::ScalarCache,
    refusal: &mut ScalarBodyRefusal,
    grid: Option<arrays::ScalarExtent<[u32; 2]>>,
) -> Option<Result<SurfaceNamedValue, CodecError>> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Some(Err(refusal.into()));
    }
    if body.is_empty() {
        return Some(Ok(SurfaceNamedValue::Empty));
    }
    let radius_field = matches!(name, "radius" | "radius1" | "radius2");
    let parameter_bound_field = matches!(name, "par_v_0" | "par_v_1");
    let scalar_field = radius_field || parameter_bound_field || name == "half_angle";
    let compact_integer_field = matches!(
        name,
        "id" | "type"
            | "tan_cond"
            | "degree"
            | "frst_cntr_ptr"
            | "frst_cntr_crv_hdr_ptr"
            | "trv"
            | "next_cntr_ptr"
            | "data_type"
    );
    if scalar_field && body == [0x18] {
        return Some(
            ctx.alloc_filled(1, 0.0, "creo zero surface scalar")
                .map(SurfaceNamedValue::ScalarSequence),
        );
    }
    if name == "flip" {
        if body.first() == Some(&0xf1) {
            let (value, end) = compact_int(body, 1);
            if end > 1 && end == body.len() {
                return Some(Ok(SurfaceNamedValue::CompactInt(value)));
            }
        }
        return None;
    }
    if name == "offset_type" {
        let (value, separator) = compact_int(body, 0);
        if let Some(reference_start) = separator.checked_add(2) {
            if separator > 0
                && body
                    .get(separator..)
                    .is_some_and(|rest| rest.starts_with(&[0xf1, psb::token::ENTITY_REF]))
            {
                let (reference, end) = psb::reference_id(body, reference_start).ok()?;
                if reference != 0 && end == body.len() {
                    return Some(Ok(SurfaceNamedValue::CompactInt(value)));
                }
            }
        }
        return None;
    }
    if body.first() == Some(&psb::token::ARRAY_OPEN) {
        let (count, mut cursor) = compact_int(body, 1);
        if cursor > 1 {
            let values_start = cursor;
            if body.get(cursor) == Some(&psb::token::ENTITY_REF) {
                if let Ok((start_id, next)) = psb::reference_id(body, cursor + 1) {
                    if body.get(next) == Some(&psb::token::ARRAY_CLOSE) {
                        if let Some(end_id) = start_id.checked_add(count) {
                            let mut references = Vec::new();
                            let count = usize::try_from(count).ok()?;
                            if let Err(error) = ctx.reserve_vec(
                                &mut references,
                                count,
                                "creo contiguous surface references",
                            ) {
                                return Some(Err(error));
                            }
                            let references_source = match ctx.admit_iter(
                                start_id..end_id,
                                "creo contiguous surface reference values",
                            ) {
                                Ok(source) => source,
                                Err(error) => return Some(Err(error.into())),
                            };
                            references.extend(references_source);
                            return Some(Ok(SurfaceNamedValue::ContiguousEntityReferences(
                                references,
                            )));
                        }
                    }
                }
            }
            if matches!(name, "u_params" | "v_params") {
                // Each declared slot is at least a one-byte scalar token in the
                // value bytes, so the count cannot exceed the remaining bytes.
                let remaining = body.get(values_start..)?;
                bounded_len(u64::from(count), 1, remaining.len())?;
                let extent = arrays::CountedScalars::extent(count)?;
                let slots = match named_spline_scalar_slots(
                    ctx,
                    family,
                    name,
                    remaining,
                    extent.len(),
                    cache,
                    refusal,
                ) {
                    Ok(Some(slots)) => slots,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
                return arrays::CountedScalars::from_tokens(ctx, extent, slots)
                    .map(|array| array.map(SurfaceNamedValue::CountedScalarArray))
                    .transpose();
            }
            let mut values = Vec::new();
            let mut remaining = count;
            let mut positions = cursor..body.len();
            while remaining != 0 && !positions.is_empty() {
                let Some(position) = (match ctx
                    .next_charged(&mut positions, "creo compact surface integer dispatch")
                {
                    Ok(position) => position,
                    Err(error) => return Some(Err(error)),
                }) else {
                    break;
                };
                // A present compact integer always consumes at least one byte.
                let (value, next) = compact_int(body, position);
                if let Err(error) = ctx.reserve_vec(&mut values, 1, "creo compact surface integers")
                {
                    return Some(Err(error));
                }
                values.push(value);
                positions.start = next;
                remaining -= 1;
            }
            cursor = positions.start;
            if Some(values.len()) == usize::try_from(count).ok() && cursor == body.len() {
                return Some(Ok(SurfaceNamedValue::CompactIntArray(values)));
            }
            if name == "parent_feats" && parent_feature_array_trailer(&body[cursor..]) {
                return Some(Ok(SurfaceNamedValue::CompactIntArray(values)));
            }
            if name == "params" {
                let remaining = admitted_counted_parameter_body(body, values_start, count)?;
                let extent = arrays::CountedScalars::extent(count)?;
                let slots =
                    match counted_parameter_scalar_slots(ctx, remaining, extent.len(), cache) {
                        Ok(Some(slots)) => slots,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };
                return arrays::CountedScalars::from_tokens(ctx, extent, slots)
                    .map(|array| array.map(SurfaceNamedValue::CountedScalarArray))
                    .transpose();
            }
        }
    }
    if body.first() == Some(&psb::token::SCALAR_BODY) {
        let (dimensions, dimensions_end) = compact_int(body, 1);
        let (count, values_start) = compact_int(body, dimensions_end);
        let slot_count = usize::try_from(dimensions).ok().and_then(|dimensions| {
            usize::try_from(count)
                .ok()
                .and_then(|count| dimensions.checked_mul(count))
        });
        let remaining = slot_count.and_then(|slot_count| {
            crate::scalar::admitted_scalar_body(body, dimensions_end, values_start, slot_count)
        })?;
        let extent = grid?;
        let slot_count = extent.len();
        let spline_field = matches!(
            name,
            "i_pnts"
                | "i_points"
                | "end_u_tangts"
                | "end_v_tangts"
                | "end_uv_deriv"
                | "tangts"
                | "end_tangts"
        );
        let array = if spline_field {
            let slots = match named_spline_scalar_slots(
                ctx, family, name, remaining, slot_count, cache, refusal,
            ) {
                Ok(Some(slots)) => slots,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
            match arrays::DimensionedScalars::from_tokens(ctx, extent, slots) {
                Ok(Some(array)) => array,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            }
        } else if name == "local_sys" {
            let values = match sequential_named_local_system_slots(
                ctx, remaining, slot_count, cache, refusal,
            ) {
                Ok(Some(values)) => values,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
            match arrays::DimensionedScalars::from_values(ctx, extent, values) {
                Ok(Some(array)) => array,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            }
        } else {
            let values = match scalar_slots(ctx, remaining, slot_count, cache, refusal) {
                Ok(Some(values)) => values,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            };
            match arrays::DimensionedScalars::from_values(ctx, extent, values) {
                Ok(Some(array)) => array,
                Ok(None) => return None,
                Err(error) => return Some(Err(error)),
            }
        };
        return Some(Ok(SurfaceNamedValue::ScalarArray(array)));
    }
    if compact_integer_field {
        let (value, end) = compact_int(body, 0);
        if end == body.len() && end != 0 {
            return Some(Ok(SurfaceNamedValue::CompactInt(value)));
        }
        return None;
    }
    let mut values = Vec::new();
    let mut positions = 0..body.len();
    while !positions.is_empty() {
        let Some(cursor) =
            (match ctx.next_charged(&mut positions, "creo named surface scalar dispatch") {
                Ok(cursor) => cursor,
                Err(error) => return Some(Err(error)),
            })
        else {
            break;
        };
        if matches!(body[cursor], 0xe0..=0xe3 | 0xf1 | 0xf7 | 0xfb) {
            break;
        }
        let decoded = if matches!(name, "radius" | "radius1" | "radius2")
            && matches!(body[cursor], 0x0d | 0x0e)
        {
            Some((if body[cursor] == 0x0d { 0.25 } else { 0.5 }, cursor + 1))
        } else if name == "half_angle" {
            scalar::decode_positive_dict(body, cursor)
                .filter(|(value, _)| ApexConeHalfAngle::new(*value).is_some())
        } else if radius_field {
            scalar::decode_named_surface_radius(body, cursor, cache)
        } else if parameter_bound_field {
            scalar::decode_named_positive_dict_scalar(body, cursor, cache)
        } else {
            scalar::decode_in_lane(body, cursor, cache)
        };
        let Some((value, next)) = decoded else {
            values.clear();
            break;
        };
        if let Err(error) = ctx.reserve_vec(&mut values, 1, "creo named surface scalar sequence") {
            return Some(Err(error));
        }
        values.push(value);
        positions.start = next;
    }
    if !values.is_empty() {
        return Some(Ok(SurfaceNamedValue::ScalarSequence(values)));
    }
    let (value, end) = compact_int(body, 0);
    if !scalar_field && end == body.len() && end != 0 {
        Some(Ok(SurfaceNamedValue::CompactInt(value)))
    } else {
        None
    }
}

#[derive(Debug)]
struct NamedPrototypeParameterRange<'a> {
    name: &'a str,
    offset: usize,
    value_offset: usize,
    value_end: usize,
}

#[derive(Debug)]
struct NamedPrototypeFrame<'a> {
    family: SurfacePrototypeFamily,
    offset: usize,
    parameters: Vec<NamedPrototypeParameterRange<'a>>,
}

fn named_prototype_frames<'a>(
    ctx: &DecodeContext<'_>,
    payload: &'a [u8],
    cache: &scalar::ScalarCache,
) -> Result<Vec<NamedPrototypeFrame<'a>>, CodecError> {
    let mut frames = Vec::new();
    let mut search = 0;
    while let Some(record_start) = ctx.find_map(
        payload
            .get(search..)
            .unwrap_or_default()
            .windows(b"srf_prim_ptr(".len())
            .enumerate(),
        |(offset, bytes)| Ok((bytes == b"srf_prim_ptr(").then_some(search + offset)),
        "find Creo surface marker",
    )? {
        let family_start = record_start + b"srf_prim_ptr(".len();
        let Some(close) = ctx.find_map(
            payload
                .get(family_start..)
                .unwrap_or_default()
                .windows(b")\0".len())
                .enumerate(),
            |(offset, bytes)| Ok((bytes == b")\0").then_some(family_start + offset)),
            "find Creo surface marker",
        )?
        else {
            break;
        };
        let family_bytes = &payload[family_start..close];
        let family = match ctx.validate_utf8(family_bytes, "creo UTF-8 validation")? {
            Ok(name) => SurfacePrototypeFamily::from_known_name(name).map_or_else(
                || {
                    ctx.copy_retained_text(name, "creo prototype family name")
                        .map(SurfacePrototypeFamily::Other)
                },
                Ok,
            )?,
            Err(_) => SurfacePrototypeFamily::Other(ctx.copy_retained_lossy_utf8(
                family_bytes, "creo prototype family name",
            )?),
        };
        const BOUNDARY_MARKERS: [&[u8]; 6] = [
            b"srf_prim_ptr(", b"srf_prim_ptr\0", b"\xe0\x00entity_ptr(",
            b"crv_array\0", b"lo_array\0", b"qlt_array\0",
        ];
        let record_end = ctx.find_map(
            close + 2..payload.len(),
            |offset| Ok(BOUNDARY_MARKERS.iter().any(|marker| payload[offset..].starts_with(marker)).then_some(offset)),
            "find Creo surface marker",
        )?.unwrap_or(payload.len());
        let mut position_scope = ctx.reserve_scoped(0, "creo named prototype position scratch")?;
        let mut named = Vec::new();
        for token in psb::tokens(ctx, &payload[close + 2..record_end]) {
            let token = token?;
            let token_offset = close + 2 + token.offset;
            if token.kind == psb::TokenKind::NamedRecord
                && named_record_length(ctx, payload, token_offset)? == Some(token.length)
            {
                position_scope.with_storage(|| {
                    ctx.reserve_vec(&mut named, 1, "creo named prototype field positions")
                })?;
                named.push((token_offset, token.length));
            }
        }
        for token_offset in
            ctx.admit_iter(close + 2..record_end, "creo named prototype byte traversal")?
        {
            let Some(length) = named_record_length(ctx, payload, token_offset)? else {
                continue;
            };
            let name_start = token_offset + 2;
            let name_end = token_offset + length - 1;
            let name = ctx
                .validate_utf8(&payload[name_start..name_end], "creo UTF-8 validation")?
                .ok();
            if name.is_some_and(|name| prototype_parameter_allowed(&family, name)) {
                position_scope.with_storage(|| {
                    ctx.reserve_vec(&mut named, 1, "creo named prototype field positions")
                })?;
                named.push((token_offset, length));
            }
        }
        ctx.sort_unstable_by(
            &mut named,
            |value| value,
            Ord::cmp,
            "creo named prototype field position sort",
        )?;
        ctx.dedup_vec(
            &mut named,
            "creo named prototype field position deduplication",
        )?;
        let mut owned_scalar_end = close + 2;
        ctx.retain_vec(
            &mut named,
            |(token_offset, token_length)| {
                if *token_offset < owned_scalar_end {
                    return Ok(false);
                }
                let name_start = *token_offset + 2;
                let name_end = *token_offset + *token_length - 1;
                if let Ok(name) =
                    ctx.validate_utf8(&payload[name_start..name_end], "creo UTF-8 validation")?
                {
                    if prototype_parameter_allowed(&family, name) {
                        let value_offset = *token_offset + *token_length;
                        if let Some(length) = named_vector_scalar_body_len(
                            ctx,
                            &family,
                            name,
                            &payload[value_offset..record_end],
                            cache,
                        )? {
                            owned_scalar_end = value_offset + length;
                        }
                    }
                }
                Ok(true)
            },
            "creo named prototype field retain",
        )?;
        let mut parameters = Vec::new();
        for (position, (token_offset, token_length)) in ctx
            .admit_iter(&named, "creo named prototype field traversal")?
            .copied()
            .enumerate()
        {
            let name_start = token_offset + 2;
            let name_end = token_offset + token_length - 1;
            let Some(name) = ctx
                .validate_utf8(&payload[name_start..name_end], "creo UTF-8 validation")?
                .ok()
            else {
                continue;
            };
            if !prototype_parameter_allowed(&family, name) {
                continue;
            }
            let value_offset = token_offset + token_length;
            let mut value_end = named
                .get(position + 1)
                .map_or(record_end, |(next, _)| *next);
            if let Some(length) = named_vector_scalar_body_len(
                ctx,
                &family,
                name,
                &payload[value_offset..record_end],
                cache,
            )? {
                value_end = value_offset + length;
            } else if let Some(compound_close) = psb::tokens(ctx, &payload[value_offset..value_end])
                .find(|token| match token { Ok(token) => token.kind == psb::TokenKind::CompoundClose, Err(_) => true })
                .transpose()?
            {
                value_end = value_offset + compound_close.offset;
            }
            ctx.reserve_vec(&mut parameters, 1, "creo named prototype field ranges")?;
            parameters.push(NamedPrototypeParameterRange {
                name,
                offset: token_offset,
                value_offset,
                value_end,
            });
        }
        ctx.reserve_vec(&mut frames, 1, "creo named prototype frames")?;
        frames.push(NamedPrototypeFrame {
            family,
            parameters,
            offset: record_start,
        });
        search = record_end;
    }
    Ok(frames)
}

/// Decode bounded named surface-prototype parameter records.
/// Bounded named `srf_prim_ptr(<kind>)` prototype records in `payload`.
///
/// A named field whose bounded scalar body the decoder refuses is retained
/// opaque and stated in `refusals`, against the prototype record and field
/// that hold it.
pub(crate) fn named_prototype_records(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    refusals: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Vec<SurfacePrototypeRecord>, CodecError> {
    let cache = scalar::ScalarCache::from_section_checked(ctx, payload)?;
    let mut records = Vec::new();
    let mut scratch = ctx.reserve_scoped(0, "creo named prototype frame scratch")?;
    let frames = scratch.with_storage(|| named_prototype_frames(ctx, payload, &cache))?;
    for frame in ctx.admit_iter(&frames, "creo named prototype record traversal")? {
        let record = decode_named_prototype_frame(ctx, payload, frame, &cache, refusals)?;
        ctx.reserve_vec(&mut records, 1, "creo named prototype records")?;
        records.push(record);
    }
    Ok(records)
}

fn decode_named_prototype_frame(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    frame: &NamedPrototypeFrame<'_>,
    cache: &scalar::ScalarCache,
    refusals: &mut crate::lane_refusal::LaneRefusals,
) -> Result<SurfacePrototypeRecord, CodecError> {
    let mut parameters = Vec::new();
    for range in ctx.admit_iter(
        &frame.parameters,
        "creo named prototype parameter traversal",
    )? {
        ctx.reserve_vec(&mut parameters, 1, "creo named prototype parameters")?;
        let body = ctx.copy_retained(
            &payload[range.value_offset..range.value_end],
            "creo named prototype parameter body",
        )?;
        let value = named_surface_value(
            ctx,
            &frame.family,
            range.name,
            &body,
            cache,
            &format_args!(
                "creo surface prototype {} at offset {}",
                frame.family.name(),
                frame.offset
            ),
            refusals,
        )?;
        parameters.push(SurfaceNamedParameter {
            name: ctx.copy_retained_text(range.name, "creo named prototype parameter name")?,
            value,
            body,
            offset: range.offset,
            value_offset: range.value_offset,
        });
    }
    let family = match &frame.family {
        SurfacePrototypeFamily::Other(name) => SurfacePrototypeFamily::Other(
            ctx.copy_retained_text(name, "creo prototype family name")?,
        ),
        family => family.clone(),
    };
    SurfacePrototypeRecord::new(ctx, family, parameters, frame.offset)
}

fn parent_feature_array_trailer(body: &[u8]) -> bool {
    let Some(rest) = body.strip_prefix(&[psb::token::ENTITY_REF]) else {
        return false;
    };
    let Ok((class_id, cursor)) = psb::reference_id(rest, 0) else {
        return false;
    };
    let Ok((entity_id, cursor)) = psb::reference_id(rest, cursor) else {
        return false;
    };
    if class_id == 0 || entity_id == 0 {
        return false;
    }
    matches!(&rest[cursor..], [] | [0xe1] | [0xe1, 0xf6, 0xf6])
}

fn positional_body_start(payload: &[u8], row: &SurfaceRow) -> Option<usize> {
    let (_, after_id) = compact_int(payload, row.offset);
    (payload
        .get(after_id)
        .and_then(|byte| SurfaceKind::from_byte(*byte))
        == Some(row.kind))
    .then_some(())?;
    let mut cursor = after_id + 1;
    let (feature_id, next) = compact_int(payload, cursor);
    (next > cursor && feature_id == row.feature_id).then_some(())?;
    cursor = next;
    let orientation = *payload.get(cursor)?;
    let boundary = *payload.get(cursor + 1)?;
    (matches!(orientation, 0x01 | 0xf6) && BoundaryType::from_byte(boundary).is_some())
        .then_some(())?;
    cursor += 2;
    let (_, next) = compact_int(payload, cursor);
    (next > cursor).then_some(next)
}

fn decode_row_scalar(
    kind: SurfaceKind,
    body: &[u8],
    offset: usize,
    cache: &scalar::ScalarCache,
) -> Option<(f64, usize)> {
    if kind == SurfaceKind::TorusOrSphere {
        scalar::decode_in_torus_row_lane(body, offset, cache)
    } else {
        scalar::decode_in_surface_row_lane(body, offset, cache)
    }
}

fn torus_outline_marker(body: &[u8], offset: usize) -> Option<(usize, usize, u32)> {
    if body.get(offset..offset + 3) != Some(&[0x01, 0x12, 0x50]) {
        return None;
    }
    let (selector, end) = compact_int(body, offset + 3);
    (end > offset + 3).then_some((offset, end, selector))
}

fn torus_radius_override_layout(
    ctx: &DecodeContext<'_>,
    body: &[u8],
) -> Result<Option<TorusRadiusOverrideLayout>, CodecError> {
    let mut markers = body.windows(2);
    let Some(offset) = ctx.position_by(
        &mut markers,
        |marker| Ok(marker == [0x18, 0x0d]),
        "creo torus radius marker search",
    )?
    else {
        return Ok(None);
    };
    if ctx.any_by(
        markers,
        |marker| Ok(marker == [0x18, 0x0d]),
        "creo torus radius marker search",
    )? {
        return Ok(None);
    }
    Ok(torus_radius_override_at(body, offset))
}

fn torus_radius_override_at(body: &[u8], offset: usize) -> Option<TorusRadiusOverrideLayout> {
    let radius2_start = offset + 2;
    let (stored_radius2, radius2_end) = scalar::decode(body, radius2_start)?;
    let radius1_marker = (radius2_end..=radius2_end.checked_add(1)?)
        .find(|candidate| body.get(*candidate) == Some(&0x0e))?;
    let mut radius1_candidates = (radius1_marker + 1..=radius1_marker + 2).filter_map(|start| {
        let (value, end) = scalar::decode(body, start)?;
        (end == body.len()).then_some((value, start))
    });
    let (radius1, radius1_start) = radius1_candidates.next()?;
    radius1_candidates.next().is_none().then_some(())?;
    let (radius2, radius2_encoding) =
        if body.get(radius2_end..radius1_start) == Some(&[0x00, 0x0e, 0x01]) {
            (
                stored_radius2 - radius1,
                TorusRadius2Encoding::OuterRingDifference,
            )
        } else {
            (stored_radius2, TorusRadius2Encoding::Direct)
        };
    (radius1.is_finite() && radius1 >= 0.0 && radius2.is_finite() && radius2 > 0.0).then_some(
        TorusRadiusOverrideLayout {
            overrides: TorusRadiusOverrides {
                radius1,
                radius2,
                radius2_encoding,
                offset,
            },
            radius2_start,
            radius2_end,
            radius1_start,
        },
    )
}



fn terminal_cone_half_angle_layout(body: &[u8]) -> Option<ConeHalfAngleLayout> {
    // Every positive-DICT token consumes exactly seven bytes. Only this start
    // can reach the terminal boundary; earlier tokens cannot compete with it.
    let start = body.len().checked_sub(7)?;
    let (value, end) = scalar::decode_positive_dict(body, start)?;
    let value = ApexConeHalfAngle::new(value)?;
    Some(ConeHalfAngleLayout { value, start, end })
}

fn cone_half_angle_before_close(
    ctx: &DecodeContext<'_>,
    body: &[u8],
) -> Result<Option<ConeHalfAngleLayout>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    // Seven token bytes and the following close must fit. Shorter suffixes
    // cannot produce a layout and need no scalar dispatch.
    let mut starts = 0..body.len().saturating_sub(7);
    let mut layout = None;
    while !starts.is_empty() {
        let Some(start) = ctx.next_charged(&mut starts, "creo cone close angle scan")? else {
            break;
        };
        let Some((value, end)) = scalar::decode_positive_dict(body, start) else {
            continue;
        };
        let Some(value) = ApexConeHalfAngle::new(value) else {
            continue;
        };
        if body.get(end) != Some(&psb::token::COMPOUND_CLOSE) {
            continue;
        }
        if layout.is_some() {
            return Ok(None);
        }
        layout = Some(ConeHalfAngleLayout { value, start, end });
    }
    Ok(layout)
}

fn scalar_tokens(
    ctx: &DecodeContext<'_>,
    kind: SurfaceKind,
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Result<Vec<SurfaceParameterScalar>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut tokens = Vec::new();
    let positional_plane_corners = (kind == SurfaceKind::Plane)
        .then(|| first_coordinate_plane_corner_tokens(body, cache))
        .flatten();
    let radius_layout = if kind == SurfaceKind::TorusOrSphere {
        torus_radius_override_layout(ctx, body)?
    } else {
        None
    };
    let cone_half_angle = if kind == SurfaceKind::Cone {
        terminal_cone_half_angle_layout(body)
    } else {
        None
    };
    let mut cursor = 0;
    let mut steps = 0..body.len();
    while cursor < body.len() {
        if ctx
            .next_charged(&mut steps, "creo surface scalar token dispatch")?
            .is_none()
        {
            break;
        }
        if let Some(&(value, start, end)) = positional_plane_corners
            .as_ref()
            .and_then(|corners| corners.iter().find(|(_, start, _)| *start == cursor))
        {
            let raw = ctx.copy_retained(&body[start..end], "creo surface scalar token bytes")?;
            ctx.reserve_vec(&mut tokens, 1, "creo surface scalar token items")?;
            tokens.push(SurfaceParameterScalar {
                value: Some(value),
                raw,
                offset: start,
            });
            cursor = end;
            continue;
        }
        if kind == SurfaceKind::TorusOrSphere {
            if let Some((_, end, _)) = torus_outline_marker(body, cursor) {
                cursor = end;
                continue;
            }
        }
        if let Some(layout) = radius_layout {
            if cursor == layout.overrides.offset {
                cursor = layout.radius2_start;
                continue;
            }
            if cursor == layout.radius2_end {
                cursor = layout.radius1_start;
                continue;
            }
        }
        if let Some(layout) = cone_half_angle {
            if cursor == layout.start {
                let raw = ctx.copy_retained(
                    &body[layout.start..layout.end],
                    "creo surface scalar token bytes",
                )?;
                ctx.reserve_vec(&mut tokens, 1, "creo surface scalar token items")?;
                tokens.push(SurfaceParameterScalar {
                    value: Some(layout.value.get().get()),
                    raw,
                    offset: layout.start,
                });
                cursor = layout.end;
                continue;
            }
        }
        if let Some((value, next)) = decode_row_scalar(kind, body, cursor, cache) {
            if positional_plane_corners.as_ref().is_some_and(|corners| {
                corners
                    .iter()
                    .any(|(_, start, _)| cursor < *start && next > *start)
            }) {
                cursor += 1;
                continue;
            }
            // Row scalar tokens consume at most eight bytes. Check each of
            // their seven interior positions before accepting the token.
            if kind == SurfaceKind::TorusOrSphere && (1..8).any(|distance| {
                let offset = cursor + distance;
                offset < next && torus_outline_marker(body, offset).is_some()
            }) {
                cursor += 1;
                continue;
            }
            if radius_layout.is_some_and(|layout| {
                cursor < layout.overrides.offset && next > layout.overrides.offset
            }) {
                cursor += 1;
                continue;
            }
            if cone_half_angle.is_some_and(|layout| cursor < layout.start && next > layout.start) {
                cursor += 1;
                continue;
            }
            let raw = ctx.copy_retained(&body[cursor..next], "creo surface scalar token bytes")?;
            ctx.reserve_vec(&mut tokens, 1, "creo surface scalar token items")?;
            tokens.push(SurfaceParameterScalar {
                value: Some(value),
                raw,
                offset: cursor,
            });
            cursor = next;
        } else {
            cursor += 1;
        }
    }
    Ok(tokens)
}

fn first_coordinate_plane_corner_tokens(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<[(f64, usize, usize); 6]> {
    let frame_end = if body.ends_with(&[0xf7, 0x0c]) {
        body.len() - 2
    } else {
        body.len()
    };
    // Six scalar tokens end at `frame_end`. Both coordinate lanes consume
    // at most eight bytes per token, so earlier starts cannot reach that end.
    const MAX_CORNER_FRAME_BYTES: usize = 6 * 8;
    let lower = frame_end.saturating_sub(MAX_CORNER_FRAME_BYTES);
    let mut candidates = (lower..frame_end).filter_map(|start| {
        (start >= 3 && body.get(start - 3..start) == Some(&[0x00, 0x0c, 0x9a])).then_some(())?;
        let (stored_first_x, first_end) =
            scalar::decode_tabulated_cylinder_first_coordinate(body, start, cache)?;
        (first_end > start && stored_first_x.is_finite() && stored_first_x < 0.0).then_some(())?;
        let (first_y, first_z_start) = scalar::decode_in_surface_row_lane(body, first_end, cache)?;
        let (first_z, second_x_start) =
            scalar::decode_in_surface_row_lane(body, first_z_start, cache)?;
        let (stored_second_x, second_y_start) =
            scalar::decode_tabulated_cylinder_first_coordinate(body, second_x_start, cache)?;
        let (second_y, second_z_start) =
            scalar::decode_in_surface_row_lane(body, second_y_start, cache)?;
        let (second_z, end) = scalar::decode_in_surface_row_lane(body, second_z_start, cache)?;
        (end == frame_end
            && [first_y, first_z, stored_second_x, second_y, second_z]
                .iter()
                .all(|value| value.is_finite())
            && stored_second_x < 0.0
            && second_y_start > second_x_start)
            .then_some(())?;
        Some([
            (-stored_first_x, start, first_end),
            (first_y, first_end, first_z_start),
            (first_z, first_z_start, second_x_start),
            (-stored_second_x, second_x_start, second_y_start),
            (second_y, second_y_start, second_z_start),
            (second_z, second_z_start, end),
        ])
    });
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

fn opaque_spans(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    tokens: &[SurfaceParameterScalar],
) -> Result<Vec<SurfaceParameterOpaqueSpan>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut spans = Vec::new();
    let mut cursor = 0;
    for token in ctx.admit_iter(tokens, "creo surface opaque span traversal")? {
        if cursor < token.offset {
            let raw = ctx.copy_retained(
                &body[cursor..token.offset],
                "creo surface opaque span bytes",
            )?;
            ctx.reserve_vec(&mut spans, 1, "creo surface opaque span items")?;
            spans.push(SurfaceParameterOpaqueSpan {
                raw,
                offset: cursor,
            });
        }
        cursor = token.offset + token.raw.len();
    }
    if cursor < body.len() {
        let raw = ctx.copy_retained(&body[cursor..], "creo surface opaque span bytes")?;
        ctx.reserve_vec(&mut spans, 1, "creo surface opaque span items")?;
        spans.push(SurfaceParameterOpaqueSpan {
            raw,
            offset: cursor,
        });
    }
    Ok(spans)
}

fn scalar_frames(
    ctx: &DecodeContext<'_>,
    tokens: &[SurfaceParameterScalar],
) -> Result<Vec<SurfaceParameterScalarFrame>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut frames: Vec<SurfaceParameterScalarFrame> = Vec::new();
    for token in ctx.admit_iter(tokens, "creo surface scalar frame traversal")? {
        let contiguous = frames
            .last()
            .and_then(|frame| frame.slots.last())
            .is_some_and(|last| last.offset + last.raw.len() == token.offset);
        if !contiguous {
            ctx.reserve_vec(&mut frames, 1, "creo surface scalar frame items")?;
            frames.push(SurfaceParameterScalarFrame {
                offset: token.offset,
                slots: Vec::new(),
            });
        }
        let Some(frame) = frames.last_mut() else {
            return Err(CodecError::malformed("scalar frame has no first token"));
        };
        ctx.reserve_vec(&mut frame.slots, 1, "creo surface scalar frame slots")?;
        frame.slots.push(SurfaceParameterScalar {
            value: token.value,
            raw: ctx.copy_retained(&token.raw, "creo surface scalar frame bytes")?,
            offset: token.offset,
        });
    }
    Ok(frames)
}

fn terminal_scalar_frame_of<'a>(
    body: &[u8],
    frames: &'a [SurfaceParameterScalarFrame],
) -> Option<&'a SurfaceParameterScalarFrame> {
    let frame = frames.last()?;
    let last = frame.slots.last()?;
    (last.offset + last.raw.len() == body.len()).then_some(frame)
}

fn split_cylinder_outline_bounds(
    body: &[u8],
    slots: &[SurfaceParameterScalar],
) -> Option<[[f64; 2]; 2]> {
    let [first_u, first_v, second_u, second_v, orientation] =
        slots.get(slots.len().checked_sub(5)?..)?
    else {
        return None;
    };
    (first_u.offset + first_u.raw.len() == first_v.offset
        && body.get(first_v.offset + first_v.raw.len()..second_u.offset)
            == Some(&[0x00, 0x0c, 0x98][..])
        && second_u.offset + second_u.raw.len() == second_v.offset
        && second_v.offset + second_v.raw.len() == orientation.offset
        && orientation.raw == [0x0d]
        && orientation.value == Some(-1.0)
        && matches!(
            body.get(orientation.offset + orientation.raw.len()..),
            Some([] | [0xf7, 0x17])
        ))
    .then_some(())?;
    let bounds = [
        [first_u.value?, first_v.value?],
        [second_u.value?, second_v.value?],
    ];
    bounds
        .iter()
        .flatten()
        .all(|value| value.is_finite())
        .then_some(bounds)
}

fn named_record_length(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    offset: usize,
) -> Result<Option<usize>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if body.get(offset) != Some(&psb::token::NAMED_RECORD)
        || !body
            .get(offset + 1)
            .is_some_and(|field_type| *field_type <= 0x24)
    {
        return Ok(None);
    }
    let mut name = body[offset + 2..].iter().take(96).enumerate();
    while name.len() != 0 {
        let Some((index, byte)) = ctx.next_charged(&mut name, "creo surface name prefix scan")?
        else {
            break;
        };
        if *byte == 0 {
            return Ok((index > 0).then_some(index + 3));
        }
        if (index == 0 && !byte.is_ascii_alphabetic())
            || (!byte.is_ascii_alphanumeric() && !matches!(byte, b'_' | b'(' | b')'))
        {
            return Ok(None);
        }
    }
    Ok(None)
}

fn named_record_boundary(
    ctx: &DecodeContext<'_>,
    kind: SurfaceKind,
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Result<Option<usize>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let cone_half_angle = if kind == SurfaceKind::Cone {
        terminal_cone_half_angle_layout(body)
    } else {
        None
    };
    let mut cursor = 0;
    let mut steps = 0..body.len();
    while cursor < body.len() {
        if ctx
            .next_charged(&mut steps, "creo surface named boundary traversal")?
            .is_none()
        {
            break;
        }
        if let Some(layout) = cone_half_angle {
            if cursor == layout.start {
                cursor = layout.end;
                continue;
            }
        }
        if named_record_length(ctx, body, cursor)?.is_some() {
            return Ok(Some(cursor));
        }
        if let Some((_, next)) = decode_row_scalar(kind, body, cursor, cache) {
            if cone_half_angle.is_some_and(|layout| cursor < layout.start && next > layout.start) {
                cursor += 1;
                continue;
            }
            cursor = next;
        } else {
            cursor += 1;
        }
    }
    Ok(None)
}

/// Decode bounded parameter bodies for positional `srf_array` rows.
pub(crate) fn parameter_records(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<SurfaceParameterRecord>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo parameter_records row scratch")?;
    let rows = scratch.with_storage(|| rows(ctx, payload))?;
    let cache = scalar::ScalarCache::from_section_checked(ctx, payload)?;
    parameter_records_for_rows(ctx, payload, &rows, &cache)
}

/// Decode bounded positional parameter bodies from a DEPDB cross-section
/// surface namespace.
pub(crate) fn cross_section_parameter_records(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<SurfaceParameterRecord>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo parameter_records row scratch")?;
    let rows = scratch.with_storage(|| cross_section_rows(ctx, payload))?;
    let cache = scalar::ScalarCache::from_section_checked(ctx, payload)?;
    parameter_records_for_rows(ctx, payload, &rows, &cache)
}

#[derive(Debug, Clone, Copy)]
struct InlineSurfaceEnvelope {
    /// Axial parameter bounds.
    axial: [f64; 2],
    /// Two model-space outline corners.
    corners: [[Option<f64>; 3]; 2],
    /// Offset of the envelope close relative to the bounded row body.
    close: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum InlineSurfaceCarrier {
    Cylinder {
        frame: PositionalCylinderFrame,
        split_bounds: Option<[[f64; 2]; 2]>,
    },
    CylinderBounds([[f64; 2]; 2]),
    Cone(PositionalConeFrame),
    Torus(PositionalTorusFrame),
    Tabulated {
        variant: ExtrusionVariant,
        frame: TabulatedCylinderFrame,
    },
}

#[derive(Debug, Clone, Copy)]
struct InlineSurfaceBody {
    terminal_close: usize,
    carrier: Option<InlineSurfaceCarrier>,
}

const EPS_INLINE_WITNESS: f64 = 1.0e-9;
const EPS_INLINE_FRAME: f64 = 1.0e-8;
const MAX_INLINE_FRAME_CANDIDATES: usize = 24;

fn inline_surface_body(
    ctx: &DecodeContext<'_>,
    kind: SurfaceKind,
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Result<Option<InlineSurfaceBody>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let standard_envelope = decode_inline_surface_envelope(kind, body, cache);
    let four_bound_envelope = decode_inline_four_bound_cylinder_envelope(kind, body, cache);
    let referenced_envelope = decode_inline_referenced_cylinder_envelope(kind, body, cache);
    let selector_envelope = decode_inline_selector_cylinder_envelope(ctx, kind, body, cache)?;
    let Some(envelope) = standard_envelope
        .or(four_bound_envelope)
        .or(referenced_envelope)
        .or(selector_envelope)
    else {
        return inline_surface_suffix_body(ctx, kind, body, cache);
    };
    let Some(local_start) = envelope.close.checked_add(1) else {
        return Ok(None);
    };
    let mut sole_layout = None;
    let mut terminal_closes = local_start..body.len();
    while !terminal_closes.is_empty() {
        let Some(terminal_close) = ctx.next_charged(
            &mut terminal_closes,
            "creo inline terminal-close candidate visits",
        )?
        else {
            break;
        };
        if body.get(terminal_close) != Some(&psb::token::COMPOUND_CLOSE) {
            continue;
        }
        let Some(local) = body.get(local_start..terminal_close) else {
            return Ok(None);
        };
        let mut structurally_complete = false;
        let mut geometric_interpretation_count = 0;
        let mut first_carrier = None;
        let mut conflicting_carriers = false;
        if kind == SurfaceKind::Cone {
            if let Some(angle) = terminal_cone_half_angle_layout(local) {
                if let Some(frame) =
                    decode_support_apex_cone_frame(&local[..angle.start], angle.value, cache)
                {
                    structurally_complete = true;
                    geometric_interpretation_count += 1;
                    first_carrier = Some(InlineSurfaceCarrier::Cone(frame));
                }
            }
        }
        for prefix in scalar::decode_inline_non_plane_local_system_prefix(ctx, local, cache)? {
            for frame in inline_resolved_frames(ctx, local, prefix, cache)? {
                if inline_surface_suffix(kind, local, frame.cursor, cache).is_none() {
                    continue;
                }
                structurally_complete = true;
                if inline_frame_directions(frame).is_none() {
                    continue;
                }
                geometric_interpretation_count += 1;
                if let Some(carrier) = inline_surface_carrier(kind, envelope, local, frame, cache) {
                    if first_carrier.is_some_and(|first| first != carrier) {
                        conflicting_carriers = true;
                    } else {
                        first_carrier = Some(carrier);
                    }
                }
            }
        }
        if !structurally_complete || geometric_interpretation_count == 0 {
            continue;
        }
        let carrier = if conflicting_carriers {
            None
        } else {
            first_carrier
        };
        let layout = InlineSurfaceBody {
            terminal_close,
            carrier,
        };
        if sole_layout.replace(layout).is_some() {
            return Ok(None);
        }
    }
    if sole_layout.is_some() {
        Ok(sole_layout)
    } else {
        inline_surface_suffix_body(ctx, kind, body, cache)
    }
}

fn sorted_inline_terminal_closes(
    closes: &mut [usize; MAX_INLINE_FRAME_CANDIDATES], count: usize,
) -> Option<&[usize]> {
    let closes = closes.get_mut(..count)?;
    closes.sort_unstable();
    Some(closes)
}

/// Decode the local-system suffix form used by positional rows that have no
/// axial envelope. The terminal close is part of the row grammar, so a frame
/// is admitted only when its exact suffix ends immediately before that close.
fn inline_surface_suffix_body(
    ctx: &DecodeContext<'_>,
    kind: SurfaceKind,
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Result<Option<InlineSurfaceBody>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut sole_layout = None;
    for local_start in inline_local_starts(ctx, body) {
        let local_start = local_start?;
        let Some(local) = body.get(local_start..) else {
            return Ok(None);
        };
        let mut terminal_closes = [0usize; MAX_INLINE_FRAME_CANDIDATES];
        let mut close_count = 0;
        for prefix in scalar::decode_inline_non_plane_local_system_prefix(ctx, local, cache)? {
            for frame in inline_resolved_frames(ctx, local, prefix, cache)? {
                if let Some((_, end)) =
                    decode_inline_surface_suffix_at(kind, local, frame.cursor, cache)
                {
                    if local.get(end) == Some(&psb::token::COMPOUND_CLOSE) {
                        *match terminal_closes.get_mut(close_count) {
                            Some(value) => value,
                            None => return Ok(None),
                        } = end;
                        close_count += 1;
                    }
                }
            }
        }
        let Some(terminal_closes) = sorted_inline_terminal_closes(&mut terminal_closes, close_count) else {
            return Ok(None);
        };
        let mut previous_close = None;
        for relative_close in terminal_closes.iter().copied() {
            if previous_close == Some(relative_close) {
                continue;
            }
            previous_close = Some(relative_close);
            let terminal_close = local_start + relative_close;
            let Some(local) = body.get(local_start..terminal_close) else {
                return Ok(None);
            };
            let mut structurally_complete = false;
            let mut geometric_interpretation_count = 0;
            let mut carriers = [None; MAX_INLINE_FRAME_CANDIDATES];
            let mut carrier_count = 0;
            for prefix in scalar::decode_inline_non_plane_local_system_prefix(ctx, local, cache)? {
                for frame in inline_resolved_frames(ctx, local, prefix, cache)? {
                    if inline_surface_suffix(kind, local, frame.cursor, cache).is_none() {
                        continue;
                    }
                    structurally_complete = true;
                    if inline_suffix_frame_directions(frame).is_none() {
                        continue;
                    }
                    geometric_interpretation_count += 1;
                    if let Some(carrier) = inline_surface_suffix_carrier(kind, local, frame, cache)
                    {
                        *match carriers.get_mut(carrier_count) {
                            Some(value) => value,
                            None => return Ok(None),
                        } = Some(carrier);
                        carrier_count += 1;
                    }
                }
            }
            if !structurally_complete || geometric_interpretation_count == 0 {
                continue;
            }
            let carrier = (carrier_count == geometric_interpretation_count)
                .then(|| carriers.first().copied().flatten())
                .flatten()
                .filter(|first| {
                    carriers[..carrier_count]
                        .iter()
                        .all(|candidate| *candidate == Some(*first))
                });
            let carrier = match inline_suffix_witness(kind, body, local_start, cache) {
                Some(witness)
                    if inline_suffix_witness_agrees(
                        witness,
                        &carriers,
                        carrier_count,
                        geometric_interpretation_count,
                    ) =>
                {
                    Some(witness)
                }
                _ => carrier,
            };
            let layout = InlineSurfaceBody {
                terminal_close,
                carrier,
            };
            if sole_layout.replace(layout).is_some() {
                return Ok(None);
            }
        }
    }
    Ok(sole_layout)
}

/// Recover a complete legacy analytic envelope immediately before an inline
/// local-system suffix. The prefix is a separate bounded operand, so it is
/// useful only when its family agrees with the suffix and its placement is
/// independently complete.
fn inline_suffix_witness(
    kind: SurfaceKind,
    body: &[u8],
    local_start: usize,
    cache: &scalar::ScalarCache,
) -> Option<InlineSurfaceCarrier> {
    let prefix_end = local_start.checked_sub(1)?;
    let prefix = body.get(..prefix_end)?;
    match kind {
        SurfaceKind::Cylinder => decode_11_10_13_cylinder_witness(prefix, cache)
            .or_else(|| cylinder_frame_readers::decode_held_axis_cylinder_frame(prefix, cache))
            .map(|frame| InlineSurfaceCarrier::Cylinder {
                frame,
                split_bounds: None,
            }),
        SurfaceKind::Cone => {
            decode_planar_envelope_cone_frame(prefix, cache).map(InlineSurfaceCarrier::Cone)
        }
        _ => None,
    }
}

/// Decode the placement witness preceding an inline cylinder suffix.
///
/// The `11 10 13` prefix stores a zero auxiliary slot, two transverse bounds,
/// the other transverse center coordinate, a model reference, a signed-half
/// marker, and a replay reference. The axial center coordinate is omitted and
/// is zero. The bounds identify the cylinder center and radius; the suffix
/// supplies the complete local-system frame and confirms the Z-axis family.
fn decode_11_10_13_cylinder_witness(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    body.starts_with(&[0x11, 0x10, 0x13]).then_some(())?;
    let decode = |cursor| {
        let (value, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
        value.is_finite().then_some((value, next))
    };
    let (auxiliary, mut cursor) = decode(3)?;
    (auxiliary == 0.0).then_some(())?;
    let (first_bound, next) = decode(cursor)?;
    cursor = next;
    (body.get(cursor) == Some(&0x10)).then_some(())?;
    cursor += 1;
    let (center, next) = decode(cursor)?;
    cursor = next;
    let (second_bound, next) = decode(cursor)?;
    cursor = next;
    matches!(body.get(cursor), Some(0x19 | 0x32)).then_some(())?;
    let (_, next) = scalar::decode_model_reference_coordinate(body, cursor, cache)?;
    cursor = next;
    (body.get(cursor) == Some(&0x0e)).then_some(())?;
    cursor += 1;
    (body.get(cursor) == Some(&0xf7)).then_some(())?;
    let (_, cursor) = psb::reference_id(body, cursor + 1).ok()?;
    (cursor == body.len()).then_some(())?;

    let scale = [first_bound, second_bound, center]
        .into_iter()
        .map(f64::abs)
        .fold(1.0, f64::max);
    let radius = 0.5 * (first_bound - second_bound).abs();
    (radius > EPS_INLINE_WITNESS * scale).then_some(())?;
    PositionalCylinderFrame::new(
        [f64::midpoint(first_bound, second_bound), 0.0, center],
        [0.0, 0.0, 1.0],
        [(first_bound - second_bound).signum(), 0.0, 0.0],
        radius,
        None,
    )
}

fn inline_suffix_witness_agrees(
    witness: InlineSurfaceCarrier,
    candidates: &[Option<InlineSurfaceCarrier>; MAX_INLINE_FRAME_CANDIDATES],
    candidate_count: usize,
    interpretation_count: usize,
) -> bool {
    let Some(candidates) = candidates.get(..candidate_count) else { return false; };
    if candidates.is_empty() || candidates.len() != interpretation_count {
        return false;
    }
    match witness {
        InlineSurfaceCarrier::Cylinder { frame: witness, .. } => {
            let matching = candidates.iter()
                .filter(|candidate| {
                    let Some(InlineSurfaceCarrier::Cylinder {
                        frame: candidate, ..
                    }) = **candidate
                    else {
                        return false;
                    };
                    let axis_dot = witness
                        .frame()
                        .axis()
                        .into_iter()
                        .zip(candidate.frame().axis())
                        .map(|(left, right)| left * right)
                        .sum::<f64>();
                    witness
                        .frame()
                        .origin()
                        .into_iter()
                        .zip(candidate.frame().origin())
                        .all(|(left, right)| inline_close(left, right))
                        && inline_close(witness.radius.get(), candidate.radius.get())
                        && axis_dot.abs() >= 1.0 - EPS_INLINE_FRAME
                })
                .count();
            matching == 1
        }
        InlineSurfaceCarrier::Cone(witness) => candidates.iter().flatten().all(|candidate| {
            let InlineSurfaceCarrier::Cone(candidate) = *candidate else {
                return false;
            };
            let axis_dot = witness
                .frame()
                .axis()
                .into_iter()
                .zip(candidate.frame().axis())
                .map(|(left, right)| left * right)
                .sum::<f64>();
            inline_close(
                witness.half_angle.get().get(),
                candidate.half_angle.get().get(),
            ) && axis_dot.abs() >= 1.0 - EPS_INLINE_FRAME
        }),
        InlineSurfaceCarrier::Torus(_)
        | InlineSurfaceCarrier::CylinderBounds(_)
        | InlineSurfaceCarrier::Tabulated { .. } => false,
    }
}

fn decode_inline_surface_envelope(
    kind: SurfaceKind,
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<InlineSurfaceEnvelope> {
    let mut cursor = 0;
    let mut values = [0.0; 9];
    for value in &mut values[..2] {
        let (decoded, next) = decode_row_scalar(kind, body, cursor, cache)?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    (body.get(cursor) == Some(&0x12)).then_some(())?;
    cursor += 1;
    for value in &mut values[2..] {
        let (decoded, next) = decode_row_scalar(kind, body, cursor, cache)?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    let close = inline_surface_envelope_close(body, cursor)?;
    Some(InlineSurfaceEnvelope {
        axial: [values[1], values[2]],
        corners: [
            [values[3], values[4], values[5]],
            [values[6], values[7], values[8]],
        ]
        .map(|corner| corner.map(Some)),
        close,
    })
}

fn inline_surface_envelope_close(body: &[u8], cursor: usize) -> Option<usize> {
    if body.get(cursor) == Some(&psb::token::COMPOUND_CLOSE) {
        return Some(cursor);
    }
    (body.get(cursor) == Some(&0xf7)).then_some(())?;
    let (_, close) = psb::reference_id(body, cursor + 1).ok()?;
    (body.get(close) == Some(&psb::token::COMPOUND_CLOSE)).then_some(close)
}

fn decode_inline_four_bound_cylinder_envelope(
    kind: SurfaceKind,
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<InlineSurfaceEnvelope> {
    (kind == SurfaceKind::Cylinder).then_some(())?;
    let mut cursor = 0;
    let decode_u = |cursor| {
        scalar::decode_positive_dict(body, cursor)
            .or_else(|| decode_row_scalar(kind, body, cursor, cache))
    };
    let decode_v = |cursor| {
        if body.get(cursor) == Some(&0x18) {
            return Some((0.0, cursor + 1));
        }
        scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)
    };
    let (u_low, next) = decode_u(cursor)?;
    cursor = next;
    let (v_low, next) = decode_v(cursor)?;
    cursor = next;
    let (u_high, next) = decode_u(cursor)?;
    cursor = next;
    let (v_high, next) = decode_v(cursor)?;
    cursor = next;
    [u_low, v_low, u_high, v_high]
        .into_iter()
        .all(f64::is_finite)
        .then_some(())?;
    (u_high > u_low && v_high != v_low).then_some(())?;

    let directrix_outline = decode_row_scalar(kind, body, cursor, cache).is_none();
    let mut corners = [[0.0; 3]; 2];
    for value in corners.iter_mut().flatten() {
        let (decoded, next) = if directrix_outline {
            let (stored, next) =
                scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
            (-stored, next)
        } else {
            decode_row_scalar(kind, body, cursor, cache)?
        };
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    let close = inline_surface_envelope_close(body, cursor)?;
    Some(InlineSurfaceEnvelope {
        axial: [v_low, v_high],
        corners: corners.map(|corner| corner.map(Some)),
        close,
    })
}

fn decode_inline_selector_cylinder_envelope(
    ctx: &DecodeContext<'_>,
    kind: SurfaceKind,
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Result<Option<InlineSurfaceEnvelope>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let candidate = (|| {
        (kind == SurfaceKind::Cylinder).then_some(())?;
        let selector_end = |cursor: usize| match body.get(cursor..) {
            Some([0x00, 0x11, 0x13, ..]) => Some(cursor + 3),
            Some([0x11..=0x14 | 0x17 | 0x18 | 0x20, ..]) => Some(cursor + 1),
            _ => None,
        };
        let decode_coordinate = |cursor| {
            if body.get(cursor) == Some(&0x18) {
                return Some((0.0, cursor + 1));
            }
            let (value, next) =
                scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
            let value = -value;
            value.is_finite().then_some((value, next))
        };
        let mut cursor = selector_end(0)?;
        let (first_axial, next) = decode_coordinate(cursor)?;
        cursor = selector_end(next)?;
        let (second_axial, next) = decode_coordinate(cursor)?;
        cursor = next;
        (first_axial != second_axial).then_some(())?;

        let mut corners = [[None; 3]; 2];
        for coordinate in corners.iter_mut().flatten() {
            if matches!(body.get(cursor), Some(0x92 | 0xda)) {
                body.get(cursor..cursor.checked_add(7)?)?;
                cursor += 7;
                continue;
            }
            let (decoded, next) = decode_coordinate(cursor)?;
            *coordinate = Some(decoded);
            cursor = next;
        }
        let close = inline_surface_envelope_close(body, cursor)?;
        let axial_span = (second_axial - first_axial).abs();
        let spans =
            std::array::from_fn::<_, 3, _>(|axis| Some(corners[1][axis]? - corners[0][axis]?));
        let mut axial_axes = (0..3)
            .filter(|axis| spans[*axis].is_some_and(|span| inline_close(span.abs(), axial_span)));
        let axis_index = axial_axes.next()?;
        axial_axes.next().is_none().then_some(())?;
        let mut radial = (0..3).filter(|axis| *axis != axis_index);
        let radial_axes = [radial.next()?, radial.next()?];
        radial.next().is_none().then_some(())?;
        Some((
            InlineSurfaceEnvelope {
                axial: [first_axial, second_axial],
                corners,
                close,
            },
            radial_axes,
            spans,
        ))
    })();
    let Some((envelope, radial_axes, spans)) = candidate else {
        return Ok(None);
    };
    let absent = radial_axes
        .iter()
        .filter(|axis| spans[**axis].is_none())
        .count();
    Ok((absent <= 1).then_some(envelope))
}

fn decode_inline_referenced_cylinder_envelope(
    kind: SurfaceKind,
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<InlineSurfaceEnvelope> {
    (kind == SurfaceKind::Cylinder && body.first() == Some(&0x32)).then_some(())?;
    let (_, mut cursor) = scalar::decode_model_reference_coordinate(body, 0, cache)?;
    let mut bounds = [0.0; 3];
    for value in &mut bounds {
        let (decoded, next) =
            scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    (bounds[0] != bounds[2]).then_some(())?;

    let mut corners = [[0.0; 3]; 2];
    for value in corners.iter_mut().flatten() {
        let (decoded, next) =
            scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    let close = inline_surface_envelope_close(body, cursor)?;
    Some(InlineSurfaceEnvelope {
        axial: [bounds[0], bounds[2]],
        corners: corners.map(|corner| corner.map(Some)),
        close,
    })
}

/// Resolve one inline local-system image to the complete frames it can carry.
///
/// A compact image states the direction triples only, so each decode of the
/// separate origin operand that follows it gives one complete frame. An
/// explicit image is already complete and gives exactly one. Every downstream
/// reader takes a complete frame, so the compact form cannot reach one.
/// A frame candidate before the separate compact origin has been admitted.
/// Explicit candidates carry the raw view of an admitted finite frame.
#[derive(Clone, Copy)]
struct ResolvedInlineLocalSystemFrame {
    values: [f64; 12],
    cursor: usize,
}

fn inline_resolved_frames(
    ctx: &DecodeContext<'_>,
    local: &[u8],
    prefix: scalar::InlineNonPlaneLocalSystemPrefix,
    cache: &scalar::ScalarCache,
) -> Result<impl Iterator<Item = ResolvedInlineLocalSystemFrame>, CodecError> {
    let compact = match prefix {
        scalar::InlineNonPlaneLocalSystemPrefix::Compact(frame) => Some(frame),
        scalar::InlineNonPlaneLocalSystemPrefix::Explicit(_) => None,
    };
    let explicit = match prefix {
        scalar::InlineNonPlaneLocalSystemPrefix::Explicit(frame) => Some(frame),
        scalar::InlineNonPlaneLocalSystemPrefix::Compact(_) => None,
    };
    let compact_origins = match compact {
        Some(frame) => Some((
            frame,
            scalar::decode_inline_non_plane_origin_prefix(ctx, local, frame.cursor, cache)?,
        )),
        None => None,
    };
    Ok(compact_origins
        .into_iter()
        .flat_map(|(frame, origins)| {
            origins.map(move |(origin, cursor)| {
                let mut values = frame.values.get();
                values[9..12].copy_from_slice(&origin);
                ResolvedInlineLocalSystemFrame { values, cursor }
            })
        })
        .chain(
            explicit
                .into_iter()
                .map(|frame| ResolvedInlineLocalSystemFrame {
                    values: frame.values.get(),
                    cursor: frame.cursor,
                }),
        ))
}

fn inline_surface_carrier(
    kind: SurfaceKind,
    envelope: InlineSurfaceEnvelope,
    local: &[u8],
    prefix: ResolvedInlineLocalSystemFrame,
    cache: &scalar::ScalarCache,
) -> Option<InlineSurfaceCarrier> {
    let (suffix, _) = inline_surface_suffix(kind, local, prefix.cursor, cache)?;

    let (axis_index, reference_direction) = inline_frame_directions(prefix)?;
    let axis_index = witnessed_inline_axis_index(envelope, axis_index)?;
    let [.., stored_origin] = local_system_lanes(prefix.values);
    let stored_axis_sense = prefix.values[6 + axis_index];
    let (origin_axis, axis_sense) = solve_inline_axis_endpoint(
        envelope,
        axis_index,
        stored_origin[axis_index],
        stored_axis_sense,
    )?;
    let mut origin = stored_origin;
    origin[axis_index] = origin_axis;
    let mut axis = [0.0; 3];
    axis[axis_index] = axis_sense;

    match kind {
        SurfaceKind::Cylinder => {
            let radius = suffix[0];
            for coordinate in 0..3 {
                if coordinate != axis_index {
                    origin[coordinate] = unique_inline_center(
                        envelope,
                        coordinate,
                        stored_origin[coordinate],
                        radius,
                    )?;
                }
            }
            Some(InlineSurfaceCarrier::Cylinder {
                frame: PositionalCylinderFrame::new(
                    origin,
                    axis,
                    reference_direction,
                    radius,
                    Some((envelope.axial[1] - envelope.axial[0]).abs()),
                )?,
                split_bounds: None,
            })
        }
        SurfaceKind::Cone => {
            let half_angle = ApexConeHalfAngle::new(suffix[0])?;
            let radial_extent = envelope.axial.into_iter().map(f64::abs).fold(0.0, f64::max)
                * half_angle.get().get().tan();
            for coordinate in 0..3 {
                if coordinate != axis_index {
                    origin[coordinate] = unique_inline_center(
                        envelope,
                        coordinate,
                        stored_origin[coordinate],
                        radial_extent,
                    )?;
                }
            }
            Some(InlineSurfaceCarrier::Cone(PositionalConeFrame::new(
                origin,
                axis,
                reference_direction,
                half_angle,
            )?))
        }
        SurfaceKind::TorusOrSphere => {
            let [major_radius, minor_radius] = suffix;
            let radial_extent = major_radius + minor_radius;
            for coordinate in 0..3 {
                if coordinate != axis_index {
                    origin[coordinate] = unique_inline_center(
                        envelope,
                        coordinate,
                        stored_origin[coordinate],
                        radial_extent,
                    )?;
                }
            }
            Some(InlineSurfaceCarrier::Torus(PositionalTorusFrame::new(
                origin,
                axis,
                reference_direction,
                major_radius,
                minor_radius,
            )?))
        }
        _ => None,
    }
}

fn inline_surface_suffix(
    kind: SurfaceKind,
    local: &[u8],
    cursor: usize,
    cache: &scalar::ScalarCache,
) -> Option<([f64; 2], usize)> {
    let (values, end) = decode_inline_surface_suffix_at(kind, local, cursor, cache)?;
    (end == local.len()).then_some((values, end))
}

fn decode_inline_surface_suffix_at(
    kind: SurfaceKind,
    local: &[u8],
    cursor: usize,
    cache: &scalar::ScalarCache,
) -> Option<([f64; 2], usize)> {
    let (values, end) = match kind {
        SurfaceKind::Cylinder | SurfaceKind::Cone => {
            let (value, end) = scalar::decode_inline_surface_suffix_scalar(local, cursor, cache)?;
            ([value, 0.0], end)
        }
        SurfaceKind::TorusOrSphere => {
            let (major, next) = scalar::decode_inline_surface_suffix_scalar(local, cursor, cache)?;
            let (minor, end) = scalar::decode_inline_surface_suffix_scalar(local, next, cache)?;
            ([major, minor], end)
        }
        _ => return None,
    };
    Some((values, end))
}

fn inline_surface_suffix_carrier(
    kind: SurfaceKind,
    local: &[u8],
    prefix: ResolvedInlineLocalSystemFrame,
    cache: &scalar::ScalarCache,
) -> Option<InlineSurfaceCarrier> {
    let (suffix, _) = inline_surface_suffix(kind, local, prefix.cursor, cache)?;
    let (axis, ref_direction) = inline_suffix_frame_directions(prefix)?;
    let [.., origin] = local_system_lanes(prefix.values);
    origin.into_iter().all(f64::is_finite).then_some(())?;
    match kind {
        SurfaceKind::Cylinder => {
            let radius = suffix[0];
            Some(InlineSurfaceCarrier::Cylinder {
                frame: PositionalCylinderFrame::new(origin, axis, ref_direction, radius, None)?,
                split_bounds: None,
            })
        }
        SurfaceKind::Cone => {
            let half_angle = ApexConeHalfAngle::new(suffix[0])?;
            Some(InlineSurfaceCarrier::Cone(PositionalConeFrame::new(
                origin,
                axis,
                ref_direction,
                half_angle,
            )?))
        }
        SurfaceKind::TorusOrSphere => {
            let [major_radius, minor_radius] = suffix;
            Some(InlineSurfaceCarrier::Torus(PositionalTorusFrame::new(
                origin,
                axis,
                ref_direction,
                major_radius,
                minor_radius,
            )?))
        }
        _ => None,
    }
}

fn inline_suffix_frame_directions(
    prefix: ResolvedInlineLocalSystemFrame,
) -> Option<([f64; 3], [f64; 3])> {
    let [first, second, stored_axis, _] = local_system_lanes(prefix.values);
    let norm = |vector: [f64; 3]| {
        vector
            .into_iter()
            .map(|component| component * component)
            .sum::<f64>()
            .sqrt()
    };
    let first_norm = norm(first);
    let second_norm = norm(second);
    let axis_norm = norm(stored_axis);
    let direction_scale = first_norm.max(second_norm).max(axis_norm).max(1.0);
    let close_unit = |value: f64| (value - 1.0).abs() <= EPS_INLINE_FRAME;
    (close_unit(first_norm) && close_unit(second_norm) && close_unit(axis_norm)).then_some(())?;
    let dot = |left: [f64; 3], right: [f64; 3]| {
        left.into_iter()
            .zip(right)
            .map(|(left, right)| left * right)
            .sum::<f64>()
    };
    (dot(first, second).abs() <= EPS_INLINE_FRAME * direction_scale
        && dot(first, stored_axis).abs() <= EPS_INLINE_FRAME * direction_scale
        && dot(second, stored_axis).abs() <= EPS_INLINE_FRAME * direction_scale)
        .then_some(())?;
    let reference_direction = first.map(|value| value / first_norm);
    let axis = stored_axis.map(|value| value / axis_norm);
    Some((axis, reference_direction))
}

/// Recover the frame axis coordinate and the reference direction of a complete
/// inline local system.
///
/// The axis coordinate is the one model axis the stored axis direction lies
/// along; a stored axis that names no single coordinate has no reading here.
fn inline_frame_directions(prefix: ResolvedInlineLocalSystemFrame) -> Option<(usize, [f64; 3])> {
    let [first, second, stored_axis, _] = local_system_lanes(prefix.values);
    let norm = |vector: [f64; 3]| {
        vector
            .into_iter()
            .map(|component| component * component)
            .sum::<f64>()
            .sqrt()
    };
    let first_norm = norm(first);
    let second_norm = norm(second);
    let axis_norm = norm(stored_axis);
    let direction_scale = first_norm.max(second_norm).max(axis_norm).max(1.0);
    let close_unit = |value: f64| (value - 1.0).abs() <= EPS_INLINE_FRAME;
    (close_unit(first_norm) && close_unit(second_norm) && close_unit(axis_norm)).then_some(())?;
    let dot = |left: [f64; 3], right: [f64; 3]| {
        left.into_iter()
            .zip(right)
            .map(|(left, right)| left * right)
            .sum::<f64>()
    };
    (dot(first, second).abs() <= EPS_INLINE_FRAME * direction_scale
        && dot(first, stored_axis).abs() <= EPS_INLINE_FRAME * direction_scale
        && dot(second, stored_axis).abs() <= EPS_INLINE_FRAME * direction_scale)
        .then_some(())?;
    let mut indices = (0..3).filter(|index| {
        stored_axis[*index].abs() >= 1.0 - EPS_INLINE_FRAME
            && (0..3)
                .filter(|other| *other != *index)
                .all(|other| stored_axis[other].abs() <= EPS_INLINE_FRAME)
    });
    let axis_index = indices.next()?;
    indices.next().is_none().then_some(())?;
    let mut reference_direction = first.map(|value| value / first_norm);
    if reference_direction[axis_index].abs() > EPS_INLINE_FRAME {
        return None;
    }
    reference_direction[axis_index] = 0.0;
    let reference_norm = norm(reference_direction);
    (reference_norm.is_finite() && reference_norm > EPS_INLINE_WITNESS).then_some(())?;
    reference_direction = reference_direction.map(|value| value / reference_norm);
    Some((axis_index, reference_direction))
}

fn witnessed_inline_axis_index(
    envelope: InlineSurfaceEnvelope,
    hinted_axis: usize,
) -> Option<usize> {
    let axial_span = (envelope.axial[1] - envelope.axial[0]).abs();
    (axial_span.is_finite() && axial_span > EPS_INLINE_WITNESS).then_some(())?;
    let hinted_span = (envelope.corners[1][hinted_axis]? - envelope.corners[0][hinted_axis]?).abs();
    hinted_span.is_finite().then_some(hinted_axis)
}

fn solve_inline_axis_endpoint(
    envelope: InlineSurfaceEnvelope,
    axis_index: usize,
    stored_axis_origin: f64,
    stored_axis_sense: f64,
) -> Option<(f64, f64)> {
    let stored_magnitude = stored_axis_origin.abs();
    let first_corner = envelope.corners[0][axis_index]?;
    let second_corner = envelope.corners[1][axis_index]?;
    let lower = first_corner.min(second_corner);
    let upper = first_corner.max(second_corner);
    let mut direct = [None; 4];
    let mut crosswise = [None; 4];
    let mut containing = [None; 4];
    let mut endpoint_anchored = [None; 4];
    let push_unique = |candidates: &mut [Option<(f64, f64)>; 4], candidate: (f64, f64)| {
        if !candidates.iter().flatten().any(|existing| {
            inline_close(existing.0, candidate.0) && inline_close(existing.1, candidate.1)
        }) {
            if let Some(slot) = candidates.iter_mut().find(|slot| slot.is_none()) {
                *slot = Some(candidate);
            }
        }
    };
    for origin_sign in [-1.0, 1.0] {
        for direction in [-1.0, 1.0] {
            let origin = origin_sign * stored_magnitude;
            let first = origin + envelope.axial[0] * direction;
            let second = origin + envelope.axial[1] * direction;
            let candidate = (origin, direction);
            if inline_close(first, first_corner) && inline_close(second, second_corner) {
                push_unique(&mut direct, candidate);
            } else if (inline_close(first, lower) && inline_close(second, upper))
                || (inline_close(first, upper) && inline_close(second, lower))
            {
                push_unique(&mut crosswise, candidate);
            } else {
                let candidate_lower = first.min(second);
                let candidate_upper = first.max(second);
                let scale = candidate_lower
                    .abs()
                    .max(candidate_upper.abs())
                    .max(lower.abs())
                    .max(upper.abs())
                    .max(1.0);
                if candidate_lower <= lower + EPS_INLINE_WITNESS * scale
                    && candidate_upper >= upper - EPS_INLINE_WITNESS * scale
                {
                    push_unique(&mut containing, candidate);
                } else if candidate_lower < upper
                    && candidate_upper > lower
                    && ([first, second].into_iter().any(|endpoint| {
                        inline_close(endpoint, lower) || inline_close(endpoint, upper)
                    }))
                {
                    // An oblique trim can extend one outline corner beyond the
                    // stored axial interval. The other endpoint still anchors
                    // the signed local axis to the model-space outline.
                    push_unique(&mut endpoint_anchored, candidate);
                }
            }
        }
    }
    let mut stored_direction = direct
        .iter()
        .chain(&crosswise)
        .chain(&containing)
        .chain(&endpoint_anchored)
        .flatten()
        .filter(|candidate| inline_close(candidate.1, stored_axis_sense))
        .copied();
    if let (Some(candidate), None) = (stored_direction.next(), stored_direction.next()) {
        return Some(candidate);
    }
    for candidates in [direct, crosswise, containing, endpoint_anchored] {
        let mut populated = candidates.into_iter().flatten();
        if let Some(candidate) = populated.next() {
            return populated.next().is_none().then_some(candidate);
        }
    }
    None
}

fn unique_inline_center(
    envelope: InlineSurfaceEnvelope,
    coordinate: usize,
    stored_center: f64,
    radius: f64,
) -> Option<f64> {
    let bounds = envelope.corners.map(|corner| corner[coordinate]);
    if let [None, Some(known)] | [Some(known), None] = bounds {
        let magnitude = stored_center.abs();
        let mut candidates = [-magnitude, magnitude]
            .into_iter()
            .filter(|center| inline_close((known - center).abs(), radius));
        let center = candidates.next()?;
        return candidates
            .all(|candidate| inline_close(candidate, center))
            .then_some(center);
    }
    let [Some(first), Some(second)] = bounds else {
        return None;
    };
    let lower = first.min(second);
    let upper = first.max(second);
    let magnitude = stored_center.abs();
    let mut candidates = [-magnitude, magnitude].into_iter().filter(|center| {
        lower >= center - radius - EPS_INLINE_WITNESS * inline_scale(*center, radius)
            && upper <= center + radius + EPS_INLINE_WITNESS * inline_scale(*center, radius)
    });
    let center = candidates.next()?;
    candidates
        .all(|candidate| inline_close(candidate, center))
        .then_some(center)
}

fn inline_scale(first: f64, second: f64) -> f64 {
    first.abs().max(second.abs()).max(1.0)
}

fn inline_close(first: f64, second: f64) -> bool {
    if !first.is_finite() || !second.is_finite() {
        return false;
    }
    (first - second).abs() <= EPS_INLINE_WITNESS * inline_scale(first, second)
}

pub(crate) fn parameter_records_for_rows(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    rows: &[SurfaceRow],
    cache: &scalar::ScalarCache,
) -> Result<Vec<SurfaceParameterRecord>, CodecError> {
    let mut header_scope = ctx.reserve_scoped(0, "creo surface parameter scratch")?;
    let mut headers = Vec::<(SurfaceRow, usize)>::new();
    for row in ctx.admit_iter(rows, "creo surface parameter rows")? {
        let Some(body_start) = positional_body_start(payload, row) else {
            continue;
        };
        if headers
            .last()
            .is_some_and(|(_, previous_body_start)| row.offset < *previous_body_start)
        {
            continue;
        }
        header_scope
            .with_storage(|| ctx.reserve_vec(&mut headers, 1, "creo surface parameter headers"))?;
        headers.push((row.clone(), body_start));
    }
    let mut records = Vec::new();
    for (index, (row, body_start)) in ctx
        .admit_iter(&headers, "creo surface parameter headers traversal")?
        .enumerate()
    {
        let next_row = headers
            .get(index + 1)
            .map_or(payload.len(), |(next, _)| next.offset);
        let mut body_end = next_row;
        let mut boundary = if next_row < payload.len() {
            SurfaceBodyBoundary::NextRow
        } else {
            SurfaceBodyBoundary::SectionEnd
        };
        let positional_spline_close = if row.kind == SurfaceKind::Spline {
            positional_spline_replay_body_end(
                ctx,
                payload,
                rows,
                row,
                *body_start,
                body_end,
                cache,
            )?
        } else {
            None
        };
        let inline = if positional_spline_close.is_none() {
            inline_surface_body(ctx, row.kind, &payload[*body_start..body_end], &cache)?
        } else {
            None
        };
        if let Some(close) = positional_spline_close {
            body_end = close;
            boundary = SurfaceBodyBoundary::CompoundClose;
        } else if let Some(layout) = inline {
            body_end = body_start + layout.terminal_close;
            boundary = SurfaceBodyBoundary::CompoundClose;
        } else {
            if let Some(relative) =
                surface_body_compound_close(ctx, row.kind, &payload[*body_start..body_end], &cache)?
            {
                body_end = body_start + relative;
                boundary = SurfaceBodyBoundary::CompoundClose;
            }
            if let Some(relative) =
                named_record_boundary(ctx, row.kind, &payload[*body_start..body_end], &cache)?
            {
                body_end = body_start + relative;
                boundary = SurfaceBodyBoundary::NamedRecord;
            }
        }
        let body = ctx.copy_retained(
            &payload[*body_start..body_end],
            "creo surface parameter body",
        )?;
        let scalar_tokens = scalar_tokens(ctx, row.kind, &body, &cache)?;
        let opaque_spans = opaque_spans(ctx, &body, &scalar_tokens)?;
        let scalar_frames = scalar_frames(ctx, &scalar_tokens)?;
        let mut record = SurfaceParameterRecord {
            surface_id: row.id,
            scalar_tokens,
            opaque_spans,
            scalar_frames,
            carrier: SurfaceParameterCarrier::Unresolved(row.kind),
            body,
            boundary,
            offset: row.offset,
            body_offset: *body_start,
        };
        let inline_carrier = inline.and_then(|layout| layout.carrier);
        let carrier = match row.kind {
            SurfaceKind::Cylinder => {
                let frame = match inline_carrier {
                    Some(InlineSurfaceCarrier::Cylinder { frame, .. }) => Some(frame),
                    _ => cylinder_frame_readers::decode_positional_cylinder_frame(
                        ctx,
                        &record.body,
                        cache,
                    )?
                    .or_else(|| record.type24_round_frame(cache)),
                };
                let split_bounds =
                    split_cylinder_outline_bounds(&record.body, &record.scalar_tokens);
                match (frame, split_bounds) {
                    (Some(frame), split_bounds) => Some(InlineSurfaceCarrier::Cylinder {
                        frame,
                        split_bounds,
                    }),
                    (None, Some(bounds)) => Some(InlineSurfaceCarrier::CylinderBounds(bounds)),
                    (None, None) => None,
                }
            }
            SurfaceKind::Cone => match inline_carrier {
                Some(InlineSurfaceCarrier::Cone(frame)) => Some(InlineSurfaceCarrier::Cone(frame)),
                _ => decode_positional_cone_frame_checked(ctx, &record.body, &cache)?
                    .map(InlineSurfaceCarrier::Cone),
            },
            SurfaceKind::TorusOrSphere => match inline_carrier {
                Some(InlineSurfaceCarrier::Torus(frame)) => {
                    Some(InlineSurfaceCarrier::Torus(frame))
                }
                _ => decode_positional_torus_frame(&record.body, &cache)
                    .map(InlineSurfaceCarrier::Torus),
            },
            SurfaceKind::Extrusion(variant) => {
                decode_tabulated_cylinder_frame(ctx, &record.body, &cache)?
                    .map(|(frame, _)| InlineSurfaceCarrier::Tabulated { variant, frame })
            }
            SurfaceKind::Plane | SurfaceKind::Spline | SurfaceKind::Fillet => None,
        };
        if let Some(carrier) = carrier {
            record.carrier = SurfaceParameterCarrier::Resolved(carrier);
        }
        ctx.reserve_vec(&mut records, 1, "creo surface parameter records")?;
        records.push(record);
    }
    Ok(records)
}

/// Decode complete positional surface contour chains from the visible
/// `srf_array` namespace.
pub(crate) fn contour_records(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<SurfaceContourRecord>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo contour_records row scratch")?;
    let rows = scratch.with_storage(|| rows(ctx, payload))?;
    contour_records_for_rows(ctx, payload, &rows)
}

/// Decode complete positional surface contour chains from a DEPDB
/// cross-section namespace.
pub(crate) fn cross_section_contour_records(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<SurfaceContourRecord>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo contour_records row scratch")?;
    let rows = scratch.with_storage(|| cross_section_rows(ctx, payload))?;
    contour_records_for_rows(ctx, payload, &rows)
}

fn contour_records_for_rows(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    rows: &[SurfaceRow],
) -> Result<Vec<SurfaceContourRecord>, CodecError> {
    let cache = scalar::ScalarCache::from_section_checked(ctx, payload)?;
    let mut frame_scope = ctx.reserve_scoped(0, "creo contour frame scratch")?;
    let mut frames = Vec::new();
    for frame in surface_array_frames(ctx, payload) {
        let frame = frame?;
        frame_scope
            .with_storage(|| ctx.reserve_vec(&mut frames, 1, "creo contour surface frames"))?;
        frames.push(frame);
    }
    let mut records = Vec::new();
    for (index, row) in ctx
        .admit_iter(rows, "creo surface contour rows")?
        .enumerate()
    {
        let frame_end = ctx
            .find_by(
                &frames,
                |frame| Ok(row.offset >= frame.start && row.offset < frame.end),
                "creo contour frame lookup",
            )?
            .map_or(payload.len(), |frame| frame.end);
        let row_end = rows
            .get(index + 1)
            .filter(|next| next.offset < frame_end)
            .map_or(frame_end, |next| next.offset);
        let Some(body_start) = positional_body_start(payload, row) else {
            continue;
        };
        if body_start >= row_end {
            continue;
        }
        let positional_spline_close = if row.kind == SurfaceKind::Spline {
            positional_spline_replay_body_end(ctx, payload, rows, row, body_start, row_end, &cache)?
        } else {
            None
        };
        let contour_start = if let Some(close) = positional_spline_close {
            close.checked_add(1)
        } else if let Some(layout) =
            inline_surface_body(ctx, row.kind, &payload[body_start..row_end], &cache)?
        {
            let Some(contour_start) = body_start
                .checked_add(layout.terminal_close)
                .and_then(|close| close.checked_add(1))
            else {
                continue;
            };
            Some(contour_start)
        } else {
            let Some(envelope_close) =
                surface_body_compound_close(ctx, row.kind, &payload[body_start..row_end], &cache)?
                    .map(|relative| body_start + relative)
            else {
                continue;
            };
            let Some(local_system_start) = envelope_close.checked_add(1) else {
                continue;
            };
            let Some(local_system_close) =
                first_compound_close(ctx, payload, local_system_start, row_end)?
            else {
                continue;
            };
            let Some(contour_start) = local_system_close.checked_add(1) else {
                continue;
            };
            Some(contour_start)
        };
        let Some(contour_start) = contour_start else {
            continue;
        };
        append_surface_contour_chain(ctx, payload, contour_start, row_end, row, &cache, &mut records)?;
    }
    ctx.stable_sort_by(
        records.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo contour records for rows records ordering",
    )?;
    Ok(records)
}

fn append_surface_contour_chain(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    row: &SurfaceRow,
    cache: &scalar::ScalarCache,
    records: &mut Vec<SurfaceContourRecord>,
) -> Result<bool, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    struct PendingContour<'a> {
        curve_header_id: u32,
        trv: u8,
        parameter_envelope: [Option<f64>; 4],
        separator_reference: Option<u32>,
        body: &'a [u8],
        offset: usize,
        envelope_offset: usize,
    }
    let mut cursor = start;
    let mut scratch = ctx.reserve_scoped(0, "creo contour chain scratch")?;
    let mut chain = Vec::new();
    let mut steps = start..end;
    loop {
        let contour_start = cursor;
        if cursor.checked_add(2).is_none_or(|next| next > end) { return Ok(false); }
        let Some(_) = ctx.next_charged(&mut steps, "creo surface contour chain traversal")? else { return Ok(false); };
        let Some(head) = payload.get(cursor..cursor + 2) else {
            return Ok(false);
        };
        if !(0x80..=0xbf).contains(&head[0]) {
            return Ok(false);
        }
        let (curve_header_id, after_head) = compact_int(payload, cursor);
        if after_head != cursor + 2 || curve_header_id == 0 {
            return Ok(false);
        }
        let Some(&trv) = payload.get(after_head) else {
            return Ok(false);
        };
        if !matches!(trv, 0x00..=0x03 | 0xf6) {
            return Ok(false);
        }
        let envelope_offset = after_head + 1;
        let mut parameter_envelope = [None; 4];
        cursor = envelope_offset;
        for value in &mut parameter_envelope {
            let Some((decoded, next)) =
                decode_surface_contour_envelope_scalar(payload, cursor, cache)
            else {
                return Ok(false);
            };
            if next > end || decoded.is_some_and(|value| !value.is_finite()) {
                return Ok(false);
            }
            *value = decoded;
            cursor = next;
        }
        if cursor >= end { return Ok(false); }
        let Some(&close) = payload.get(cursor) else {
            return Ok(false);
        };
        if !matches!(close, psb::token::COMPOUND_CLOSE | 0xe1) {
            return Ok(false);
        }
        cursor += 1;
        let contour_end = cursor;
        let terminal = close == 0xe1;
        let separator_reference =
            if !terminal && payload.get(cursor) == Some(&psb::token::ENTITY_REF) {
                let Ok((reference, next)) = psb::reference_id(payload, cursor + 1) else {
                    return Ok(false);
                };
                if next > end {
                    return Ok(false);
                }
                cursor = next;
                Some(reference)
            } else {
                None
            };
        scratch.with_storage(|| {
            ctx.reserve_vec(&mut chain, 1, "creo contour chain candidate entries")
        })?;
        chain.push(PendingContour {
            curve_header_id,
            trv,
            parameter_envelope,
            separator_reference,
            body: &payload[contour_start..contour_end],
            offset: contour_start,
            envelope_offset,
        });
        if terminal {
            ctx.reserve_vec(records, chain.len(), "creo contour chain entries")?;
            let chain_start = records.len();
            for candidate in ctx.admit_iter(chain, "creo contour chain projection")? {
                records.push(SurfaceContourRecord {
                    surface_id: row.id,
                    chain_index: records.len() - chain_start,
                    curve_header_id: candidate.curve_header_id,
                    trv: candidate.trv,
                    parameter_envelope: candidate.parameter_envelope,
                    separator_reference: candidate.separator_reference,
                    body: ctx.copy_retained(candidate.body, "creo contour chain body")?,
                    offset: candidate.offset,
                    envelope_offset: candidate.envelope_offset,
                    surface_row_offset: row.offset,
                });
            }
            return Ok(true);
        }
        if cursor >= end {
            return Ok(false);
        }
    }
}

fn decode_surface_contour_envelope_scalar(
    payload: &[u8],
    offset: usize,
    cache: &scalar::ScalarCache,
) -> Option<(Option<f64>, usize)> {
    if payload.get(offset) == Some(&0x34) {
        payload
            .get(offset..offset.checked_add(3)?)
            .map(|_| (None, offset + 3))
    } else {
        scalar::decode_in_surface_row_lane(payload, offset, cache)
            .map(|(value, next)| (Some(value), next))
    }
}

fn decode_positional_torus_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalTorusFrame> {
    (body.get(8..19)
        == Some(&[
            0x18, 0x94, 0x3f, 0x02, 0x70, 0x16, 0xbe, 0xfc, 0x00, 0x12, 0x20,
        ])
        && body.get(44..49) == Some(&[0x21, 0xb1, 0x48, 0x0a, 0xe3]))
    .then_some(())?;
    let (slots, frame_end) =
        scalar::decode_positional_torus_local_system_prefix(body.get(49..)?, cache)?;
    let mut cursor = 49usize.checked_add(frame_end)?;
    let (major_radius, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
    cursor = next;
    let (signed_minor_radius, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
    (next == body.len()
        && major_radius.is_finite()
        && major_radius > 0.0
        && signed_minor_radius.is_finite()
        && signed_minor_radius != 0.0)
        .then_some(())?;
    let mut envelope = [0.0; 5];
    cursor = 19;
    for coordinate in &mut envelope {
        let (value, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
        value.is_finite().then_some(())?;
        *coordinate = value;
        cursor = next;
    }
    (cursor == 44).then_some(())?;
    let minor_radius = signed_minor_radius.abs();
    let scale = envelope
        .into_iter()
        .chain([major_radius, minor_radius])
        .map(f64::abs)
        .fold(1.0, f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= EPS_SURFACE_AGREEMENT * scale;
    let [a1, a2, b0, b1, b2] = envelope;
    let proves_radii = |outer_delta: f64, minor_delta: f64| {
        close(outer_delta.abs(), 2.0 * (major_radius + minor_radius))
            && close(minor_delta.abs(), minor_radius)
    };
    (close(a1, b0) && (proves_radii(b1 - a1, b2 - a2) ^ proves_radii(b2 - a1, b1 - a2)))
        .then_some(())?;

    let [first, _, second, origin] = local_system_lanes(slots.get());
    let first_norm = first.iter().map(|value| value * value).sum::<f64>().sqrt();
    let second_norm = second.iter().map(|value| value * value).sum::<f64>().sqrt();
    let scale = first_norm.max(second_norm).max(1.0);
    (first_norm.is_finite()
        && second_norm.is_finite()
        && first_norm > EPS_SURFACE_NONZERO
        && second_norm > EPS_SURFACE_NONZERO
        && (first_norm - second_norm).abs() <= EPS_FRAME_AGREEMENT * scale)
        .then_some(())?;
    let ref_direction = first.map(|value| value / first_norm);
    let second = second.map(|value| value / second_norm);
    let orthogonality = ref_direction
        .iter()
        .zip(second)
        .map(|(first, second)| first * second)
        .sum::<f64>();
    (orthogonality.abs() <= EPS_TORUS_FRAME_COLUMN_DOT).then_some(())?;
    let axis = [
        ref_direction[1] * second[2] - ref_direction[2] * second[1],
        ref_direction[2] * second[0] - ref_direction[0] * second[2],
        ref_direction[0] * second[1] - ref_direction[1] * second[0],
    ];
    let axis_norm = axis.iter().map(|value| value * value).sum::<f64>().sqrt();
    (axis_norm.is_finite() && axis_norm > EPS_SURFACE_NONZERO).then_some(())?;
    let axis = axis.map(|value| value / axis_norm);

    PositionalTorusFrame::new(origin, axis, ref_direction, major_radius, minor_radius)
}

fn decode_positional_cone_frame_checked(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Result<Option<PositionalConeFrame>, CodecError> {
    if let Some(frame) = decode_planar_envelope_cone_frame(body, cache) {
        return Ok(Some(frame));
    }
    if let Some(frame) = decode_compound_support_apex_cone_frame(ctx, body, cache)? {
        return Ok(Some(frame));
    }
    Ok(terminal_cone_half_angle_layout(body)
        .and_then(|angle| decode_support_apex_cone_frame(&body[..angle.start], angle.value, cache)))
}

#[cfg(test)]
fn decode_positional_cone_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalConeFrame> {
    crate::decode::with_test_decode_ctx(|ctx| {
        decode_positional_cone_frame_checked(ctx, body, cache)
    })
    .expect("cone fixture admission")
}

fn decode_compound_support_apex_cone_frame(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Result<Option<PositionalConeFrame>, CodecError> {
    let mut candidate = None;
    let mut segment_start = 0;
    let mut offsets = body.iter().enumerate();
    while offsets.len() != 0 {
let Some((segment_end, byte)) =
        ctx.next_charged(&mut offsets, "creo compound cone segment traversal")? else { break; };
        if *byte != psb::token::COMPOUND_CLOSE {
            continue;
        }
        let segment = &body[segment_start..segment_end];
        if let Some(angle) = terminal_cone_half_angle_layout(segment) {
            if let Some(frame) =
                decode_support_apex_cone_frame(&segment[..angle.start], angle.value, cache)
            {
                if candidate.is_some_and(|previous| previous != frame) {
                    return Ok(None);
                }
                candidate = Some(frame);
            }
        }
        segment_start = segment_end + 1;
    }
    Ok(candidate)
}

fn decode_planar_envelope_cone_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalConeFrame> {
    let close = |left: f64, right: f64| {
        let scale = left.abs().max(right.abs()).max(1.0);
        (left - right).abs() <= EPS_SURFACE_AGREEMENT * scale
    };
    let mut cursor = 1;
    let (outer_distance, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
    cursor = next;
    let first_separator = match body.first()? {
        0x15 => 0x18,
        0x17 => 0x15,
        _ => return None,
    };
    (body.get(cursor) == Some(&first_separator)).then_some(())?;
    cursor += 1;
    let (inner_distance, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
    cursor = next;
    let (radial_low, next) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
    cursor = next;
    let (inner_axial, next) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
    cursor = next;
    if body[0] == 0x15 {
        (body.get(cursor) == Some(&0x18)).then_some(())?;
        cursor += 1;
    } else {
        let (repeated_radial_low, next) =
            scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
        close(repeated_radial_low, radial_low).then_some(())?;
        cursor = next;
    }
    let (radial_high, next) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
    cursor = next;
    let (outer_axial, next) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
    cursor = next;
    if body[0] == 0x15 {
        let (repeated_radial_high, next) =
            scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
        close(repeated_radial_high, radial_high).then_some(())?;
        cursor = next;
        (cursor == body.len()).then_some(())?;
    } else {
        let (_, next) = scalar::decode_model_reference_coordinate(body, cursor, cache)?;
        (body.get(next..) == Some(&[0xf7, 0x2c])).then_some(())?;
    }

    [
        outer_distance,
        inner_distance,
        radial_low,
        radial_high,
        inner_axial,
        outer_axial,
    ]
    .into_iter()
    .all(f64::is_finite)
    .then_some(())?;
    (outer_distance > 0.0 && inner_distance > 0.0 && radial_high > 0.0).then_some(())?;
    close(radial_low, -radial_high).then_some(())?;
    let outer_apex = outer_axial - outer_distance;
    let inner_apex = inner_axial - inner_distance;
    close(outer_apex, inner_apex).then_some(())?;
    let half_angle = ApexConeHalfAngle::new(radial_high.atan2(outer_distance))?;
    PositionalConeFrame::new(
        [0.0, outer_apex.midpoint(inner_apex), 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        half_angle,
    )
}

fn decode_support_apex_cone_frame(
    body: &[u8],
    half_angle: ApexConeHalfAngle,
    cache: &scalar::ScalarCache,
) -> Option<PositionalConeFrame> {
    const MAX_SUPPORT_FRAME_BYTES: usize = 12 * 9;

    // Both admitted reference heads consume eight bytes, followed by the
    // three-byte station. There is exactly one possible reference start.
    let reference_start = body.len().checked_sub(8 + 3)?;
    matches!(body.get(reference_start), Some(0x19 | 0x32)).then_some(())?;
    scalar::decode_model_reference_coordinate(body, reference_start, cache)?;
    // The surface-row lane consumes at most eight bytes per scalar, including
    // the eight-byte IEEE form; compact cache references consume at most three.
    let apex_lower = reference_start.saturating_sub(8);
    let mut apex_candidates = (apex_lower..reference_start).filter_map(|start| {
        let (apex, end) = scalar::decode_in_surface_row_lane(body, start, cache)?;
        (end == reference_start && apex.is_finite()).then_some((apex, start))
    });
    let (apex_coordinate, apex_start) = apex_candidates.next()?;
    apex_candidates.next().is_none().then_some(())?;
    // Twelve scalar slots each consume at most nine bytes in this lane.

    let mut sole_slots = None;
    let support_lower = apex_start.saturating_sub(MAX_SUPPORT_FRAME_BYTES - 3);
    for start in support_lower..apex_start {
        let prefix = body.get(start..apex_start)?;
        let mut frame = [0; MAX_SUPPORT_FRAME_BYTES];
        frame[..prefix.len()].copy_from_slice(prefix);
        frame[prefix.len()..prefix.len() + 3].copy_from_slice(&[0x18, 0x18, 0x18]);
        let Some(slots) =
            scalar::decode_positional_plane_local_system_slots(&frame[..prefix.len() + 3], cache)
                .map(cadmpeg_ir::units::FiniteVector::get)
                .filter(|slots| slots[9..12] == [0.0, 0.0, 0.0])
        else {
            continue;
        };
        if sole_slots.replace(slots).is_some() {
            return None;
        }
    }
    let [first, _, second, _] = local_system_lanes(sole_slots?);
    let normalize = |vector: [f64; 3]| {
        let magnitude = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
        (magnitude.is_finite() && magnitude > 0.0).then(|| vector.map(|value| value / magnitude))
    };
    let first = normalize(first)?;
    let second = normalize(second)?;
    (first
        .iter()
        .zip(second)
        .map(|(a, b)| a * b)
        .sum::<f64>()
        .abs()
        <= EPS_SUPPORT_ORTHOGONALITY)
        .then_some(())?;
    let cross = [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ];
    let mut axis = normalize(cross)?;
    let mut axis_indices = axis
        .iter()
        .enumerate()
        .filter_map(|(index, value)| (value.abs() >= 1.0 - EPS_AXIS_ALIGNMENT).then_some(index));
    let axis_index = axis_indices.next()?;
    axis_indices.next().is_none().then_some(())?;
    let mut apex = [0.0; 3];
    apex[axis_index] = apex_coordinate;
    (apex_coordinate.abs() > EPS_SURFACE_NONZERO).then_some(())?;
    if axis[axis_index] * apex_coordinate > 0.0 {
        axis = axis.map(|value| -value);
    }
    PositionalConeFrame::new(apex, axis, second.map(|value| -value), half_angle)
}

/// Decode a named cone prototype whose local-system body carries the complete
/// support-apex suffix and whose half-angle is a single scalar field.
pub(crate) fn prototype_cone_frame(record: &SurfacePrototypeRecord) -> Option<PositionalConeFrame> {
    (record.family == SurfacePrototypeFamily::Cone).then_some(())?;
    let local_system = record.field("local_sys")?;
    local_system
        .body
        .starts_with(&[0xf9, 0x04, 0x03])
        .then_some(())?;
    let SurfaceNamedValue::ScalarSequence(angles) = &record.field("half_angle")?.value else {
        return None;
    };
    let [half_angle] = angles.as_slice() else {
        return None;
    };
    decode_support_apex_cone_frame(
        &local_system.body[3..],
        ApexConeHalfAngle::new(*half_angle)?,
        &scalar::ScalarCache::default(),
    )
}

pub(crate) fn decode_tabulated_cylinder_frame(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Result<Option<(TabulatedCylinderFrame, usize)>, CodecError> {
    const FRAME_MARKER: &[u8] = &[0x00, 0x0c, 0x9a];
    let Some(marker) = ctx.find_map(
        body.get(0..)
            .unwrap_or_default()
            .windows(FRAME_MARKER.len())
            .enumerate(),
        |(offset, bytes)| Ok((bytes == FRAME_MARKER).then_some(offset)),
        "find Creo tabulated cylinder frame",
    )?
    else {
        return Ok(None);
    };
    Ok(tabulated_cylinder_frame_at(
        body,
        marker + FRAME_MARKER.len(),
        cache,
    ))
}

fn tabulated_cylinder_frame_at(
    body: &[u8],
    start: usize,
    cache: &scalar::ScalarCache,
) -> Option<(TabulatedCylinderFrame, usize)> {
    let mut cursor = start;
    let mut values = [0.0; 6];
    let mut prefixes = [0; 6];
    for slot in 0..6 {
        prefixes[slot] = *body.get(cursor)?;
        let (value, next) = if matches!(slot, 1 | 4) && body.get(cursor) == Some(&0x18) {
            (0.0, cursor + 1)
        } else if matches!(slot, 0 | 3)
            || (matches!(slot, 1 | 4) && body.get(cursor) == Some(&0x2d))
        {
            scalar::decode_tabulated_cylinder_first_frame_coordinate(body, cursor, cache)?
        } else {
            scalar::decode_tabulated_cylinder_frame_coordinate(body, cursor, cache)?
        };
        values[slot] = value;
        cursor = next;
    }
    Some((TabulatedCylinderFrame::new(values, prefixes)?, cursor))
}

/// Decode the cubic curve replay owned by the preceding positional
/// `geom_type = 2c` tabulated-cylinder row.
pub(crate) fn tabulated_cylinder_curve_replays(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<TabulatedCylinderCurveReplay>, CodecError> {
    const SIGNATURE: &[u8] = &[
        0x13, 0xe2, 0x01, 0x00, 0x03, 0x18, 0xe6, 0x0f, 0xe6, 0xf8, 0x04, 0xf7,
    ];
    let cache = scalar::ScalarCache::from_section_checked(ctx, payload)?;
    let mut scratch = ctx.reserve_scoped(0, "creo tabulated curve scratch")?;
    let surface_rows = scratch.with_storage(|| rows(ctx, payload))?;
    let mut signatures = Vec::new();
    let mut search = 0;
    while let Some(offset) = ctx.find_map(
        payload
            .get(search..)
            .unwrap_or_default()
            .windows(SIGNATURE.len())
            .enumerate(),
        |(offset, bytes)| Ok((bytes == SIGNATURE).then_some(search + offset)),
        "find Creo surface marker",
    )? {
        scratch.with_storage(|| {
            ctx.reserve_vec(&mut signatures, 1, "creo tabulated curve signatures")
        })?;
        signatures.push(offset);
        search = offset + SIGNATURE.len();
    }
    let mut replays = Vec::new();
    for (index, signature) in ctx
        .admit_iter(&signatures, "creo tabulated curve signature traversal")?
        .copied()
        .enumerate()
    {
        let owner_lower_bound = index
            .checked_sub(1)
            .and_then(|previous| signatures.get(previous).copied())
            .unwrap_or(0);
        let limit = signatures.get(index + 1).copied().unwrap_or(payload.len());
        let Some((curve_id, replay_offset)) = id_ending_at(payload, signature) else {
            continue;
        };
        let reference_start = signature + SIGNATURE.len();
        let Ok((control_point_start, after_control_point_start)) =
            psb::reference_id(payload, reference_start)
        else {
            continue;
        };
        if payload.get(after_control_point_start..after_control_point_start + 3)
            != Some(&[0xfb, 0xe2, 0xf7])
        {
            continue;
        }
        let Ok((successor_reference, control_body_start)) =
            psb::reference_id(payload, after_control_point_start + 3)
        else {
            continue;
        };
        let Some(separator_limit) = limit.checked_sub(3) else {
            continue;
        };
        let mut first_offsets = control_body_start..separator_limit;
        let first_separator_at = |offset| {
            (payload.get(offset..offset + 3) == Some(&[0x18, 0xf1, 0xf7]))
                .then(|| {
                    let (reference, after) = psb::reference_id(payload, offset + 3).ok()?;
                    (reference == control_point_start && payload.get(after) == Some(&0xe2))
                        .then_some((offset, after + 1))
                })
                .flatten()
        };
        let Some((first_separator, first_body_start)) = ctx.find_map(
            &mut first_offsets,
            |offset| Ok(first_separator_at(offset)),
            "creo tabulated first separator search",
        )?
        else {
            continue;
        };
        if ctx
            .find_map(
                &mut first_offsets,
                |offset| Ok(first_separator_at(offset)),
                "creo tabulated first separator search",
            )?
            .is_some()
        {
            continue;
        }
        let Some(terminal_limit) = limit.checked_sub(4) else {
            continue;
        };
        let mut terminal_offsets = first_body_start..terminal_limit;
        let terminal_at = |offset| {
            (payload.get(offset..offset + 3) == Some(&[0x18, 0xf2, 0xf7]))
                .then(|| {
                    let (reference, after) = psb::reference_id(payload, offset + 3).ok()?;
                    (payload.get(after..after + 2) == Some(&[0xf6, 0xe3])).then_some((
                        offset,
                        after + 2,
                        reference,
                    ))
                })
                .flatten()
        };
        let Some((terminal, terminal_end, terminal_reference)) = ctx.find_map(
            &mut terminal_offsets,
            |offset| Ok(terminal_at(offset)),
            "creo tabulated terminal separator search",
        )?
        else {
            continue;
        };
        if ctx
            .find_map(
                &mut terminal_offsets,
                |offset| Ok(terminal_at(offset)),
                "creo tabulated terminal separator search",
            )?
            .is_some()
        {
            continue;
        }
        let mut middle_offsets = first_body_start..terminal;
        let middle_separator_at =
            |offset| Ok((payload.get(offset..offset + 2) == Some(&[0x18, 0xe2])).then_some(offset));
        let Some(second_separator) = ctx.find_map(
            &mut middle_offsets, middle_separator_at,
            "creo tabulated middle separator search",
        )? else { continue; };
        let Some(third_separator) = ctx.find_map(
            &mut middle_offsets, middle_separator_at,
            "creo tabulated middle separator search",
        )? else { continue; };
        if ctx.find_map(
            &mut middle_offsets, middle_separator_at,
            "creo tabulated middle separator search",
        )?.is_some() { continue; }
        let bodies = [
            &payload[control_body_start..first_separator],
            &payload[first_body_start..second_separator],
            &payload[second_separator + 2..third_separator],
            &payload[third_separator + 2..terminal],
        ];
        if bodies.iter().any(|body| body.is_empty()) {
            continue;
        }
        let decode_point = |body: &[u8]| {
            let (first, after_first) =
                scalar::decode_tabulated_cylinder_first_coordinate(body, 0, &cache)?;
            let (second, end) =
                scalar::decode_tabulated_cylinder_second_coordinate(body, after_first, &cache)?;
            (end == body.len() && first.is_finite() && second.is_finite())
                .then_some([first, second])
        };
        let control_points = std::array::from_fn(|index| decode_point(bodies[index]));
        let owner_end = ctx.partition_point(
            &surface_rows,
            |row| Ok(row.offset < replay_offset),
            "creo tabulated curve owner lookup",
        )?;
        let Some(owner) = owner_end
            .checked_sub(1)
            .and_then(|index| surface_rows.get(index))
        else {
            continue;
        };
        if owner.offset <= owner_lower_bound {
            continue;
        }
        if owner.kind != SurfaceKind::Extrusion(ExtrusionVariant::TabulatedCylinder) {
            continue;
        }
        let Some(last_control_point) = control_point_start.checked_add(3) else {
            continue;
        };
        let control_point_bodies = [
            ctx.copy_retained(bodies[0], "creo tabulated control-point body")?,
            ctx.copy_retained(bodies[1], "creo tabulated control-point body")?,
            ctx.copy_retained(bodies[2], "creo tabulated control-point body")?,
            ctx.copy_retained(bodies[3], "creo tabulated control-point body")?,
        ];
        let body = ctx.copy_retained(
            &payload[replay_offset..terminal_end],
            "creo tabulated curve replay body",
        )?;
        let parameter_body =
            ctx.copy_retained(&[0x18, 0xe6, 0x0f, 0xe6], "creo tabulated parameter body")?;
        ctx.reserve_vec(&mut replays, 1, "creo tabulated curve replays")?;
        replays.push(TabulatedCylinderCurveReplay {
            body,
            surface_id: owner.id,
            curve_id,
            curve_type: 0x13,
            flip: 0x01,
            tangent_condition: 0x00,
            degree: 3,
            parameter_body,
            control_point_ids: [
                control_point_start,
                control_point_start + 1,
                control_point_start + 2,
                last_control_point,
            ],
            successor_reference,
            control_point_bodies,
            control_points,
            terminal_reference,
            offset: replay_offset,
            surface_row_offset: owner.offset,
        });
    }
    ctx.stable_sort_by(
        replays.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo tabulated cylinder curve replays replays ordering",
    )?;
    Ok(replays)
}

fn surface_body_compound_close(
    ctx: &DecodeContext<'_>,
    kind: SurfaceKind,
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Result<Option<usize>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if body.is_empty() {
        return Ok(None);
    }
    if kind == SurfaceKind::Plane {
        if let Some(close) = plane_envelope_compound_close(ctx, body, cache)? {
            return Ok(Some(close));
        }
    }
    if kind == SurfaceKind::Cone {
        if let Some(layout) = cone_half_angle_before_close(ctx, body)? {
            return Ok(Some(layout.end));
        }
    }
    if matches!(kind, SurfaceKind::Extrusion(_)) {
        if let Some((_, mut cursor)) = decode_tabulated_cylinder_frame(ctx, body, cache)? {
            if body.get(cursor) == Some(&psb::token::ENTITY_REF) {
                if let Ok((_, next)) = psb::reference_id(body, cursor + 1) {
                    cursor = next;
                }
            }
            if body.get(cursor) == Some(&psb::token::COMPOUND_CLOSE) {
                return Ok(Some(cursor));
            }
        }
    }
    let mut cursor = 0;
    while cursor < body.len() {
        let Some(&byte) = ctx.next_charged(
            &mut body[cursor..].iter(),
            "creo surface compound-close scalar dispatch",
        )?
        else {
            break;
        };
        if byte == psb::token::COMPOUND_CLOSE {
            return Ok(Some(cursor));
        }
        if let Some((_, next)) = decode_row_scalar(kind, body, cursor, cache) {
            cursor = next;
        } else {
            cursor += 1;
        }
    }
    Ok(None)
}

fn first_compound_close(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
) -> Result<Option<usize>, CodecError> {
    const OUTLINE_PAIR_CLOSE: &[u8; 4] = &[0x00, 0x0c, 0x98, psb::token::COMPOUND_CLOSE];
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(body) = payload.get(start..end).filter(|body| !body.is_empty()) else {
        return Ok(None);
    };
    let mut windows = body.windows(OUTLINE_PAIR_CLOSE.len()).enumerate();
    let mut separator_close = None;
    while windows.len() != 0 {
        let Some((offset, window)) =
            ctx.next_charged(&mut windows, "creo outline pair close scan")?
        else {
            break;
        };
        if window == OUTLINE_PAIR_CLOSE {
            separator_close = Some(start + offset + OUTLINE_PAIR_CLOSE.len() - 1);
            break;
        }
    }
    for token in psb::tokens(ctx, body) {
        let token = token?;
        match token.kind {
            psb::TokenKind::CompoundClose => {
                return Ok(Some(
                    separator_close.map_or(start + token.offset, |separator| {
                        separator.min(start + token.offset)
                    }),
                ))
            }
            psb::TokenKind::NamedRecord => return Ok(separator_close),
            _ => {}
        }
    }
    Ok(separator_close)
}

fn plane_local_system_compound_close(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<usize>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    // Keep the established structural boundary when it exists. Some local
    // systems contain an e0 byte inside a numeric token; in that case the
    // generic scanner can stop without finding the following e3. Validate a
    // complete frame only as the recovery path for that false boundary.
    if let Some(close) = first_compound_close(ctx, payload, start, end)? {
        return Ok(Some(close));
    }
    let mut closes = start..end;
    while !closes.is_empty() {
        let Some(close) = ctx.next_charged(&mut closes, "creo plane local-system close scan")?
        else {
            break;
        };
        if payload.get(close) == Some(&psb::token::COMPOUND_CLOSE)
            && complete_plane_local_system(ctx, &payload[start..close], cache)?.is_some()
        {
            return Ok(Some(close));
        }
    }
    Ok(None)
}

/// The declared slots of a bounded spline scalar body, with their source
/// tokens, or `None` when the body does not encode exactly its declared slots.
///
/// This is the same bounded scalar body [`scalar_slots`] reads, with the spline
/// coordinate lanes added, and it obeys the same two rules: a body that ends
/// before its declared count is refused, and a body with bytes left after its
/// last declared slot is refused. An unresolved seven-byte token is a slot the
/// body does encode; it stays in position with no value. A `f9 00`
/// continuation in an interpolation-point field encodes the final zero slot of
/// its tuple with no token bytes of its own.
fn named_spline_scalar_slots(
    ctx: &DecodeContext<'_>,
    family: &SurfacePrototypeFamily,
    name: &str,
    body: &[u8],
    count: usize,
    cache: &scalar::ScalarCache,
    refusal: &mut ScalarBodyRefusal,
) -> Result<Option<Vec<ScalarTokenSlot>>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut slots = Vec::new();
    ctx.reserve_vec(&mut slots, count, "creo named spline scalar slots")?;
    let mut cursor = psb::Cursor::new(body);
    let mut continued_tuple = false;
    let mut positions = 0..body.len();
    while slots.len() < count && !positions.is_empty() {
        let Some(start) = ctx.next_charged(&mut positions, "creo named spline scalar dispatch")?
        else {
            break;
        };
        if matches!(name, "i_pnts" | "i_points")
            && cursor.take_slice_if(&[psb::token::SCALAR_BODY, 0x00])
        {
            continued_tuple = true;
            positions.start = cursor.pos();
            continue;
        }
        let Some(value) =
            cursor.take_with(|data, pos| named_spline_scalar_slot(family, name, data, pos, cache))
        else {
            break;
        };
        slots.push((
            value,
            ctx.copy_retained(&body[start..cursor.pos()], "creo named spline scalar token")?,
        ));
        positions.start = cursor.pos();
    }
    if matches!(name, "i_pnts" | "i_points")
        && continued_tuple
        && cursor.pos() == body.len()
        && slots.len() + 1 == count
    {
        slots.push((Some(0.0), Vec::new()));
    }
    if slots.len() != count {
        refusal.state(scalar_body_refusal(
            ctx,
            body,
            count,
            slots.len(),
            cursor.pos(),
        )?);
        return Ok(None);
    }
    if cursor.pos() != body.len() {
        refusal.state(trailing_scalar_body_refusal(
            ctx,
            body,
            count,
            cursor.pos(),
        )?);
        return Ok(None);
    }
    Ok(Some(slots))
}

fn named_vector_scalar_body_len(
    ctx: &DecodeContext<'_>,
    family: &SurfacePrototypeFamily,
    name: &str,
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Result<Option<usize>, CodecError> {
    let Some((values_start, slot_count)) = named_vector_scalar_extent(name, body) else {
        return Ok(None);
    };
    let mut cursor = psb::Cursor::at(body, values_start);
    let mut slots = 0;
    let mut steps = 0..body.len();
    while slots < slot_count {
        if ctx
            .next_charged(&mut steps, "creo named vector extent traversal")?
            .is_none()
        {
            return Ok(None);
        }
        if matches!(name, "i_pnts" | "i_points")
            && cursor.take_slice_if(&[psb::token::SCALAR_BODY, 0x00])
        {
            continue;
        }
        if cursor
            .take_with(|data, pos| named_spline_scalar_slot(family, name, data, pos, cache))
            .is_none()
        {
            return Ok(None);
        }
        slots += 1;
    }
    Ok(Some(cursor.pos()))
}

fn named_vector_scalar_extent(name: &str, body: &[u8]) -> Option<(usize, usize)> {
    matches!(
        name,
        "i_pnts"
            | "i_points"
            | "end_u_tangts"
            | "end_v_tangts"
            | "end_uv_deriv"
            | "tangts"
            | "end_tangts"
    )
    .then_some(())?;
    (body.first() == Some(&psb::token::SCALAR_BODY)).then_some(())?;
    let (dimensions, dimensions_end) = compact_int(body, 1);
    let (count, values_start) = compact_int(body, dimensions_end);
    (dimensions_end > 1 && values_start > dimensions_end).then_some(())?;
    let slot_count = usize::try_from(dimensions)
        .ok()?
        .checked_mul(usize::try_from(count).ok()?)?;
    Some((values_start, slot_count))
}

/// The most slots one byte of a counted `params` value body states.
///
/// Read from `docs/formats/creo_prt.md`: "In a counted `params` scalar array,
/// `e5` supplies two consecutive zero slots and `e6` supplies three. The
/// expanded slots must exactly match the declared count." Every other form
/// [`counted_parameter_scalar_slots`] admits states one slot, and every step of
/// that walk consumes at least one byte, so three slots per byte is the densest
/// parse it can answer.
const MAX_COUNTED_PARAMETER_SLOTS_PER_BYTE: u64 = 3;

/// The value bytes of a counted `params` body whose declared slot count those
/// bytes can carry, or `None` when the record states no such body.
///
/// `body.get(values_start..)` refuses a `values_start` past the body: such a
/// record states no value bytes at all, and it is refused rather than read as
/// "zero bytes remain". The count bound is the one
/// [`counted_parameter_scalar_slots`] enforces: at most
/// [`MAX_COUNTED_PARAMETER_SLOTS_PER_BYTE`] slots come from one byte, and that
/// walk answers only a parse ending on the last byte with exactly `count`
/// slots, so a denser count states more slots than the bytes carry. The bound
/// runs before the counted token walk and final scalar construction, so an
/// impossible count cannot allocate slot storage.
fn admitted_counted_parameter_body(body: &[u8], values_start: usize, count: u32) -> Option<&[u8]> {
    let remaining = body.get(values_start..)?;
    let value_bytes = u64::from(count).div_ceil(MAX_COUNTED_PARAMETER_SLOTS_PER_BYTE);
    bounded_len(value_bytes, 1, remaining.len()).map(|_| remaining)
}

fn counted_parameter_scalar_slots(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    count: usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<Vec<ScalarTokenSlot>>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    if count == 0 || body.is_empty() {
        return Ok((count == 0 && body.is_empty()).then(Vec::new));
    }
    let state_count = body.len().checked_add(1).ok_or_else(|| {
        ctx.refuse_codec_limit("creo_counted_parameter_slots", u64::MAX, u64::MAX)
    })?;
    let mut scratch = ctx.reserve_scoped(0, "creo counted parameter scratch")?;
    let mut states =
        scratch.with_storage(|| ctx.collection_vec(state_count, "creo_counted_parameter_slots"))?;
    for _ in ctx.admit_iter(
        0..state_count,
        "creo counted parameter state initialization",
    )? {
        states.push((
            BTreeMap::<usize, CountedParameterParse>::new(),
            ctx.reserve_scoped(0, "creo counted parameter state scratch")?,
        ));
    }
    let mut nodes = Vec::new();
    let (initial, initial_scope) = &mut states[0];
    initial_scope.with_storage(|| {
        ctx.insert_btree_map(
            initial,
            0,
            CountedParameterParse::Unique(None),
            "creo counted parameter initial state",
        )
    })?;
    for cursor in ctx.admit_iter(0..body.len(), "creo counted parameter state traversal")? {
        let (current, _current_scope) = std::mem::replace(
            &mut states[cursor],
            (
                BTreeMap::new(),
                ctx.reserve_scoped(0, "creo counted parameter state scratch")?,
            ),
        );
        for (slots_used, parse) in ctx.admit_iter(current, "creo counted parameter alternatives")? {
            if slots_used >= count {
                continue;
            }
            let run = match body[cursor] {
                0xe5 => 2,
                0xe6 => 3,
                _ => 0,
            };
            if run > 0 {
                if slots_used
                    .checked_add(run)
                    .is_some_and(|next| next <= count)
                {
                    let mut candidate = parse;
                    for position in 0..run {
                        let raw = if position == 0 {
                            &body[cursor..=cursor]
                        } else {
                            &[]
                        };
                        candidate = append_counted_parameter_node(
                            ctx,
                            &mut scratch,
                            &mut nodes,
                            candidate,
                            Some(0.0),
                            raw,
                        )?;
                    }
                    let (state, scope) = &mut states[cursor + 1];
                    scope.with_storage(|| {
                        add_counted_parameter_state(ctx, state, slots_used + run, candidate)
                    })?;
                }
                continue;
            }
            if body[cursor] == 0x18 {
                let candidate = append_counted_parameter_node(
                    ctx,
                    &mut scratch,
                    &mut nodes,
                    parse,
                    Some(0.0),
                    &body[cursor..=cursor],
                )?;
                let (state, scope) = &mut states[cursor + 1];
                scope.with_storage(|| {
                    add_counted_parameter_state(ctx, state, slots_used + 1, candidate)
                })?;
                if let Some((value, next)) = scalar::decode_in_lane(body, cursor, cache)
                    .filter(|(_, next)| *next > cursor + 1)
                {
                    let candidate = append_counted_parameter_node(
                        ctx,
                        &mut scratch,
                        &mut nodes,
                        parse,
                        Some(value),
                        &body[cursor..next],
                    )?;
                    let (state, scope) = &mut states[next];
                    scope.with_storage(|| {
                        add_counted_parameter_state(ctx, state, slots_used + 1, candidate)
                    })?;
                }
                continue;
            }
            if let Some((value, next)) = named_spline_scalar_slot(
                &SurfacePrototypeFamily::Spline(SplineLabel::Spline),
                "params",
                body,
                cursor,
                cache,
            ) {
                let candidate = append_counted_parameter_node(
                    ctx,
                    &mut scratch,
                    &mut nodes,
                    parse,
                    value,
                    &body[cursor..next],
                )?;
                let (state, scope) = &mut states[next];
                scope.with_storage(|| {
                    add_counted_parameter_state(ctx, state, slots_used + 1, candidate)
                })?;
            }
        }
    }
    let Some(CountedParameterParse::Unique(mut tail)) = ctx.remove_btree_map(
        &mut states[body.len()].0,
        &count,
        "creo counted parameter final lookup",
    )?
    else {
        return Ok(None);
    };
    let mut slots = ctx.collection_vec(count, "creo counted parameter result slots")?;
    while let Some(index) = tail {
        let mut step = std::iter::once(index);
        let Some(index) = ctx.next_charged(&mut step, "creo counted parameter result traversal")? else { break; };
        let node = &nodes[index];
        tail = node.previous;
        slots.push((node.value, ctx.copy_retained(node.raw, "creo counted parameter token bytes")?));
    }
    ctx.reverse(&mut slots, "creo counted parameter result reversal")?;
    Ok(Some(slots))
}

#[derive(Clone, Copy)]
enum CountedParameterParse {
    Unique(Option<usize>),
    Ambiguous,
}

struct CountedParameterNode<'a> {
    previous: Option<usize>,
    value: Option<f64>,
    raw: &'a [u8],
}

fn append_counted_parameter_node<'a>(
    ctx: &DecodeContext<'_>,
    scratch: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    nodes: &mut Vec<CountedParameterNode<'a>>,
    parse: CountedParameterParse,
    value: Option<f64>,
    raw: &'a [u8],
) -> Result<CountedParameterParse, CodecError> {
    let CountedParameterParse::Unique(previous) = parse else {
        return Ok(CountedParameterParse::Ambiguous);
    };
    scratch.with_storage(|| ctx.reserve_vec(nodes, 1, "creo counted parameter path nodes"))?;
    let tail = nodes.len();
    nodes.push(CountedParameterNode {
        previous,
        value,
        raw,
    });
    Ok(CountedParameterParse::Unique(Some(tail)))
}

fn add_counted_parameter_state(
    ctx: &DecodeContext<'_>,
    states: &mut BTreeMap<usize, CountedParameterParse>,
    slots_used: usize,
    candidate: CountedParameterParse,
) -> Result<(), CodecError> {
    match ctx.entry_btree_map(states, slots_used, "creo counted parameter state entries")? {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(candidate);
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            entry.insert(CountedParameterParse::Ambiguous);
        }
    }
    Ok(())
}

fn named_spline_scalar_slot(
    family: &SurfacePrototypeFamily,
    name: &str,
    body: &[u8],
    offset: usize,
    cache: &scalar::ScalarCache,
) -> Option<(Option<f64>, usize)> {
    let head = *body.get(offset)?;
    if head == 0x18 && offset + 1 == body.len() {
        return Some((Some(0.0), offset + 1));
    }
    if matches!(head, 0x0d | 0x0f | 0x18 | 0xe4 | 0xe6) {
        return scalar::decode_in_lane(body, offset, cache)
            .map(|(value, next)| (Some(value), next));
    }
    if matches!(head, 0x29 | 0x2a | 0x2e | 0x2f | 0x42 | 0x43 | 0x47 | 0x48) {
        return scalar::decode_in_lane(body, offset, cache)
            .map(|(value, next)| (Some(value), next));
    }
    if matches!(head, 0x28 | 0x41) {
        return scalar::ieee8(body, offset, 0x3f).map(|(value, next)| (Some(value), next));
    }
    if name == "params" && head == 0x2d {
        return scalar::ieee8(body, offset, 0x40).map(|(value, next)| (Some(value), next));
    }
    if matches!(head, 0x2d | 0x46) {
        return scalar::decode_in_lane(body, offset, cache)
            .map(|(value, next)| (Some(value), next));
    }
    if matches!(name, "end_v_tangts" | "end_uv_deriv" | "end_tangts") {
        return scalar::decode_tabulated_cylinder_second_coordinate(body, offset, cache)
            .map(|(value, next)| (Some(value), next));
    }
    if matches!(family, SurfacePrototypeFamily::Fillet(_))
        && name == "tangts"
        && scalar::is_tabulated_cylinder_second_coordinate_opener(head)
    {
        return scalar::decode_tabulated_cylinder_second_coordinate(body, offset, cache)
            .map(|(value, next)| (Some(value), next));
    }
    if matches!(family, SurfacePrototypeFamily::Fillet(_))
        && matches!(name, "i_pnts" | "i_points")
        && matches!(head, 0xa4..=0xdf)
    {
        return scalar::decode_tabulated_cylinder_second_coordinate(body, offset, cache)
            .map(|(value, next)| (Some(value), next));
    }
    if matches!(name, "i_pnts" | "i_points") && matches!(head, 0x5b..=0xa3) {
        return named_positive_dict(body, offset).map(|(value, next)| (Some(value), next));
    }
    if head == 0x71 {
        return scalar::decode_in_lane(body, offset, cache)
            .map(|(value, next)| (Some(value), next));
    }
    if matches!(name, "i_pnts" | "i_points") && matches!(head, 0xb3 | 0xb9) {
        return scalar::decode_in_lane(body, offset, cache)
            .map(|(value, next)| (Some(value), next));
    }
    if matches!(name, "u_params" | "v_params" | "params") {
        return named_positive_dict(body, offset).map(|(value, next)| (Some(value), next));
    }
    if name == "end_u_tangts" && head == 0x31 {
        return scalar::ieee7(body, offset, 0x40).map(|(value, next)| (Some(value), next));
    }
    if name == "end_uv_deriv" && matches!(head, 0x7d | 0x8f) {
        return named_positive_dict(body, offset).map(|(value, next)| (Some(value), next));
    }
    let next = offset.checked_add(7)?;
    (next <= body.len()).then_some((None, next))
}

fn named_positive_dict(body: &[u8], offset: usize) -> Option<(f64, usize)> {
    // wrapping-exception: DICT prefix remapping reconstructs the low IEEE byte modulo 256
    let second = body.get(offset)?.wrapping_sub(0x8b);
    let first = if second >= 0x80 { 0x3f } else { 0x40 };
    scalar::ieee7_with_prefix(body, offset, first, second)
}

/// Why a bounded scalar body was refused, stated for the record that holds it.
///
/// The decoder that refuses knows the slot and the byte; the reader that
/// reports knows the record and the field. This carries the first to the
/// second, so a refusal names its instance instead of leaving only an opaque
/// body behind.
#[derive(Debug, Default)]
struct ScalarBodyRefusal(Option<String>);

impl ScalarBodyRefusal {
    fn state(&mut self, reason: String) {
        self.0 = Some(reason);
    }

    /// The stated reason, when the body was refused for one.
    fn reason(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

/// The declared slots of a bounded scalar body, in stored order, or `None` when
/// the body does not encode exactly its declared slots.
///
/// `docs/formats/creo_prt.md` states of this body that it "encodes its declared
/// slots sequentially; no byte may be skipped between slot encodings". A byte
/// that no scalar encoding defines therefore cannot be passed over, and that
/// line gives such a byte no width of its own. A body that states a byte no
/// encoding defines is refused; the record then takes the route a named value
/// that does not decode already takes.
///
/// The bounded scalar body obeys two further rules, shared with
/// [`sequential_named_local_system_slots`] and [`named_spline_scalar_slots`]:
///
/// * A body that ends before its declared count is refused. The format states
///   that the field "declares exactly `dimensions * count` scalar slots" and
///   that "[e]ach declared slot consumes one complete scalar token"
///   (`docs/formats/creo_prt.md`). A slot past the end of the body consumes no
///   token, so the body does not encode what it declares.
/// * A body with bytes left after its last declared slot is refused. The
///   format states no rule for such a byte in this field, so the decoder
///   invents none and refuses.
///
/// An unresolved slot is a slot whose token the body does encode and whose
/// value no lane defines. It is not a slot the body omits.
fn scalar_slots(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    count: usize,
    cache: &scalar::ScalarCache,
    refusal: &mut ScalarBodyRefusal,
) -> Result<Option<Vec<Option<f64>>>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut slots = Vec::new();
    ctx.reserve_vec(&mut slots, count, "creo scalar body slots")?;
    let mut positions = 0..body.len();
    while slots.len() < count && !positions.is_empty() {
        let Some(cursor) = ctx.next_charged(&mut positions, "creo scalar body dispatch")? else {
            break;
        };
        let Some((value, next)) = scalar::decode_in_lane(body, cursor, cache) else {
            refusal.state(scalar_body_refusal(ctx, body, count, slots.len(), cursor)?);
            return Ok(None);
        };
        slots.push(Some(value));
        positions.start = next;
    }
    let cursor = positions.start;
    if slots.len() != count {
        refusal.state(scalar_body_refusal(ctx, body, count, slots.len(), cursor)?);
        return Ok(None);
    }
    if cursor != body.len() {
        refusal.state(trailing_scalar_body_refusal(ctx, body, count, cursor)?);
        return Ok(None);
    }
    Ok(Some(slots))
}

/// The reason a bounded scalar body states no slot at `cursor`: the byte no
/// scalar form defines, or the end of a body that declares more slots than it
/// encodes.
fn scalar_body_refusal(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    count: usize,
    slot: usize,
    cursor: usize,
) -> Result<String, CodecError> {
    match body.get(cursor) {
        Some(byte) => ctx.format_retained(
            format_args!(
                "declares {count} scalar slots and states byte 0x{byte:02x} at slot {slot}, \
             which no scalar form defines"
            ),
            "creo scalar body refusal text",
        ),
        None => ctx.format_retained(
            format_args!(
                "declares {count} scalar slots and encodes {slot} in {} bytes",
                body.len()
            ),
            "creo scalar body refusal text",
        ),
    }
}

/// The reason a bounded scalar body that encodes every declared slot is still
/// refused: bytes are left after the last slot.
fn trailing_scalar_body_refusal(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    count: usize,
    cursor: usize,
) -> Result<String, CodecError> {
    ctx.format_retained(
        format_args!(
            "declares {count} scalar slots and ends them at byte {cursor} of {}",
            body.len()
        ),
        "creo trailing scalar body refusal text",
    )
}

type ScalarTokenSlot = (Option<f64>, Vec<u8>);

/// The `count` surface-row scalar slots `body` states, each with the bytes it
/// was decoded from, and the offset the decode ended at.
///
/// `None` when the body states no such table. The format skips no byte between
/// slot encodings, so a byte this lane defines no scalar form for ends the
/// table, and a body that runs out before `count` slots states fewer slots than
/// it declares. Neither is padded. Every slot of a returned table therefore
/// holds a value and the non-empty byte run it was decoded from, and those byte
/// runs are exactly the bytes from zero to the returned offset.
///
/// The offset is returned because a caller that owns the field's end decides
/// whether the table is allowed to stop short of it; that is the caller's
/// question, not this one's.
#[derive(Debug)]
struct ScalarTokenTable<'a> {
    slots: Vec<(Option<f64>, &'a [u8])>,
    consumed: usize,
}

fn scalar_slots_with_tokens_and_end<'a>(
    ctx: &DecodeContext<'_>,
    body: &'a [u8],
    count: usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<ScalarTokenTable<'a>>, CodecError> {
    let mut slots = Vec::new();
    ctx.reserve_vec(&mut slots, count, "creo surface scalar token slots")?;
    let mut cursor = 0;
    while cursor < body.len() && slots.len() < count {
        if body[cursor] == 0x18 && cursor + 1 == body.len() {
            slots.push((Some(0.0), &body[cursor..=cursor]));
            cursor += 1;
        } else if let Some((value, next)) = scalar::decode_in_surface_row_lane(body, cursor, cache)
        {
            // Every arm of `decode_in_surface_row_lane` reads the bytes it
            // reports, so `cursor < next <= body.len()`. The `get` is the
            // bounded access, not a second test of that range.
            let Some(token) = body.get(cursor..next) else {
                return Ok(None);
            };
            slots.push((Some(value), token));
            cursor = next;
        } else {
            return Ok(None);
        }
    }
    Ok((slots.len() == count).then_some(ScalarTokenTable {
        slots,
        consumed: cursor,
    }))
}

/// The `count` plane-envelope scalar slots `body` states, each with the bytes
/// it was decoded from, and the offset the decode ended at.
///
/// The envelope lane adds the compact positive half `0e` to the surface-row
/// lane. Everything [`scalar_slots_with_tokens_and_end`] states about an
/// undefined byte, a short body and the returned offset holds here too.
fn plane_envelope_scalar_slots_with_tokens_and_end<'a>(
    ctx: &DecodeContext<'_>,
    body: &'a [u8],
    count: usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<ScalarTokenTable<'a>>, CodecError> {
    let mut slots = Vec::new();
    ctx.reserve_vec(&mut slots, count, "creo plane envelope token slots")?;
    let mut cursor = 0;
    while cursor < body.len() && slots.len() < count {
        if body[cursor] == 0x0e {
            slots.push((Some(0.5), &body[cursor..=cursor]));
            cursor += 1;
        } else if body[cursor] == 0x18 && cursor + 1 == body.len() {
            slots.push((Some(0.0), &body[cursor..=cursor]));
            cursor += 1;
        } else if let Some((value, next)) = scalar::decode_in_surface_row_lane(body, cursor, cache)
        {
            // Every arm of `decode_in_surface_row_lane` reads the bytes it
            // reports, so `cursor < next <= body.len()`. The `get` is the
            // bounded access, not a second test of that range.
            let Some(token) = body.get(cursor..next) else {
                return Ok(None);
            };
            slots.push((Some(value), token));
            cursor = next;
        } else {
            return Ok(None);
        }
    }
    Ok((slots.len() == count).then_some(ScalarTokenTable {
        slots,
        consumed: cursor,
    }))
}

fn complete_plane_envelope_slots<'a>(
    ctx: &DecodeContext<'_>,
    body: &'a [u8],
    count: usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<ScalarTokenTable<'a>>, CodecError> {
    let Some(slots) = plane_envelope_scalar_slots_with_tokens_and_end(ctx, body, count, cache)?
    else {
        return Ok(None);
    };
    // The helper states every other condition. This is the caller's own: the
    // envelope owns the whole body, so a table that stops short of its end
    // leaves bytes no slot accounts for.
    Ok((slots.consumed == body.len()).then_some(slots))
}

fn complete_plane_envelope_slots_with_final_positive_dict<'a>(
    ctx: &DecodeContext<'_>,
    body: &'a [u8],
    preceding_count: usize,
    cache: &scalar::ScalarCache,
) -> Result<Option<ScalarTokenTable<'a>>, CodecError> {
    let Some(positive_start) = body.len().checked_sub(7) else {
        return Ok(None);
    };
    let Some((value, end)) = scalar::decode_positive_dict(body, positive_start) else {
        return Ok(None);
    };
    if end != body.len() {
        return Ok(None);
    }
    let Some(mut slots) =
        complete_plane_envelope_slots(ctx, &body[..positive_start], preceding_count, cache)?
    else {
        return Ok(None);
    };
    ctx.reserve_vec(&mut slots.slots, 1, "creo plane envelope final token slot")?;
    slots.slots.push((Some(value), &body[positive_start..end]));
    slots.consumed = body.len();
    Ok(Some(slots))
}

fn plane_envelope_compound_close(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Result<Option<usize>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    // The standard prefix has nine scalar slots, each at most eight bytes;
    // the compact prefix has one marker and eight such slots. The final
    // positive-DICT scalar consumes seven bytes. No later close can qualify.
    const MAX_ENVELOPE_CLOSE: usize = 9 * 8 + 7;
    for (offset, byte) in body.iter().take(MAX_ENVELOPE_CLOSE + 1).enumerate() {
        if *byte != psb::token::COMPOUND_CLOSE {
            continue;
        }
        let Some(positive_start) = offset.checked_sub(7) else {
            continue;
        };
        let Some((positive_value, positive_end)) =
            scalar::decode_positive_dict(body, positive_start)
        else {
            continue;
        };
        if positive_end != offset {
            continue;
        }
        let prefix = &body[..positive_start];
        let (slots, pairs) = if prefix.first() == Some(&0x0e) {
            let Some(slots) = complete_plane_envelope_slots(ctx, &prefix[1..], 8, cache)? else {
                continue;
            };
            (slots, [[3, 6], [4, 7], [5, 8]])
        } else if let Some(slots) = complete_plane_envelope_slots(ctx, prefix, 9, cache)? {
            (slots, [[4, 7], [5, 8], [6, 9]])
        } else {
            continue;
        };
        let mut slots = slots;
        ctx.reserve_vec(&mut slots.slots, 1, "creo plane envelope close token slot")?;
        slots
            .slots
            .push((Some(positive_value), &body[positive_start..positive_end]));
        let axis_aligned = plane_envelope_has_one_held_coordinate(ctx, &slots.slots, pairs)?;
        if axis_aligned || plane_envelope_boundary_has_local_system(ctx, body, offset, cache)? {
            return Ok(Some(offset));
        }
    }
    Ok(None)
}

fn plane_envelope_has_one_held_coordinate(
    ctx: &DecodeContext<'_>,
    slots: &[(Option<f64>, &[u8])],
    pairs: [[usize; 2]; 3],
) -> Result<bool, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut count = 0usize;
    for [first, second] in pairs {
        if ctx.equal_bytes(
            slots[first].1,
            slots[second].1,
            "creo plane envelope held coordinate bytes",
        )? {
            count += 1;
        }
    }
    Ok(count == 1)
}

fn plane_envelope_boundary_has_local_system(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    envelope_close: usize,
    cache: &scalar::ScalarCache,
) -> Result<bool, CodecError> {
    let Some(local_system_start) = envelope_close.checked_add(1) else {
        return Ok(false);
    };
    if local_system_start == body.len() {
        return Ok(false);
    }
    let Some(local_system_close) =
        plane_local_system_compound_close(ctx, body, local_system_start, body.len(), cache)?
    else {
        return Ok(false);
    };
    Ok(
        complete_plane_local_system(ctx, &body[local_system_start..local_system_close], cache)?
            .is_some(),
    )
}

fn slot_equality(first: &(Option<f64>, &[u8]), second: &(Option<f64>, &[u8])) -> Option<bool> {
    match (first.0, second.0) {
        (Some(first), Some(second)) => {
            let scale = first.abs().max(second.abs()).max(1.0);
            Some((first - second).abs() <= EPS_SURFACE_AGREEMENT * scale)
        }
        (None, None) if !first.1.is_empty() && !second.1.is_empty() => Some(first.1 == second.1),
        _ => None,
    }
}

/// The declared slots of a bounded `local_sys` scalar body, in stored order,
/// or `None` when the body does not encode exactly its declared slots.
///
/// This body is the same bounded scalar body [`scalar_slots`] reads, with the
/// `local_sys` slot forms added, and it obeys the same two rules: a body that
/// ends before its declared count is refused, and a body with bytes left after
/// its last declared slot is refused. An `e7 <count>` run advances over
/// inherited slots that the body does encode and that carry no value; those
/// slots are `None` and are not omitted slots.
fn sequential_named_local_system_slots(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    count: usize,
    cache: &scalar::ScalarCache,
    refusal: &mut ScalarBodyRefusal,
) -> Result<Option<Vec<Option<f64>>>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut slots = Vec::new();
    ctx.reserve_vec(&mut slots, count, "creo local-system scalar slots")?;
    let mut positions = 0..body.len();
    while !positions.is_empty() && slots.len() < count {
        let Some(cursor) = ctx.next_charged(&mut positions, "creo local-system scalar dispatch")?
        else {
            break;
        };
        if body.get(cursor) == Some(&0xe7) {
            let (inherited_count, next) = compact_int(body, cursor + 1);
            let Ok(inherited_count) = usize::try_from(inherited_count) else {
                return Ok(None);
            };
            if next <= cursor + 1
                || inherited_count == 0
                || slots
                    .len()
                    .checked_add(inherited_count)
                    .is_none_or(|end| end > count)
            {
                return Ok(None);
            }
            slots.extend(
                ctx.admit_iter(0..inherited_count, "creo local-system inherited slots")?
                    .map(|_| None),
            );
            positions.start = next;
            continue;
        }
        if body[cursor] == 0x18 && cursor + 1 == body.len() {
            slots.push(Some(0.0));
            positions.start = cursor + 1;
            continue;
        }
        if body.get(cursor..cursor + 2) == Some(&[0x18, 0xe5]) {
            if slots.len() + 3 > count {
                return Ok(None);
            }
            slots.extend([Some(0.0), Some(1.0), Some(0.0)]);
            positions.start = cursor + 2;
            continue;
        }
        if body.get(cursor) == Some(&0x18)
            && (body
                .get(cursor + 1)
                .is_some_and(|byte| matches!(byte, 0x10 | 0xe4 | 0xe6 | 0xe7))
                || (body
                    .get(cursor + 1)
                    .is_some_and(|byte| scalar::is_named_local_system_coordinate_opener(*byte))
                    && scalar::decode_named_local_system_coordinate(
                        body,
                        cursor + 1,
                        slots.len() + 1,
                        cache,
                    )
                    .is_some()))
        {
            slots.push(Some(0.0));
            positions.start = cursor + 1;
            continue;
        }
        if body.get(cursor) == Some(&0x10) {
            slots.push(Some(0.0));
            positions.start = cursor + 1;
            continue;
        }
        if let Some((value, next)) =
            scalar::decode_named_local_system_coordinate(body, cursor, slots.len(), cache)
        {
            slots.push(Some(value));
            positions.start = next;
        } else {
            refusal.state(scalar_body_refusal(ctx, body, count, slots.len(), cursor)?);
            return Ok(None);
        }
    }
    if slots.len() != count {
        refusal.state(scalar_body_refusal(
            ctx,
            body,
            count,
            slots.len(),
            body.len(),
        )?);
        return Ok(None);
    }
    let cursor = positions.start;
    if cursor != body.len() {
        refusal.state(trailing_scalar_body_refusal(ctx, body, count, cursor)?);
        return Ok(None);
    }
    Ok(Some(slots))
}

pub(crate) struct PlaneFrame {
    pub(crate) origin: Option<[f64; 3]>,
    pub(crate) u_axis: Option<UnitVector3>,
    pub(crate) normal: Option<UnitVector3>,
    pub(crate) cross_overflow: bool,
}

fn plane_unit_direction(value: [f64; 3], magnitude: f64) -> Option<UnitVector3> {
    let quotient = Vector3::from(value.map(|component| component / magnitude));
    UnitVector3::new(quotient).or_else(|| UnitVector3::normalized(Vector3::from(value)))
}

impl PlaneFrame {
    fn without_directions(origin: Option<[f64; 3]>) -> Self {
        Self {
            origin,
            u_axis: None,
            normal: None,
            cross_overflow: false,
        }
    }

    fn with_directions(
        origin: Option<[f64; 3]>,
        u_axis: Option<UnitVector3>,
        normal: Option<UnitVector3>,
    ) -> Self {
        match (u_axis, normal) {
            (Some(u_axis), Some(normal)) => Self {
                origin,
                u_axis: Some(u_axis),
                normal: Some(normal),
                cross_overflow: false,
            },
            _ => Self::without_directions(origin),
        }
    }

    pub(crate) fn normal(&self) -> Option<[f64; 3]> {
        self.normal.map(|direction| Vector3::from(direction).into())
    }

    pub(crate) fn u_axis(&self) -> Option<[f64; 3]> {
        self.u_axis.map(|direction| Vector3::from(direction).into())
    }
}

fn plane_frame(slots: &[Option<f64>]) -> PlaneFrame {
    let triple = |indices: [usize; 3]| {
        Some([
            slots.get(indices[0]).copied()??,
            slots.get(indices[1]).copied()??,
            slots.get(indices[2]).copied()??,
        ])
    };
    let origin = triple([9, 10, 11]);
    let (Some(first), Some(middle), Some(third)) =
        (triple([0, 1, 2]), triple([3, 4, 5]), triple([6, 7, 8]))
    else {
        return PlaneFrame::without_directions(origin);
    };
    let supports = [first, middle, third];
    let magnitudes = supports.map(|support| {
        support
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt()
    });
    if magnitudes
        .into_iter()
        .filter(|magnitude| *magnitude <= EPS_PLANE_FRAME_NONZERO)
        .any(|magnitude| magnitude > EPS_PLANE_FRAME_ZERO)
    {
        return PlaneFrame::without_directions(origin);
    }
    let mut pairs = [(0, 1), (0, 2), (1, 2)]
        .into_iter()
        .filter(|(first, second)| {
            let first_magnitude = magnitudes[*first];
            let second_magnitude = magnitudes[*second];
            let scale = first_magnitude.max(second_magnitude);
            let support_dot = supports[*first]
                .iter()
                .zip(supports[*second])
                .map(|(first, second)| first * second)
                .sum::<f64>();
            first_magnitude > EPS_PLANE_FRAME_NONZERO
                && second_magnitude > EPS_PLANE_FRAME_NONZERO
                && (first_magnitude - second_magnitude).abs()
                    <= EPS_PLANE_FRAME_SCALE * scale.max(1.0)
                && support_dot.abs()
                    <= EPS_PLANE_FRAME_ORTHOGONAL * first_magnitude * second_magnitude
        });
    let Some((first_index, second_index)) = pairs.next() else {
        return PlaneFrame::without_directions(origin);
    };
    if pairs.next().is_some() {
        return PlaneFrame::without_directions(origin);
    }
    let first = supports[first_index];
    let second = supports[second_index];
    let u_axis = plane_unit_direction(first, magnitudes[first_index]);
    let cross = [
        first[1].mul_add(second[2], -(first[2] * second[1])),
        first[2].mul_add(second[0], -(first[0] * second[2])),
        first[0].mul_add(second[1], -(first[1] * second[0])),
    ];
    if cross.iter().any(|component| !component.is_finite()) {
        return PlaneFrame {
            cross_overflow: true,
            ..PlaneFrame::without_directions(origin)
        };
    }
    let magnitude = cross.iter().map(|value| value * value).sum::<f64>().sqrt();
    let normal = (magnitude > EPS_PLANE_FRAME_NONZERO)
        .then(|| plane_unit_direction(cross, magnitude))
        .flatten();
    PlaneFrame::with_directions(origin, u_axis, normal)
}

fn plane_direct_frame(slots: &[Option<f64>]) -> PlaneFrame {
    let triple = |indices: [usize; 3]| {
        Some([
            slots.get(indices[0]).copied()??,
            slots.get(indices[1]).copied()??,
            slots.get(indices[2]).copied()??,
        ])
    };
    let origin = triple([9, 10, 11]);
    let (Some(u_axis), Some(zero_rank), Some(normal)) =
        (triple([0, 1, 2]), triple([3, 4, 5]), triple([6, 7, 8]))
    else {
        return PlaneFrame::without_directions(origin);
    };
    let u_magnitude = u_axis.iter().map(|value| value * value).sum::<f64>().sqrt();
    let normal_magnitude = normal.iter().map(|value| value * value).sum::<f64>().sqrt();
    let scale = u_magnitude.max(normal_magnitude).max(1.0);
    let orthogonal = u_axis
        .into_iter()
        .zip(normal)
        .map(|(u, normal)| u * normal)
        .sum::<f64>();
    if zero_rank.into_iter().any(|value| value != 0.0)
        || !u_magnitude.is_finite()
        || !normal_magnitude.is_finite()
        || u_magnitude <= EPS_PLANE_FRAME_NONZERO
        || normal_magnitude <= EPS_PLANE_FRAME_NONZERO
        || (u_magnitude - normal_magnitude).abs() > EPS_PLANE_FRAME_SCALE * scale
        || orthogonal.abs() > EPS_PLANE_FRAME_ORTHOGONAL * u_magnitude * normal_magnitude
    {
        return PlaneFrame::without_directions(origin);
    }
    PlaneFrame::with_directions(
        origin,
        plane_unit_direction(u_axis, u_magnitude),
        plane_unit_direction(normal, normal_magnitude),
    )
}

fn plane_matrix_frame(slots: &[Option<f64>]) -> PlaneFrame {
    let triple = |indices: [usize; 3]| {
        Some([
            slots.get(indices[0]).copied()??,
            slots.get(indices[1]).copied()??,
            slots.get(indices[2]).copied()??,
        ])
    };
    let origin = triple([9, 10, 11]);
    let (Some(u_column), Some(rank_column), Some(normal_column)) =
        (triple([0, 3, 6]), triple([1, 4, 7]), triple([2, 5, 8]))
    else {
        return PlaneFrame::without_directions(origin);
    };
    if rank_column.into_iter().any(|value| value != 0.0) {
        return PlaneFrame::without_directions(origin);
    }
    let magnitudes = [u_column, normal_column].map(|direction| {
        direction
            .into_iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt()
    });
    let [u_magnitude, normal_magnitude] = magnitudes;
    let scale = u_magnitude.max(normal_magnitude);
    let dot = u_column
        .into_iter()
        .zip(normal_column)
        .map(|(u, normal)| u * normal)
        .sum::<f64>();
    if !u_magnitude.is_finite()
        || !normal_magnitude.is_finite()
        || u_magnitude <= EPS_PLANE_FRAME_NONZERO
        || normal_magnitude <= EPS_PLANE_FRAME_NONZERO
        || (u_magnitude - normal_magnitude).abs() > EPS_PLANE_FRAME_SCALE * scale.max(1.0)
        || dot.abs() > EPS_PLANE_FRAME_ORTHOGONAL * u_magnitude * normal_magnitude
    {
        return PlaneFrame::without_directions(origin);
    }
    PlaneFrame::with_directions(
        origin,
        plane_unit_direction(u_column, u_magnitude),
        plane_unit_direction(normal_column, normal_magnitude),
    )
}

#[cfg(test)]
fn complete_plane_local_system_slots(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<[f64; 12]> {
    crate::decode::with_test_decode_ctx(|ctx| complete_plane_local_system(ctx, body, cache))
        .expect("plane local system fits service limits")
        .map(|(slots, _)| slots.get())
}

fn complete_plane_local_system(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Result<
    Option<(
        cadmpeg_ir::units::FiniteVector<12>,
        scalar::PlaneSupportFrameLayout,
    )>,
    CodecError,
> {
    let frame_body = body.strip_suffix(&[0xe1]).unwrap_or(body);
    if let Some(prefix) = frame_body.strip_suffix(&[0x00, 0x0c, 0x98]) {
        let Some(count) = prefix.len().checked_add(1) else {
            return Ok(None);
        };
        let (mut normalized, _reservation) =
            ctx.temporary_vec(count, "creo normalized plane frame bytes")?;
        for &byte in ctx.admit_iter(prefix, "creo normalized plane frame projection")? {
            normalized.push(byte);
        }
        normalized.push(0x0f);
        return scalar::decode_plane_support_local_system(ctx, &normalized, cache);
    }
    scalar::decode_plane_support_local_system(ctx, frame_body, cache)
}

/// Decode the e3-bounded local-system chunk following each plane envelope.
pub(crate) fn plane_local_systems(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<PlaneLocalSystem>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo plane_local_systems row scratch")?;
    let rows = scratch.with_storage(|| rows(ctx, payload))?;
    plane_local_systems_for_rows(ctx, payload, &rows)
}

/// Decode plane local-system chunks from a DEPDB cross-section namespace.
pub(crate) fn cross_section_plane_local_systems(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<PlaneLocalSystem>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo plane_local_systems row scratch")?;
    let rows = scratch.with_storage(|| cross_section_rows(ctx, payload))?;
    plane_local_systems_for_rows(ctx, payload, &rows)
}

fn plane_local_systems_for_rows(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    rows: &[SurfaceRow],
) -> Result<Vec<PlaneLocalSystem>, CodecError> {
    struct Candidate<'a> {
        scanner_owned: bool,
        fallback_owned: bool,
        body: &'a [u8],
        slots: [Option<f64>; 12],
        layout: Option<scalar::PlaneSupportFrameLayout>,
        simple: bool,
        offset: usize,
    }

    let cache = scalar::ScalarCache::from_section_checked(ctx, payload)?;
    let mut scratch = ctx.reserve_scoped(0, "creo plane parameter scratch")?;
    let parameters = scratch.with_storage(|| {
        SurfaceParameters::new(
            ctx,
            parameter_records_for_rows(ctx, payload, rows, &cache)?,
            "creo plane local system parameter index",
        )
    })?;
    let headers = ctx
        .admit_iter(rows, "creo plane row traversal")?
        .enumerate()
        .filter(|(_, row)| row.kind == SurfaceKind::Plane)
        .map(|(index, row)| {
            let row_end = rows
                .get(index + 1)
                .map_or(payload.len(), |next| next.offset);
            (row, row_end)
        });
    let mut systems = Vec::new();
    for (row, row_end) in headers {
        let Some(envelope_start) = positional_body_start(payload, row) else {
            continue;
        };
        let parameter = unique_surface_parameter(&parameters, row.id)
            .filter(|parameter| parameter.offset == row.offset);
        let parameter_close = parameter
            .filter(|parameter| parameter.boundary == SurfaceBodyBoundary::CompoundClose)
            .map(|parameter| parameter.body_offset + parameter.body.len());
        let scanner_close = first_compound_close(ctx, payload, envelope_start, row_end)?;
        let envelope_closes = match (scanner_close, parameter_close) {
            (Some(first), Some(second)) if first < second => [Some(first), Some(second)],
            (Some(first), Some(second)) if second < first => [Some(second), Some(first)],
            (Some(first), Some(_)) => [Some(first), None],
            (Some(first), None) | (None, Some(first)) => [Some(first), None],
            (None, None) => [None, None],
        };
        let mut row_systems = [None, None];
        let mut system_count = 0;
        for envelope_close in envelope_closes.into_iter().flatten() {
            let chunk_start = envelope_close + 1;
            let chunk_end =
                plane_local_system_compound_close(ctx, payload, chunk_start, row_end, &cache)?;
            let Some(chunk_end) = chunk_end else {
                continue;
            };
            if chunk_end <= chunk_start {
                continue;
            }
            let body = &payload[chunk_start..chunk_end];
            let decoded = complete_plane_local_system(ctx, body, &cache)?;
            let slots = decoded
                .as_ref()
                .map_or([None; 12], |(slots, _)| slots.get().map(Some));
            let layout = decoded.as_ref().map(|(_, layout)| *layout);
            let frame_body = body.strip_suffix(&[0xe1]).unwrap_or(body);
            let simple = matches!(frame_body.first(), Some(0x0f | 0x10 | 0x18))
                && frame_body.len() <= 24
                && !frame_body
                    .iter()
                    .any(|byte| matches!(byte, 0xe0..=0xe2 | 0xf1 | 0xf2 | 0xf7 | 0xf8));
            row_systems[system_count] = Some(Candidate {
                scanner_owned: Some(envelope_close) == scanner_close,
                fallback_owned: Some(envelope_close) == scanner_close.or(parameter_close),
                body,
                slots,
                layout,
                simple,
                offset: chunk_start,
            });
            system_count += 1;
        }
        let has_complete = row_systems
            .iter()
            .flatten()
            .any(|candidate| candidate.layout.is_some());
        let has_complete_scanner = row_systems
            .iter()
            .flatten()
            .any(|candidate| candidate.layout.is_some() && candidate.scanner_owned);
        for candidate in row_systems.into_iter().flatten() {
            let selected = if has_complete_scanner {
                candidate.layout.is_some() && candidate.scanner_owned
            } else if has_complete {
                candidate.layout.is_some()
            } else {
                candidate.fallback_owned
            };
            if selected {
                let body = ctx.copy_retained(candidate.body, "creo plane local-system body")?;
                ctx.reserve_vec(&mut systems, 1, "creo plane local systems")?;
                systems.push(PlaneLocalSystem {
                    surface_id: row.id,
                    body,
                    slots: candidate.slots,
                    layout: candidate.layout,
                    classification: if candidate.simple {
                        LocalSystemClassification::Simple
                    } else {
                        LocalSystemClassification::Unclassified
                    },
                    row_offset: row.offset,
                    offset: candidate.offset,
                });
            }
        }
    }
    Ok(systems)
}

/// Decode plane positional envelope bodies into their two defined layouts.
pub(crate) fn plane_envelopes(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<PlaneEnvelopeRecord>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo plane_envelopes row scratch")?;
    let rows = scratch.with_storage(|| rows(ctx, payload))?;
    plane_envelopes_for_rows(ctx, payload, &rows)
}

/// Decode plane envelopes from a DEPDB cross-section namespace.
pub(crate) fn cross_section_plane_envelopes(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Vec<PlaneEnvelopeRecord>, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "creo plane_envelopes row scratch")?;
    let rows = scratch.with_storage(|| cross_section_rows(ctx, payload))?;
    plane_envelopes_for_rows(ctx, payload, &rows)
}

fn copied_plane_envelope_tokens(
    ctx: &DecodeContext<'_>,
    slots: &ScalarTokenTable<'_>,
) -> Result<Vec<Vec<u8>>, CodecError> {
    let mut tokens = Vec::new();
    ctx.reserve_vec(
        &mut tokens,
        slots.slots.len(),
        "creo plane envelope scalar token items",
    )?;
    for (_, raw) in &slots.slots {
        tokens.push(ctx.copy_retained(raw, "creo plane envelope scalar token bytes")?);
    }
    Ok(tokens)
}

fn plane_envelopes_for_rows(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    all_rows: &[SurfaceRow],
) -> Result<Vec<PlaneEnvelopeRecord>, CodecError> {
    const NAMED_OUTLINE: &[u8] = b"outline\0\xf9\x02\x03";
    let cache = scalar::ScalarCache::from_section_checked(ctx, payload)?;
    let headers = ctx
        .admit_iter(all_rows, "creo plane envelope row traversal")?
        .enumerate()
        .filter(|(_, row)| row.kind == SurfaceKind::Plane)
        .filter_map(|(index, row)| {
            positional_body_start(payload, row).map(|body_start| {
                let row_end = all_rows
                    .get(index + 1)
                    .map_or(payload.len(), |next| next.offset);
                (row, body_start, row_end)
            })
        });
    let mut envelopes = Vec::new();
    for (row, body_start, row_end) in headers {
        let Some(body) = payload.get(body_start..row_end) else {
            continue;
        };
        let Some(body_end) = surface_body_compound_close(ctx, SurfaceKind::Plane, body, &cache)?
            .map(|relative| body_start + relative)
        else {
            continue;
        };
        let body = &payload[body_start..body_end];
        let mut slot_scope = ctx.reserve_scoped(0, "creo plane envelope token scratch")?;
        let (envelope, corner_coordinate_equal, slots) = if body.first() == Some(&0x0e) {
            let slots = match slot_scope
                .with_storage(|| complete_plane_envelope_slots(ctx, &body[1..], 9, &cache))?
            {
                Some(slots) => Some(slots),
                None => slot_scope.with_storage(|| {
                    complete_plane_envelope_slots_with_final_positive_dict(
                        ctx,
                        &body[1..],
                        8,
                        &cache,
                    )
                })?,
            };
            let Some(slots) = slots else {
                continue;
            };
            (
                PlaneEnvelope::Compact {
                    prefix: [slots.slots[0].0, slots.slots[1].0, slots.slots[2].0],
                    corners_3d: [
                        [slots.slots[3].0, slots.slots[4].0, slots.slots[5].0],
                        [slots.slots[6].0, slots.slots[7].0, slots.slots[8].0],
                    ],
                },
                [
                    slot_equality(&slots.slots[3], &slots.slots[6]),
                    slot_equality(&slots.slots[4], &slots.slots[7]),
                    slot_equality(&slots.slots[5], &slots.slots[8]),
                ],
                slots,
            )
        } else if let Some(slots) =
            slot_scope.with_storage(|| complete_plane_compact_scalar_suffix(ctx, body, &cache))?
        {
            (
                PlaneEnvelope::Compact {
                    prefix: [slots.slots[0].0, slots.slots[1].0, slots.slots[2].0],
                    corners_3d: [
                        [slots.slots[3].0, slots.slots[4].0, slots.slots[5].0],
                        [slots.slots[6].0, slots.slots[7].0, slots.slots[8].0],
                    ],
                },
                [
                    slot_equality(&slots.slots[3], &slots.slots[6]),
                    slot_equality(&slots.slots[4], &slots.slots[7]),
                    slot_equality(&slots.slots[5], &slots.slots[8]),
                ],
                slots,
            )
        } else {
            let slots = match slot_scope
                .with_storage(|| complete_plane_envelope_slots(ctx, body, 10, &cache))?
            {
                Some(slots) => Some(slots),
                None => slot_scope.with_storage(|| {
                    complete_plane_envelope_slots_with_final_positive_dict(ctx, body, 9, &cache)
                })?,
            };
            let Some(slots) = slots else {
                continue;
            };
            (
                PlaneEnvelope::Standard {
                    bounds_2d: [
                        [slots.slots[0].0, slots.slots[1].0],
                        [slots.slots[2].0, slots.slots[3].0],
                    ],
                    corners_3d: [
                        [slots.slots[4].0, slots.slots[5].0, slots.slots[6].0],
                        [slots.slots[7].0, slots.slots[8].0, slots.slots[9].0],
                    ],
                },
                [
                    slot_equality(&slots.slots[4], &slots.slots[7]),
                    slot_equality(&slots.slots[5], &slots.slots[8]),
                    slot_equality(&slots.slots[6], &slots.slots[9]),
                ],
                slots,
            )
        };
        let scalar_tokens = copied_plane_envelope_tokens(ctx, &slots)?;
        let body = ctx.copy_retained(body, "creo plane envelope body")?;
        ctx.reserve_vec(&mut envelopes, 1, "creo plane envelopes")?;
        envelopes.push(PlaneEnvelopeRecord {
            surface_id: row.id,
            body,
            envelope,
            corner_coordinate_equal,
            scalar_tokens,
            row_offset: row.offset,
            offset: body_start,
        });
    }
    for (index, row) in ctx
        .admit_iter(all_rows, "creo named plane row traversal")?
        .enumerate()
        .filter(|(_, row)| row.kind == SurfaceKind::Plane)
    {
        let row_end = all_rows
            .get(index + 1)
            .map_or(payload.len(), |next| next.offset);
        let named_end = ctx
            .find_map(
                payload[row.offset..row_end]
                    .windows(b"srf_prim_ptr(".len())
                    .enumerate(),
                |(offset, bytes)| Ok((bytes == b"srf_prim_ptr(").then_some(offset)),
                "creo plane prototype boundary search",
            )?
            .map_or(row_end, |relative| {
                let prototype = row.offset + relative;
                prototype
                    .checked_sub(2)
                    .filter(|start| {
                        payload.get(*start..prototype) == Some(&[psb::token::NAMED_RECORD, 0x00])
                    })
                    .unwrap_or(prototype)
            });
        let Some(relative) = ctx.find_map(
            payload[row.offset..named_end]
                .windows(NAMED_OUTLINE.len())
                .enumerate(),
            |(offset, bytes)| Ok((bytes == NAMED_OUTLINE).then_some(offset)),
            "creo named plane outline search",
        )?
        else {
            continue;
        };
        let outline = row.offset + relative;
        let scalar_start = outline + NAMED_OUTLINE.len();
        let field_end = named_record_boundary(
            ctx,
            SurfaceKind::Plane,
            &payload[scalar_start..named_end],
            &cache,
        )?
        .map_or(named_end, |relative| scalar_start + relative);
        let mut slot_scope = ctx.reserve_scoped(0, "creo named plane envelope token scratch")?;
        let Some(slots) = slot_scope.with_storage(|| {
            scalar_slots_with_tokens_and_end(ctx, &payload[scalar_start..field_end], 6, &cache)
        })?
        else {
            continue;
        };
        // The helper states every other condition. This is this caller's own:
        // the outline field owns the bytes up to `field_end`.
        let consumed = slots.consumed;
        if consumed != field_end - scalar_start {
            continue;
        }
        let scalar_tokens = copied_plane_envelope_tokens(ctx, &slots)?;
        let body = ctx.copy_retained(
            &payload[scalar_start..scalar_start + consumed],
            "creo plane envelope body",
        )?;
        ctx.reserve_vec(&mut envelopes, 1, "creo plane envelopes")?;
        envelopes.push(PlaneEnvelopeRecord {
            surface_id: row.id,
            body,
            envelope: PlaneEnvelope::Standard {
                bounds_2d: [[None; 2]; 2],
                corners_3d: [
                    [slots.slots[0].0, slots.slots[1].0, slots.slots[2].0],
                    [slots.slots[3].0, slots.slots[4].0, slots.slots[5].0],
                ],
            },
            corner_coordinate_equal: [
                slot_equality(&slots.slots[0], &slots.slots[3]),
                slot_equality(&slots.slots[1], &slots.slots[4]),
                slot_equality(&slots.slots[2], &slots.slots[5]),
            ],
            scalar_tokens,
            row_offset: row.offset,
            offset: scalar_start,
        });
    }
    ctx.stable_sort_by(
        envelopes.as_mut_slice(),
        |value| &value.offset,
        Ord::cmp,
        "creo plane envelopes for rows envelopes ordering",
    )?;
    Ok(envelopes)
}

fn complete_plane_compact_scalar_suffix<'a>(
    ctx: &DecodeContext<'_>,
    body: &'a [u8],
    cache: &scalar::ScalarCache,
) -> Result<Option<ScalarTokenTable<'a>>, CodecError> {
    if complete_plane_envelope_slots(ctx, body, 10, cache)?.is_some() {
        return Ok(None);
    }
    let tokens = scalar_tokens(ctx, SurfaceKind::Plane, body, cache)?;
    let frames = scalar_frames(ctx, &tokens)?;
    let Some(frame) = terminal_scalar_frame_of(body, &frames) else {
        return Ok(None);
    };
    if frame.offset == 0 || frame.slots.len() != 9 {
        return Ok(None);
    }
    let Some(slots) = complete_plane_envelope_slots(ctx, &body[frame.offset..], 9, cache)? else {
        return Ok(None);
    };
    Ok(slots
        .slots
        .iter()
        .all(|(value, _)| value.is_some())
        .then_some(slots))
}

/// Count labeled `srf_prim_ptr` prototypes whose family is known, plus unlabeled
/// `geom_type` prototype records. Production readers use only this count.
pub(crate) fn prototype_count(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<usize, CodecError> {
    let mut named = 0;
    let mut search = 0;
    while let Some(record_start) = ctx.find_map(
        payload
            .get(search..)
            .unwrap_or_default()
            .windows(b"srf_prim_ptr(".len())
            .enumerate(),
        |(offset, bytes)| Ok((bytes == b"srf_prim_ptr(").then_some(search + offset)),
        "find Creo surface marker",
    )? {
        let family_start = record_start + b"srf_prim_ptr(".len();
        let Some(close) = ctx.find_map(
            payload
                .get(family_start..)
                .unwrap_or_default()
                .windows(b")\0".len())
                .enumerate(),
            |(offset, bytes)| Ok((bytes == b")\0").then_some(family_start + offset)),
            "find Creo surface marker",
        )?
        else {
            break;
        };
        if ctx
            .validate_utf8(&payload[family_start..close], "creo UTF-8 validation")?
            .ok()
            .and_then(SurfacePrototypeFamily::from_known_name)
            .is_some()
        {
            named += 1;
        }
        search = close + 2;
    }
    let mut unlabeled = 0;
    let mut start = 0;
    while let Some(record) = ctx.find_map(
        payload
            .get(start..)
            .unwrap_or_default()
            .windows(b"srf_prim_ptr\0".len())
            .enumerate(),
        |(offset, bytes)| Ok((bytes == b"srf_prim_ptr\0").then_some(start + offset)),
        "find Creo surface marker",
    )? {
        start = record + b"srf_prim_ptr\0".len();
        let end = ctx
            .find_map(
                payload
                    .get(start..)
                    .unwrap_or_default()
                    .windows(b"srf_prim_ptr\0".len())
                    .enumerate(),
                |(offset, bytes)| Ok((bytes == b"srf_prim_ptr\0").then_some(start + offset)),
                "find Creo surface marker",
            )?
            .unwrap_or(payload.len());
        let Some(kind_label) = ctx.find_map(
            payload
                .get(start..end)
                .unwrap_or_default()
                .windows(b"geom_type\0".len())
                .enumerate(),
            |(offset, bytes)| Ok((bytes == b"geom_type\0").then_some(start + offset)),
            "find Creo surface marker",
        )?
        else {
            continue;
        };
        if payload
            .get(kind_label + b"geom_type\0".len())
            .and_then(|value| SurfaceKind::from_byte(*value))
            .is_some()
        {
            unlabeled += 1;
        }
    }
    Ok(named + unlabeled)
}

/// Half angle of an apex cone in radians: finite and in `(0, pi/2)`.
///
/// A cone surface record carries an apex, an axis, a reference direction and this half angle,
/// and no radius, so the surface radius is the distance from the apex times the tangent of the
/// half angle. At zero the locus is the axis line and not a surface; the record family states a
/// cylinder through [`SurfaceKind::Cylinder`], whose [`PositionalCylinderFrame`] carries the
/// radius. At `pi/2` the locus is the apex plane, and above it the tangent is negative, which is
/// the same cone about the opposite axis direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ApexConeHalfAngle(PositiveAngle);

impl ApexConeHalfAngle {
    /// Admits a finite half angle in `(0, pi/2)`.
    pub(crate) fn new(value: f64) -> Option<Self> {
        (value < std::f64::consts::FRAC_PI_2).then_some(())?;
        PositiveAngle::new(value).map(Self)
    }
    /// Returns the half angle.
    pub(crate) fn get(self) -> PositiveAngle {
        self.0
    }
}

fn id_ending_at(payload: &[u8], type_offset: usize) -> Option<(u32, usize)> {
    if type_offset >= 2 && matches!(payload[type_offset - 2], 0x80..=0xbf) {
        let (value, end) = compact_int(payload, type_offset - 2);
        if end == type_offset {
            return Some((value, type_offset - 2));
        }
    }
    let start = type_offset.checked_sub(1)?;
    (payload[start] < 0x80).then_some((u32::from(payload[start]), start))
}


fn inline_local_starts<'a, 'ctx>(
    ctx: &'a DecodeContext<'ctx>, body: &'a [u8],
) -> impl Iterator<Item = Result<usize, CodecError>> + use<'a, 'ctx> {
    let mut initial = true;
    let mut positions = body.iter().enumerate();
    let mut finished = false;
    std::iter::from_fn(move || {
        if finished { return None; }
        if let Some(refusal) = ctx.resource_refusal() {
            finished = true;
            return Some(Err(refusal.into()));
        }
        if initial { initial = false; return Some(Ok(0)); }
        while positions.len() != 0 {
            match ctx.next_charged(&mut positions, "creo inline local-system boundary visits") {
                Ok(Some((offset, byte))) if *byte == psb::token::COMPOUND_CLOSE => return Some(Ok(offset + 1)),
                Ok(Some(_)) => {},
                Ok(None) => break,
                Err(error) => { finished = true; return Some(Err(error)); },
            }
        }
        finished = true;
        None
    })
}

#[cfg(test)]
fn parse_surface_contour_chain(
    ctx: &DecodeContext<'_>, payload: &[u8], start: usize, end: usize,
    row: &SurfaceRow, cache: &scalar::ScalarCache,
) -> Result<Option<Vec<SurfaceContourRecord>>, CodecError> {
    let mut records = Vec::new();
    Ok(append_surface_contour_chain(ctx, payload, start, end, row, cache, &mut records)?.then_some(records))
}

#[cfg(test)]
mod tests;
