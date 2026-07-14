// SPDX-License-Identifier: Apache-2.0
//! Surface namespace rows and prototype parameters.
//!
//! A [`SurfaceRow`] identifies a surface family and its feature, orientation,
//! boundary, and namespace links. A [`SurfacePrototype`] contains named template
//! parameters. A named prototype locates its adjacent first positional instance.

use crate::psb::{self, compact_int};
use crate::scalar;

/// Surface family encoded by an `srf_array` row's `geom_type` byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceKind {
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
    /// `geom_type = 0x2a` or `0x2c`: linear-extrusion family. The raw variant
    /// remains available as [`SurfaceRow::type_byte`].
    Extrusion,
}

impl SurfaceKind {
    fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x22 => Some(Self::Plane),
            0x24 => Some(Self::Cylinder),
            0x25 => Some(Self::Cone),
            0x26 => Some(Self::TorusOrSphere),
            0x28 => Some(Self::Spline),
            0x29 => Some(Self::Fillet),
            0x2a | 0x2c => Some(Self::Extrusion),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) const fn canonical_type_byte(self) -> u8 {
        match self {
            Self::Plane => 0x22,
            Self::Cylinder => 0x24,
            Self::Cone => 0x25,
            Self::TorusOrSphere => 0x26,
            Self::Spline => 0x28,
            Self::Fillet => 0x29,
            Self::Extrusion => 0x2a,
        }
    }
}

/// One `srf_array` row whose fixed prefix passed the row grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceRow {
    /// The row's `geom_id`: the surface's identifier in the `srf_array`
    /// namespace, referenced by curve `F0`/`F1` face fields and by
    /// `next_surface` links.
    pub id: u32,
    /// Raw `geom_type` byte selecting the surface-family encoding variant.
    pub type_byte: u8,
    /// The row's surface family, from `geom_type`.
    pub kind: SurfaceKind,
    /// The `feat_id` compact integer: the feature that generated this
    /// surface, joining `AllFeatur`/`MdlStatus` feature rows.
    pub feature_id: u32,
    /// `true` when the row's orientation byte is `0xf6` (reversed), `false`
    /// when it is `0x01` (as-stored orientation).
    pub reversed: bool,
    /// The row's `boundary_type` byte: one of `0x00`, `0x01`, `0x06`, or
    /// `0xf6`.
    pub boundary_type: u8,
    /// The `next_geom_ptr` compact integer: the identifier of the next
    /// `srf_array` row in this namespace's link chain.
    pub next_surface: u32,
    /// Byte offset of the row's `geom_id` field in the original stream.
    pub offset: usize,
}

/// Named scalar parameters from one surface-family prototype.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfacePrototype {
    /// The prototype's surface family, from its labeled `geom_type` field.
    pub kind: SurfaceKind,
    /// The `radius` scalar field for a cylinder prototype, or `radius1` for
    /// a torus/sphere prototype (nonzero for a torus, zero for a sphere).
    pub radius: Option<f64>,
    /// The `radius2` scalar field for a torus/sphere prototype.
    pub radius2: Option<f64>,
    /// The `half_angle` scalar field for a cone prototype, in radians, in
    /// the range `(0, pi/2)` ([spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/creo_prt.md#32-surface-prototypes)).
    pub half_angle: Option<f64>,
    /// Byte offset of the `srf_prim_ptr` record's label in the original
    /// stream.
    pub offset: usize,
}

/// Named `srf_prim_ptr(<kind>)` prototype family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfacePrototypeFamily {
    /// Plane prototype.
    Plane,
    /// Cylinder prototype.
    Cylinder,
    /// Cone prototype.
    Cone,
    /// Torus or sphere prototype.
    Torus,
    /// Spline-surface prototype.
    Spline,
    /// Fillet-surface prototype.
    Fillet,
    /// Surface-of-extrusion prototype.
    Extrusion,
    /// Structurally valid family name outside the defined set.
    Other(String),
}

impl SurfacePrototypeFamily {
    fn from_name(name: &str) -> Self {
        match name {
            "plane" => Self::Plane,
            "cylinder" => Self::Cylinder,
            "cone" => Self::Cone,
            "torus" | "sphere" => Self::Torus,
            "spline" | "splsrf" => Self::Spline,
            "fillet" | "fillet_srf" => Self::Fillet,
            "surface_of_extrusion" | "extrusion" | "tab_cyl" | "ruled_srf" => Self::Extrusion,
            other => Self::Other(other.to_string()),
        }
    }
}

/// Typed wrapper carried by a named surface-prototype parameter.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceNamedValue {
    /// One compact integer.
    CompactInt(u32),
    /// Count-bounded compact-integer array.
    CompactIntArray(Vec<u32>),
    /// Count consecutive entity IDs beginning at one stored reference.
    ContiguousEntityReferences {
        /// First entity identifier.
        start_id: u32,
        /// Expanded consecutive identifiers.
        entity_ids: Vec<u32>,
    },
    /// Dimensioned `f9` scalar body.
    ScalarArray {
        /// Stored dimension value.
        dimensions: u8,
        /// Stored element count.
        count: u8,
        /// Decoded slots with unresolved values retained.
        values: Vec<Option<f64>>,
    },
    /// One or more consecutive scalar tokens.
    ScalarSequence(Vec<f64>),
    /// Exact bytes of a wrapper that is not structurally defined.
    Opaque(Vec<u8>),
}

/// One selected named parameter inside a surface prototype.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceNamedParameter {
    /// Named-record field name.
    pub name: String,
    /// Typed interpretation of the field body.
    pub value: SurfaceNamedValue,
    /// Exact field body bytes.
    pub body: Vec<u8>,
    /// Byte offset of the named-record header.
    pub offset: usize,
    /// Byte offset of the first value byte.
    pub value_offset: usize,
}

/// Bounded `srf_prim_ptr(<kind>)` prototype and its named parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfacePrototypeRecord {
    /// Exact family name inside `srf_prim_ptr(<family>)`.
    pub declared_family: String,
    /// Surface family named by the prototype label.
    pub family: SurfacePrototypeFamily,
    /// Selected named parameters in byte order.
    pub parameters: Vec<SurfaceNamedParameter>,
    /// Byte offset of the prototype label.
    pub offset: usize,
}

impl SurfacePrototypeRecord {
    /// Return the first selected parameter with `name`.
    pub fn field(&self, name: &str) -> Option<&SurfaceNamedParameter> {
        self.parameters.iter().find(|field| field.name == name)
    }
}

/// Structural boundary that terminates a positional surface parameter body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceBodyBoundary {
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
pub struct SurfaceParameterRecord {
    /// Owning `srf_array` geometry identifier.
    pub surface_id: u32,
    /// Exact bytes after `next_geom_ptr` and before the structural boundary.
    pub body: Vec<u8>,
    /// Context-independent scalar values decoded from the body in byte order.
    pub scalar_values: Vec<f64>,
    /// Decoded scalar tokens with byte spans relative to `body`.
    pub scalar_tokens: Vec<SurfaceParameterScalar>,
    /// Exact byte spans not owned by a recognized scalar token.
    pub opaque_spans: Vec<SurfaceParameterOpaqueSpan>,
    /// Maximal contiguous scalar-token frames in byte order.
    pub scalar_frames: Vec<SurfaceParameterScalarFrame>,
    /// Maximal scalar-token frame ending at the body boundary.
    pub terminal_scalar_frame: Option<SurfaceParameterScalarFrame>,
    /// Structural form that bounded the body.
    pub boundary: SurfaceBodyBoundary,
    /// Byte offset of the positional surface row in the original stream.
    pub offset: usize,
    /// Byte offset of the first parameter-body byte in the original stream.
    pub body_offset: usize,
}

/// One contiguous positional scalar frame with no intervening bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceParameterScalarFrame {
    /// Byte offset relative to the start of the parameter body.
    pub offset: usize,
    /// Ordered scalar tokens occupying the frame.
    pub slots: Vec<SurfaceParameterScalar>,
}

/// One maximal unframed span inside a positional surface parameter body.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceParameterOpaqueSpan {
    /// Exact source bytes in the span.
    pub raw: Vec<u8>,
    /// Byte offset relative to the start of the parameter body.
    pub offset: usize,
    /// Number of source bytes in the span.
    pub length: usize,
}

/// One scalar token located within a positional surface parameter body.
#[derive(Debug, Clone, PartialEq)]
pub struct SurfaceParameterScalar {
    /// Decoded scalar value, or `None` for a structurally framed token whose
    /// numeric mapping is not defined.
    pub value: Option<f64>,
    /// Exact source bytes occupied by the token.
    pub raw: Vec<u8>,
    /// Byte offset relative to the start of the parameter body.
    pub offset: usize,
    /// Number of source bytes occupied by the token.
    pub length: usize,
}

/// Complete positional construction for a line-generated extrusion surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineExtrusionFrame {
    /// Stored model-space sweep direction.
    pub direction: [f64; 3],
    /// Two model-space points defining the straight directrix.
    pub directrix: [[f64; 3]; 2],
}

/// One positional cubic B-spline replay bound to a following tabulated-
/// cylinder surface row.
#[derive(Debug, Clone, PartialEq)]
pub struct TabulatedCylinderCurveReplay {
    /// Owning `geom_type = 2c` surface identifier.
    pub surface_id: u32,
    /// Replayed curve identifier.
    pub curve_id: u32,
    /// Raw curve-family discriminator.
    pub curve_type: u8,
    /// Stored curve flip byte.
    pub flip: u8,
    /// Stored tangent-condition byte.
    pub tangent_condition: u8,
    /// B-spline degree.
    pub degree: u8,
    /// Exact count-budgeted parameter body.
    pub parameter_body: Vec<u8>,
    /// Four contiguous control-point entity identifiers.
    pub control_point_ids: [u32; 4],
    /// Reference following the control-point-array header.
    pub successor_reference: u32,
    /// Four individually bounded packed control-point bodies.
    pub control_point_bodies: [Vec<u8>; 4],
    /// Two-coordinate control points when both scalar tokens consume their
    /// complete packed bodies.
    pub control_points: [Option<[f64; 2]>; 4],
    /// Reference in the terminal control-point trailer.
    pub terminal_reference: u32,
    /// Byte offset of the replay curve identifier.
    pub offset: usize,
    /// Byte offset of the owning surface row.
    pub surface_row_offset: usize,
}

impl SurfaceParameterRecord {
    /// Decode the common model-space sweep-direction prefix of a positional
    /// `surface_of_extrusion` body.
    #[must_use]
    pub fn extrusion_direction(&self, type_byte: u8) -> Option<[f64; 3]> {
        if type_byte != 0x2c {
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
                .map(|slot| slot.length)
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

    /// Decode the two-frame positional body used by an unbound straight
    /// `surface_of_extrusion` instance.
    #[must_use]
    pub fn line_extrusion_frame(&self, type_byte: u8) -> Option<LineExtrusionFrame> {
        if self.boundary != SurfaceBodyBoundary::CompoundClose {
            return None;
        }
        let [direction, directrix] = self.scalar_frames.as_slice() else {
            return None;
        };
        let direction_values = self.extrusion_direction(type_byte)?;
        let [start_x, start_y, start_z, end_x, end_y, end_z] = directrix.slots.as_slice() else {
            return None;
        };
        let values = [
            direction_values[0],
            direction_values[1],
            direction_values[2],
            start_x.value?,
            start_y.value?,
            start_z.value?,
            end_x.value?,
            end_y.value?,
            end_z.value?,
        ];
        values.iter().all(|value| value.is_finite()).then_some(())?;
        let first_gap = self.opaque_spans.first()?;
        if first_gap.offset
            != direction.offset
                + direction
                    .slots
                    .iter()
                    .map(|slot| slot.length)
                    .sum::<usize>()
            || first_gap.raw != [0x00, 0x0c, 0x9a]
            || directrix.offset != first_gap.offset + first_gap.length
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
                    .map(|slot| slot.length)
                    .sum::<usize>();
            if reference.offset != directrix_end || reference.raw.first() != Some(&0xf7) {
                return None;
            }
            let (_, end) = psb::reference_id(&reference.raw, 1).ok()?;
            if end != reference.raw.len() {
                return None;
            }
        }
        Some(LineExtrusionFrame {
            direction: values[0..3].try_into().ok()?,
            directrix: [values[3..6].try_into().ok()?, values[6..9].try_into().ok()?],
        })
    }
}

/// Structural classification of a plane-row local-system chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalSystemClassification {
    /// Compact byte-characterised local-system form.
    Simple,
    /// Structurally bounded chunk outside the compact form.
    Unclassified,
}

/// Inherited twelve-slot support frame following a plane-row envelope.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaneLocalSystem {
    /// Owning plane surface identifier.
    pub surface_id: u32,
    /// Exact bytes between the envelope close and local-system close.
    pub body: Vec<u8>,
    /// Twelve inherited `f9 04 03` scalar slots; unresolved slots remain `None`.
    pub slots: Vec<Option<f64>>,
    /// Slots 9 through 11 when all three decode.
    pub origin: Option<[f64; 3]>,
    /// Normalized first in-plane direction from slots 0 through 2.
    pub u_axis: Option<[f64; 3]>,
    /// Normalized cross product of valid equal-scale in-plane directions.
    pub normal: Option<[f64; 3]>,
    /// Compact versus raw-preserved chunk classification.
    pub classification: LocalSystemClassification,
    /// Byte offset of the plane row in the original stream.
    pub row_offset: usize,
    /// Byte offset of the local-system chunk in the original stream.
    pub offset: usize,
}

/// Plane-specific positional envelope layout.
#[derive(Debug, Clone, PartialEq)]
pub enum PlaneEnvelope {
    /// Four 2D bound values followed by two 3D corner triples.
    Standard {
        /// Two parameter-space bound pairs.
        bounds_2d: [[Option<f64>; 2]; 2],
        /// Two surface-space corner triples.
        corners_3d: [[Option<f64>; 3]; 2],
    },
    /// `0x0e` variant with three prefix values and two 3D corner triples.
    Compact {
        /// Three envelope-prefix values.
        prefix: [Option<f64>; 3],
        /// Two surface-space corner triples.
        corners_3d: [[Option<f64>; 3]; 2],
    },
}

/// Decoded positional envelope for one plane row.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaneEnvelopeRecord {
    /// Owning plane surface identifier.
    pub surface_id: u32,
    /// Exact envelope bytes, including a compact-variant marker.
    pub body: Vec<u8>,
    /// Plane-specific envelope layout.
    pub envelope: PlaneEnvelope,
    /// Per-coordinate equality of the two stored model-space corners. `None`
    /// means equality cannot be decided from the scalar token pair.
    pub corner_coordinate_equal: [Option<bool>; 3],
    /// Byte offset of the plane row in the original stream.
    pub row_offset: usize,
    /// Byte offset of the envelope body in the original stream.
    pub offset: usize,
}

/// Axis-aligned model-space plane established by two outline corners with one
/// and only one held coordinate.
#[derive(Debug, Clone, PartialEq)]
pub struct OutlinePlane {
    /// Owning `srf_array` surface identifier.
    pub surface_id: u32,
    /// Model-space plane origin with only the held coordinate populated.
    pub origin: [f64; 3],
    /// Positive model-space basis normal of the held coordinate.
    pub normal: [f64; 3],
    /// Deterministic positive in-plane reference direction.
    pub u_axis: [f64; 3],
    /// Byte offset of the outline body.
    pub offset: usize,
}

/// Derive axis-aligned plane equations from complete, non-degenerate outline
/// corner pairs. Ambiguous pairs with zero or multiple held axes are withheld.
pub fn outline_planes(envelopes: &[PlaneEnvelopeRecord]) -> Vec<OutlinePlane> {
    let mut result = Vec::new();
    for record in envelopes {
        let corners = match &record.envelope {
            PlaneEnvelope::Standard { corners_3d, .. }
            | PlaneEnvelope::Compact { corners_3d, .. } => corners_3d,
        };
        let held = (0..3)
            .filter(|axis| record.corner_coordinate_equal[*axis] == Some(true))
            .collect::<Vec<_>>();
        if held.len() != 1
            || record
                .corner_coordinate_equal
                .iter()
                .enumerate()
                .any(|(axis, equal)| axis != held[0] && *equal != Some(false))
        {
            continue;
        }
        let axis = held[0];
        let Some(coordinate) = corners[0][axis] else {
            continue;
        };
        let mut origin = [0.0; 3];
        origin[axis] = coordinate;
        let mut normal = [0.0; 3];
        normal[axis] = 1.0;
        let u_axis = if axis == 0 {
            [0.0, 1.0, 0.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        result.push(OutlinePlane {
            surface_id: record.surface_id,
            origin,
            normal,
            u_axis,
            offset: record.offset,
        });
    }
    result.sort_by_key(|plane| plane.offset);
    result
}

const BOUNDARY_TYPES: &[u8] = &[0x00, 0x01, 0x06, 0xf6];

#[derive(Debug, Clone, Copy)]
struct SurfaceArrayFrame {
    start: usize,
    end: usize,
    count: usize,
}

fn surface_array_frames(payload: &[u8]) -> Vec<SurfaceArrayFrame> {
    const LABEL: &[u8] = b"srf_array\0";
    let mut labels = Vec::new();
    let mut search = 0;
    while let Some(offset) = find(payload, LABEL, search) {
        labels.push(offset);
        search = offset + LABEL.len();
    }
    let mut frames = Vec::new();
    for (index, label) in labels.iter().copied().enumerate() {
        let start = label + LABEL.len();
        if payload.get(start) != Some(&psb::token::ARRAY_OPEN) {
            continue;
        }
        let (count, after_count) = compact_int(payload, start + 1);
        if after_count == start + 1 {
            continue;
        }
        let mut end = labels.get(index + 1).copied().unwrap_or(payload.len());
        for terminator in [b"crv_array\0".as_slice(), b"lo_array\0", b"qlt_array\0"] {
            if let Some(offset) = find(payload, terminator, after_count) {
                end = end.min(offset);
            }
        }
        frames.push(SurfaceArrayFrame {
            start: after_count,
            end,
            count: usize::try_from(count).unwrap_or(usize::MAX),
        });
    }
    frames
}

/// Discover positional rows from every `srf_array` namespace in `payload`.
/// The scan anchors on the surface-kind byte and validates both adjacent
/// compact-integer fields and the orientation/boundary discriminators. This
/// retains only byte-backed rows; a link target never inherits a kind.
pub fn rows(payload: &[u8]) -> Vec<SurfaceRow> {
    let mut result = Vec::new();
    let mut namespace_start = 0;
    while let Some(array) = find(payload, b"srf_array\0", namespace_start) {
        let start = array + b"srf_array\0".len();
        let end = find(payload, b"srf_array\0", start).unwrap_or(payload.len());
        namespace_start = start;
        let value = |label: &[u8]| {
            find_in(payload, label, start, end).and_then(|at| {
                let value_start = at + label.len();
                let (value, after) = compact_int(payload, value_start);
                (after > value_start).then_some((value, at))
            })
        };
        let typed_kind = find_in(payload, b"geom_type\0", start, end)
            .and_then(|at| payload.get(at + b"geom_type\0".len()))
            .and_then(|byte| SurfaceKind::from_byte(*byte).map(|kind| (*byte, kind)));
        if let (
            Some((id, id_offset)),
            Some((type_byte, kind)),
            Some((feature_id, _)),
            Some((next_surface, _)),
        ) = (
            value(b"geom_id\0"),
            typed_kind,
            value(b"feat_id\0"),
            value(b"next_geom_ptr\0"),
        ) {
            let reversed = find_in(payload, b"orient\0", start, end)
                .and_then(|at| payload.get(at + b"orient\0".len()))
                == Some(&0xf6);
            let boundary_type = find_in(payload, b"boundary_type\0", start, end)
                .and_then(|at| payload.get(at + b"boundary_type\0".len()))
                .copied()
                .filter(|byte| BOUNDARY_TYPES.contains(byte))
                .unwrap_or(0);
            result.push(SurfaceRow {
                id,
                type_byte,
                kind,
                feature_id,
                reversed,
                boundary_type,
                next_surface,
                offset: id_offset,
            });
        }
    }
    for type_offset in 1..payload.len() {
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
        if !BOUNDARY_TYPES.contains(&boundary_type) {
            continue;
        }
        pos += 2;
        let (next_surface, end) = compact_int(payload, pos);
        if end == pos {
            continue;
        }
        result.push(SurfaceRow {
            id,
            type_byte: payload[type_offset],
            kind,
            feature_id,
            reversed: orientation == 0xf6,
            boundary_type,
            next_surface,
            offset: id_start,
        });
    }
    result.sort_by_key(|row| row.offset);
    result.dedup_by_key(|row| row.offset);
    let id_counts = result.iter().fold(
        std::collections::BTreeMap::<u32, usize>::new(),
        |mut counts, row| {
            *counts.entry(row.id).or_default() += 1;
            counts
        },
    );
    result.retain(|row| id_counts.get(&row.id) == Some(&1));
    let prototype_parameter_spans = named_prototype_records(payload)
        .into_iter()
        .flat_map(|record| record.parameters)
        .map(|parameter| {
            (
                parameter.value_offset,
                parameter.value_offset + parameter.body.len(),
            )
        })
        .collect::<Vec<_>>();
    result.retain(|row| {
        !prototype_parameter_spans
            .iter()
            .any(|(start, end)| row.offset >= *start && row.offset < *end)
    });
    let frames = surface_array_frames(payload);
    if !frames.is_empty() {
        let unframed = result.clone();
        let mut framed = Vec::new();
        let mut saw_framed_candidate = false;
        for frame in frames {
            let mut selected = Vec::<SurfaceRow>::with_capacity(frame.count);
            for row in result
                .iter()
                .filter(|row| row.offset >= frame.start && row.offset < frame.end)
            {
                selected.push(row.clone());
            }
            saw_framed_candidate |= !selected.is_empty();
            if selected.len() == frame.count {
                framed.extend(selected);
            }
        }
        result = if saw_framed_candidate {
            framed
        } else {
            unframed
        };
    }
    result
}

const PROTOTYPE_PARAMETER_NAMES: &[&str] = &[
    "local_sys",
    "radius",
    "radius1",
    "radius2",
    "half_angle",
    "i_pnts",
    "c_pnts",
    "parent_feats",
    "frst_cntr_crv_hdr_ptr",
    "id",
    "type",
    "flip",
    "tan_cond",
    "degree",
    "params",
    "dum_array",
    "data_dbls",
    "data_type",
];

fn prototype_parameter_allowed(family: &SurfacePrototypeFamily, name: &str) -> bool {
    PROTOTYPE_PARAMETER_NAMES.contains(&name)
        && !(matches!(family, SurfacePrototypeFamily::Torus) && matches!(name, "i_pnts" | "c_pnts"))
}

fn named_surface_value(name: &str, body: &[u8], cache: &scalar::ScalarCache) -> SurfaceNamedValue {
    let scalar_field = matches!(name, "radius" | "radius1" | "radius2" | "half_angle");
    let compact_integer_field = matches!(
        name,
        "id" | "type" | "tan_cond" | "degree" | "frst_cntr_crv_hdr_ptr" | "data_type"
    );
    if scalar_field && body == [0x18] {
        return SurfaceNamedValue::ScalarSequence(vec![0.0]);
    }
    if body.first() == Some(&psb::token::ARRAY_OPEN) {
        let (count, mut cursor) = compact_int(body, 1);
        if cursor > 1 {
            if body.get(cursor) == Some(&psb::token::ENTITY_REF) {
                if let Ok((start_id, next)) = psb::reference_id(body, cursor + 1) {
                    if body.get(next) == Some(&psb::token::ARRAY_CLOSE) {
                        if let Some(end_id) = start_id.checked_add(count) {
                            return SurfaceNamedValue::ContiguousEntityReferences {
                                start_id,
                                entity_ids: (start_id..end_id).collect(),
                            };
                        }
                    }
                }
            }
            let mut values = Vec::new();
            for _ in 0..count {
                let (value, next) = compact_int(body, cursor);
                if next == cursor {
                    break;
                }
                values.push(value);
                cursor = next;
            }
            if values.len() == usize::try_from(count).unwrap_or(usize::MAX) && cursor == body.len()
            {
                return SurfaceNamedValue::CompactIntArray(values);
            }
        }
    }
    if body.first() == Some(&psb::token::SCALAR_BODY) && body.len() >= 3 {
        let dimensions = body[1];
        let count = body[2];
        let slot_count = usize::from(dimensions) * usize::from(count);
        return SurfaceNamedValue::ScalarArray {
            dimensions,
            count,
            values: if name == "local_sys" {
                row_scalar_slots(&body[3..], slot_count, cache)
            } else {
                scalar_slots(&body[3..], slot_count, cache)
            },
        };
    }
    if compact_integer_field {
        let (value, end) = compact_int(body, 0);
        if end == body.len() && end != 0 {
            return SurfaceNamedValue::CompactInt(value);
        }
        return SurfaceNamedValue::Opaque(body.to_vec());
    }
    let mut values = Vec::new();
    let mut cursor = 0;
    while cursor < body.len() {
        if matches!(body[cursor], 0xe0..=0xe3 | 0xf1 | 0xf7 | 0xfb) {
            break;
        }
        let decoded = if name == "half_angle" {
            scalar::decode_positive_dict(body, cursor).filter(|(value, _)| valid_half_angle(*value))
        } else {
            scalar::decode_in_lane(body, cursor, cache)
        };
        let Some((value, next)) = decoded else {
            values.clear();
            break;
        };
        values.push(value);
        cursor = next;
    }
    if !values.is_empty() {
        return SurfaceNamedValue::ScalarSequence(values);
    }
    let (value, end) = compact_int(body, 0);
    if !scalar_field && end == body.len() && end != 0 {
        SurfaceNamedValue::CompactInt(value)
    } else {
        SurfaceNamedValue::Opaque(body.to_vec())
    }
}

/// Decode bounded named surface-prototype parameter records.
pub fn named_prototype_records(payload: &[u8]) -> Vec<SurfacePrototypeRecord> {
    let cache = scalar::ScalarCache::from_section(payload);
    let mut records = Vec::new();
    let mut search = 0;
    while let Some(record_start) = find(payload, b"srf_prim_ptr(", search) {
        let family_start = record_start + b"srf_prim_ptr(".len();
        let Some(close) = find(payload, b")\0", family_start) else {
            break;
        };
        let family_name = String::from_utf8_lossy(&payload[family_start..close]);
        let family = SurfacePrototypeFamily::from_name(&family_name);
        let mut record_end = find(payload, b"srf_prim_ptr(", close + 2).unwrap_or(payload.len());
        if let Some(at) = find(payload, b"srf_prim_ptr\0", close + 2) {
            record_end = record_end.min(at);
        }
        if let Some(at) = find(payload, b"\xe0\x00entity_ptr(", close + 2) {
            record_end = record_end.min(at);
        }
        for marker in [b"crv_array\0".as_slice(), b"lo_array\0", b"qlt_array\0"] {
            if let Some(at) = find(payload, marker, close + 2) {
                record_end = record_end.min(at);
            }
        }
        let tokens = psb::tokens(&payload[close + 2..record_end]);
        let named = tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| {
                let token_offset = close + 2 + token.offset;
                token.kind == psb::TokenKind::NamedRecord
                    && named_record_length(payload, token_offset) == Some(token.length)
            })
            .collect::<Vec<_>>();
        let mut parameters = Vec::new();
        for (position, (_, token)) in named.iter().enumerate() {
            let token_offset = close + 2 + token.offset;
            let name_start = token_offset + 2;
            let name_end = token_offset + token.length - 1;
            let name = String::from_utf8_lossy(&payload[name_start..name_end]);
            if !prototype_parameter_allowed(&family, &name) {
                continue;
            }
            let value_offset = token_offset + token.length;
            let mut value_end = named
                .get(position + 1)
                .map_or(record_end, |(_, next)| close + 2 + next.offset);
            if let Some(compound_close) = psb::tokens(&payload[value_offset..value_end])
                .into_iter()
                .find(|token| token.kind == psb::TokenKind::CompoundClose)
            {
                value_end = value_offset + compound_close.offset;
            }
            let body = payload[value_offset..value_end].to_vec();
            let value = named_surface_value(&name, &body, &cache);
            parameters.push(SurfaceNamedParameter {
                name: name.into_owned(),
                value,
                body,
                offset: token_offset,
                value_offset,
            });
        }
        records.push(SurfacePrototypeRecord {
            declared_family: family_name.into_owned(),
            family,
            parameters,
            offset: record_start,
        });
        search = close + 2;
    }
    records.sort_by_key(|record| record.offset);
    records
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
    (matches!(orientation, 0x01 | 0xf6) && BOUNDARY_TYPES.contains(&boundary)).then_some(())?;
    cursor += 2;
    let (_, next) = compact_int(payload, cursor);
    (next > cursor).then_some(next)
}

fn decode_row_scalar(
    _kind: SurfaceKind,
    body: &[u8],
    offset: usize,
    cache: &scalar::ScalarCache,
) -> Option<(f64, usize)> {
    scalar::decode_in_surface_row_lane(body, offset, cache)
}

fn scalar_tokens(
    kind: SurfaceKind,
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Vec<SurfaceParameterScalar> {
    let mut tokens = Vec::new();
    let mut cursor = 0;
    while cursor < body.len() {
        if let Some((value, next)) = decode_row_scalar(kind, body, cursor, cache) {
            tokens.push(SurfaceParameterScalar {
                value: Some(value),
                raw: body[cursor..next].to_vec(),
                offset: cursor,
                length: next - cursor,
            });
            cursor = next;
        } else if matches!(body.get(cursor), Some(0x73 | 0xbb)) && cursor + 7 <= body.len() {
            tokens.push(SurfaceParameterScalar {
                value: None,
                raw: body[cursor..cursor + 7].to_vec(),
                offset: cursor,
                length: 7,
            });
            cursor += 7;
        } else {
            cursor += 1;
        }
    }
    tokens
}

fn opaque_spans(body: &[u8], tokens: &[SurfaceParameterScalar]) -> Vec<SurfaceParameterOpaqueSpan> {
    let mut spans = Vec::new();
    let mut cursor = 0;
    for token in tokens {
        if cursor < token.offset {
            spans.push(SurfaceParameterOpaqueSpan {
                raw: body[cursor..token.offset].to_vec(),
                offset: cursor,
                length: token.offset - cursor,
            });
        }
        cursor = token.offset + token.length;
    }
    if cursor < body.len() {
        spans.push(SurfaceParameterOpaqueSpan {
            raw: body[cursor..].to_vec(),
            offset: cursor,
            length: body.len() - cursor,
        });
    }
    spans
}

fn scalar_frames(tokens: &[SurfaceParameterScalar]) -> Vec<SurfaceParameterScalarFrame> {
    let mut frames = Vec::new();
    let mut start = 0;
    while start < tokens.len() {
        let mut end = start + 1;
        while end < tokens.len()
            && tokens[end - 1].offset + tokens[end - 1].length == tokens[end].offset
        {
            end += 1;
        }
        frames.push(SurfaceParameterScalarFrame {
            offset: tokens[start].offset,
            slots: tokens[start..end].to_vec(),
        });
        start = end;
    }
    frames
}

fn terminal_scalar_frame(
    body: &[u8],
    frames: &[SurfaceParameterScalarFrame],
) -> Option<SurfaceParameterScalarFrame> {
    let frame = frames.last()?;
    let last = frame.slots.last()?;
    (last.offset + last.length == body.len()).then(|| frame.clone())
}

fn named_record_length(body: &[u8], offset: usize) -> Option<usize> {
    (body.get(offset) == Some(&psb::token::NAMED_RECORD)).then_some(())?;
    let field_type = *body.get(offset + 1)?;
    (field_type <= 0x24).then_some(())?;
    let name = body.get(offset + 2..)?;
    let name_len = name.iter().take(96).position(|byte| *byte == 0)?;
    let name = name.get(..name_len)?;
    (!name.is_empty()
        && name[0].is_ascii_alphabetic()
        && name
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'(' | b')')))
    .then_some(name_len + 3)
}

fn named_record_boundary(
    kind: SurfaceKind,
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<usize> {
    let mut cursor = 0;
    while cursor < body.len() {
        if named_record_length(body, cursor).is_some() {
            return Some(cursor);
        }
        if let Some((_, next)) = decode_row_scalar(kind, body, cursor, cache) {
            cursor = next;
        } else if matches!(body.get(cursor), Some(0x73 | 0xbb)) && cursor + 7 <= body.len() {
            cursor += 7;
        } else {
            cursor += 1;
        }
    }
    None
}

/// Decode bounded parameter bodies for positional `srf_array` rows.
pub fn parameter_records(payload: &[u8]) -> Vec<SurfaceParameterRecord> {
    let cache = scalar::ScalarCache::from_section(payload);
    let mut headers = Vec::<(SurfaceRow, usize)>::new();
    for row in rows(payload) {
        let Some(body_start) = positional_body_start(payload, &row) else {
            continue;
        };
        if headers
            .last()
            .is_some_and(|(_, previous_body_start)| row.offset < *previous_body_start)
        {
            continue;
        }
        headers.push((row, body_start));
    }
    let mut records = Vec::new();
    for (index, (row, body_start)) in headers.iter().enumerate() {
        let next_row = headers
            .get(index + 1)
            .map_or(payload.len(), |(next, _)| next.offset);
        let mut body_end = next_row;
        let mut boundary = if next_row < payload.len() {
            SurfaceBodyBoundary::NextRow
        } else {
            SurfaceBodyBoundary::SectionEnd
        };
        if let Some(relative) =
            surface_body_compound_close(row.kind, &payload[*body_start..body_end], &cache)
        {
            body_end = body_start + relative;
            boundary = SurfaceBodyBoundary::CompoundClose;
        }
        if let Some(relative) =
            named_record_boundary(row.kind, &payload[*body_start..body_end], &cache)
        {
            body_end = body_start + relative;
            boundary = SurfaceBodyBoundary::NamedRecord;
        }
        let body = payload[*body_start..body_end].to_vec();
        let scalar_tokens = scalar_tokens(row.kind, &body, &cache);
        let opaque_spans = opaque_spans(&body, &scalar_tokens);
        let scalar_frames = scalar_frames(&scalar_tokens);
        let terminal_scalar_frame = terminal_scalar_frame(&body, &scalar_frames);
        records.push(SurfaceParameterRecord {
            surface_id: row.id,
            scalar_values: scalar_tokens
                .iter()
                .filter_map(|token| token.value)
                .collect(),
            scalar_tokens,
            opaque_spans,
            scalar_frames,
            terminal_scalar_frame,
            body,
            boundary,
            offset: row.offset,
            body_offset: *body_start,
        });
    }
    records
}

/// Decode the cubic curve replay owned by the preceding positional
/// `geom_type = 2c` tabulated-cylinder row.
pub fn tabulated_cylinder_curve_replays(payload: &[u8]) -> Vec<TabulatedCylinderCurveReplay> {
    const SIGNATURE: &[u8] = &[
        0x13, 0xe2, 0x01, 0x00, 0x03, 0x18, 0xe6, 0x0f, 0xe6, 0xf8, 0x04, 0xf7,
    ];
    let cache = scalar::ScalarCache::from_section(payload);
    let surface_rows = rows(payload);
    let mut signatures = Vec::new();
    let mut search = 0;
    while let Some(offset) = find(payload, SIGNATURE, search) {
        signatures.push(offset);
        search = offset + SIGNATURE.len();
    }
    let mut replays = Vec::new();
    for (index, signature) in signatures.iter().copied().enumerate() {
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
        let first_separators = (control_body_start..limit.saturating_sub(3))
            .filter_map(|offset| {
                (payload.get(offset..offset + 3) == Some(&[0x18, 0xf1, 0xf7]))
                    .then(|| {
                        let (reference, after) = psb::reference_id(payload, offset + 3).ok()?;
                        (reference == control_point_start && payload.get(after) == Some(&0xe2))
                            .then_some((offset, after + 1))
                    })
                    .flatten()
            })
            .collect::<Vec<_>>();
        let [(first_separator, first_body_start)] = first_separators.as_slice() else {
            continue;
        };
        let terminals = (*first_body_start..limit.saturating_sub(4))
            .filter_map(|offset| {
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
            })
            .collect::<Vec<_>>();
        let [(terminal, _terminal_end, terminal_reference)] = terminals.as_slice() else {
            continue;
        };
        let middle_separators = (*first_body_start..*terminal)
            .filter(|offset| payload.get(*offset..*offset + 2) == Some(&[0x18, 0xe2]))
            .collect::<Vec<_>>();
        let [second_separator, third_separator] = middle_separators.as_slice() else {
            continue;
        };
        let bodies = [
            payload[control_body_start..*first_separator].to_vec(),
            payload[*first_body_start..*second_separator].to_vec(),
            payload[*second_separator + 2..*third_separator].to_vec(),
            payload[*third_separator + 2..*terminal].to_vec(),
        ];
        if bodies.iter().any(Vec::is_empty) {
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
        let control_points = std::array::from_fn(|index| decode_point(&bodies[index]));
        let Some(owner) = surface_rows.iter().rev().find(|row| {
            row.offset > owner_lower_bound && row.offset < replay_offset && row.type_byte == 0x2c
        }) else {
            continue;
        };
        let Some(last_control_point) = control_point_start.checked_add(3) else {
            continue;
        };
        replays.push(TabulatedCylinderCurveReplay {
            surface_id: owner.id,
            curve_id,
            curve_type: 0x13,
            flip: 0x01,
            tangent_condition: 0x00,
            degree: 3,
            parameter_body: vec![0x18, 0xe6, 0x0f, 0xe6],
            control_point_ids: [
                control_point_start,
                control_point_start + 1,
                control_point_start + 2,
                last_control_point,
            ],
            successor_reference,
            control_point_bodies: bodies,
            control_points,
            terminal_reference: *terminal_reference,
            offset: replay_offset,
            surface_row_offset: owner.offset,
        });
    }
    replays.sort_by_key(|replay| replay.offset);
    replays
}

fn surface_body_compound_close(
    kind: SurfaceKind,
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<usize> {
    let mut cursor = 0;
    while cursor < body.len() {
        if body[cursor] == psb::token::COMPOUND_CLOSE {
            return Some(cursor);
        }
        if let Some((_, next)) = decode_row_scalar(kind, body, cursor, cache) {
            cursor = next;
        } else if matches!(body.get(cursor), Some(0x73 | 0xbb)) && cursor + 7 <= body.len() {
            cursor += 7;
        } else {
            cursor += 1;
        }
    }
    None
}

fn first_compound_close(payload: &[u8], start: usize, end: usize) -> Option<usize> {
    for token in psb::tokens(payload.get(start..end)?) {
        match token.kind {
            psb::TokenKind::CompoundClose => return Some(start + token.offset),
            psb::TokenKind::NamedRecord => return None,
            _ => {}
        }
    }
    None
}

fn scalar_slots(body: &[u8], count: usize, cache: &scalar::ScalarCache) -> Vec<Option<f64>> {
    let mut slots = Vec::with_capacity(count);
    let mut cursor = 0;
    while cursor < body.len() && slots.len() < count {
        if let Some((value, next)) = scalar::decode_in_lane(body, cursor, cache) {
            slots.push(Some(value));
            cursor = next;
        } else {
            cursor += 1;
        }
    }
    slots.resize(count, None);
    slots
}

type ScalarTokenSlot = (Option<f64>, Vec<u8>);

fn scalar_slots_with_tokens(
    body: &[u8],
    count: usize,
    cache: &scalar::ScalarCache,
) -> Vec<ScalarTokenSlot> {
    scalar_slots_with_tokens_and_end(body, count, cache).0
}

fn scalar_slots_with_tokens_and_end(
    body: &[u8],
    count: usize,
    cache: &scalar::ScalarCache,
) -> (Vec<ScalarTokenSlot>, usize) {
    let mut slots = Vec::with_capacity(count);
    let mut cursor = 0;
    while cursor < body.len() && slots.len() < count {
        if body[cursor] == 0x18 && cursor + 1 == body.len() {
            slots.push((Some(0.0), vec![0x18]));
            cursor += 1;
        } else if let Some((value, next)) = scalar::decode_in_surface_row_lane(body, cursor, cache)
        {
            slots.push((Some(value), body[cursor..next].to_vec()));
            cursor = next;
        } else if matches!(body.get(cursor), Some(0x73 | 0xbb)) && cursor + 7 <= body.len() {
            slots.push((None, body[cursor..cursor + 7].to_vec()));
            cursor += 7;
        } else {
            cursor += 1;
        }
    }
    slots.resize_with(count, || (None, Vec::new()));
    (slots, cursor)
}

fn slot_equality(first: &(Option<f64>, Vec<u8>), second: &(Option<f64>, Vec<u8>)) -> Option<bool> {
    match (first.0, second.0) {
        (Some(first), Some(second)) => {
            let scale = first.abs().max(second.abs()).max(1.0);
            Some((first - second).abs() <= 1e-9 * scale)
        }
        (None, None) if !first.1.is_empty() && !second.1.is_empty() => Some(first.1 == second.1),
        _ => None,
    }
}

fn row_scalar_slots(body: &[u8], count: usize, cache: &scalar::ScalarCache) -> Vec<Option<f64>> {
    let mut slots = Vec::with_capacity(count);
    let mut cursor = 0;
    while cursor < body.len() && slots.len() < count {
        if body[cursor] == 0x18 && cursor + 1 == body.len() {
            slots.push(Some(0.0));
            cursor += 1;
            continue;
        }
        if body.get(cursor..cursor + 2) == Some(&[0x18, 0xe5]) {
            slots.extend([Some(0.0), Some(1.0), Some(0.0)]);
            cursor += 2;
            continue;
        }
        if body.get(cursor) == Some(&0x18)
            && body
                .get(cursor + 1)
                .is_some_and(|byte| matches!(byte, 0x10 | 0xe4 | 0xe6))
        {
            slots.push(Some(0.0));
            cursor += 1;
            continue;
        }
        if body.get(cursor) == Some(&0x10) {
            slots.push(Some(0.0));
            cursor += 1;
            continue;
        }
        if let Some((value, next)) = scalar::decode_in_surface_row_lane(body, cursor, cache) {
            slots.push(Some(value));
            cursor = next;
        } else {
            cursor += 1;
        }
    }
    slots.resize(count, None);
    slots
}

struct PlaneFrame {
    origin: Option<[f64; 3]>,
    u_axis: Option<[f64; 3]>,
    normal: Option<[f64; 3]>,
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
    let (Some(first), Some(second)) = (triple([0, 1, 2]), triple([6, 7, 8])) else {
        return PlaneFrame {
            origin,
            u_axis: None,
            normal: None,
        };
    };
    let first_magnitude = first.iter().map(|value| value * value).sum::<f64>().sqrt();
    let second_magnitude = second.iter().map(|value| value * value).sum::<f64>().sqrt();
    let scale = first_magnitude.max(second_magnitude);
    if (first_magnitude - second_magnitude).abs() > 0.05 * scale.max(1e-9) {
        return PlaneFrame {
            origin,
            u_axis: None,
            normal: None,
        };
    }
    let u_axis = (first_magnitude > 1e-6).then(|| {
        [
            first[0] / first_magnitude,
            first[1] / first_magnitude,
            first[2] / first_magnitude,
        ]
    });
    let cross = [
        first[1].mul_add(second[2], -(first[2] * second[1])),
        first[2].mul_add(second[0], -(first[0] * second[2])),
        first[0].mul_add(second[1], -(first[1] * second[0])),
    ];
    let magnitude = cross.iter().map(|value| value * value).sum::<f64>().sqrt();
    let normal = (magnitude > 1e-6).then(|| {
        [
            cross[0] / magnitude,
            cross[1] / magnitude,
            cross[2] / magnitude,
        ]
    });
    PlaneFrame {
        origin,
        u_axis,
        normal,
    }
}

/// Decode the e3-bounded local-system chunk following each plane envelope.
pub fn plane_local_systems(payload: &[u8]) -> Vec<PlaneLocalSystem> {
    let cache = scalar::ScalarCache::from_section(payload);
    let headers = rows(payload)
        .into_iter()
        .filter(|row| row.kind == SurfaceKind::Plane)
        .filter_map(|row| positional_body_start(payload, &row).map(|body_start| (row, body_start)))
        .collect::<Vec<_>>();
    let mut systems = Vec::new();
    for (index, (row, envelope_start)) in headers.iter().enumerate() {
        let row_end = headers
            .get(index + 1)
            .map_or(payload.len(), |(next, _)| next.offset);
        let Some(envelope_close) = first_compound_close(payload, *envelope_start, row_end) else {
            continue;
        };
        let chunk_start = envelope_close + 1;
        let Some(chunk_end) = first_compound_close(payload, chunk_start, row_end) else {
            continue;
        };
        if chunk_end <= chunk_start {
            continue;
        }
        let body = payload[chunk_start..chunk_end].to_vec();
        let slots = row_scalar_slots(&body, 12, &cache);
        let frame = plane_frame(&slots);
        let simple = matches!(body.first(), Some(0x0f | 0x10 | 0x18))
            && body.len() <= 24
            && !body
                .iter()
                .any(|byte| matches!(byte, 0xe0..=0xe2 | 0xf1 | 0xf2 | 0xf7 | 0xf8));
        systems.push(PlaneLocalSystem {
            surface_id: row.id,
            body,
            slots,
            origin: frame.origin,
            u_axis: frame.u_axis,
            normal: frame.normal,
            classification: if simple {
                LocalSystemClassification::Simple
            } else {
                LocalSystemClassification::Unclassified
            },
            row_offset: row.offset,
            offset: chunk_start,
        });
    }
    systems
}

/// Decode plane positional envelope bodies into their two defined layouts.
pub fn plane_envelopes(payload: &[u8]) -> Vec<PlaneEnvelopeRecord> {
    const NAMED_OUTLINE: &[u8] = b"outline\0\xf9\x02\x03";
    let cache = scalar::ScalarCache::from_section(payload);
    let all_rows = rows(payload);
    let headers = all_rows
        .iter()
        .filter(|row| row.kind == SurfaceKind::Plane)
        .cloned()
        .filter_map(|row| positional_body_start(payload, &row).map(|body_start| (row, body_start)))
        .collect::<Vec<_>>();
    let mut envelopes = Vec::new();
    for (index, (row, body_start)) in headers.iter().enumerate() {
        let row_end = headers
            .get(index + 1)
            .map_or(payload.len(), |(next, _)| next.offset);
        let Some(body_end) = first_compound_close(payload, *body_start, row_end) else {
            continue;
        };
        let body = payload[*body_start..body_end].to_vec();
        let (envelope, corner_coordinate_equal) = if body.first() == Some(&0x0e) {
            let slots = scalar_slots_with_tokens(&body[1..], 9, &cache);
            let values = slots.iter().map(|slot| slot.0).collect::<Vec<_>>();
            (
                PlaneEnvelope::Compact {
                    prefix: [values[0], values[1], values[2]],
                    corners_3d: [
                        [values[3], values[4], values[5]],
                        [values[6], values[7], values[8]],
                    ],
                },
                [
                    slot_equality(&slots[3], &slots[6]),
                    slot_equality(&slots[4], &slots[7]),
                    slot_equality(&slots[5], &slots[8]),
                ],
            )
        } else {
            let slots = scalar_slots_with_tokens(&body, 10, &cache);
            let values = slots.iter().map(|slot| slot.0).collect::<Vec<_>>();
            (
                PlaneEnvelope::Standard {
                    bounds_2d: [[values[0], values[1]], [values[2], values[3]]],
                    corners_3d: [
                        [values[4], values[5], values[6]],
                        [values[7], values[8], values[9]],
                    ],
                },
                [
                    slot_equality(&slots[4], &slots[7]),
                    slot_equality(&slots[5], &slots[8]),
                    slot_equality(&slots[6], &slots[9]),
                ],
            )
        };
        envelopes.push(PlaneEnvelopeRecord {
            surface_id: row.id,
            body,
            envelope,
            corner_coordinate_equal,
            row_offset: row.offset,
            offset: *body_start,
        });
    }
    for (index, row) in all_rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.kind == SurfaceKind::Plane && row.boundary_type != 0)
    {
        let row_end = all_rows
            .get(index + 1)
            .map_or(payload.len(), |next| next.offset);
        let named_end = payload[row.offset..row_end]
            .windows(b"srf_prim_ptr(".len())
            .position(|window| window == b"srf_prim_ptr(")
            .map_or(row_end, |relative| row.offset + relative);
        let Some(relative) = payload[row.offset..named_end]
            .windows(NAMED_OUTLINE.len())
            .position(|window| window == NAMED_OUTLINE)
        else {
            continue;
        };
        let outline = row.offset + relative;
        let scalar_start = outline + NAMED_OUTLINE.len();
        let (slots, consumed) =
            scalar_slots_with_tokens_and_end(&payload[scalar_start..named_end], 6, &cache);
        if slots
            .iter()
            .any(|slot| slot.0.is_none() || slot.1.is_empty())
        {
            continue;
        }
        let values = slots.iter().map(|slot| slot.0).collect::<Vec<_>>();
        envelopes.push(PlaneEnvelopeRecord {
            surface_id: row.id,
            body: payload[scalar_start..scalar_start + consumed].to_vec(),
            envelope: PlaneEnvelope::Standard {
                bounds_2d: [[None; 2]; 2],
                corners_3d: [
                    [values[0], values[1], values[2]],
                    [values[3], values[4], values[5]],
                ],
            },
            corner_coordinate_equal: [
                slot_equality(&slots[0], &slots[3]),
                slot_equality(&slots[1], &slots[4]),
                slot_equality(&slots[2], &slots[5]),
            ],
            row_offset: row.offset,
            offset: scalar_start,
        });
    }
    envelopes.sort_by_key(|envelope| envelope.offset);
    envelopes
}

/// Decode fully specified scalar fields in labeled `srf_prim_ptr` prototype
/// records. A prototype is emitted only when its named kind is known.
pub fn prototypes(payload: &[u8]) -> Vec<SurfacePrototype> {
    let mut prototypes = named_prototype_records(payload)
        .into_iter()
        .filter_map(|record| {
            let kind = match record.family {
                SurfacePrototypeFamily::Plane => SurfaceKind::Plane,
                SurfacePrototypeFamily::Cylinder => SurfaceKind::Cylinder,
                SurfacePrototypeFamily::Cone => SurfaceKind::Cone,
                SurfacePrototypeFamily::Torus => SurfaceKind::TorusOrSphere,
                SurfacePrototypeFamily::Spline => SurfaceKind::Spline,
                SurfacePrototypeFamily::Fillet => SurfaceKind::Fillet,
                SurfacePrototypeFamily::Extrusion => SurfaceKind::Extrusion,
                SurfacePrototypeFamily::Other(_) => return None,
            };
            let scalar = |name: &str| match &record.field(name)?.value {
                SurfaceNamedValue::ScalarSequence(values) if values.len() == 1 => Some(values[0]),
                _ => None,
            };
            let radius = match kind {
                SurfaceKind::TorusOrSphere => scalar("radius1"),
                _ => scalar("radius"),
            };
            Some(SurfacePrototype {
                kind,
                radius,
                radius2: scalar("radius2"),
                half_angle: scalar("half_angle").filter(|value| valid_half_angle(*value)),
                offset: record.offset,
            })
        })
        .collect::<Vec<_>>();
    let mut start = 0;
    while let Some(record) = find(payload, b"srf_prim_ptr\0", start) {
        start = record + b"srf_prim_ptr\0".len();
        let end = find(payload, b"srf_prim_ptr\0", start).unwrap_or(payload.len());
        let Some(kind_label) = find_in(payload, b"geom_type\0", start, end) else {
            continue;
        };
        let Some(kind) = payload
            .get(kind_label + b"geom_type\0".len())
            .and_then(|value| SurfaceKind::from_byte(*value))
        else {
            continue;
        };
        let scalar_at = |label: &[u8]| {
            find_in(payload, label, start, end)
                .and_then(|pos| scalar::decode(payload, pos + label.len()).map(|(value, _)| value))
        };
        let half_angle = find_in(payload, b"half_angle\0", start, end).and_then(|pos| {
            scalar::decode_positive_dict(payload, pos + b"half_angle\0".len())
                .map(|(value, _)| value)
                .filter(|value| valid_half_angle(*value))
        });
        prototypes.push(SurfacePrototype {
            kind,
            radius: scalar_at(b"radius\0"),
            radius2: scalar_at(b"radius2\0"),
            half_angle,
            offset: record,
        });
    }
    prototypes.sort_by_key(|prototype| prototype.offset);
    prototypes
}

fn valid_half_angle(value: f64) -> bool {
    value.is_finite() && (0.0..std::f64::consts::FRAC_PI_2).contains(&value)
}

fn find(data: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    data.get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|relative| from + relative)
}

fn find_in(data: &[u8], needle: &[u8], from: usize, end: usize) -> Option<usize> {
    data.get(from..end)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|relative| from + relative)
}

fn id_ending_at(payload: &[u8], type_offset: usize) -> Option<(u32, usize)> {
    if type_offset >= 2 && matches!(payload[type_offset - 2], 0x80..=0xbf) {
        let (value, end) = compact_int(payload, type_offset - 2);
        if end == type_offset {
            return Some((value, type_offset - 2));
        }
    }
    let start = type_offset.checked_sub(1)?;
    (payload[start] < 0x80).then_some((payload[start] as u32, start))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_one_byte_and_two_byte_surface_rows() {
        let payload = [
            7, 0x22, 4, 0x01, 0, 0x80, 0x80, // plane id 7 -> 128
            0x80, 0x80, 0x24, 0x81, 0x01, 0xf6, 0x06, 7,
        ]; // cylinder id 128, feature 257, reversed -> 7
        assert_eq!(
            rows(&payload),
            vec![
                SurfaceRow {
                    id: 7,
                    type_byte: 0x22,
                    kind: SurfaceKind::Plane,
                    feature_id: 4,
                    reversed: false,
                    boundary_type: 0,
                    next_surface: 128,
                    offset: 0,
                },
                SurfaceRow {
                    id: 128,
                    type_byte: 0x24,
                    kind: SurfaceKind::Cylinder,
                    feature_id: 257,
                    reversed: true,
                    boundary_type: 6,
                    next_surface: 7,
                    offset: 7,
                },
            ]
        );
    }

    #[test]
    fn rejects_duplicate_surface_ids() {
        let duplicate_ids = [
            7, 0x22, 4, 0x01, 0, 0, // first id 7
            7, 0x24, 4, 0x01, 0, 0, // second id 7
        ];
        assert!(rows(&duplicate_ids).is_empty());
    }

    #[test]
    fn surface_array_frame_excludes_following_curve_namespace_bytes() {
        let mut payload = b"srf_array\0\xf8\x01".to_vec();
        payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
        payload.extend_from_slice(b"crv_array\0\xf8\x00");
        payload.extend_from_slice(&[8, 0x24, 5, 0x01, 0, 0]);

        assert_eq!(
            rows(&payload).iter().map(|row| row.id).collect::<Vec<_>>(),
            [7]
        );
    }

    #[test]
    fn surface_array_frame_withholds_a_count_mismatch() {
        let mut payload = b"srf_array\0\xf8\x01".to_vec();
        payload.extend_from_slice(&[7, 0x22, 4, 0x01, 0, 0]);
        payload.extend_from_slice(&[8, 0x24, 5, 0x01, 0, 0]);
        payload.extend_from_slice(b"crv_array\0\xf8\x00");

        assert!(rows(&payload).is_empty());
    }

    #[test]
    fn named_prototype_parameter_body_cannot_start_a_surface_row() {
        let payload = b"srf_array\0\xf8\x01srf_prim_ptr(torus)\0\xe3\
            \xe0\x02radius1\0\x07\x26\x04\x01\x00\x00\
            \xe0\x02radius2\0\xe4\xe3";

        assert!(rows(payload).is_empty());
    }

    #[test]
    fn opaque_seven_byte_surface_scalar_owns_its_tail() {
        let body = [0x73, 0xe4, 0x2f, 0x43, 0, 0xe3, 0xe0];
        let tokens = scalar_tokens(
            SurfaceKind::TorusOrSphere,
            &body,
            &scalar::ScalarCache::default(),
        );

        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].value, None);
        assert_eq!(tokens[0].offset, 0);
        assert_eq!(tokens[0].length, 7);
        assert_eq!(tokens[0].raw, body);
    }

    #[test]
    fn rejects_rows_without_the_fixed_discriminators() {
        assert!(rows(&[7, 0x22, 4, 0x02, 0, 8]).is_empty());
        assert!(rows(&[7, 0x22, 4, 0x01, 0x20, 8]).is_empty());
    }

    #[test]
    fn decodes_named_prototype_scalars_without_promoting_them_to_instances() {
        let payload = b"srf_prim_ptr\0geom_type\0\x24radius\0\x2a\xf4\0\
                        srf_prim_ptr\0geom_type\0\x25half_angle\0\x74\x21\xfb\x54\x44\x2d\x23";
        assert_eq!(
            prototypes(payload),
            vec![
                SurfacePrototype {
                    kind: SurfaceKind::Cylinder,
                    radius: Some(1.25),
                    radius2: None,
                    half_angle: None,
                    offset: 0
                },
                SurfacePrototype {
                    kind: SurfaceKind::Cone,
                    radius: None,
                    radius2: None,
                    half_angle: Some(f64::from_be_bytes([
                        0x3f, 0xe9, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x23,
                    ])),
                    offset: 34
                },
            ]
        );
    }

    #[test]
    fn bounds_last_named_prototype_field_at_compound_close() {
        let payload = b"srf_prim_ptr(torus)\0\xe0\x01radius2\0\x2e\x05\x33\xf1\xf7\x0e\xe3\
                        \x07\x26\x04\x01\0\0";
        let records = named_prototype_records(payload);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].parameters.len(), 1);
        let field = &records[0].parameters[0];
        assert_eq!(field.name, "radius2");
        assert_eq!(field.body, [0x2e, 0x05, 0x33, 0xf1, 0xf7, 0x0e]);
        assert_eq!(field.value, SurfaceNamedValue::ScalarSequence(vec![2.65]));
    }

    #[test]
    fn parenthesized_prototype_ends_at_legacy_prototype_record() {
        let payload = b"srf_prim_ptr(plane)\0\xe0\x02local_sys\0\xf9\x04\x03\
            \x0f\x18\xe5\x0f\x18\xe5\x0f\x18\xe5\
            \xe0\x00srf_prim_ptr\0geom_type\0\x24\
            \xe0\x02radius\0\x2f\x05\x00";

        let records = named_prototype_records(payload);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].family, SurfacePrototypeFamily::Plane);
        assert_eq!(records[0].parameters.len(), 1);
        assert_eq!(records[0].parameters[0].name, "local_sys");
    }

    #[test]
    fn parenthesized_prototype_ends_at_peer_entity_record() {
        let payload = b"srf_prim_ptr(plane)\0\xe0\x02local_sys\0\xf9\x04\x03\
            \x0f\x18\xe5\x0f\x18\xe5\x0f\x18\xe5\
            \xe0\x00entity_ptr(coord_sys)\0\xe3\
            \xe0\x02radius\0\x2f\x05\x00";

        let records = named_prototype_records(payload);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].parameters.len(), 1);
        assert_eq!(records[0].parameters[0].name, "local_sys");
    }

    #[test]
    fn analytic_prototype_does_not_claim_nested_curve_parameters() {
        let payload = b"srf_prim_ptr(torus)\0\
            \xe0\x02local_sys\0\xf9\x04\x03\x0f\x18\xe5\x0f\x18\xe5\x0f\x18\xe5\
            \xe0\x02radius1\0\x18\
            \xe0\x02radius2\0\x2f\x05\x00\
            \xe0\x00curve(b_spline)\0\xe3\
            \xe0\x00c_pnts\0\xf8\x04\xf7\x50\xfb";

        let records = named_prototype_records(payload);

        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0]
                .parameters
                .iter()
                .map(|parameter| parameter.name.as_str())
                .collect::<Vec<_>>(),
            ["local_sys", "radius1", "radius2"]
        );
    }

    #[test]
    fn terminal_zero_decodes_in_a_bounded_named_scalar_field() {
        let payload = b"srf_prim_ptr(torus)\0\xe0\x01radius1\0\x18\xe3";
        let records = named_prototype_records(payload);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].parameters.len(), 1);
        assert_eq!(
            records[0].parameters[0].value,
            SurfaceNamedValue::ScalarSequence(vec![0.0])
        );
    }

    #[test]
    fn summarizes_parenthesized_analytic_prototypes() {
        let payload = b"srf_prim_ptr(torus)\0\xe0\x01radius1\0\x18\xe0\x01radius2\0\x2e\x05\x33\xf1\xf7\x0e\xe3";

        assert_eq!(
            prototypes(payload),
            vec![SurfacePrototype {
                kind: SurfaceKind::TorusOrSphere,
                radius: Some(0.0),
                radius2: Some(2.65),
                half_angle: None,
                offset: 0,
            }]
        );
    }

    #[test]
    fn distinguishes_spline_and_fillet_surface_families() {
        let payload = b"srf_prim_ptr(splsrf)\0\xe3srf_prim_ptr(fillet_srf)\0\xe3";
        let records = named_prototype_records(payload);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].family, SurfacePrototypeFamily::Spline);
        assert_eq!(records[1].family, SurfacePrototypeFamily::Fillet);
        assert_eq!(
            prototypes(payload)
                .into_iter()
                .map(|prototype| prototype.kind)
                .collect::<Vec<_>>(),
            [SurfaceKind::Spline, SurfaceKind::Fillet]
        );
    }

    #[test]
    fn summarizes_seven_byte_torus_radius() {
        let payload = b"srf_prim_ptr(torus)\0\xe0\x01radius1\0\x5e\x33\x33\x33\x33\x33\x2c\xe0\x01radius2\0\x29\xc9\x99\xe3";

        assert!(matches!(
            prototypes(payload).as_slice(),
            [SurfacePrototype {
                kind: SurfaceKind::TorusOrSphere,
                radius: Some(major),
                radius2: Some(minor),
                ..
            }] if (*major - 0.3).abs() < 1e-12 && (*minor - 0.2).abs() < 1e-12
        ));
    }

    #[test]
    fn scalar_tail_named_marker_does_not_end_prototype_field() {
        let payload = b"srf_prim_ptr(torus)\0\xe0\x01radius1\0\xe4\xe0\x01radius2\0\x71\xe0\0\0\0\0\0\0\xe0\x01c_pnts\0\xf8\0";
        let records = named_prototype_records(payload);
        let radius2 = records[0].field("radius2").expect("radius2 field");

        assert_eq!(radius2.body, [0x71, 0xe0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            radius2.value,
            SurfaceNamedValue::ScalarSequence(vec![f64::from_be_bytes([
                0x3f, 0xe0, 0, 0, 0, 0, 0, 0,
            ])])
        );
    }

    #[test]
    fn derives_one_held_coordinate_outline_plane() {
        let records = [PlaneEnvelopeRecord {
            surface_id: 42,
            body: Vec::new(),
            envelope: PlaneEnvelope::Standard {
                bounds_2d: [[Some(0.0), Some(1.0)], [Some(0.0), Some(1.0)]],
                corners_3d: [
                    [Some(3.0), Some(-2.0), Some(4.0)],
                    [Some(3.0), Some(5.0), Some(9.0)],
                ],
            },
            corner_coordinate_equal: [Some(true), Some(false), Some(false)],
            row_offset: 10,
            offset: 20,
        }];
        assert_eq!(
            outline_planes(&records),
            vec![OutlinePlane {
                surface_id: 42,
                origin: [3.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
                u_axis: [0.0, 1.0, 0.0],
                offset: 20,
            }]
        );
    }

    #[test]
    fn derives_plane_with_unresolved_distinct_corner_coordinates() {
        let records = [PlaneEnvelopeRecord {
            surface_id: 42,
            body: Vec::new(),
            envelope: PlaneEnvelope::Standard {
                bounds_2d: [[None; 2]; 2],
                corners_3d: [
                    [Some(-3.0), Some(-4.0), None],
                    [Some(5.0), Some(-4.0), None],
                ],
            },
            corner_coordinate_equal: [Some(false), Some(true), Some(false)],
            row_offset: 10,
            offset: 20,
        }];
        assert_eq!(outline_planes(&records)[0].origin, [0.0, -4.0, 0.0]);
        assert_eq!(outline_planes(&records)[0].normal, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn unresolved_seven_byte_scalars_preserve_slot_identity() {
        let body = [
            0xbb, 1, 2, 3, 4, 5, 6, 0xbb, 1, 2, 3, 4, 5, 6, 0x73, 1, 2, 3, 4, 5, 6,
        ];
        let slots = scalar_slots_with_tokens(&body, 3, &scalar::ScalarCache::default());

        assert_eq!(
            slots.iter().map(|slot| slot.0).collect::<Vec<_>>(),
            vec![None; 3]
        );
        assert_eq!(slot_equality(&slots[0], &slots[1]), Some(true));
        assert_eq!(slot_equality(&slots[1], &slots[2]), Some(false));
    }

    #[test]
    fn terminal_positional_slot_zero_occupies_one_byte() {
        let slots = scalar_slots_with_tokens(&[0xe4, 0x18], 2, &scalar::ScalarCache::default());

        assert_eq!(slots, [(Some(1.0), vec![0xe4]), (Some(0.0), vec![0x18])]);
    }

    #[test]
    fn named_local_system_expands_row_lane_zero_forms() {
        let body = [
            0xf9, 0x04, 0x03, 0x18, 0xe4, 0x0f, 0x18, 0x0f, 0x18, 0x10, 0x18, 0xe4, 0x43, 0xe0,
            0x00, 0x18, 0xe4,
        ];

        assert_eq!(
            named_surface_value("local_sys", &body, &scalar::ScalarCache::default()),
            SurfaceNamedValue::ScalarArray {
                dimensions: 4,
                count: 3,
                values: vec![
                    Some(0.0),
                    Some(1.0),
                    Some(0.0),
                    Some(0.0),
                    Some(0.0),
                    Some(0.0),
                    Some(0.0),
                    Some(0.0),
                    Some(1.0),
                    Some(-0.5),
                    Some(0.0),
                    Some(1.0),
                ],
            }
        );
    }

    #[test]
    fn named_local_system_decodes_terminal_zero_slot() {
        let payload = b"srf_prim_ptr(cylinder)\0\xe0\x02local_sys\0\xf9\x04\x03\x18\xe5\x0f\x0f\x0f\xe4\x0f\x0f\x0f\x2f\x2e\0\x18\xe0\x01radius\0\xe4";
        let records = named_prototype_records(payload);

        assert_eq!(
            records[0].field("local_sys").map(|field| &field.value),
            Some(&SurfaceNamedValue::ScalarArray {
                dimensions: 4,
                count: 3,
                values: vec![
                    Some(0.0),
                    Some(1.0),
                    Some(0.0),
                    Some(0.0),
                    Some(0.0),
                    Some(0.0),
                    Some(1.0),
                    Some(0.0),
                    Some(0.0),
                    Some(0.0),
                    Some(15.0),
                    Some(0.0),
                ],
            })
        );
    }

    #[test]
    fn withholds_ambiguous_outline_plane() {
        let records = [PlaneEnvelopeRecord {
            surface_id: 42,
            body: Vec::new(),
            envelope: PlaneEnvelope::Compact {
                prefix: [None; 3],
                corners_3d: [
                    [Some(3.0), Some(2.0), Some(4.0)],
                    [Some(3.0), Some(2.0), Some(9.0)],
                ],
            },
            corner_coordinate_equal: [Some(true), Some(true), Some(false)],
            row_offset: 10,
            offset: 20,
        }];
        assert!(outline_planes(&records).is_empty());
    }
}
