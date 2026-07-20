// SPDX-License-Identifier: Apache-2.0
//! Surface namespace rows and prototype parameters.
//!
//! A [`SurfaceRow`] identifies a surface family and its feature, orientation,
//! boundary, and namespace links. A [`SurfacePrototype`] contains named template
//! parameters. A named prototype locates its adjacent first positional instance.

use cadmpeg_ir::cursor::bounded_len;

use crate::psb::{self, compact_int};
use crate::scalar;
use std::collections::{BTreeMap, BTreeSet};

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

/// Return the surface row for `id` only when the namespace contains one match.
pub(crate) fn unique_surface_row(rows: &[SurfaceRow], id: u32) -> Option<&SurfaceRow> {
    let mut matches = rows.iter().filter(|row| row.id == id);
    let row = matches.next()?;
    matches.next().is_none().then_some(row)
}

/// Return rows whose native surface identifier occurs exactly once.
pub(crate) fn uniquely_identified_rows(rows: &[SurfaceRow]) -> Vec<&SurfaceRow> {
    let mut counts = BTreeMap::<u32, usize>::new();
    for row in rows {
        *counts.entry(row.id).or_default() += 1;
    }
    rows.iter()
        .filter(|row| counts.get(&row.id) == Some(&1))
        .collect()
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
        dimensions: u32,
        /// Stored element count.
        count: u32,
        /// Decoded slots with unresolved values retained.
        values: Vec<Option<f64>>,
        /// Exact token bytes for each declared slot.
        tokens: Vec<Vec<u8>>,
    },
    /// Counted `f8` scalar body.
    CountedScalarArray {
        /// Stored element count.
        count: u32,
        /// Decoded slots with unresolved values retained.
        values: Vec<Option<f64>>,
        /// Exact token bytes for each declared slot.
        tokens: Vec<Vec<u8>>,
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
    /// Replay-bound tabulated-cylinder envelope frame decoded with the
    /// containing section's scalar cache.
    pub tabulated_cylinder_frame: Option<TabulatedCylinderFrame>,
    /// Complete analytic carrier decoded from a positional cylinder row.
    pub positional_cylinder_frame: Option<PositionalCylinderFrame>,
    /// Complete analytic carrier decoded from a positional cone row.
    pub positional_cone_frame: Option<PositionalConeFrame>,
    /// Structural form that bounded the body.
    pub boundary: SurfaceBodyBoundary,
    /// Byte offset of the positional surface row in the original stream.
    pub offset: usize,
    /// Byte offset of the first parameter-body byte in the original stream.
    pub body_offset: usize,
}

/// Return the positional parameter record for `surface_id` only when exactly
/// one exists.
pub(crate) fn unique_surface_parameter(
    records: &[SurfaceParameterRecord],
    surface_id: u32,
) -> Option<&SurfaceParameterRecord> {
    let mut matches = records
        .iter()
        .filter(|record| record.surface_id == surface_id);
    let record = matches.next()?;
    matches.next().is_none().then_some(record)
}

/// Six-slot model-space envelope frame following a tabulated-cylinder marker.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabulatedCylinderFrame {
    /// Ordered frame coordinates.
    pub values: [f64; 6],
    /// Scalar-lane prefix byte for each coordinate.
    pub prefixes: [u8; 6],
}

/// Complete model-space carrier and optional axial extent from a positional cylinder row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionalCylinderFrame {
    /// Model-space origin at one axial end of the bounded cylinder.
    pub origin: [f64; 3],
    /// Unit axis directed from `origin` toward the other axial end.
    pub axis: [f64; 3],
    /// Unit parameter-space reference direction.
    pub ref_direction: [f64; 3],
    /// Cylinder radius.
    pub radius: f64,
    /// Positive distance between axial ends when the body stores an extent.
    pub length: Option<f64>,
}

/// Complete model-space carrier decoded from a positional cone row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionalConeFrame {
    /// Model-space cone apex.
    pub apex: [f64; 3],
    /// Unit axis directed from the apex toward increasing radius.
    pub axis: [f64; 3],
    /// Unit parameter-space reference direction.
    pub ref_direction: [f64; 3],
    /// Positive cone half-angle in radians.
    pub half_angle: f64,
}

/// Six-slot outline frame in a positional torus-or-sphere body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TorusOutlineFrame {
    /// Ordered outline coordinates.
    pub values: [f64; 6],
    /// Compact selector following the outline marker.
    pub selector: u32,
    /// Byte offset of the outline marker relative to the parameter body.
    pub offset: usize,
}

/// Five-coordinate endpoint envelope in an untagged type-26 body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Type26FiveCoordinateEnvelope {
    /// Final five coordinates after the leading body-local scalar.
    pub values: [f64; 5],
    /// Byte offset of the first retained coordinate relative to the body.
    pub offset: usize,
}

/// Four coordinates separated by a body-local control payload in a type-26 body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Type26SplitCoordinateEnvelope {
    /// Two coordinates before and two coordinates after the control payload.
    pub values: [f64; 4],
    /// Byte offset of the first coordinate relative to the body.
    pub offset: usize,
}

/// Tagged radius overrides in a positional torus-or-sphere body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TorusRadiusOverrides {
    /// Major torus radius, or zero for a sphere.
    pub radius1: f64,
    /// Minor torus radius, or sphere radius.
    pub radius2: f64,
    /// Whether the first stored scalar is `radius2` or `radius1 + radius2`.
    pub radius2_encoding: TorusRadius2Encoding,
    /// Byte offset of the `18 0d` radius trailer marker.
    pub offset: usize,
}

/// Interpretation of the first radial scalar in a tagged type-26 trailer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TorusRadius2Encoding {
    /// The scalar stores `radius2` directly.
    Direct,
    /// The scalar stores the outer ring radius `radius1 + radius2`.
    OuterRingDifference,
}

/// Terminal half-angle override in a positional cone body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConeHalfAngleOverride {
    /// Cone half-angle in radians, strictly between zero and pi/2.
    pub radians: f64,
    /// Byte offset of the positive-DICT token relative to the parameter body.
    pub offset: usize,
}

#[derive(Debug, Clone, Copy)]
struct ConeHalfAngleLayout {
    value: f64,
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
    /// Decode the terminal positive-DICT half-angle of a positional cone body.
    #[must_use]
    pub fn cone_half_angle_override(&self, type_byte: u8) -> Option<ConeHalfAngleOverride> {
        if type_byte != 0x25 {
            return None;
        }
        let layout = terminal_cone_half_angle_layout(&self.body)?;
        Some(ConeHalfAngleOverride {
            radians: layout.value,
            offset: layout.start,
        })
    }

    /// Decode the tagged radius trailer of a positional torus-or-sphere body.
    #[must_use]
    pub fn torus_radius_overrides(&self, type_byte: u8) -> Option<TorusRadiusOverrides> {
        if type_byte != 0x26 {
            return None;
        }
        torus_radius_override_layout(&self.body).map(|layout| layout.overrides)
    }

    /// Decode the terminal outline frame of a positional torus-or-sphere body.
    #[must_use]
    pub fn torus_outline_frame(&self, type_byte: u8) -> Option<TorusOutlineFrame> {
        if type_byte != 0x26 {
            return None;
        }
        let markers = torus_outline_markers(&self.body);
        let [(marker, after_selector, selector)] = markers.as_slice() else {
            return None;
        };
        let slots = self
            .scalar_tokens
            .iter()
            .filter(|slot| slot.offset >= *after_selector)
            .collect::<Vec<_>>();
        let [a0, a1, a2, b0, b1, b2] = slots.as_slice() else {
            return None;
        };
        let mut cursor = *after_selector;
        for slot in &slots {
            (slot.offset == cursor).then_some(())?;
            cursor = cursor.checked_add(slot.length)?;
        }
        (cursor == self.body.len()).then_some(())?;
        let values = [
            a0.value?, a1.value?, a2.value?, b0.value?, b1.value?, b2.value?,
        ];
        values
            .iter()
            .all(|value| value.is_finite())
            .then_some(TorusOutlineFrame {
                values,
                selector: *selector,
                offset: *marker,
            })
    }

    /// Decode the bounded untagged five-coordinate type-26 envelope.
    #[must_use]
    pub fn type26_five_coordinate_envelope(
        &self,
        type_byte: u8,
    ) -> Option<Type26FiveCoordinateEnvelope> {
        (type_byte == 0x26).then_some(())?;
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
            cursor = cursor.checked_add(slot.length)?;
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
    pub fn type26_split_coordinate_envelope(
        &self,
        type_byte: u8,
    ) -> Option<Type26SplitCoordinateEnvelope> {
        (type_byte == 0x26
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
    pub fn type24_round_radius(&self, type_byte: u8) -> Option<f64> {
        (type_byte == 0x24).then_some(())?;
        let contiguous_values = |frame: &SurfaceParameterScalarFrame| {
            let mut cursor = frame.offset;
            let mut values = Vec::with_capacity(frame.slots.len());
            for slot in &frame.slots {
                (slot.offset == cursor).then_some(())?;
                cursor = cursor.checked_add(slot.length)?;
                values.push(slot.value?);
            }
            values.iter().all(|value| value.is_finite()).then_some(())?;
            Some((values, cursor))
        };
        let (diameter_endpoints, extent_endpoints) = match self.scalar_frames.as_slice() {
            [frame]
                if matches!(
                    self.body.get(..frame.offset),
                    Some([0x15] | [0x00, 0x15, 0x1c])
                ) =>
            {
                let (values, end) = contiguous_values(frame)?;
                let [first, _, second, a0, a1, a2, b0, b1, b2] = values.as_slice() else {
                    return None;
                };
                (end == self.body.len()).then_some(())?;
                ([*first, *second], [[*a0, *a1, *a2], [*b0, *b1, *b2]])
            }
            [leading, trailing] => {
                let (leading_values, leading_end) = contiguous_values(leading)?;
                let [_, first] = leading_values.as_slice() else {
                    return None;
                };
                let (trailing_values, trailing_end) = contiguous_values(trailing)?;
                let [second, a0, a1, a2, b0, b1, b2] = trailing_values.as_slice() else {
                    return None;
                };
                (leading.offset == 0
                    && self.body.get(leading_end..trailing.offset) == Some(&[0x12])
                    && trailing_end == self.body.len())
                .then_some(())?;
                ([*first, *second], [[*a0, *a1, *a2], [*b0, *b1, *b2]])
            }
            _ => return None,
        };
        let diameter = (diameter_endpoints[1] - diameter_endpoints[0]).abs();
        let scale = diameter_endpoints
            .iter()
            .chain(extent_endpoints.iter().flatten())
            .map(|value| value.abs())
            .fold(1.0, f64::max);
        (diameter > 1e-12 * scale).then_some(())?;
        extent_endpoints[0]
            .iter()
            .zip(extent_endpoints[1])
            .any(|(first, second)| ((second - first).abs() - diameter).abs() <= 1e-9 * scale)
            .then_some(0.5 * diameter)
    }

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
    /// Exact token bytes for each declared envelope scalar slot.
    pub scalar_tokens: Vec<Vec<u8>>,
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
    /// Deterministic positive in-plane direction for carrier constructions.
    /// The outline does not define the surface parameter chart.
    pub u_axis: [f64; 3],
    /// Byte offset of the outline body.
    pub offset: usize,
}

/// Return the outline plane for `surface_id` only when exactly one exists.
pub(crate) fn unique_outline_plane(
    planes: &[OutlinePlane],
    surface_id: u32,
) -> Option<&OutlinePlane> {
    let mut matches = planes.iter().filter(|plane| plane.surface_id == surface_id);
    let plane = matches.next()?;
    matches.next().is_none().then_some(plane)
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

/// Place axis-aligned plane outlines whose support frame selects one proven
/// held coordinate even when other outline-coordinate relations are unresolved.
#[must_use]
pub fn frame_bound_outline_planes(
    envelopes: &[PlaneEnvelopeRecord],
    frames: &[PlaneLocalSystem],
) -> Vec<OutlinePlane> {
    let vectors_agree = |first: [f64; 3], second: [f64; 3]| {
        first.iter().zip(second).all(|(first, second)| {
            (first - second).abs() <= 1e-10 * first.abs().max(second.abs()).max(1.0)
        })
    };
    let mut result = Vec::new();
    for record in envelopes {
        let support_frames = frames
            .iter()
            .filter(|frame| frame.surface_id == record.surface_id)
            .filter_map(|frame| Some((frame.normal?, frame.u_axis?)))
            .collect::<Vec<_>>();
        let Some(&(normal, u_axis)) = support_frames.first() else {
            continue;
        };
        if support_frames
            .iter()
            .any(|(candidate_normal, candidate_u_axis)| {
                !vectors_agree(normal, *candidate_normal)
                    || !vectors_agree(u_axis, *candidate_u_axis)
            })
        {
            continue;
        }
        let axes = normal
            .iter()
            .enumerate()
            .filter_map(|(axis, value)| (value.abs() > 1e-9).then_some(axis))
            .collect::<Vec<_>>();
        let [axis] = axes.as_slice() else {
            continue;
        };
        let shortened_held_coordinate = record.scalar_tokens.len() == 10
            && record.scalar_tokens[..8]
                .iter()
                .all(|token| !token.is_empty())
            && record.scalar_tokens[8..].iter().all(Vec::is_empty)
            && !record.scalar_tokens[4 + *axis].is_empty()
            && record.scalar_tokens[4 + *axis] == record.scalar_tokens[7];
        if record.corner_coordinate_equal[*axis] != Some(true) && !shortened_held_coordinate {
            continue;
        }
        let corners = match &record.envelope {
            PlaneEnvelope::Standard { corners_3d, .. }
            | PlaneEnvelope::Compact { corners_3d, .. } => corners_3d,
        };
        let Some(coordinate) = corners[0][*axis] else {
            continue;
        };
        let mut origin = [0.0; 3];
        origin[*axis] = coordinate;
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

/// Derive outline plane equations and retain complete support-frame directions
/// for carrier constructions when available.
#[must_use]
pub fn placed_outline_planes(
    envelopes: &[PlaneEnvelopeRecord],
    frames: &[PlaneLocalSystem],
) -> Vec<OutlinePlane> {
    let frame_bound = frame_bound_outline_planes(envelopes, frames);
    let frame_bound_ids = frame_bound
        .iter()
        .map(|plane| plane.surface_id)
        .collect::<BTreeSet<_>>();
    let mut result = outline_planes(envelopes)
        .into_iter()
        .filter(|plane| !frame_bound_ids.contains(&plane.surface_id))
        .collect::<Vec<_>>();
    result.extend(frame_bound);
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
    rows_with_boundaries(payload, BOUNDARY_TYPES)
}

/// Discover rows from a DEPDB `Sld_Xsections` surface namespace.
/// Named prototype rows use boundary type `00`; positional replays use `06`.
#[must_use]
pub fn cross_section_rows(payload: &[u8]) -> Vec<SurfaceRow> {
    rows_with_boundaries(payload, &[0x00, 0x06])
}

fn rows_with_boundaries(payload: &[u8], boundary_types: &[u8]) -> Vec<SurfaceRow> {
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
    result.retain(|row| boundary_types.contains(&row.boundary_type));
    let frames = surface_array_frames(payload);
    if !frames.is_empty() {
        let unframed = result.clone();
        let mut framed = Vec::new();
        let mut saw_framed_candidate = false;
        for frame in frames {
            // Each framed row occupies at least one payload byte in the frame
            // span, so the declared count cannot exceed the span byte length.
            let capacity =
                bounded_len(frame.count as u64, 1, frame.end.saturating_sub(frame.start))
                    .unwrap_or(0);
            let mut selected = Vec::<SurfaceRow>::with_capacity(capacity);
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
        && !(matches!(family, SurfacePrototypeFamily::Torus)
            && matches!(name, "i_pnts" | "i_points" | "c_pnts"))
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
            let values_start = cursor;
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
            if matches!(name, "u_params" | "v_params") {
                // Each declared slot is at least a one-byte scalar token in the
                // value bytes, so the count cannot exceed the remaining bytes.
                let Some(slot_count) =
                    bounded_len(u64::from(count), 1, body.len().saturating_sub(values_start))
                else {
                    return SurfaceNamedValue::Opaque(body.to_vec());
                };
                let slots =
                    named_spline_scalar_slots(name, &body[values_start..], slot_count, cache);
                return SurfaceNamedValue::CountedScalarArray {
                    count,
                    values: slots.iter().map(|slot| slot.0).collect(),
                    tokens: slots.into_iter().map(|slot| slot.1).collect(),
                };
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
            if name == "params" {
                // Each declared slot is at least a one-byte scalar token in the
                // value bytes, so the count cannot exceed the remaining bytes.
                let Some(slot_count) =
                    bounded_len(u64::from(count), 1, body.len().saturating_sub(values_start))
                else {
                    return SurfaceNamedValue::Opaque(body.to_vec());
                };
                let slots =
                    named_spline_scalar_slots(name, &body[values_start..], slot_count, cache);
                return SurfaceNamedValue::CountedScalarArray {
                    count,
                    values: slots.iter().map(|slot| slot.0).collect(),
                    tokens: slots.into_iter().map(|slot| slot.1).collect(),
                };
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
        let Some(slot_count) = slot_count.filter(|slot_count| {
            dimensions_end > 1
                && values_start > dimensions_end
                && *slot_count
                    <= body
                        .len()
                        .saturating_sub(values_start)
                        .saturating_mul(2)
                        .max(12)
        }) else {
            return SurfaceNamedValue::Opaque(body.to_vec());
        };
        let spline_slots = matches!(
            name,
            "i_pnts"
                | "i_points"
                | "end_u_tangts"
                | "end_v_tangts"
                | "end_uv_deriv"
                | "tangts"
                | "end_tangts"
        )
        .then(|| named_spline_scalar_slots(name, &body[values_start..], slot_count, cache));
        return SurfaceNamedValue::ScalarArray {
            dimensions,
            count,
            values: if let Some(slots) = &spline_slots {
                slots.iter().map(|slot| slot.0).collect()
            } else if name == "local_sys" {
                row_scalar_slots(&body[values_start..], slot_count, cache)
            } else {
                scalar_slots(&body[values_start..], slot_count, cache)
            },
            tokens: if let Some(slots) = spline_slots {
                slots.into_iter().map(|slot| slot.1).collect()
            } else {
                Vec::new()
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
        let decoded = if matches!(name, "radius" | "radius1" | "radius2")
            && matches!(body[cursor], 0x0d | 0x0e)
        {
            Some((if body[cursor] == 0x0d { 0.25 } else { 0.5 }, cursor + 1))
        } else if name == "half_angle" {
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
        let mut named = tokens
            .iter()
            .filter_map(|token| {
                let token_offset = close + 2 + token.offset;
                (token.kind == psb::TokenKind::NamedRecord
                    && named_record_length(payload, token_offset) == Some(token.length))
                .then_some((token_offset, token.length))
            })
            .collect::<Vec<_>>();
        for token_offset in close + 2..record_end {
            let Some(length) = named_record_length(payload, token_offset) else {
                continue;
            };
            let name_start = token_offset + 2;
            let name_end = token_offset + length - 1;
            let name = String::from_utf8_lossy(&payload[name_start..name_end]);
            if prototype_parameter_allowed(&family, &name) {
                named.push((token_offset, length));
            }
        }
        named.sort_unstable();
        named.dedup();
        let mut parameters = Vec::new();
        for (position, (token_offset, token_length)) in named.iter().copied().enumerate() {
            let name_start = token_offset + 2;
            let name_end = token_offset + token_length - 1;
            let name = String::from_utf8_lossy(&payload[name_start..name_end]);
            if !prototype_parameter_allowed(&family, &name) {
                continue;
            }
            let value_offset = token_offset + token_length;
            let mut value_end = named
                .get(position + 1)
                .map_or(record_end, |(next, _)| *next);
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

fn torus_outline_markers(body: &[u8]) -> Vec<(usize, usize, u32)> {
    (0..body.len())
        .filter_map(|offset| {
            (body.get(offset..offset + 3) == Some(&[0x01, 0x12, 0x50])).then_some(())?;
            let (selector, end) = compact_int(body, offset + 3);
            (end > offset + 3).then_some((offset, end, selector))
        })
        .collect()
}

fn torus_radius_override_layout(body: &[u8]) -> Option<TorusRadiusOverrideLayout> {
    let mut markers = body
        .windows(2)
        .enumerate()
        .filter(|(_, bytes)| *bytes == [0x18, 0x0d]);
    let (offset, _) = markers.next()?;
    markers.next().is_none().then_some(())?;
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

fn unique_cone_half_angle_layout(
    body: &[u8],
    accepts_end: impl Fn(usize) -> bool,
) -> Option<ConeHalfAngleLayout> {
    let mut layouts = (0..body.len()).filter_map(|start| {
        let (value, end) = scalar::decode_positive_dict(body, start)?;
        (valid_half_angle(value) && accepts_end(end)).then_some(ConeHalfAngleLayout {
            value,
            start,
            end,
        })
    });
    let layout = layouts.next()?;
    layouts.next().is_none().then_some(layout)
}

fn terminal_cone_half_angle_layout(body: &[u8]) -> Option<ConeHalfAngleLayout> {
    unique_cone_half_angle_layout(body, |end| end == body.len())
}

fn cone_half_angle_before_close(body: &[u8]) -> Option<ConeHalfAngleLayout> {
    unique_cone_half_angle_layout(body, |end| {
        body.get(end) == Some(&psb::token::COMPOUND_CLOSE)
    })
}

fn scalar_tokens(
    kind: SurfaceKind,
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Vec<SurfaceParameterScalar> {
    let mut tokens = Vec::new();
    let outline_markers = if kind == SurfaceKind::TorusOrSphere {
        torus_outline_markers(body)
    } else {
        Vec::new()
    };
    let radius_layout = if kind == SurfaceKind::TorusOrSphere {
        torus_radius_override_layout(body)
    } else {
        None
    };
    let cone_half_angle = if kind == SurfaceKind::Cone {
        terminal_cone_half_angle_layout(body)
    } else {
        None
    };
    let mut cursor = 0;
    while cursor < body.len() {
        if let Some((_, end, _)) = outline_markers
            .iter()
            .find(|(offset, _, _)| *offset == cursor)
        {
            cursor = *end;
            continue;
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
                tokens.push(SurfaceParameterScalar {
                    value: Some(layout.value),
                    raw: body[layout.start..layout.end].to_vec(),
                    offset: layout.start,
                    length: layout.end - layout.start,
                });
                cursor = layout.end;
                continue;
            }
        }
        if let Some((value, next)) = decode_row_scalar(kind, body, cursor, cache) {
            if outline_markers
                .iter()
                .any(|(offset, _, _)| cursor < *offset && next > *offset)
            {
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
            tokens.push(SurfaceParameterScalar {
                value: Some(value),
                raw: body[cursor..next].to_vec(),
                offset: cursor,
                length: next - cursor,
            });
            cursor = next;
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
    let cone_half_angle = if kind == SurfaceKind::Cone {
        terminal_cone_half_angle_layout(body)
    } else {
        None
    };
    let mut cursor = 0;
    while cursor < body.len() {
        if let Some(layout) = cone_half_angle {
            if cursor == layout.start {
                cursor = layout.end;
                continue;
            }
        }
        if named_record_length(body, cursor).is_some() {
            return Some(cursor);
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
    None
}

/// Decode bounded parameter bodies for positional `srf_array` rows.
pub fn parameter_records(payload: &[u8]) -> Vec<SurfaceParameterRecord> {
    parameter_records_for_rows(payload, rows(payload))
}

/// Decode bounded positional parameter bodies from a DEPDB cross-section
/// surface namespace.
#[must_use]
pub fn cross_section_parameter_records(payload: &[u8]) -> Vec<SurfaceParameterRecord> {
    parameter_records_for_rows(payload, cross_section_rows(payload))
}

fn parameter_records_for_rows(
    payload: &[u8],
    rows: Vec<SurfaceRow>,
) -> Vec<SurfaceParameterRecord> {
    let cache = scalar::ScalarCache::from_section(payload);
    let mut headers = Vec::<(SurfaceRow, usize)>::new();
    for row in rows {
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
        let tabulated_cylinder_frame = (row.kind == SurfaceKind::Extrusion)
            .then(|| decode_tabulated_cylinder_frame(&body, &cache))
            .flatten()
            .map(|(frame, _)| frame);
        let positional_cylinder_frame = (row.kind == SurfaceKind::Cylinder)
            .then(|| decode_positional_cylinder_frame(&body, &cache))
            .flatten();
        let positional_cone_frame = (row.kind == SurfaceKind::Cone)
            .then(|| decode_positional_cone_frame(&body, &cache))
            .flatten();
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
            tabulated_cylinder_frame,
            positional_cylinder_frame,
            positional_cone_frame,
            body,
            boundary,
            offset: row.offset,
            body_offset: *body_start,
        });
    }
    records
}

fn decode_positional_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    decode_compact_y_axis_cylinder_frame(body, cache)
        .or_else(|| decode_local_system_cylinder_frame(body, cache))
        .or_else(|| decode_zero_support_cylinder_frame(body, cache))
        .or_else(|| decode_referenced_planar_envelope_cylinder_frame(body, cache))
        .or_else(|| decode_held_axis_cylinder_frame(body, cache))
        .or_else(|| decode_axial_radial_cylinder_frame(body, cache))
        .or_else(|| decode_compact_axis_aligned_cylinder_frame(body, cache))
        .or_else(|| decode_directrix_lane_axis_aligned_cylinder_frame(body, cache))
}

fn decode_compact_y_axis_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    let decode_values = |start: usize, count: usize| {
        let mut cursor = start;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            let (value, next) =
                scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
            value.is_finite().then_some(())?;
            values.push(value);
            cursor = next;
        }
        Some((values, cursor))
    };
    let (
        axial_start,
        axial_end,
        transverse_center,
        transverse_edge,
        radial_low,
        radial_high,
        repeated_start,
        repeated_end,
    ) = match body.first()? {
        0x14 => {
            let (values, end) = decode_values(1, 9)?;
            (end == body.len()).then_some(())?;
            let [axial_start, _, axial_end, transverse_center, repeated_start, radial_low, transverse_edge, repeated_end, radial_high] =
                values.as_slice()
            else {
                return None;
            };
            (
                *axial_start,
                *axial_end,
                *transverse_center,
                *transverse_edge,
                *radial_low,
                *radial_high,
                *repeated_start,
                *repeated_end,
            )
        }
        0x12 => {
            let (leading, marker) = decode_values(1, 1)?;
            (body.get(marker) == Some(&0x14)).then_some(())?;
            let (trailing, end) = decode_values(marker + 1, 7)?;
            (end == body.len()).then_some(())?;
            let [axial_end, transverse_edge, repeated_start, radial_low, transverse_center, repeated_end, radial_high] =
                trailing.as_slice()
            else {
                return None;
            };
            (
                leading[0],
                *axial_end,
                *transverse_center,
                *transverse_edge,
                *radial_low,
                *radial_high,
                *repeated_start,
                *repeated_end,
            )
        }
        _ => return None,
    };
    let scale = [
        axial_start,
        axial_end,
        transverse_center,
        transverse_edge,
        radial_low,
        radial_high,
    ]
    .into_iter()
    .map(f64::abs)
    .fold(1.0, f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    close(axial_start, repeated_start).then_some(())?;
    close(axial_end, repeated_end).then_some(())?;
    let radius = 0.5 * (radial_high - radial_low).abs();
    (radius > 1e-12 * scale).then_some(())?;
    close((transverse_edge - transverse_center).abs(), radius).then_some(())?;
    let length = (axial_end - axial_start).abs();
    (length > 1e-12 * scale).then_some(())?;
    Some(PositionalCylinderFrame {
        origin: [
            transverse_center,
            axial_start,
            f64::midpoint(radial_low, radial_high),
        ],
        axis: [0.0, (axial_end - axial_start).signum(), 0.0],
        ref_direction: [(transverse_edge - transverse_center).signum(), 0.0, 0.0],
        radius,
        length: Some(length),
    })
}

fn decode_positional_cone_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalConeFrame> {
    decode_planar_envelope_cone_frame(body, cache).or_else(|| {
        let angle = terminal_cone_half_angle_layout(body)?;
        decode_support_apex_cone_frame(&body[..angle.start], angle.value, cache)
    })
}

fn decode_planar_envelope_cone_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalConeFrame> {
    let close = |left: f64, right: f64| {
        let scale = left.abs().max(right.abs()).max(1.0);
        (left - right).abs() <= 1e-9 * scale
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
    let half_angle = radial_high.atan2(outer_distance);
    valid_half_angle(half_angle).then_some(())?;
    Some(PositionalConeFrame {
        apex: [0.0, outer_apex.midpoint(inner_apex), 0.0],
        axis: [0.0, 1.0, 0.0],
        ref_direction: [1.0, 0.0, 0.0],
        half_angle,
    })
}

fn decode_support_apex_cone_frame(
    body: &[u8],
    half_angle: f64,
    cache: &scalar::ScalarCache,
) -> Option<PositionalConeFrame> {
    valid_half_angle(half_angle).then_some(())?;
    let reference_candidates = (0..body.len())
        .filter_map(|start| {
            matches!(body.get(start), Some(0x19 | 0x32)).then_some(())?;
            let (_, end) = scalar::decode_model_reference_coordinate(body, start, cache)?;
            (end + 3 == body.len()).then_some(start)
        })
        .collect::<Vec<_>>();
    let [reference_start] = reference_candidates.as_slice() else {
        return None;
    };
    let apex_candidates = (0..*reference_start)
        .filter_map(|start| {
            let (apex, end) = scalar::decode_in_surface_row_lane(body, start, cache)?;
            (end == *reference_start && apex.is_finite()).then_some((apex, start))
        })
        .collect::<Vec<_>>();
    let [(apex_coordinate, apex_start)] = apex_candidates.as_slice() else {
        return None;
    };
    let support_candidates = (0..*apex_start)
        .filter_map(|start| {
            let mut frame = body.get(start..*apex_start)?.to_vec();
            frame.extend_from_slice(&[0x18, 0x18, 0x18]);
            let slots = scalar::decode_positional_plane_local_system_slots(&frame, cache)?;
            (slots[9..12] == [0.0, 0.0, 0.0]).then_some(slots)
        })
        .collect::<Vec<_>>();
    let [slots] = support_candidates.as_slice() else {
        return None;
    };
    let first: [f64; 3] = slots[0..3].try_into().ok()?;
    let second: [f64; 3] = slots[6..9].try_into().ok()?;
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
        <= 1e-9)
        .then_some(())?;
    let cross = [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ];
    let mut axis = normalize(cross)?;
    let axis_indices = axis
        .iter()
        .enumerate()
        .filter_map(|(index, value)| (value.abs() >= 1.0 - 1e-9).then_some(index))
        .collect::<Vec<_>>();
    let [axis_index] = axis_indices.as_slice() else {
        return None;
    };
    let mut apex = [0.0; 3];
    apex[*axis_index] = *apex_coordinate;
    (apex_coordinate.abs() > 1e-12).then_some(())?;
    if axis[*axis_index] * apex_coordinate > 0.0 {
        axis = axis.map(|value| -value);
    }
    Some(PositionalConeFrame {
        apex,
        axis,
        ref_direction: second.map(|value| -value),
        half_angle,
    })
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
        *half_angle,
        &scalar::ScalarCache::default(),
    )
}

fn decode_referenced_planar_envelope_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    body.starts_with(&[0x11, 0x18, 0x13]).then_some(())?;
    let mut cursor = 3;
    let (length, next) = scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
    cursor = next;
    let (first_radial, next) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
    cursor = next;
    let (first_axial, next) =
        if body.get(cursor) == Some(&0x18) && matches!(body.get(cursor + 1), Some(0x19 | 0x32)) {
            (0.0, cursor + 1)
        } else {
            scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?
        };
    cursor = next;
    matches!(body.get(cursor), Some(0x19 | 0x32)).then_some(())?;
    let (_, next) = scalar::decode_model_reference_coordinate(body, cursor, cache)?;
    cursor = next;
    let (second_radial, next) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
    cursor = next;
    let (second_axial, next) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
    cursor = next;
    let (radius, next) = scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
    let reversed = if next == body.len() {
        false
    } else if matches!(body.get(next..), Some([0xf7, 0x17 | 0x19])) {
        true
    } else {
        return None;
    };

    let values = [
        length,
        first_radial,
        first_axial,
        second_radial,
        second_axial,
        radius,
    ];
    values.iter().all(|value| value.is_finite()).then_some(())?;
    (length > 0.0 && radius > 0.0).then_some(())?;
    let scale = values.iter().map(|value| value.abs()).fold(1.0, f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    close((second_axial - first_axial).abs(), length).then_some(())?;
    close((second_radial - first_radial).abs(), 2.0 * radius).then_some(())?;

    let radial_midpoint = f64::midpoint(first_radial, second_radial);
    let orientation = if reversed { -1.0 } else { 1.0 };
    let axial_sign = orientation * (second_axial - first_axial).signum();
    let radial_sign = orientation * (second_radial - first_radial).signum();
    Some(PositionalCylinderFrame {
        origin: [radial_midpoint, second_axial, 0.0],
        axis: [0.0, axial_sign, 0.0],
        ref_direction: [radial_sign, 0.0, 0.0],
        radius,
        length: Some(length),
    })
}

fn decode_held_axis_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    body.starts_with(&[0x11, 0x18, 0x13]).then_some(())?;
    let mut cursor = 3;
    let decode = |cursor| {
        let (value, next) =
            scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
        value.is_finite().then_some(())?;
        Some((value, next))
    };
    let (held, next) = decode(cursor)?;
    cursor = next;
    let (first_radial, next) = decode(cursor)?;
    cursor = next;
    (body.get(cursor) == Some(&0x10)).then_some(())?;
    cursor += 1;
    let (first_axial, next) = decode(cursor)?;
    cursor = next;
    let (second_radial, next) = decode(cursor)?;
    cursor = next;
    (body.get(cursor) == Some(&0x19)).then_some(())?;
    let (_, next) = scalar::decode_model_reference_coordinate(body, cursor, cache)?;
    cursor = next;
    let (second_axial, next) = decode(cursor)?;
    cursor = next;
    (body.get(cursor..) == Some(&[0xf7, 0x17])).then_some(())?;

    let scale = [held, first_radial, first_axial, second_radial, second_axial]
        .into_iter()
        .map(f64::abs)
        .fold(1.0, f64::max);
    ((second_axial - first_axial).abs() <= 1e-9 * scale).then_some(())?;
    let radius = 0.5 * (second_radial - first_radial).abs();
    (radius > 0.0).then_some(())?;
    Some(PositionalCylinderFrame {
        origin: [
            f64::midpoint(first_radial, second_radial),
            held,
            second_axial,
        ],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [(second_radial - first_radial).signum(), 0.0, 0.0],
        radius,
        length: None,
    })
}

fn decode_axial_radial_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    body.starts_with(&[0x11, 0x18, 0x13]).then_some(())?;
    let decode = |cursor| {
        let (value, next) =
            scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
        value.is_finite().then_some((value, next))
    };
    let (length, mut cursor) = decode(3)?;
    let (first_axial, next) = decode(cursor)?;
    cursor = next;
    let origin_at_first = body.get(cursor) == Some(&0x10);
    if origin_at_first {
        cursor += 1;
    }
    let (radial_sample, next) = decode(cursor)?;
    cursor = next;
    if !origin_at_first {
        (body.get(cursor) == Some(&0x10)).then_some(())?;
        cursor += 1;
    }
    let (second_axial, next) = decode(cursor)?;
    cursor = next;
    (body.get(cursor) == Some(&0x19)).then_some(())?;
    let (_, next) = scalar::decode_model_reference_coordinate(body, cursor, cache)?;
    cursor = next;
    let (radial_center, next) = decode(cursor)?;
    (body.get(next..) == Some(&[0xf7, 0x17])).then_some(())?;

    (length > 0.0).then_some(())?;
    let scale = [
        length,
        first_axial,
        radial_sample,
        second_axial,
        radial_center,
    ]
    .into_iter()
    .map(f64::abs)
    .fold(1.0, f64::max);
    (((second_axial - first_axial).abs() - length).abs() <= 1e-9 * scale).then_some(())?;
    let radius = (radial_sample - radial_center).abs();
    (radius > 0.0).then_some(())?;
    let (origin_x, axis_x) = if origin_at_first {
        (first_axial, (second_axial - first_axial).signum())
    } else {
        (second_axial, (first_axial - second_axial).signum())
    };
    Some(PositionalCylinderFrame {
        origin: [origin_x, 0.0, radial_center],
        axis: [axis_x, 0.0, 0.0],
        ref_direction: [0.0, 0.0, -(radial_sample - radial_center).signum()],
        radius,
        length: Some(length),
    })
}

fn decode_local_system_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    body.starts_with(&[0x11, 0x18, 0x13]).then_some(())?;
    let mut cursor = 3;
    let mut envelope = [0.0; 6];
    for value in &mut envelope {
        let (decoded, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    let radius_start = (cursor..body.len()).find(|start| {
        scalar::decode(body, *start)
            .is_some_and(|(value, end)| end == body.len() && value.is_finite() && value > 0.0)
    })?;
    let (radius, _) = scalar::decode(body, radius_start)?;
    let frames = (cursor..radius_start)
        .filter_map(|start| {
            scalar::decode_positional_plane_local_system_slots(
                body.get(start..radius_start)?,
                cache,
            )
        })
        .collect::<Vec<_>>();
    let [slots] = frames.as_slice() else {
        return None;
    };
    let length = envelope[0];
    (length.is_finite() && length > 0.0).then_some(())?;
    let scale = envelope
        .iter()
        .chain(slots.iter())
        .chain([radius, length].iter())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let close = |first: f64, second: f64| (first - second).abs() <= 1e-9 * scale;
    let axis_indices = (0..2)
        .filter(|index| close((envelope[1 + index] - envelope[4 + index]).abs(), length))
        .collect::<Vec<_>>();
    let [axis_index] = axis_indices.as_slice() else {
        return None;
    };
    let radial_index = 1 - axis_index;
    close(
        (envelope[1 + radial_index] - envelope[4 + radial_index]).abs(),
        2.0 * radius,
    )
    .then_some(())?;
    let origin: [f64; 3] = slots[9..12].try_into().ok()?;
    let first_axial = envelope[1 + axis_index];
    let second_axial = envelope[4 + axis_index];
    let origin_at_first = close(origin[*axis_index], first_axial);
    let origin_at_second = close(origin[*axis_index], second_axial);
    (origin_at_first ^ origin_at_second).then_some(())?;
    let sign = if origin_at_first {
        (second_axial - first_axial).signum()
    } else {
        (first_axial - second_axial).signum()
    };
    let mut axis = [0.0; 3];
    axis[*axis_index] = sign;
    let support: [f64; 3] = slots[0..3].try_into().ok()?;
    let magnitude = support
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    (magnitude.is_finite() && magnitude > 0.0).then_some(())?;
    (support[*axis_index].abs() <= 1e-9 * magnitude).then_some(())?;
    let ref_direction = support.map(|value| sign * value / magnitude);
    Some(PositionalCylinderFrame {
        origin,
        axis,
        ref_direction,
        radius,
        length: Some(length),
    })
}

fn decode_zero_support_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    const ZERO_SUPPORT: &[u8] = &[0x0f, 0x18, 0xe6, 0x10, 0x18, 0x0f, 0x18];
    body.starts_with(&[0x11, 0x18, 0x13]).then_some(())?;
    let mut cursor = 3;
    let mut envelope = [0.0; 6];
    for value in &mut envelope {
        let (decoded, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    let radius_start = (cursor..body.len()).find(|start| {
        scalar::decode(body, *start)
            .is_some_and(|(value, end)| end == body.len() && value.is_finite() && value > 0.0)
    })?;
    let (radius, _) = scalar::decode(body, radius_start)?;
    let origins = (cursor + ZERO_SUPPORT.len()..radius_start)
        .filter_map(|start| {
            (body.get(start - ZERO_SUPPORT.len()..start) == Some(ZERO_SUPPORT))
                .then(|| decode_positional_cylinder_origin(body, start, radius_start, cache))?
        })
        .collect::<Vec<_>>();
    let [origin] = origins.as_slice() else {
        return None;
    };
    let length = envelope[0];
    (length.is_finite() && length > 0.0).then_some(())?;
    let scale = envelope
        .iter()
        .chain(origin.iter())
        .chain([radius, length].iter())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let close = |first: f64, second: f64| (first - second).abs() <= 1e-9 * scale;
    let axes = (0..2)
        .filter_map(|axis_index| {
            let radial_index = 1 - axis_index;
            (close(
                (envelope[1 + axis_index] - envelope[4 + axis_index]).abs(),
                length,
            ) && close(
                (envelope[1 + radial_index] - envelope[4 + radial_index]).abs(),
                2.0 * radius,
            ) && close(
                origin[radial_index],
                f64::midpoint(envelope[1 + radial_index], envelope[4 + radial_index]),
            ))
            .then_some((axis_index, radial_index))
        })
        .collect::<Vec<_>>();
    let [(axis_index, radial_index)] = axes.as_slice() else {
        return None;
    };
    let first_axial = envelope[1 + axis_index];
    let second_axial = envelope[4 + axis_index];
    let origin_at_first = close(origin[*axis_index], first_axial);
    let origin_at_second = close(origin[*axis_index], second_axial);
    (origin_at_first ^ origin_at_second).then_some(())?;
    let other_axial = if origin_at_first {
        second_axial
    } else {
        first_axial
    };
    let mut axis = [0.0; 3];
    axis[*axis_index] = (other_axial - origin[*axis_index]).signum();
    let mut ref_direction = [0.0; 3];
    ref_direction[*radial_index] = (envelope[4 + radial_index] - origin[*radial_index]).signum();
    Some(PositionalCylinderFrame {
        origin: *origin,
        axis,
        ref_direction,
        radius,
        length: Some(length),
    })
}

fn decode_positional_cylinder_origin(
    body: &[u8],
    start: usize,
    end: usize,
    cache: &scalar::ScalarCache,
) -> Option<[f64; 3]> {
    let mut cursor = start;
    let mut origin = [0.0; 3];
    for (index, value) in origin.iter_mut().enumerate() {
        if body.get(cursor) == Some(&0x18) && cursor + 1 == end {
            *value = 0.0;
            cursor += 1;
            continue;
        }
        let row = scalar::decode_in_row_lane(body, cursor, cache);
        let (decoded, next) = match index {
            0 => row.or_else(|| {
                scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)
            })?,
            _ => row.or_else(|| {
                scalar::decode_tabulated_cylinder_second_coordinate(body, cursor, cache)
            })?,
        };
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    (cursor == end).then_some(origin)
}

fn decode_compact_axis_aligned_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    body.starts_with(&[0x11, 0x18, 0x13]).then_some(())?;
    let mut cursor = 3;
    let mut values = [0.0; 7];
    for value in &mut values {
        let (decoded, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    (cursor == body.len() && values[0] > 0.0).then_some(())?;
    axis_aligned_cylinder_from_corners(
        values[1..4].try_into().ok()?,
        values[4..7].try_into().ok()?,
        Some(values[0]),
        AxisAlignedCornerOrientation::SecondToFirst,
    )
}

fn decode_directrix_lane_axis_aligned_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    body.starts_with(&[0x11, 0x18, 0x13]).then_some(())?;
    let mut cursor = 3;
    let mut values = [0.0; 7];
    for value in &mut values {
        let (decoded, next) =
            scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    let orientation = if cursor == body.len() {
        AxisAlignedCornerOrientation::SecondToFirst
    } else if matches!(body.get(cursor..), Some([0xf7, 0x17 | 0x19])) {
        AxisAlignedCornerOrientation::FirstToSecond
    } else {
        return None;
    };
    (values[0] > 0.0).then_some(())?;
    axis_aligned_cylinder_from_corners(
        values[1..4].try_into().ok()?,
        values[4..7].try_into().ok()?,
        None,
        orientation,
    )
}

#[derive(Clone, Copy)]
enum AxisAlignedCornerOrientation {
    FirstToSecond,
    SecondToFirst,
}

fn axis_aligned_cylinder_from_corners(
    first: [f64; 3],
    second: [f64; 3],
    stored_length: Option<f64>,
    orientation: AxisAlignedCornerOrientation,
) -> Option<PositionalCylinderFrame> {
    let spans = std::array::from_fn::<_, 3, _>(|index| (second[index] - first[index]).abs());
    let scale = first
        .iter()
        .chain(second.iter())
        .chain(stored_length.iter())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    let radial_pairs = [(0, 1, 2), (0, 2, 1), (1, 2, 0)]
        .into_iter()
        .filter_map(|(first_radial, second_radial, axis_index)| {
            let (diameter_index, radius_index) = match (
                close(spans[first_radial], 2.0 * spans[second_radial]),
                close(spans[second_radial], 2.0 * spans[first_radial]),
            ) {
                (true, false) => (first_radial, second_radial),
                (false, true) => (second_radial, first_radial),
                _ => return None,
            };
            stored_length
                .is_none_or(|length| close(spans[axis_index], length))
                .then_some((diameter_index, radius_index, axis_index))
        })
        .collect::<Vec<_>>();
    let [(diameter_index, radius_index, axis_index)] = radial_pairs.as_slice() else {
        return None;
    };
    let radius = spans[*radius_index];
    let length = spans[*axis_index];
    (radius > 0.0 && length > 0.0).then_some(())?;
    let mut origin = second;
    origin[*diameter_index] = f64::midpoint(first[*diameter_index], second[*diameter_index]);
    if matches!(orientation, AxisAlignedCornerOrientation::FirstToSecond) {
        origin[*axis_index] = first[*axis_index];
    }
    let (from, to) = match orientation {
        AxisAlignedCornerOrientation::FirstToSecond => (first, second),
        AxisAlignedCornerOrientation::SecondToFirst => (second, first),
    };
    let mut axis = [0.0; 3];
    axis[*axis_index] = (to[*axis_index] - from[*axis_index]).signum();
    let mut ref_direction = [0.0; 3];
    ref_direction[*diameter_index] = (to[*diameter_index] - from[*diameter_index]).signum();
    Some(PositionalCylinderFrame {
        origin,
        axis,
        ref_direction,
        radius,
        length: Some(length),
    })
}

fn decode_tabulated_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<(TabulatedCylinderFrame, usize)> {
    const FRAME_MARKER: &[u8] = &[0x00, 0x0c, 0x9a];
    let marker = find(body, FRAME_MARKER, 0)?;
    let mut cursor = marker + FRAME_MARKER.len();
    let mut values = Vec::with_capacity(6);
    let mut prefixes = Vec::with_capacity(6);
    for slot in 0..6 {
        prefixes.push(*body.get(cursor)?);
        let (value, next) = if matches!(slot, 0 | 3)
            || (matches!(slot, 1 | 4) && body.get(cursor) == Some(&0x2d))
        {
            scalar::decode_tabulated_cylinder_first_frame_coordinate(body, cursor, cache)?
        } else {
            scalar::decode_tabulated_cylinder_frame_coordinate(body, cursor, cache)?
        };
        value.is_finite().then_some(())?;
        values.push(value);
        cursor = next;
    }
    Some((
        TabulatedCylinderFrame {
            values: values.try_into().ok()?,
            prefixes: prefixes.try_into().ok()?,
        },
        cursor,
    ))
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
        let Some(owner) = surface_rows
            .iter()
            .rev()
            .find(|row| row.offset > owner_lower_bound && row.offset < replay_offset)
        else {
            continue;
        };
        if owner.type_byte != 0x2c {
            continue;
        }
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
    if kind == SurfaceKind::Cone {
        if let Some(layout) = cone_half_angle_before_close(body) {
            return Some(layout.end);
        }
    }
    if kind == SurfaceKind::Extrusion {
        if let Some((_, mut cursor)) = decode_tabulated_cylinder_frame(body, cache) {
            if body.get(cursor) == Some(&psb::token::ENTITY_REF) {
                if let Ok((_, next)) = psb::reference_id(body, cursor + 1) {
                    cursor = next;
                }
            }
            if body.get(cursor) == Some(&psb::token::COMPOUND_CLOSE) {
                return Some(cursor);
            }
        }
    }
    let mut cursor = 0;
    while cursor < body.len() {
        if body[cursor] == psb::token::COMPOUND_CLOSE {
            return Some(cursor);
        }
        if let Some((_, next)) = decode_row_scalar(kind, body, cursor, cache) {
            cursor = next;
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

fn named_spline_scalar_slots(
    name: &str,
    body: &[u8],
    count: usize,
    cache: &scalar::ScalarCache,
) -> Vec<ScalarTokenSlot> {
    let mut slots = Vec::with_capacity(count);
    let mut cursor = 0;
    let mut continued_tuple = false;
    while slots.len() < count {
        if matches!(name, "i_pnts" | "i_points")
            && body.get(cursor..cursor + 2) == Some(&[psb::token::SCALAR_BODY, 0x00])
        {
            cursor += 2;
            continued_tuple = true;
            continue;
        }
        let Some((value, next)) = named_spline_scalar_slot(name, body, cursor, cache) else {
            break;
        };
        slots.push((value, body[cursor..next].to_vec()));
        cursor = next;
    }
    if matches!(name, "i_pnts" | "i_points")
        && continued_tuple
        && cursor == body.len()
        && slots.len() + 1 == count
    {
        slots.push((Some(0.0), Vec::new()));
    }
    slots.resize_with(count, || (None, Vec::new()));
    slots
}

fn named_spline_scalar_slot(
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
        return named_ieee8(body, offset, 0x3f).map(|(value, next)| (Some(value), next));
    }
    if name == "params" && head == 0x2d {
        return named_ieee8(body, offset, 0x40).map(|(value, next)| (Some(value), next));
    }
    if matches!(head, 0x2d | 0x46 | 0x71) {
        return scalar::decode_in_lane(body, offset, cache)
            .map(|(value, next)| (Some(value), next));
    }
    if matches!(name, "end_v_tangts" | "end_tangts") {
        return scalar::decode_tabulated_cylinder_second_coordinate(body, offset, cache)
            .map(|(value, next)| (Some(value), next));
    }
    if matches!(name, "i_pnts" | "i_points") && matches!(head, 0x63 | 0x68 | 0x6e | 0x70) {
        return named_positive_dict(body, offset).map(|(value, next)| (Some(value), next));
    }
    if matches!(name, "i_pnts" | "i_points") && matches!(head, 0xb3 | 0xb9) {
        return scalar::decode_in_lane(body, offset, cache)
            .map(|(value, next)| (Some(value), next));
    }
    if matches!(name, "u_params" | "v_params" | "params") {
        return named_positive_dict(body, offset).map(|(value, next)| (Some(value), next));
    }
    if name == "end_u_tangts" && head == 0x31 {
        return named_ieee7(body, offset, 0x40).map(|(value, next)| (Some(value), next));
    }
    if name == "end_uv_deriv" && matches!(head, 0x7d | 0x8f) {
        return named_positive_dict(body, offset).map(|(value, next)| (Some(value), next));
    }
    let next = offset.checked_add(7)?;
    (next <= body.len()).then_some((None, next))
}

fn named_positive_dict(body: &[u8], offset: usize) -> Option<(f64, usize)> {
    let second = body.get(offset)?.wrapping_sub(0x8b);
    let first = if second >= 0x80 { 0x3f } else { 0x40 };
    let tail = body.get(offset + 1..offset + 7)?;
    let mut raw = [0; 8];
    raw[0] = first;
    raw[1] = second;
    raw[2..].copy_from_slice(tail);
    Some((f64::from_be_bytes(raw), offset + 7))
}

fn named_ieee8(body: &[u8], offset: usize, first: u8) -> Option<(f64, usize)> {
    let tail = body.get(offset + 1..offset + 8)?;
    let mut raw = [0; 8];
    raw[0] = first;
    raw[1..].copy_from_slice(tail);
    Some((f64::from_be_bytes(raw), offset + 8))
}

fn named_ieee7(body: &[u8], offset: usize, first: u8) -> Option<(f64, usize)> {
    let tail = body.get(offset + 1..offset + 7)?;
    let mut raw = [0; 8];
    raw[0] = first;
    raw[1..7].copy_from_slice(tail);
    Some((f64::from_be_bytes(raw), offset + 7))
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
        if let Some((value, next)) =
            scalar::decode_named_local_system_coordinate(body, cursor, cache)
        {
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
    let (Some(first), Some(middle), Some(third)) =
        (triple([0, 1, 2]), triple([3, 4, 5]), triple([6, 7, 8]))
    else {
        return PlaneFrame {
            origin,
            u_axis: None,
            normal: None,
        };
    };
    let first_magnitude = first.iter().map(|value| value * value).sum::<f64>().sqrt();
    let middle_magnitude = middle.iter().map(|value| value * value).sum::<f64>().sqrt();
    let third_magnitude = third.iter().map(|value| value * value).sum::<f64>().sqrt();
    let second = match (middle_magnitude > 1e-6, third_magnitude > 1e-6) {
        (true, false) if third_magnitude <= 1e-9 => middle,
        (false, true) if middle_magnitude <= 1e-9 => third,
        _ => {
            return PlaneFrame {
                origin,
                u_axis: None,
                normal: None,
            };
        }
    };
    let second_magnitude = second.iter().map(|value| value * value).sum::<f64>().sqrt();
    let scale = first_magnitude.max(second_magnitude);
    let support_dot = first
        .iter()
        .zip(second)
        .map(|(first, second)| first * second)
        .sum::<f64>();
    if (first_magnitude - second_magnitude).abs() > 1e-9 * scale.max(1.0)
        || support_dot.abs() > 1e-9 * first_magnitude * second_magnitude
    {
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

fn complete_plane_local_system_slots(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<[f64; 12]> {
    let frame_body = body.strip_suffix(&[0xe1]).unwrap_or(body);
    scalar::decode_positional_plane_local_system_slots(frame_body, cache)
}

/// Decode the e3-bounded local-system chunk following each plane envelope.
pub fn plane_local_systems(payload: &[u8]) -> Vec<PlaneLocalSystem> {
    plane_local_systems_for_rows(payload, rows(payload))
}

/// Decode plane local-system chunks from a DEPDB cross-section namespace.
#[must_use]
pub fn cross_section_plane_local_systems(payload: &[u8]) -> Vec<PlaneLocalSystem> {
    plane_local_systems_for_rows(payload, cross_section_rows(payload))
}

fn plane_local_systems_for_rows(payload: &[u8], rows: Vec<SurfaceRow>) -> Vec<PlaneLocalSystem> {
    let cache = scalar::ScalarCache::from_section(payload);
    let headers = rows
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
        let slots = complete_plane_local_system_slots(&body, &cache)
            .map_or([None; 12], |slots| slots.map(Some));
        let frame = plane_frame(&slots);
        let simple = matches!(body.first(), Some(0x0f | 0x10 | 0x18))
            && body.len() <= 24
            && !body
                .iter()
                .any(|byte| matches!(byte, 0xe0..=0xe2 | 0xf1 | 0xf2 | 0xf7 | 0xf8));
        systems.push(PlaneLocalSystem {
            surface_id: row.id,
            body,
            slots: slots.to_vec(),
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
    plane_envelopes_for_rows(payload, &rows(payload))
}

/// Decode plane envelopes from a DEPDB cross-section namespace.
#[must_use]
pub fn cross_section_plane_envelopes(payload: &[u8]) -> Vec<PlaneEnvelopeRecord> {
    plane_envelopes_for_rows(payload, &cross_section_rows(payload))
}

fn plane_envelopes_for_rows(payload: &[u8], all_rows: &[SurfaceRow]) -> Vec<PlaneEnvelopeRecord> {
    const NAMED_OUTLINE: &[u8] = b"outline\0\xf9\x02\x03";
    let cache = scalar::ScalarCache::from_section(payload);
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
        let scalar_tokens;
        let (envelope, corner_coordinate_equal) = if body.first() == Some(&0x0e) {
            let slots = scalar_slots_with_tokens(&body[1..], 9, &cache);
            let values = slots.iter().map(|slot| slot.0).collect::<Vec<_>>();
            scalar_tokens = slots.iter().map(|slot| slot.1.clone()).collect();
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
        } else if let Some(slots) = complete_plane_compact_scalar_suffix(&body, &cache) {
            let values = slots.iter().map(|slot| slot.0).collect::<Vec<_>>();
            scalar_tokens = slots.iter().map(|slot| slot.1.clone()).collect();
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
            scalar_tokens = slots.iter().map(|slot| slot.1.clone()).collect();
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
            scalar_tokens,
            row_offset: row.offset,
            offset: *body_start,
        });
    }
    for (index, row) in all_rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.kind == SurfaceKind::Plane)
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
            scalar_tokens: slots.iter().map(|slot| slot.1.clone()).collect(),
            row_offset: row.offset,
            offset: scalar_start,
        });
    }
    envelopes.sort_by_key(|envelope| envelope.offset);
    envelopes
}

fn complete_plane_compact_scalar_suffix(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<Vec<(Option<f64>, Vec<u8>)>> {
    let standard = scalar_slots_with_tokens(body, 10, cache);
    if standard
        .iter()
        .all(|(value, token)| value.is_some() && !token.is_empty())
    {
        return None;
    }
    let tokens = scalar_tokens(SurfaceKind::Plane, body, cache);
    let frames = scalar_frames(&tokens);
    let frame = terminal_scalar_frame(body, &frames)?;
    (frame.offset > 0 && frame.slots.len() == 9).then_some(())?;
    frame
        .slots
        .into_iter()
        .map(|slot| Some((Some(slot.value?), slot.raw)))
        .collect()
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
        let decoded = rows(&payload);
        assert_eq!(
            decoded,
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
        assert_eq!(
            unique_surface_row(&decoded, 7).map(|row| row.offset),
            Some(0)
        );
        let mut duplicate = decoded.clone();
        duplicate.push(decoded[0].clone());
        assert!(unique_surface_row(&duplicate, 7).is_none());
    }

    #[test]
    fn cross_section_count_rejects_boundary_one_body_candidate() {
        let payload =
            b"Sld_Xsections\0srf_array\0\xf8\x01\x07\x24\x04\x01\x06\0\x2d\x25\x32\xf6\x01\x01\xe2";

        assert!(rows(payload).is_empty());
        let rows = cross_section_rows(payload);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, 7);
        assert_eq!(rows[0].boundary_type, 0x06);
        let parameters = cross_section_parameter_records(payload);
        assert_eq!(parameters.len(), 1);
        assert_eq!(parameters[0].surface_id, 7);
        assert!(parameters[0]
            .body
            .windows(6)
            .any(|bytes| bytes == b"\x2d\x25\x32\xf6\x01\x01"));
    }

    #[test]
    fn cross_section_plane_envelope_retains_its_namespace_geometry() {
        let payload = b"Sld_Xsections\0srf_array\0\xf8\x01\x07\x22\x04\x01\x06\0\xe4\xe4\xe4\xe4\x0f\x0f\x0f\xe4\x0f\xe4\xe3";

        let envelopes = cross_section_plane_envelopes(payload);
        assert_eq!(envelopes.len(), 1);
        let planes = outline_planes(&envelopes);
        assert_eq!(planes.len(), 1);
        assert_eq!(planes[0].surface_id, 7);
        assert_eq!(planes[0].origin, [0.0, 0.0, 0.0]);
        assert_eq!(planes[0].normal, [0.0, 1.0, 0.0]);
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
    fn unique_surface_projection_excludes_every_collided_identity() {
        let row = |id, offset| SurfaceRow {
            id,
            type_byte: 0x22,
            kind: SurfaceKind::Plane,
            feature_id: 4,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset,
        };
        let rows = [row(7, 10), row(8, 20), row(7, 30)];

        assert_eq!(
            uniquely_identified_rows(&rows)
                .iter()
                .map(|row| row.id)
                .collect::<Vec<_>>(),
            [8]
        );
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
    fn signed_surface_dict_scalar_owns_its_tail() {
        let body = [0x73, 0xe4, 0x2f, 0x43, 0, 0xe3, 0xe0];
        let tokens = scalar_tokens(
            SurfaceKind::TorusOrSphere,
            &body,
            &scalar::ScalarCache::default(),
        );

        assert_eq!(tokens.len(), 1);
        assert_eq!(
            tokens[0].value,
            Some(f64::from_be_bytes([
                0x3f, 0xe8, 0xe4, 0x2f, 0x43, 0, 0xe3, 0xe0
            ]))
        );
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
    fn retains_named_spline_point_and_tangent_arrays() {
        let payload = b"srf_prim_ptr(splsrf)\0\
            \xe0\x02i_points\0\xf9\x02\x02\xe4\x0f\xe4\x0f\
            \xe0\x02end_u_tangts\0\xf9\x01\x02\x0f\xe4\
            \xe0\x02u_params\0\xf8\x02\x0f\xe4\xe3";
        let records = named_prototype_records(payload);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].family, SurfacePrototypeFamily::Spline);
        assert_eq!(
            records[0].field("i_points").map(|field| &field.value),
            Some(&SurfaceNamedValue::ScalarArray {
                dimensions: 2,
                count: 2,
                values: vec![Some(1.0), Some(0.0), Some(1.0), Some(0.0)],
                tokens: vec![vec![0xe4], vec![0x0f], vec![0xe4], vec![0x0f]],
            })
        );
        assert_eq!(
            records[0].field("end_u_tangts").map(|field| &field.value),
            Some(&SurfaceNamedValue::ScalarArray {
                dimensions: 1,
                count: 2,
                values: vec![Some(0.0), Some(1.0)],
                tokens: vec![vec![0x0f], vec![0xe4]],
            })
        );
        assert_eq!(
            records[0].field("u_params").map(|field| &field.value),
            Some(&SurfaceNamedValue::CountedScalarArray {
                count: 2,
                values: vec![Some(0.0), Some(1.0)],
                tokens: vec![vec![0x0f], vec![0xe4]],
            })
        );
    }

    #[test]
    fn spline_slots_consume_unresolved_tokens_without_scanning_their_payloads() {
        let body = [0xaa, 0xe4, 1, 2, 3, 4, 5, 0xe4];
        let slots = named_spline_scalar_slots("tangts", &body, 2, &scalar::ScalarCache::default());

        assert_eq!(
            slots,
            [
                (None, vec![0xaa, 0xe4, 1, 2, 3, 4, 5]),
                (Some(1.0), vec![0xe4]),
            ]
        );
    }

    #[test]
    fn interpolation_point_aliases_expand_continuation_and_terminal_zero() {
        let body = [0xe4, 0x0f, 0xe4, 0xf9, 0x00, 0x2f, 0x14, 0x00, 0x18];
        for name in ["i_pnts", "i_points"] {
            let slots = named_spline_scalar_slots(name, &body, 6, &scalar::ScalarCache::default());
            assert_eq!(
                slots.iter().map(|slot| slot.0).collect::<Vec<_>>(),
                [
                    Some(1.0),
                    Some(0.0),
                    Some(1.0),
                    Some(5.0),
                    Some(0.0),
                    Some(0.0)
                ]
            );
            assert_eq!(slots[3].1, [0x2f, 0x14, 0x00]);
            assert!(slots[5].1.is_empty());
        }
    }

    #[test]
    fn spline_tangents_use_the_signed_coordinate_dict_lattice() {
        let body = [
            0xce, 1, 2, 3, 4, 5, 6, 0x2d, 1, 2, 3, 4, 5, 6, 7, 0x46, 1, 2, 3, 4, 5, 6, 7,
        ];
        for name in ["end_v_tangts", "end_tangts"] {
            let slots = named_spline_scalar_slots(name, &body, 3, &scalar::ScalarCache::default());

            assert_eq!(
                slots[0].0,
                Some(f64::from_be_bytes([0xbf, 0xfb, 1, 2, 3, 4, 5, 6]))
            );
            assert_eq!(slots[0].1, body[..7]);
            assert_eq!(
                slots[1].0,
                Some(f64::from_be_bytes([0xc0, 1, 2, 3, 4, 5, 6, 7]))
            );
            assert_eq!(slots[1].1, body[7..15]);
            assert_eq!(
                slots[2].0,
                Some(f64::from_be_bytes([0x40, 1, 2, 3, 4, 5, 6, 7]))
            );
            assert_eq!(slots[2].1, body[15..]);
        }
    }

    #[test]
    fn tabulated_cylinder_parameters_end_the_tangent_field() {
        let payload = b"srf_prim_ptr(tab_cyl)\0\
            \xe0\x02end_tangts\0\xf9\x02\x03\x0f\xe4\x0f\xe4\x0f\x18\
            \xe0\x02params\0\xf8\x03\x0f\
            \x2d\x00\x00\x00\x00\x00\x00\x00\
            \x2d\x08\x00\x00\x00\x00\x00\x00\xe3";
        let records = named_prototype_records(payload);

        assert_eq!(records.len(), 1);
        assert!(matches!(
            records[0].field("end_tangts").map(|field| &field.value),
            Some(SurfaceNamedValue::ScalarArray { values, .. }) if values.len() == 6
        ));
        assert_eq!(
            records[0].field("params").map(|field| &field.value),
            Some(&SurfaceNamedValue::CountedScalarArray {
                count: 3,
                values: vec![Some(0.0), Some(2.0), Some(3.0)],
                tokens: vec![
                    vec![0x0f],
                    vec![0x2d, 0, 0, 0, 0, 0, 0, 0],
                    vec![0x2d, 8, 0, 0, 0, 0, 0, 0],
                ],
            })
        );
    }

    #[test]
    fn tabulated_cylinder_frame_owns_compound_close_bytes_inside_scalars() {
        let mut body = vec![0x00, 0x0c, 0x9a];
        body.extend_from_slice(&[0x4a, 0x13, 0x21, 0xe3, 0xe3, 0x00, 0x00]);
        body.extend_from_slice(&[0xe4, 0x0f]);
        body.extend_from_slice(&[0x4a, 0x13, 0x1f, 0x1c, 0x0b, 0x00, 0x00]);
        body.extend_from_slice(&[0xe4, 0x0f, 0xf7, 0x23, 0xe3]);

        let (frame, frame_end) =
            decode_tabulated_cylinder_frame(&body, &scalar::ScalarCache::default())
                .expect("complete tabulated-cylinder frame");
        assert_eq!(frame.prefixes, [0x4a, 0xe4, 0x0f, 0x4a, 0xe4, 0x0f]);
        assert_eq!(frame_end, body.len() - 3);

        assert_eq!(
            surface_body_compound_close(
                SurfaceKind::Extrusion,
                &body,
                &scalar::ScalarCache::default(),
            ),
            Some(body.len() - 1)
        );
    }

    #[test]
    fn positional_cylinder_frame_requires_a_complete_consistent_carrier() {
        let negative_x = [
            0x11, 0x18, 0x13, 0x29, 0xd9, 0x99, 0x47, 0x03, 0x33, 0x2d, 0x35, 0x0c, 0xcc, 0xcc,
            0xcc, 0xcc, 0xcd, 0x43, 0xe8, 0x00, 0x48, 0x00, 0x00, 0x2d, 0x36, 0x8c, 0xcc, 0xcc,
            0xcc, 0xcc, 0xcd, 0x19, 0x9a, 0x79, 0x39, 0x4c, 0x9e, 0x8a, 0x0a, 0xf7, 0x19, 0xe3,
            0x18, 0xe4, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x18, 0x47, 0x03, 0x33, 0x2e, 0x35, 0xcc,
            0x18, 0x2a, 0xe8, 0x00,
        ];
        let frame = decode_positional_cylinder_frame(&negative_x, &scalar::ScalarCache::default())
            .expect("complete positional cylinder");
        assert!((frame.origin[0] + 2.4).abs() < 1e-12);
        assert!((frame.origin[1] - 21.8).abs() < 1e-12);
        assert_eq!(frame.origin[2], 0.0);
        assert_eq!(frame.axis, [1.0, 0.0, 0.0]);
        assert_eq!(frame.ref_direction, [0.0, 1.0, 0.0]);
        assert!((frame.radius - 0.75).abs() < 1e-12);
        assert!((frame.length.expect("axial extent") - 0.4).abs() < 1e-12);

        let positive_x = [
            17, 24, 19, 41, 217, 153, 41, 255, 255, 45, 53, 12, 204, 204, 204, 204, 205, 67, 232,
            0, 46, 3, 51, 45, 54, 140, 204, 204, 204, 204, 205, 25, 154, 121, 57, 76, 158, 138, 10,
            227, 24, 228, 16, 228, 24, 229, 15, 24, 46, 3, 51, 46, 53, 204, 24, 42, 232, 0,
        ];
        let frame = decode_positional_cylinder_frame(&positive_x, &scalar::ScalarCache::default())
            .expect("oppositely oriented positional cylinder");
        assert_eq!(frame.axis, [-1.0, 0.0, 0.0]);
        assert_eq!(frame.ref_direction, [0.0, -1.0, 0.0]);

        let compact = [
            17, 24, 19, 41, 251, 51, 67, 248, 0, 47, 49, 128, 66, 235, 51, 42, 248, 0, 47, 51, 0,
            41, 235, 51,
        ];
        let frame = decode_positional_cylinder_frame(&compact, &scalar::ScalarCache::default())
            .expect("complete compact axis-aligned cylinder");
        assert_eq!(frame.origin, [0.0, 19.0, 0.85]);
        assert_eq!(frame.axis, [0.0, 0.0, -1.0]);
        assert_eq!(frame.ref_direction, [-1.0, 0.0, 0.0]);
        assert!((frame.radius - 1.5).abs() < 1e-12);
        assert!((frame.length.expect("axial extent") - 1.7).abs() < 1e-12);

        let directrix_lane = [
            17, 24, 19, 135, 122, 225, 71, 174, 20, 123, 71, 0, 204, 45, 45, 20, 122, 225, 71, 174,
            21, 65, 169, 153, 153, 153, 153, 153, 160, 46, 0, 204, 45, 48, 163, 215, 10, 61, 112,
            164, 134, 174, 20, 122, 225, 71, 174,
        ];
        let frame =
            decode_positional_cylinder_frame(&directrix_lane, &scalar::ScalarCache::default())
                .expect("complete directrix-lane axis-aligned cylinder");
        assert_eq!(frame.origin, [0.0, 16.64, 1.73]);
        assert_eq!(frame.axis, [0.0, 0.0, -1.0]);
        assert_eq!(frame.ref_direction, [-1.0, 0.0, 0.0]);
        assert!((frame.radius - 2.1).abs() < 1e-12);
        assert!((frame.length.expect("axial extent") - 1.68).abs() < 1e-12);

        let forward_trailer = [
            17, 24, 19, 114, 174, 20, 122, 225, 71, 174, 199, 163, 215, 10, 61, 112, 164, 70, 47,
            194, 86, 31, 194, 58, 188, 142, 71, 174, 20, 122, 225, 72, 146, 112, 163, 215, 10, 61,
            112, 70, 43, 138, 4, 52, 61, 28, 4, 46, 9, 51, 247, 23,
        ];
        let frame =
            decode_positional_cylinder_frame(&forward_trailer, &scalar::ScalarCache::default())
                .expect("complete forward-oriented directrix-lane cylinder");
        assert!((frame.origin[0] - 0.82).abs() < 1e-12);
        assert!((frame.origin[1] + 13.769563324412964).abs() < 1e-12);
        assert!((frame.origin[2] - 2.41).abs() < 1e-12);
        assert_eq!(frame.axis, [0.0, 0.0, 1.0]);
        assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
        assert!((frame.radius - 2.11).abs() < 1e-12);

        let compound_close_trailer = [
            17, 24, 19, 47, 33, 0, 47, 39, 0, 47, 52, 128, 71, 23, 255, 47, 50, 128, 47, 56, 0, 47,
            4, 0, 247, 25,
        ];
        let frame = decode_positional_cylinder_frame(
            &compound_close_trailer,
            &scalar::ScalarCache::default(),
        )
        .expect("complete compound-close directrix-lane cylinder");
        assert_eq!(frame.origin[0], 15.0);
        assert_eq!(frame.origin[1], 24.0);
        assert!((frame.origin[2] + 6.0).abs() < 1e-12);
        assert_eq!(frame.axis, [0.0, 0.0, 1.0]);
        assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
        assert_eq!(frame.radius, 3.5);
        assert_eq!(frame.length, Some(8.5));

        let zero_support = [
            17, 24, 19, 47, 32, 0, 72, 42, 128, 72, 16, 0, 67, 232, 0, 72, 39, 128, 47, 16, 0, 25,
            154, 121, 57, 76, 158, 138, 10, 247, 25, 227, 15, 24, 230, 16, 24, 15, 24, 72, 41, 0,
            47, 16, 0, 24, 42, 232, 0,
        ];
        let frame =
            decode_positional_cylinder_frame(&zero_support, &scalar::ScalarCache::default())
                .expect("complete zero-support positional cylinder");
        assert_eq!(frame.origin, [-12.5, 4.0, 0.0]);
        assert_eq!(frame.axis, [0.0, -1.0, 0.0]);
        assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
        assert_eq!(frame.radius, 0.75);
        assert_eq!(frame.length, Some(8.0));

        let referenced_planar_envelope = [
            17, 24, 19, 47, 48, 0, 71, 17, 204, 24, 50, 195, 162, 112, 229, 160, 63, 250, 46, 17,
            204, 47, 48, 0, 46, 17, 204,
        ];
        let frame = decode_positional_cylinder_frame(
            &referenced_planar_envelope,
            &scalar::ScalarCache::default(),
        )
        .expect("complete referenced planar-envelope cylinder");
        assert_eq!(frame.origin, [0.0, 16.0, 0.0]);
        assert_eq!(frame.axis, [0.0, 1.0, 0.0]);
        assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
        assert!((frame.radius - 4.45).abs() < 1e-12);
        assert_eq!(frame.length, Some(16.0));

        let reversed_referenced_planar_envelope = [
            17, 24, 19, 46, 17, 255, 71, 19, 204, 70, 48, 189, 112, 163, 215, 10, 62, 50, 197, 215,
            53, 172, 2, 203, 123, 46, 19, 204, 70, 40, 122, 225, 71, 174, 20, 125, 46, 19, 204,
            247, 25,
        ];
        let frame = decode_positional_cylinder_frame(
            &reversed_referenced_planar_envelope,
            &scalar::ScalarCache::default(),
        )
        .expect("complete reversed referenced planar-envelope cylinder");
        assert!((frame.origin[0]).abs() < 1e-12);
        assert!((frame.origin[1] + 12.24).abs() < 1e-12);
        assert_eq!(frame.origin[2], 0.0);
        assert_eq!(frame.axis, [0.0, -1.0, 0.0]);
        assert_eq!(frame.ref_direction, [-1.0, 0.0, 0.0]);
        assert!((frame.radius - 4.95).abs() < 1e-12);
        assert!((frame.length.expect("axial extent") - 4.5).abs() < 1e-12);

        let held_axis = [
            17, 24, 19, 15, 70, 68, 166, 102, 102, 102, 102, 102, 16, 67, 224, 0, 70, 67, 166, 102,
            102, 102, 102, 102, 25, 161, 166, 38, 51, 20, 92, 7, 14, 247, 23,
        ];
        let frame = decode_positional_cylinder_frame(&held_axis, &scalar::ScalarCache::default())
            .expect("complete held-axis cylinder");
        assert!((frame.origin[0] + 40.3).abs() < 1e-12);
        assert_eq!(frame.origin[1], 0.0);
        assert!((frame.origin[2] + 0.5).abs() < 1e-12);
        assert_eq!(frame.axis, [0.0, 0.0, 1.0]);
        assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
        assert!((frame.radius - 1.0).abs() < 1e-12);
        assert_eq!(frame.length, None);

        let first_endpoint_axial_radial = [
            17, 24, 19, 45, 26, 28, 221, 156, 226, 254, 231, 46, 61, 204, 16, 228, 45, 66, 42, 2,
            26, 2, 198, 67, 25, 161, 166, 38, 51, 20, 92, 7, 15, 247, 23,
        ];
        let frame = decode_positional_cylinder_frame(
            &first_endpoint_axial_radial,
            &scalar::ScalarCache::default(),
        )
        .expect("complete first-endpoint axial/radial cylinder");
        assert!((frame.origin[0] - 29.8).abs() < 1e-12);
        assert_eq!(frame.origin[1..], [0.0, 0.0]);
        assert_eq!(frame.axis, [1.0, 0.0, 0.0]);
        assert_eq!(frame.ref_direction, [0.0, 0.0, -1.0]);
        assert!((frame.radius - 1.0).abs() < 1e-12);
        assert!((frame.length.expect("axial extent") - 6.528189135889739).abs() < 1e-12);

        let second_endpoint_axial_radial = [
            17, 24, 19, 45, 26, 27, 232, 154, 196, 109, 12, 70, 66, 41, 227, 121, 190, 244, 8, 66,
            239, 255, 16, 71, 61, 204, 25, 192, 139, 195, 207, 227, 22, 71, 15, 247, 23,
        ];
        let frame = decode_positional_cylinder_frame(
            &second_endpoint_axial_radial,
            &scalar::ScalarCache::default(),
        )
        .expect("complete second-endpoint axial/radial cylinder");
        assert!((frame.origin[0] + 29.8).abs() < 1e-12);
        assert_eq!(frame.origin[1..], [0.0, 0.0]);
        assert_eq!(frame.axis, [-1.0, 0.0, 0.0]);
        assert_eq!(frame.ref_direction, [0.0, 0.0, 1.0]);
        assert!((frame.radius - 1.0).abs() < 1e-12);
        assert!((frame.length.expect("axial extent") - 6.527254503477945).abs() < 1e-12);

        let mut inconsistent = negative_x.to_vec();
        inconsistent[58] = 0xd0;
        assert!(
            decode_positional_cylinder_frame(&inconsistent, &scalar::ScalarCache::default())
                .is_none()
        );
    }

    #[test]
    fn positional_cylinder_frame_decodes_compact_y_axis_envelopes() {
        let direct = [
            0x14, 0x2f, 0x10, 0x00, 0x2d, 0x1f, 0x6a, 0x7a, 0x29, 0x55, 0x38, 0x5e, 0x2f, 0x43,
            0x00, 0x48, 0x29, 0x00, 0x2f, 0x10, 0x00, 0x43, 0xe8, 0x00, 0x48, 0x27, 0x80, 0x2f,
            0x43, 0x00, 0x2a, 0xe8, 0x00,
        ];
        let split = [
            0x12, 0x2f, 0x10, 0x00, 0x14, 0x2f, 0x43, 0x00, 0x2f, 0x27, 0x80, 0x2f, 0x10, 0x00,
            0x43, 0xe8, 0x00, 0x2f, 0x29, 0x00, 0x2f, 0x43, 0x00, 0x2a, 0xe8, 0x00,
        ];
        let cache = scalar::ScalarCache::default();

        assert_eq!(
            decode_positional_cylinder_frame(&direct, &cache),
            Some(PositionalCylinderFrame {
                origin: [-12.5, 4.0, 0.0],
                axis: [0.0, 1.0, 0.0],
                ref_direction: [1.0, 0.0, 0.0],
                radius: 0.75,
                length: Some(34.0),
            })
        );
        assert_eq!(
            decode_positional_cylinder_frame(&split, &cache),
            Some(PositionalCylinderFrame {
                origin: [12.5, 4.0, 0.0],
                axis: [0.0, 1.0, 0.0],
                ref_direction: [-1.0, 0.0, 0.0],
                radius: 0.75,
                length: Some(34.0),
            })
        );

        let mut inconsistent = split;
        inconsistent[20..23].copy_from_slice(&[0x2f, 0x42, 0x00]);
        assert!(decode_positional_cylinder_frame(&inconsistent, &cache).is_none());
        assert!(decode_positional_cylinder_frame(&direct[..direct.len() - 3], &cache).is_none());
    }

    #[test]
    fn positional_cone_frame_requires_complete_support_apex_and_angle() {
        let body = [
            197, 251, 126, 24, 209, 212, 112, 107, 81, 235, 133, 30, 184, 70, 125, 251, 126, 24,
            209, 212, 112, 123, 0, 68, 204, 99, 17, 228, 72, 66, 64, 192, 170, 175, 125, 232, 45,
            177, 195, 0, 68, 204, 99, 17, 220, 70, 66, 1, 69, 135, 177, 98, 82, 120, 170, 175, 125,
            232, 45, 187, 65, 200, 122, 225, 71, 174, 20, 128, 227, 24, 228, 15, 24, 15, 24, 16,
            24, 228, 70, 66, 129, 71, 174, 20, 122, 225, 25, 194, 145, 29, 33, 143, 32, 210, 52,
            233, 0, 116, 33, 251, 84, 68, 45, 5,
        ];
        let frame = decode_positional_cone_frame(&body, &scalar::ScalarCache::default())
            .expect("complete positional cone");
        assert_eq!(frame.apex, [37.01, 0.0, 0.0]);
        assert_eq!(frame.axis, [-1.0, -0.0, -0.0]);
        assert_eq!(frame.ref_direction, [-0.0, -0.0, -1.0]);
        assert!((frame.half_angle - std::f64::consts::FRAC_PI_4).abs() < 1e-12);

        let angle = terminal_cone_half_angle_layout(&body).expect("terminal half-angle");
        let mut local_system_body = vec![0xf9, 0x04, 0x03];
        local_system_body.extend_from_slice(&body[..angle.start]);
        let prototype = SurfacePrototypeRecord {
            declared_family: "cone".to_string(),
            family: SurfacePrototypeFamily::Cone,
            parameters: vec![
                SurfaceNamedParameter {
                    name: "local_sys".to_string(),
                    value: SurfaceNamedValue::Opaque(local_system_body.clone()),
                    body: local_system_body,
                    offset: 0,
                    value_offset: 0,
                },
                SurfaceNamedParameter {
                    name: "half_angle".to_string(),
                    value: SurfaceNamedValue::ScalarSequence(vec![angle.value]),
                    body: body[angle.start..].to_vec(),
                    offset: 0,
                    value_offset: 0,
                },
            ],
            offset: 0,
        };
        assert_eq!(prototype_cone_frame(&prototype), Some(frame));

        let mut incomplete = body.to_vec();
        incomplete.remove(86);
        assert!(
            decode_positional_cone_frame(&incomplete, &scalar::ScalarCache::default()).is_none()
        );
    }

    #[test]
    fn positional_cone_frame_decodes_complete_planar_envelopes() {
        let unreferenced = [
            21, 70, 34, 171, 89, 29, 204, 62, 140, 24, 70, 28, 153, 105, 188, 41, 208, 189, 71, 27,
            153, 70, 40, 122, 225, 71, 174, 20, 126, 24, 46, 27, 153, 70, 36, 28, 61, 7, 246, 190,
            80, 46, 27, 153,
        ];
        let referenced = [
            23, 70, 34, 171, 89, 29, 204, 62, 140, 21, 70, 28, 153, 105, 188, 41, 208, 189, 71, 27,
            153, 70, 40, 122, 225, 71, 174, 20, 126, 71, 27, 153, 46, 27, 153, 70, 36, 28, 61, 7,
            246, 190, 80, 25, 206, 113, 206, 177, 182, 81, 244, 247, 44,
        ];
        for body in [&unreferenced[..], &referenced[..]] {
            let frame = decode_positional_cone_frame(body, &scalar::ScalarCache::default())
                .expect("complete planar-envelope cone");
            assert_eq!(frame.apex[0], 0.0);
            assert!((frame.apex[1] + 19.389_817_409_565_175).abs() < 1e-12);
            assert_eq!(frame.apex[2], 0.0);
            assert_eq!(frame.axis, [0.0, 1.0, 0.0]);
            assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
            assert!((frame.half_angle - 0.636_540_466_818_335).abs() < 1e-12);
        }

        let mut inconsistent = unreferenced;
        inconsistent[43] = 0x98;
        assert!(
            decode_positional_cone_frame(&inconsistent, &scalar::ScalarCache::default()).is_none()
        );
    }

    #[test]
    fn tabulated_cylinder_replay_requires_the_immediately_preceding_row() {
        let mut payload = b"srf_array\0\xf8\x02".to_vec();
        payload.extend_from_slice(&[7, 0x2c, 4, 0x01, 0, 8]);
        payload.extend_from_slice(&[8, 0x22, 4, 0x01, 0, 0]);
        payload.extend_from_slice(&[
            9, 0x13, 0xe2, 0x01, 0x00, 0x03, 0x18, 0xe6, 0x0f, 0xe6, 0xf8, 0x04, 0xf7, 32, 0xfb,
            0xe2, 0xf7, 36,
        ]);
        for separator in [
            [0x18, 0xf1, 0xf7, 32, 0xe2].as_slice(),
            [0x18, 0xe2].as_slice(),
            [0x18, 0xe2].as_slice(),
            [0x18, 0xf2, 0xf7, 37, 0xf6, 0xe3].as_slice(),
        ] {
            payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
            payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
            payload.extend_from_slice(separator);
        }

        assert!(tabulated_cylinder_curve_replays(&payload).is_empty());
    }

    #[test]
    fn positional_surface_parameter_lookup_rejects_repeated_identity() {
        let payload = [7, 0x2c, 4, 0x01, 0, 0, 0x0f, 0xe4, 0xe3];
        let records = parameter_records(&payload);
        let [record] = records.as_slice() else {
            panic!("expected one positional parameter record");
        };
        assert_eq!(unique_surface_parameter(&records, 7), Some(record));
        assert!(unique_surface_parameter(&[record.clone(), record.clone()], 7).is_none());
    }

    #[test]
    fn decodes_bounded_untagged_type26_five_coordinate_envelope() {
        let body = [
            0x18, 0x18, 0x01, 0x11, 0x2e, 0xb0, 0x12, 0x47, 0x05, 0x33, 0x2d, 0x2d, 0xff, 0xff,
            0xff, 0xff, 0xff, 0x29, 0x47, 0x05, 0x33, 0x2e, 0x05, 0x33, 0x2d, 0x31, 0xa6, 0x66,
            0x66, 0x66, 0x66, 0x66, 0x18,
        ];
        let mut payload = vec![7, 0x26, 4, 0x01, 0, 0];
        payload.extend_from_slice(&body);
        payload.push(0xe3);
        let records = parameter_records(&payload);
        let [record] = records.as_slice() else {
            panic!("one type-26 parameter record");
        };
        let envelope = record
            .type26_five_coordinate_envelope(0x26)
            .expect("complete five-coordinate envelope");
        assert_eq!(envelope.offset, 7);
        assert_eq!(envelope.values[0], -2.65);
        assert!((envelope.values[1] + 15.0).abs() < 1e-12);
        assert_eq!(envelope.values[2], -2.65);
        assert_eq!(envelope.values[3], 2.65);
        assert!((envelope.values[4] + 17.65).abs() < 1e-12);

        payload[6] = 0x17;
        assert!(parameter_records(&payload)[0]
            .type26_five_coordinate_envelope(0x26)
            .is_none());
    }

    #[test]
    fn decodes_direct_and_split_type26_torus_envelopes() {
        let prefix = [
            0x28, 0x8d, 0x07, 0x1b, 0xd2, 0x65, 0x6f, 0x6c, 0x18, 0x94, 0x3f, 0x02, 0x70, 0x16,
            0xbe, 0xfc, 0x00, 0x12, 0x20,
        ];
        let direct_tail = [
            0x47, 0x13, 0xcc, 0x46, 0x31, 0x3d, 0x70, 0xa3, 0xd7, 0x0a, 0x3e, 0x47, 0x13, 0xcc,
            0x2e, 0x13, 0xcc, 0x46, 0x30, 0xbd, 0x70, 0xa3, 0xd7, 0x0a, 0x3e, 0x21,
        ];
        let split_tail = [
            0x47, 0x13, 0xcc, 0x46, 0x31, 0x3d, 0x70, 0xa3, 0xd7, 0x0a, 0x3e, 0x3a, 0xb1, 0x47,
            0xba, 0x2e, 0x13, 0xcc, 0x46, 0x30, 0xbd, 0x70, 0xa3, 0xd7, 0x0a, 0x3e, 0x2e, 0x13,
            0xcc,
        ];
        let record = |tail: &[u8]| {
            let mut payload = vec![7, 0x26, 4, 0x01, 0, 0];
            payload.extend_from_slice(&prefix);
            payload.extend_from_slice(tail);
            payload.push(0xe3);
            parameter_records(&payload).remove(0)
        };

        let direct = record(&direct_tail)
            .type26_five_coordinate_envelope(0x26)
            .expect("direct torus envelope");
        assert!(direct
            .values
            .iter()
            .zip([-4.95, 17.24, -4.95, 4.95, 16.74])
            .all(|(actual, expected)| (actual - expected).abs() < 1.0e-12));
        let split = record(&split_tail)
            .type26_split_coordinate_envelope(0x26)
            .expect("split torus envelope");
        assert!(split
            .values
            .iter()
            .zip([-4.95, 17.24, 16.74, 4.95])
            .all(|(actual, expected)| (actual - expected).abs() < 1.0e-12));
    }

    #[test]
    fn decodes_repeated_diameter_type24_round_envelopes() {
        let record = |body: &[u8]| {
            let mut payload = vec![7, 0x24, 4, 0x01, 0, 0];
            payload.extend_from_slice(body);
            payload.push(0xe3);
            parameter_records(&payload).remove(0)
        };
        let panel = [
            0x15, 0x2d, 0x2b, 0x4d, 0xd8, 0x2f, 0xd7, 0x5e, 0x1f, 0x18, 0x2d, 0x2c, 0x1a, 0xa4,
            0xfc, 0xa4, 0x2a, 0xec, 0x2f, 0x00, 0x00, 0x2d, 0x36, 0x59, 0x99, 0x99, 0x99, 0x99,
            0x9a, 0x42, 0xf7, 0x33, 0x2e, 0x03, 0x33, 0x2e, 0x37, 0xcc, 0x29, 0xf7, 0x33,
        ];
        let prefixed_panel = [
            0x00, 0x15, 0x1c, 0x2d, 0x32, 0x0d, 0x52, 0x7e, 0x52, 0x15, 0x76, 0x18, 0x2d, 0x32,
            0x73, 0xb8, 0xe4, 0xb8, 0x7b, 0xdc, 0x47, 0x03, 0x33, 0x2d, 0x36, 0x59, 0x99, 0x99,
            0x99, 0x99, 0x99, 0x42, 0xf7, 0x33, 0x48, 0x00, 0x00, 0x2e, 0x37, 0xcc, 0x29, 0xf7,
            0x33,
        ];
        let separated = [
            0x18, 0x2d, 0x31, 0xa4, 0xa8, 0xc1, 0x54, 0xc9, 0x87, 0x12, 0x2d, 0x35, 0xa4, 0xa8,
            0xc1, 0x54, 0xc9, 0x87, 0x48, 0x1c, 0x00, 0x2f, 0x22, 0x00, 0x18, 0x48, 0x00, 0x00,
            0x2f, 0x2c, 0x00, 0x2f, 0x10, 0x00,
        ];

        assert!((record(&panel).type24_round_radius(0x24).unwrap() - 0.2).abs() < 1.0e-12);
        assert!((record(&prefixed_panel).type24_round_radius(0x24).unwrap() - 0.2).abs() < 1.0e-12);
        assert!((record(&separated).type24_round_radius(0x24).unwrap() - 2.0).abs() < 1.0e-12);

        let mut inconsistent = separated;
        inconsistent[31..34].copy_from_slice(&[0x2f, 0x12, 0x00]);
        assert!(record(&inconsistent).type24_round_radius(0x24).is_none());
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
            scalar_tokens: Vec::new(),
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
    fn compact_plane_scalar_suffix_requires_one_complete_nine_slot_frame() {
        let body = [
            0x32, 0xbe, 0xe4, 0xe4, 0xe4, 0x0d, 0x0f, 0xe4, 0x0d, 0xe4, 0x0f,
        ];
        let slots = complete_plane_compact_scalar_suffix(&body, &scalar::ScalarCache::default())
            .expect("unique compact scalar suffix");

        assert_eq!(
            slots.iter().map(|slot| slot.0).collect::<Vec<_>>(),
            vec![
                Some(1.0),
                Some(1.0),
                Some(1.0),
                Some(-1.0),
                Some(0.0),
                Some(1.0),
                Some(-1.0),
                Some(1.0),
                Some(0.0),
            ]
        );
        assert!(
            complete_plane_compact_scalar_suffix(&body[2..], &scalar::ScalarCache::default())
                .is_none()
        );
    }

    #[test]
    fn decodes_named_plane_outline_with_zero_boundary_type() {
        let payload = b"srf_array\0\xf8\x01\xe0\x01geom_id\0\x07\xe0\x01geom_type\0\x22\xe0\x01feat_id\0\x04\xe0\x01orient\0\x01\xe0\x01boundary_type\0\x00\xe0\x01next_geom_ptr\0\x00\xe0\x02outline\0\xf9\x02\x03\xe4\x18\xe4\xe4\xe4\x18\xe0\x00srf_prim_ptr(plane)\0\xe3";

        assert_eq!(rows(payload).len(), 1);
        assert_eq!(plane_envelopes(payload).len(), 1);

        assert_eq!(
            outline_planes(&plane_envelopes(payload)),
            vec![OutlinePlane {
                surface_id: 7,
                origin: [1.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
                u_axis: [0.0, 1.0, 0.0],
                offset: 104,
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
            scalar_tokens: Vec::new(),
            row_offset: 10,
            offset: 20,
        }];
        assert_eq!(outline_planes(&records)[0].origin, [0.0, -4.0, 0.0]);
        assert_eq!(outline_planes(&records)[0].normal, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn support_frame_selects_held_axis_with_unresolved_other_coordinate() {
        let records = [PlaneEnvelopeRecord {
            surface_id: 42,
            body: Vec::new(),
            envelope: PlaneEnvelope::Standard {
                bounds_2d: [[None; 2]; 2],
                corners_3d: [
                    [Some(-3.0), Some(-4.0), Some(7.0)],
                    [Some(5.0), Some(-4.0), None],
                ],
            },
            corner_coordinate_equal: [Some(false), Some(true), None],
            scalar_tokens: Vec::new(),
            row_offset: 10,
            offset: 20,
        }];
        let frames = [PlaneLocalSystem {
            surface_id: 42,
            body: Vec::new(),
            slots: Vec::new(),
            origin: Some([100.0, 200.0, 300.0]),
            u_axis: Some([0.0, 0.0, 1.0]),
            normal: Some([0.0, 1.0, 0.0]),
            classification: LocalSystemClassification::Unclassified,
            row_offset: 10,
            offset: 30,
        }];

        assert_eq!(
            frame_bound_outline_planes(&records, &frames),
            [OutlinePlane {
                surface_id: 42,
                origin: [0.0, -4.0, 0.0],
                normal: [0.0, 1.0, 0.0],
                u_axis: [0.0, 0.0, 1.0],
                offset: 20,
            }]
        );

        let agreeing_frames = [frames[0].clone(), frames[0].clone()];
        assert_eq!(
            frame_bound_outline_planes(&records, &agreeing_frames),
            frame_bound_outline_planes(&records, &frames)
        );
        let mut conflicting = frames[0].clone();
        conflicting.normal = Some([1.0, 0.0, 0.0]);
        assert!(frame_bound_outline_planes(&records, &[frames[0].clone(), conflicting]).is_empty());
    }

    #[test]
    fn support_frame_maps_shortened_terminal_outline_coordinate() {
        let records = [PlaneEnvelopeRecord {
            surface_id: 42,
            body: Vec::new(),
            envelope: PlaneEnvelope::Standard {
                bounds_2d: [[None; 2]; 2],
                corners_3d: [
                    [Some(-3.0), Some(-4.0), Some(7.0)],
                    [Some(-4.0), None, None],
                ],
            },
            corner_coordinate_equal: [Some(false), None, None],
            scalar_tokens: vec![
                vec![1],
                vec![2],
                vec![3],
                vec![4],
                vec![5],
                vec![6],
                vec![7],
                vec![6],
                Vec::new(),
                Vec::new(),
            ],
            row_offset: 10,
            offset: 20,
        }];
        let frames = [PlaneLocalSystem {
            surface_id: 42,
            body: Vec::new(),
            slots: Vec::new(),
            origin: Some([100.0, 200.0, 300.0]),
            u_axis: Some([0.0, 0.0, 1.0]),
            normal: Some([0.0, 1.0, 0.0]),
            classification: LocalSystemClassification::Simple,
            row_offset: 10,
            offset: 30,
        }];

        assert_eq!(
            frame_bound_outline_planes(&records, &frames)[0].origin,
            [0.0, -4.0, 0.0]
        );
    }

    #[test]
    fn positional_plane_frame_decodes_terminal_zero_before_null_tail() {
        let body = [
            0x18, 0xe4, 0x0f, 0x10, 0x18, 0xe5, 0x10, 0x18, 0x2f, 0x18, 0x00, 0x2d, 0x29, 0x3d,
            0x70, 0xa3, 0xd7, 0x0a, 0x3d, 0x18, 0xe1,
        ];

        let slots = complete_plane_local_system_slots(&body, &scalar::ScalarCache::default())
            .expect("complete frame");
        assert_eq!(
            slots,
            [0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 6.0, -12.62, 0.0]
        );
        let frame = plane_frame(&slots.map(Some));
        assert_eq!(frame.origin, Some([6.0, -12.62, 0.0]));
        assert_eq!(frame.u_axis, Some([0.0, 1.0, 0.0]));
        assert_eq!(frame.normal, Some([1.0, 0.0, 0.0]));
    }

    #[test]
    fn positional_plane_frame_rejects_unconsumed_row_bytes() {
        let body = [
            0x18, 0xe4, 0x0f, 0x10, 0x18, 0xe5, 0x10, 0x18, 0x2f, 0x18, 0x00, 0x2d, 0x29, 0x3d,
            0x70, 0xa3, 0xd7, 0x0a, 0x3d, 0x18, 0x00,
        ];

        assert_eq!(
            complete_plane_local_system_slots(&body, &scalar::ScalarCache::default()),
            None
        );
    }

    #[test]
    fn positional_plane_frame_requires_exactly_one_zero_support_triple() {
        let options = |slots: [f64; 12]| slots.map(Some);
        assert_eq!(
            plane_frame(&options([
                1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            ]))
            .normal,
            Some([0.0, 0.0, 1.0])
        );
        assert_eq!(
            plane_frame(&options([
                1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0,
            ]))
            .normal,
            Some([0.0, 0.0, 1.0])
        );
        assert!(plane_frame(&options([
            1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
        ]))
        .normal
        .is_none());
        assert!(plane_frame(&options([
            1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        ]))
        .normal
        .is_none());
    }

    #[test]
    fn signed_surface_dict_slots_decode_as_mirrors() {
        let body = [
            0xbb, 1, 2, 3, 4, 5, 6, 0xbb, 1, 2, 3, 4, 5, 6, 0x73, 1, 2, 3, 4, 5, 6,
        ];
        let slots = scalar_slots_with_tokens(&body, 3, &scalar::ScalarCache::default());

        let magnitude = f64::from_be_bytes([0x3f, 0xe8, 1, 2, 3, 4, 5, 6]);
        assert_eq!(
            slots.iter().map(|slot| slot.0).collect::<Vec<_>>(),
            vec![Some(-magnitude), Some(-magnitude), Some(magnitude)]
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
                tokens: Vec::new(),
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
                tokens: Vec::new(),
            })
        );
    }

    #[test]
    fn named_local_system_uses_the_signed_coordinate_dict_lane() {
        let payload = b"srf_prim_ptr(torus)\0\
            \xe0\x02local_sys\0\xf9\x04\x03\
            \x7a\xeb\xb6\x28\xd0\x03\x82\
            \x28\xb2\x01\x83\xce\x09\x70\xf1\
            \x18\xe5\x10\
            \x41\xb2\x01\x83\xce\x09\x70\xf1\
            \x7a\xeb\xb6\x28\xd0\x03\x82\x18\
            \x48\x66\x80\x48\x08\x00\x2f\x44\x00";
        let records = named_prototype_records(payload);
        let SurfaceNamedValue::ScalarArray { values, .. } =
            &records[0].field("local_sys").expect("local system").value
        else {
            panic!("scalar local system");
        };

        assert_eq!(values[0], Some(0.997_523_383_819_597_8));
        assert_eq!(values[1], Some(0.070_335_614_969_227_37));
        assert_eq!(values[6], Some(0.070_335_614_969_227_37));
        assert_eq!(values[7], Some(0.997_523_383_819_597_8));
        assert_eq!(&values[9..12], &[Some(-180.0), Some(-3.0), Some(40.0)]);
    }

    #[test]
    fn named_local_system_decodes_positive_compact_half_coordinate() {
        let body = [0xf9, 0x04, 0x03, 0x0e];
        let SurfaceNamedValue::ScalarArray { values, .. } =
            named_surface_value("local_sys", &body, &scalar::ScalarCache::default())
        else {
            panic!("scalar local system");
        };

        assert_eq!(values[0], Some(0.5));
    }

    #[test]
    fn dimensioned_scalar_arrays_decode_compact_extents() {
        let mut body = vec![0xf9, 0x80, 0x88, 0x03];
        body.extend([0x0f; 136 * 3]);
        let SurfaceNamedValue::ScalarArray {
            dimensions,
            count,
            values,
            ..
        } = named_surface_value("i_points", &body, &scalar::ScalarCache::default())
        else {
            panic!("dimensioned scalar array");
        };

        assert_eq!(dimensions, 136);
        assert_eq!(count, 3);
        assert_eq!(values.len(), 408);
        assert!(values.iter().all(|value| *value == Some(0.0)));
    }

    #[test]
    fn named_torus_radii_decode_compact_positive_quarters() {
        let payload = b"srf_prim_ptr(torus)\0\
            \xe0\x01radius1\0\x0e\
            \xe0\x01radius2\0\x0d\xf1\xf7\x0e\xe3";
        let records = named_prototype_records(payload);

        assert_eq!(
            records[0].field("radius1").map(|field| &field.value),
            Some(&SurfaceNamedValue::ScalarSequence(vec![0.5]))
        );
        assert_eq!(
            records[0].field("radius2").map(|field| &field.value),
            Some(&SurfaceNamedValue::ScalarSequence(vec![0.25]))
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
            scalar_tokens: Vec::new(),
            row_offset: 10,
            offset: 20,
        }];
        assert!(outline_planes(&records).is_empty());
    }

    #[test]
    fn torus_rows_keep_the_byte_after_a_seven_byte_coordinate() {
        let body = [0x2d, 0x1c, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf6];
        let cache = scalar::ScalarCache::default();

        assert_eq!(
            decode_row_scalar(SurfaceKind::TorusOrSphere, &body, 0, &cache),
            Some((-7.0, 7))
        );
        assert_eq!(body[7], 0xf6);
    }
}
