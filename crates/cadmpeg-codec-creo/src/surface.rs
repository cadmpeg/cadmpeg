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
    /// Axis-normal rectangle corners from a split cylinder patch suffix.
    pub split_cylinder_outline_bounds: Option<[[f64; 2]; 2]>,
    /// Complete analytic carrier decoded from a positional cone row.
    pub positional_cone_frame: Option<PositionalConeFrame>,
    /// Complete analytic carrier decoded from a positional torus row.
    pub positional_torus_frame: Option<PositionalTorusFrame>,
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

/// Complete model-space carrier decoded from a positional torus row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionalTorusFrame {
    /// Model-space torus center.
    pub center: [f64; 3],
    /// Unit torus axis.
    pub axis: [f64; 3],
    /// Unit parameter-space reference direction.
    pub ref_direction: [f64; 3],
    /// Positive major radius.
    pub major_radius: f64,
    /// Positive minor radius.
    pub minor_radius: f64,
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

/// Diameter and model-space extent endpoints from a bounded type-24 round body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Type24RoundEnvelope {
    /// Positive difference between the two stored diameter endpoints.
    pub diameter: f64,
    /// Opposite model-space extent corners.
    pub extent_endpoints: [[f64; 3]; 2],
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
        if self.body.ends_with(&[0xf7, 0x1c]) {
            let frame_end = self.body.len().checked_sub(2)?;
            if let Some(frame) = self.scalar_frames.last() {
                let end = frame
                    .slots
                    .last()
                    .and_then(|slot| slot.offset.checked_add(slot.length));
                if frame.slots.len() == 5 && end == Some(frame_end) {
                    let [coordinate0, coordinate1, coordinate2, coordinate3, coordinate4] =
                        frame.slots.as_slice()
                    else {
                        unreachable!("five slots were checked above");
                    };
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
                    .and_then(|slot| slot.offset.checked_add(slot.length))?;
                let second_end = second
                    .slots
                    .last()
                    .and_then(|slot| slot.offset.checked_add(slot.length))?;
                if first.slots.len() >= 3
                    && second.slots.len() == 2
                    && first_end < second.offset
                    && second_end == frame_end
                {
                    let first_coordinates = &first.slots[first.slots.len() - 3..];
                    let [coordinate0, coordinate1, coordinate2] = first_coordinates else {
                        unreachable!("three trailing slots were selected");
                    };
                    let [coordinate3, coordinate4] = second.slots.as_slice() else {
                        unreachable!("two slots were checked above");
                    };
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
        self.type24_scalar_frame_round_layout()
            .or_else(|| self.type24_split_coordinate_round_layout())
            .map(|layout| 0.5 * layout.diameter)
            .or_else(|| {
                (self.is_type24_first_coordinate_round_body()
                    || self.is_type24_segmented_first_coordinate_round_body())
                .then_some(())?;
                self.positional_cylinder_frame.map(|frame| frame.radius)
            })
            .or_else(|| {
                self.type24_held_coordinate_round_frame()
                    .map(|frame| frame.radius)
            })
            .or_else(|| self.type24_terminal_round_radius())
    }

    fn type24_terminal_round_radius(&self) -> Option<f64> {
        let terminal_end = self
            .body
            .strip_suffix(&[0xf7, 0x17])
            .map_or(self.body.len(), <[u8]>::len);
        let terminal = self.scalar_tokens.last()?;
        (terminal.offset.checked_add(terminal.length)? == terminal_end).then_some(())?;
        (terminal.length == 7
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
    pub fn type24_scalar_frame_round_envelope(&self, type_byte: u8) -> Option<Type24RoundEnvelope> {
        (type_byte == 0x24).then_some(())?;
        self.type24_scalar_frame_round_layout()
    }

    fn type24_round_frame(
        &self,
        type_byte: u8,
        cache: &scalar::ScalarCache,
    ) -> Option<PositionalCylinderFrame> {
        (type_byte == 0x24).then_some(())?;
        self.repeated_diameter_type24_round_frame(cache)
            .or_else(|| self.type24_held_coordinate_round_frame())
            .or_else(|| self.type24_square_radial_round_frame())
    }

    fn type24_square_radial_round_frame(&self) -> Option<PositionalCylinderFrame> {
        (self.boundary == SurfaceBodyBoundary::CompoundClose).then_some(())?;
        let terminal = self.scalar_frames.last()?;
        ((6..=8).contains(&terminal.slots.len())).then_some(())?;
        let repeated_diameter_shell = if terminal.slots.len() == 7 {
            if let [leading, _] = self.scalar_frames.as_slice() {
                let leading_end = leading
                    .slots
                    .iter()
                    .try_fold(leading.offset, |cursor, slot| {
                        (slot.offset == cursor).then(|| cursor + slot.length)
                    })?;
                let control_length = terminal.offset.checked_sub(leading_end)?;
                matches!(
                    (leading.slots.len(), leading.offset, control_length),
                    (1, 1, 1 | 3) | (1, 3, 1) | (2, 0 | 1, 1) | (3, 0, 1)
                )
            } else {
                false
            }
        } else {
            false
        };
        let terminal_end = terminal
            .slots
            .iter()
            .try_fold(terminal.offset, |cursor, slot| {
                (slot.offset == cursor).then(|| cursor + slot.length)
            })?;
        if terminal_end != self.body.len() {
            let suffix = self.body.get(terminal_end..)?;
            if !matches!(suffix, [0x00 | 0x10 | 0x18]) {
                (suffix.first() == Some(&psb::token::ENTITY_REF)).then_some(())?;
                let (_, reference_end) = psb::reference_id(&self.body, terminal_end + 1).ok()?;
                (reference_end == self.body.len()).then_some(())?;
            }
        }
        let corners = &terminal.slots[terminal.slots.len() - 6..];
        let values = corners
            .iter()
            .map(|slot| slot.value)
            .collect::<Option<Vec<_>>>()?;
        values.iter().all(|value| value.is_finite()).then_some(())?;
        let first: [f64; 3] = values[..3].try_into().ok()?;
        let second: [f64; 3] = values[3..].try_into().ok()?;
        let spans = std::array::from_fn::<_, 3, _>(|axis| second[axis] - first[axis]);
        let scale = values.iter().map(|value| value.abs()).fold(1.0, f64::max);
        let equal_pairs = [(0, 1), (0, 2), (1, 2)]
            .into_iter()
            .filter(|(first, second)| {
                (spans[*first].abs() - spans[*second].abs()).abs() <= 1e-9 * scale
            })
            .collect::<Vec<_>>();
        let [(first_radial, second_radial)] = equal_pairs.as_slice() else {
            return None;
        };
        let axis_index = 3usize.checked_sub(first_radial + second_radial)?;
        let diameter = f64::midpoint(spans[*first_radial].abs(), spans[*second_radial].abs());
        let length = spans[axis_index].abs();
        (diameter > 1e-12 * scale).then_some(())?;
        let bounded = length > 1e-12 * scale;
        (!repeated_diameter_shell || !bounded).then_some(())?;
        let mut origin = first;
        origin[*first_radial] = f64::midpoint(first[*first_radial], second[*first_radial]);
        origin[*second_radial] = f64::midpoint(first[*second_radial], second[*second_radial]);
        let mut axis = [0.0; 3];
        axis[axis_index] = if bounded {
            spans[axis_index].signum()
        } else {
            1.0
        };
        let mut ref_direction = [0.0; 3];
        ref_direction[*first_radial] = spans[*first_radial].signum();
        Some(PositionalCylinderFrame {
            origin,
            axis,
            ref_direction,
            radius: 0.5 * diameter,
            length: bounded.then_some(length),
        })
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
        let radial_indices = spans
            .iter()
            .enumerate()
            .filter_map(|(index, span)| {
                ((span.abs() - layout.diameter).abs() <= 1e-9 * scale).then_some(index)
            })
            .collect::<Vec<_>>();
        let [radial_index] = radial_indices.as_slice() else {
            return None;
        };
        let mut axis_vector = spans;
        axis_vector[*radial_index] = 0.0;
        let length = axis_vector
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt();
        (length.is_finite() && length > 1e-12 * scale).then_some(())?;
        let mut origin = layout.extent_endpoints[0];
        origin[*radial_index] = f64::midpoint(
            layout.extent_endpoints[0][*radial_index],
            layout.extent_endpoints[1][*radial_index],
        );
        let mut ref_direction = [0.0; 3];
        ref_direction[*radial_index] = spans[*radial_index].signum();
        Some(PositionalCylinderFrame {
            origin,
            axis: axis_vector.map(|value| value / length),
            ref_direction,
            radius: 0.5 * layout.diameter,
            length: Some(length),
        })
    }

    fn type24_held_coordinate_round_frame(&self) -> Option<PositionalCylinderFrame> {
        let contiguous_end = |frame: &SurfaceParameterScalarFrame| {
            frame.slots.iter().try_fold(frame.offset, |cursor, slot| {
                (slot.offset == cursor).then(|| cursor + slot.length)
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
            && axial_span.abs() > 1e-12 * scale
            && radial_span.abs() > 1e-12 * scale)
            .then_some(PositionalCylinderFrame {
                origin: [axial_start, f64::midpoint(radial_start, radial_end), held],
                axis: [axial_span.signum(), 0.0, 0.0],
                ref_direction: [0.0, radial_span.signum(), 0.0],
                radius: 0.5 * radial_span.abs(),
                length: Some(axial_span.abs()),
            })
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
                (slot.offset == cursor).then(|| cursor + slot.length)
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
        (diameter > 1e-12 * scale
            && extent_endpoints[0]
                .iter()
                .zip(extent_endpoints[1])
                .any(|(first, second)| ((second - first).abs() - diameter).abs() <= 1e-9 * scale))
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
        let (first_diameter, first_end) = decode_at(7)?;
        (first_end == 15).then_some(())?;
        let (second_diameter, mut cursor) = decode_at(16)?;
        (cursor == 24).then_some(())?;
        let mut coordinates = Vec::with_capacity(6);
        for _ in 0..5 {
            let (value, next) = decode_at(cursor)?;
            coordinates.push(value);
            cursor = next;
        }
        (cursor == 49).then_some(())?;
        coordinates.push(0.0);
        let [a0, a1, a2, b0, b1, b2] = coordinates.as_slice() else {
            unreachable!("six bounded round coordinates")
        };
        let diameter = (second_diameter - first_diameter).abs();
        let scale = [first_diameter, second_diameter]
            .into_iter()
            .chain(coordinates.iter().copied())
            .map(f64::abs)
            .fold(1.0, f64::max);
        (diameter > 1e-12 * scale).then_some(Type24RoundEnvelope {
            diameter,
            extent_endpoints: [[*a0, *a1, *a2], [*b0, *b1, *b2]],
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
        let (first_diameter, first_end) = decode_at(1)?;
        (first_end == 9).then_some(())?;
        let (second_diameter, mut cursor) = decode_at(16)?;
        (cursor == 24).then_some(())?;
        let mut coordinates = Vec::with_capacity(6);
        for _ in 0..6 {
            let (value, next) = decode_at(cursor)?;
            coordinates.push(value);
            cursor = next;
        }
        (cursor == 54).then_some(())?;
        let [a0, a1, a2, b0, b1, b2] = coordinates.as_slice() else {
            unreachable!("six bounded segmented-round coordinates")
        };
        let diameter = (second_diameter - first_diameter).abs();
        let scale = [first_diameter, second_diameter]
            .into_iter()
            .chain(coordinates.iter().copied())
            .map(f64::abs)
            .fold(1.0, f64::max);
        (diameter > 1e-12 * scale).then_some(Type24RoundEnvelope {
            diameter,
            extent_endpoints: [[*a0, *a1, *a2], [*b0, *b1, *b2]],
        })
    }

    fn is_type24_first_coordinate_round_body(&self) -> bool {
        self.body.len() == 50
            && self.body.get(..2) == Some(&[0x4c, 0xb7])
            && self.body.get(15) == Some(&0x12)
            && self.body.get(49) == Some(&0x18)
    }

    fn is_type24_segmented_first_coordinate_round_body(&self) -> bool {
        self.body.len() == 56
            && self.body.first() == Some(&0x18)
            && self.body.get(9..16) == Some(&[0x70, 0xbf, 0xe3, 0x4f, 0x05, 0x11, 0x10])
            && self.body.get(54..56) == Some(&[0xf7, 0x19])
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
                frame_reaches_body_end(end).then_some(())?;
                ([*first, *second], [[*a0, *a1, *a2], [*b0, *b1, *b2]])
            }
            [leading, trailing] if leading.slots.len() == 1 => {
                let (leading_values, leading_end) = contiguous_values(leading)?;
                let [first] = leading_values.as_slice() else {
                    unreachable!("one leading diameter slot was checked above");
                };
                let (trailing_values, trailing_end) = contiguous_values(trailing)?;
                let [second, a0, a1, a2, b0, b1, b2] = trailing_values.as_slice() else {
                    return None;
                };
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
                ((leading.offset == 0
                    || (leading.offset == 1 && matches!(self.body.first(), Some(0x19 | 0x32))))
                    && self.body.get(leading_end..trailing.offset) == Some(&[0x12])
                    && frame_reaches_body_end(trailing_end))
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
            .then_some(Type24RoundEnvelope {
                diameter,
                extent_endpoints,
            })
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

/// Derive axis-aligned plane equations from complete positional corner frames
/// owned by uniquely identified plane rows.
#[must_use]
pub fn positional_frame_planes(
    parameters: &[SurfaceParameterRecord],
    rows: &[SurfaceRow],
) -> Vec<OutlinePlane> {
    let mut result = Vec::new();
    for record in parameters {
        if record.boundary != SurfaceBodyBoundary::CompoundClose
            || unique_surface_parameter(parameters, record.surface_id) != Some(record)
            || unique_surface_row(rows, record.surface_id)
                .is_none_or(|row| row.kind != SurfaceKind::Plane)
        {
            continue;
        }
        let marked_frames = record.scalar_frames.iter().filter_map(|frame| {
            (frame.slots.len() == 6
                && frame.offset >= 3
                && record.body.get(frame.offset - 3..frame.offset) == Some(&[0x00, 0x0c, 0x9a]))
            .then_some((frame.offset, frame.slots.as_slice()))
        });
        let auxiliary_frame = (|| {
            let [leading, terminal] = record.scalar_frames.as_slice() else {
                return None;
            };
            let [leading_slot] = leading.slots.as_slice() else {
                return None;
            };
            let [_, corners @ ..] = terminal.slots.as_slice() else {
                return None;
            };
            let leading_end = leading_slot.offset.checked_add(leading_slot.length)?;
            let terminal_end = terminal
                .slots
                .iter()
                .try_fold(terminal.offset, |cursor, slot| {
                    (slot.offset == cursor).then(|| cursor + slot.length)
                })?;
            (leading.offset == 3
                && leading_end == 10
                && terminal.offset == 18
                && corners.len() == 6
                && terminal_end == record.body.len()
                && record.opaque_spans.len() == 2
                && record.opaque_spans[0].offset == 0
                && record.opaque_spans[0].length == 3
                && record.opaque_spans[1].offset == 10
                && record.opaque_spans[1].length == 8)
                .then(|| (corners[0].offset, corners))
        })();
        let suffixed_auxiliary_frame = (|| {
            let frame_end = record.body.len().checked_sub(2)?;
            let mut frames = record.scalar_frames.iter().filter(|frame| {
                (7..=10).contains(&frame.slots.len())
                    && frame
                        .slots
                        .last()
                        .is_some_and(|slot| slot.offset.checked_add(slot.length) == Some(frame_end))
            });
            let terminal = frames.next()?;
            frames.next().is_none().then_some(())?;
            record.body.ends_with(&[0xf7, 0x0c]).then_some(())?;
            let corners = &terminal.slots[terminal.slots.len() - 6..];
            Some((corners[0].offset, corners))
        })();
        let terminal_corner_frame = (|| {
            let frame_end = record.body.len().checked_sub(2)?;
            record.body.ends_with(&[0xf7, 0x1f]).then_some(())?;
            let [terminal] = record.scalar_frames.as_slice() else {
                return None;
            };
            ((6..=10).contains(&terminal.slots.len())
                && terminal
                    .slots
                    .last()
                    .is_some_and(|slot| slot.offset.checked_add(slot.length) == Some(frame_end)))
            .then_some(())?;
            let corners = &terminal.slots[terminal.slots.len() - 6..];
            Some((corners[0].offset, corners))
        })();
        let split_terminal_corner_frame = (|| {
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
                    (slot.offset == cursor).then(|| cursor + slot.length)
                })?;
            let terminal_end = terminal
                .slots
                .iter()
                .try_fold(terminal.offset, |cursor, slot| {
                    (slot.offset == cursor).then(|| cursor + slot.length)
                })?;
            let [prefix, controls, trailer] = record.opaque_spans.as_slice() else {
                return None;
            };
            (prefix.offset == 0
                && prefix.length == leading.offset
                && controls.offset == leading_end
                && controls.length == terminal.offset.checked_sub(leading_end)?
                && trailer.offset == frame_end
                && trailer.length == 2
                && terminal_end == frame_end)
                .then_some(())?;
            let corners = &terminal.slots[2..];
            Some((corners[0].offset, corners))
        })();
        let mut candidates = marked_frames
            .chain(auxiliary_frame)
            .chain(suffixed_auxiliary_frame)
            .chain(terminal_corner_frame)
            .chain(split_terminal_corner_frame)
            .filter_map(|(offset, slots)| {
                let values = slots
                    .iter()
                    .map(|slot| slot.value)
                    .collect::<Option<Vec<_>>>()?;
                values.iter().all(|value| value.is_finite()).then_some(())?;
                let scale = values.iter().map(|value| value.abs()).fold(1.0, f64::max);
                let equal = std::array::from_fn::<_, 3, _>(|axis| {
                    (values[axis] - values[axis + 3]).abs() <= 1e-9 * scale
                });
                let held = equal
                    .iter()
                    .enumerate()
                    .filter_map(|(axis, equal)| equal.then_some(axis))
                    .collect::<Vec<_>>();
                let [axis] = held.as_slice() else {
                    return None;
                };
                let mut origin = [0.0; 3];
                origin[*axis] = values[*axis];
                let mut normal = [0.0; 3];
                normal[*axis] = 1.0;
                let u_axis = if *axis == 0 {
                    [0.0, 1.0, 0.0]
                } else {
                    [1.0, 0.0, 0.0]
                };
                Some(OutlinePlane {
                    surface_id: record.surface_id,
                    origin,
                    normal,
                    u_axis,
                    offset: record.body_offset + offset,
                })
            })
            .collect::<Vec<_>>();
        candidates.dedup_by(|first, second| {
            first.origin == second.origin
                && first.normal == second.normal
                && first.u_axis == second.u_axis
        });
        if let [candidate] = candidates.as_slice() {
            result.push(candidate.clone());
        }
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
                let remaining = body.len().saturating_sub(values_start);
                let Some(slot_count) = usize::try_from(count)
                    .ok()
                    .filter(|count| *count <= remaining.saturating_mul(3))
                else {
                    return SurfaceNamedValue::Opaque(body.to_vec());
                };
                let Some(slots) =
                    counted_parameter_scalar_slots(&body[values_start..], slot_count, cache)
                else {
                    return SurfaceNamedValue::Opaque(body.to_vec());
                };
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
        let values = if let Some(slots) = &spline_slots {
            slots.iter().map(|slot| slot.0).collect()
        } else if name == "local_sys" {
            let Some(values) =
                sequential_named_local_system_slots(&body[values_start..], slot_count, cache)
            else {
                return SurfaceNamedValue::Opaque(body.to_vec());
            };
            values
        } else {
            scalar_slots(&body[values_start..], slot_count, cache)
        };
        return SurfaceNamedValue::ScalarArray {
            dimensions,
            count,
            values,
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
        } else if scalar_field {
            scalar::decode_named_surface_radius(body, cursor, cache)
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
    let positional_plane_corners = (kind == SurfaceKind::Plane)
        .then(|| first_coordinate_plane_corner_tokens(body, cache))
        .flatten()
        .unwrap_or_default();
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
        if let Some(token) = positional_plane_corners
            .iter()
            .find(|token| token.offset == cursor)
        {
            tokens.push(token.clone());
            cursor += token.length;
            continue;
        }
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
            if positional_plane_corners
                .iter()
                .any(|token| cursor < token.offset && next > token.offset)
            {
                cursor += 1;
                continue;
            }
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

fn first_coordinate_plane_corner_tokens(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<Vec<SurfaceParameterScalar>> {
    let frame_end = if body.ends_with(&[0xf7, 0x0c]) {
        body.len().checked_sub(2)?
    } else {
        body.len()
    };
    let mut candidates = (0..frame_end).filter_map(|start| {
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
        let slot = |value, offset, end| SurfaceParameterScalar {
            value: Some(value),
            raw: body[offset..end].to_vec(),
            offset,
            length: end - offset,
        };
        Some(vec![
            slot(-stored_first_x, start, first_end),
            slot(first_y, first_end, first_z_start),
            slot(first_z, first_z_start, second_x_start),
            slot(-stored_second_x, second_x_start, second_y_start),
            slot(second_y, second_y_start, second_z_start),
            slot(second_z, second_z_start, end),
        ])
    });
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
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

fn split_cylinder_outline_bounds(
    body: &[u8],
    slots: &[SurfaceParameterScalar],
) -> Option<[[f64; 2]; 2]> {
    let [first_u, first_v, second_u, second_v, orientation] =
        slots.get(slots.len().checked_sub(5)?..)?
    else {
        return None;
    };
    (first_u.offset + first_u.length == first_v.offset
        && body.get(first_v.offset + first_v.length..second_u.offset)
            == Some(&[0x00, 0x0c, 0x98][..])
        && second_u.offset + second_u.length == second_v.offset
        && second_v.offset + second_v.length == orientation.offset
        && orientation.raw == [0x0d]
        && orientation.value == Some(-1.0)
        && matches!(
            body.get(orientation.offset + orientation.length..),
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
        let split_cylinder_outline_bounds = (row.kind == SurfaceKind::Cylinder)
            .then(|| split_cylinder_outline_bounds(&body, &scalar_tokens))
            .flatten();
        let positional_cone_frame = (row.kind == SurfaceKind::Cone)
            .then(|| decode_positional_cone_frame(&body, &cache))
            .flatten();
        let positional_torus_frame = (row.kind == SurfaceKind::TorusOrSphere)
            .then(|| decode_positional_torus_frame(&body, &cache))
            .flatten();
        let mut record = SurfaceParameterRecord {
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
            split_cylinder_outline_bounds,
            positional_cone_frame,
            positional_torus_frame,
            body,
            boundary,
            offset: row.offset,
            body_offset: *body_start,
        };
        if row.kind == SurfaceKind::Cylinder && record.positional_cylinder_frame.is_none() {
            record.positional_cylinder_frame = record.type24_round_frame(row.type_byte, &cache);
        }
        records.push(record);
    }
    records
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
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    let [a1, a2, b0, b1, b2] = envelope;
    let proves_radii = |outer_delta: f64, minor_delta: f64| {
        close(outer_delta.abs(), 2.0 * (major_radius + minor_radius))
            && close(minor_delta.abs(), minor_radius)
    };
    (close(a1, b0) && (proves_radii(b1 - a1, b2 - a2) ^ proves_radii(b2 - a1, b1 - a2)))
        .then_some(())?;

    let first: [f64; 3] = slots[0..3].try_into().ok()?;
    let second: [f64; 3] = slots[6..9].try_into().ok()?;
    let first_norm = first.iter().map(|value| value * value).sum::<f64>().sqrt();
    let second_norm = second.iter().map(|value| value * value).sum::<f64>().sqrt();
    let scale = first_norm.max(second_norm).max(1.0);
    (first_norm.is_finite()
        && second_norm.is_finite()
        && first_norm > 1e-12
        && second_norm > 1e-12
        && (first_norm - second_norm).abs() <= 1e-10 * scale)
        .then_some(())?;
    let ref_direction = first.map(|value| value / first_norm);
    let second = second.map(|value| value / second_norm);
    let orthogonality = ref_direction
        .iter()
        .zip(second)
        .map(|(first, second)| first * second)
        .sum::<f64>();
    (orthogonality.abs() <= 1e-10).then_some(())?;
    let axis = [
        ref_direction[1] * second[2] - ref_direction[2] * second[1],
        ref_direction[2] * second[0] - ref_direction[0] * second[2],
        ref_direction[0] * second[1] - ref_direction[1] * second[0],
    ];
    let axis_norm = axis.iter().map(|value| value * value).sum::<f64>().sqrt();
    (axis_norm.is_finite() && axis_norm > 1e-12).then_some(())?;
    let axis = axis.map(|value| value / axis_norm);

    Some(PositionalTorusFrame {
        center: slots[9..12].try_into().ok()?,
        axis,
        ref_direction,
        major_radius,
        minor_radius,
    })
}

fn decode_positional_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    decode_compact_y_axis_cylinder_frame(body, cache)
        .or_else(|| decode_local_system_cylinder_frame(body, cache))
        .or_else(|| decode_zero_support_cylinder_frame(body, cache))
        .or_else(|| decode_signed_zero_support_cylinder_frame(body, cache))
        .or_else(|| decode_signed_axis_aligned_cylinder_frame(body, cache))
        .or_else(|| decode_signed_axial_radial_cylinder_frame(body, cache))
        .or_else(|| decode_signed_radial_envelope_cylinder_frame(body, cache))
        .or_else(|| decode_xz_axis_y_radial_cylinder_frame(body, cache))
        .or_else(|| decode_symmetric_revolution_cylinder_frame(body, cache))
        .or_else(|| decode_axial_endpoint_radial_sample_cylinder_frame(body, cache))
        .or_else(|| decode_precise_center_edge_cylinder_frame(body, cache))
        .or_else(|| decode_precise_held_center_cylinder_frame(body, cache))
        .or_else(|| decode_local_system_suffix_cylinder_frame(body, cache))
        .or_else(|| decode_referenced_planar_envelope_cylinder_frame(body, cache))
        .or_else(|| decode_held_axis_cylinder_frame(body, cache))
        .or_else(|| decode_axial_radial_cylinder_frame(body, cache))
        .or_else(|| decode_compact_axis_aligned_cylinder_frame(body, cache))
        .or_else(|| decode_directrix_lane_axis_aligned_cylinder_frame(body, cache))
}

fn decode_xz_axis_y_radial_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    (body.get(..3) == Some(&[0x20, 0x10, 0x00])).then_some(())?;
    let mut values = [0.0; 9];
    let mut cursor = 3;
    for (index, value) in values.iter_mut().enumerate() {
        if index == 5 && body.get(cursor..cursor + 3) == Some(&[0x34, 0xf0, 0x00]) {
            cursor += 3;
            continue;
        }
        let (decoded, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    (cursor == body.len()).then_some(())?;

    let [first_axial, auxiliary, second_axial, x0, y0, z0, x1, y1, z1] = values;
    let axis_vector = [x1 - x0, 0.0, z1 - z0];
    let length = axis_vector
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    let radius = 0.5 * (y1 - y0).abs();
    let scale = values
        .into_iter()
        .map(f64::abs)
        .fold(length.max(radius).max(1.0), f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    (length.is_finite()
        && length > 1e-12 * scale
        && radius.is_finite()
        && radius > 1e-12 * scale
        && auxiliary.abs() < length
        && close(second_axial - first_axial, z1 - z0))
    .then_some(())?;

    Some(PositionalCylinderFrame {
        origin: [x0, f64::midpoint(y0, y1), z0],
        axis: axis_vector.map(|value| value / length),
        ref_direction: [0.0, (y1 - y0).signum(), 0.0],
        radius,
        length: Some(length),
    })
}

fn decode_axial_endpoint_radial_sample_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    let (first_leading, first_end) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, 0, cache)?;
    (first_end == 7 && body.get(first_end) == Some(&0x18)).then_some(())?;
    let (second_leading, second_end) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, first_end + 1, cache)?;
    (second_end == 15 && body.get(second_end) == Some(&0x0e)).then_some(())?;
    let (radial_x, mut cursor) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, second_end + 1, cache)?;
    let (axial_start, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
    cursor = next;
    let (auxiliary_radial, next) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
    cursor = next;
    let (radius, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
    cursor = next;
    let (axial_end, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
    cursor = next;
    let (radial_z, next) = scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
    (body.get(next..) == Some(&[0xf7, 0x19])).then_some(())?;

    let values = [
        first_leading,
        second_leading,
        radial_x,
        axial_start,
        auxiliary_radial,
        radius,
        axial_end,
        radial_z,
    ];
    values.into_iter().all(f64::is_finite).then_some(())?;
    let scale = values.into_iter().map(f64::abs).fold(1.0, f64::max);
    let tolerance = 1e-9 * scale;
    let length = (axial_end - axial_start).abs();
    (radius > 1e-12 * scale
        && length > 1e-12 * scale
        && radial_x.abs() > 1e-12 * scale
        && auxiliary_radial.abs() <= radius + tolerance
        && (radial_x * radial_x + radial_z * radial_z - radius * radius).abs()
            <= tolerance * radius.max(1.0))
    .then_some(())?;

    Some(PositionalCylinderFrame {
        origin: [0.0, axial_start, 0.0],
        axis: [0.0, (axial_end - axial_start).signum(), 0.0],
        ref_direction: [-radial_x.signum(), 0.0, 0.0],
        radius,
        length: Some(length),
    })
}

fn decode_symmetric_revolution_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    let mut cursor = 1;
    let (first_axial, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
    cursor = next;
    let separator = match body.first()? {
        0x15 => 0x18,
        0x17 => 0x15,
        _ => return None,
    };
    (body.get(cursor) == Some(&separator)).then_some(())?;
    cursor += 1;
    let (second_axial, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
    cursor = next;
    let (radial_low, next) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
    cursor = next;
    let (second_opposite, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
    cursor = next;
    if body[0] == 0x15 {
        (body.get(cursor) == Some(&0x18)).then_some(())?;
        cursor += 1;
    } else {
        let (repeated_radial_low, next) =
            scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
        cursor = next;
        let scale = radial_low.abs().max(repeated_radial_low.abs()).max(1.0);
        ((radial_low - repeated_radial_low).abs() <= 1e-9 * scale).then_some(())?;
    }
    let (radial_high, next) =
        scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
    cursor = next;
    let (first_opposite, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
    cursor = next;
    if body[0] == 0x15 {
        let (repeated_radial_high, next) =
            scalar::decode_tabulated_cylinder_first_coordinate(body, cursor, cache)?;
        cursor = next;
        let scale = radial_high.abs().max(repeated_radial_high.abs()).max(1.0);
        ((radial_high - repeated_radial_high).abs() <= 1e-9 * scale).then_some(())?;
    } else {
        let (_, next) = scalar::decode_model_reference_coordinate(body, cursor, cache)?;
        cursor = next;
    }
    (body.get(cursor..) == Some(&[0xf7, 0x19])).then_some(())?;

    let values = [
        first_axial,
        second_axial,
        radial_low,
        radial_high,
        second_opposite,
        first_opposite,
    ];
    values.into_iter().all(f64::is_finite).then_some(())?;
    let scale = values.into_iter().map(f64::abs).fold(1.0, f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    let axial_midpoint = f64::midpoint(first_axial, first_opposite);
    close(axial_midpoint, f64::midpoint(second_axial, second_opposite)).then_some(())?;
    let radius = 0.5 * (radial_high - radial_low).abs();
    (radius > 1e-12 * scale
        && close(f64::midpoint(radial_low, radial_high), 0.0)
        && (first_axial - first_opposite).abs() > 1e-12 * scale
        && (second_axial - axial_midpoint).abs() > (first_axial - axial_midpoint).abs())
    .then_some(())?;

    Some(PositionalCylinderFrame {
        origin: [0.0, axial_midpoint, 0.0],
        axis: [0.0, (first_axial - first_opposite).signum(), 0.0],
        ref_direction: [(radial_low - radial_high).signum(), 0.0, 0.0],
        radius,
        length: Some((first_opposite - first_axial).abs()),
    })
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

    axial_radial_cylinder_frame(
        length,
        first_axial,
        radial_sample,
        second_axial,
        radial_center,
        origin_at_first,
    )
}

fn axial_radial_cylinder_frame(
    length: f64,
    first_axial: f64,
    radial_sample: f64,
    second_axial: f64,
    radial_center: f64,
    origin_at_first: bool,
) -> Option<PositionalCylinderFrame> {
    (length.is_finite() && length > 0.0).then_some(())?;
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
    let (origin, radius) =
        decode_zero_support_cylinder_origin_radius(body, cursor, ZERO_SUPPORT, cache)?;
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
        origin,
        axis,
        ref_direction,
        radius,
        length: Some(length),
    })
}

fn decode_signed_zero_support_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    const ZERO_SUPPORT: &[u8] = &[0x10, 0x18, 0xe6, 0x0f, 0x18, 0x0f, 0x18];
    (body.first() == Some(&0x11)).then_some(())?;
    let (signed_length, mut cursor) = scalar::decode_in_surface_row_lane(body, 1, cache)?;
    (signed_length.is_finite() && signed_length != 0.0 && body.get(cursor) == Some(&0x13))
        .then_some(())?;
    cursor += 1;
    let mut stored = [0.0; 6];
    for value in &mut stored {
        let (decoded, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    let (origin, radius) =
        decode_zero_support_cylinder_origin_radius(body, cursor, ZERO_SUPPORT, cache)?;

    let first = [stored[1], stored[2], stored[0]];
    let second = [stored[4], stored[5], stored[3]];
    let spans = std::array::from_fn::<_, 3, _>(|index| (second[index] - first[index]).abs());
    let scale = first
        .iter()
        .chain(second.iter())
        .chain(origin.iter())
        .chain([signed_length, radius].iter())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    let candidates = (0..3)
        .filter_map(|axis_index| {
            let radial = (0..3)
                .filter(|index| *index != axis_index)
                .collect::<Vec<_>>();
            let [first_radial, second_radial] = radial.as_slice() else {
                return None;
            };
            let (diameter_index, radius_index) = match (
                close(spans[*first_radial], 2.0 * radius),
                close(spans[*second_radial], radius),
            ) {
                (true, true) => (*first_radial, *second_radial),
                _ if close(spans[*second_radial], 2.0 * radius)
                    && close(spans[*first_radial], radius) =>
                {
                    (*second_radial, *first_radial)
                }
                _ => return None,
            };
            close(spans[axis_index], signed_length.abs()).then_some((
                axis_index,
                diameter_index,
                radius_index,
            ))
        })
        .collect::<Vec<_>>();
    let [(axis_index, diameter_index, radius_index)] = candidates.as_slice() else {
        return None;
    };
    close(
        origin[*diameter_index],
        f64::midpoint(first[*diameter_index], second[*diameter_index]),
    )
    .then_some(())?;
    let radius_origin_at_first = close(origin[*radius_index], first[*radius_index]);
    let radius_origin_at_second = close(origin[*radius_index], second[*radius_index]);
    (radius_origin_at_first ^ radius_origin_at_second).then_some(())?;
    let axis_origin_at_first = close(origin[*axis_index], first[*axis_index]);
    let axis_origin_at_second = close(origin[*axis_index], second[*axis_index]);
    (axis_origin_at_first ^ axis_origin_at_second).then_some(())?;

    let other_axis = if axis_origin_at_first {
        second[*axis_index]
    } else {
        first[*axis_index]
    };
    let mut axis = [0.0; 3];
    axis[*axis_index] = (other_axis - origin[*axis_index]).signum();
    let mut ref_direction = [0.0; 3];
    ref_direction[*diameter_index] = if signed_length.is_sign_negative() {
        -(second[*diameter_index] - first[*diameter_index]).signum()
    } else {
        (second[*diameter_index] - first[*diameter_index]).signum()
    };
    Some(PositionalCylinderFrame {
        origin,
        axis,
        ref_direction,
        radius,
        length: Some(signed_length.abs()),
    })
}

fn decode_signed_axis_aligned_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    (body.first() == Some(&0x11)).then_some(())?;
    let (signed_length, mut cursor) = scalar::decode_in_surface_row_lane(body, 1, cache)?;
    (signed_length.is_finite() && signed_length != 0.0 && body.get(cursor) == Some(&0x13))
        .then_some(())?;
    cursor += 1;
    let (auxiliary, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
    auxiliary.is_finite().then_some(())?;
    cursor = next;
    let mut corners = [[0.0; 3]; 2];
    for coordinate in corners.iter_mut().flatten() {
        let (value, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
        value.is_finite().then_some(())?;
        *coordinate = value;
        cursor = next;
    }
    let reversed = if cursor == body.len() {
        false
    } else if body.get(cursor..) == Some(&[0xf7, 0x17]) {
        true
    } else {
        return None;
    };
    let scale = corners
        .iter()
        .flatten()
        .chain([signed_length, auxiliary].iter())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    (auxiliary.abs() < signed_length.abs()).then_some(())?;
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    let spans =
        std::array::from_fn::<_, 3, _>(|index| (corners[1][index] - corners[0][index]).abs());
    let mut axis_indices = (0..3).filter(|index| close(spans[*index], signed_length.abs()));
    let axis_index = axis_indices.next()?;
    axis_indices.next().is_none().then_some(())?;
    let [first_radial, second_radial] = match axis_index {
        0 => [1, 2],
        1 => [0, 2],
        2 => [0, 1],
        _ => unreachable!("three model axes"),
    };
    let (diameter_index, radius_index) = match (
        close(spans[first_radial], 2.0 * spans[second_radial]),
        close(spans[second_radial], 2.0 * spans[first_radial]),
    ) {
        (true, false) => (first_radial, second_radial),
        (false, true) => (second_radial, first_radial),
        _ => return None,
    };
    let radius = spans[radius_index];
    (radius > 1e-12 * scale).then_some(())?;

    let origin_corner = usize::from(!reversed);
    let other_corner = 1 - origin_corner;
    let mut origin = corners[origin_corner];
    origin[diameter_index] = f64::midpoint(corners[0][diameter_index], corners[1][diameter_index]);
    origin[radius_index] = corners[1][radius_index];
    let mut axis = [0.0; 3];
    axis[axis_index] =
        (corners[other_corner][axis_index] - corners[origin_corner][axis_index]).signum();
    let mut ref_direction = [0.0; 3];
    ref_direction[diameter_index] =
        (corners[other_corner][diameter_index] - origin[diameter_index]).signum();
    Some(PositionalCylinderFrame {
        origin,
        axis,
        ref_direction,
        radius,
        length: Some(signed_length.abs()),
    })
}

fn decode_signed_axial_radial_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    (body.first() == Some(&0x11)).then_some(())?;
    let decode = |offset| {
        let (value, next) = scalar::decode_in_surface_row_lane(body, offset, cache)?;
        value.is_finite().then_some((value, next))
    };
    let (signed_length, mut cursor) = decode(1)?;
    (signed_length != 0.0 && body.get(cursor) == Some(&0x13)).then_some(())?;
    cursor += 1;
    let (auxiliary, next) = decode(cursor)?;
    cursor = next;
    (auxiliary.abs() < signed_length.abs()).then_some(())?;
    let (first_axial, next) = decode(cursor)?;
    cursor = next;
    let (radial_sample, next) = decode(cursor)?;
    cursor = next;
    (body.get(cursor) == Some(&0xe4)).then_some(())?;
    cursor += 1;
    let (second_axial, next) = decode(cursor)?;
    cursor = next;
    (body.get(cursor) == Some(&0x19)).then_some(())?;
    let (_, next) = scalar::decode_model_reference_coordinate(body, cursor, cache)?;
    cursor = next;
    let (radial_center, next) = decode(cursor)?;
    (body.get(next..) == Some(&[0xf7, 0x17])).then_some(())?;

    axial_radial_cylinder_frame(
        signed_length.abs(),
        first_axial,
        radial_sample,
        second_axial,
        radial_center,
        false,
    )
}

fn decode_signed_radial_envelope_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    (body.first() == Some(&0x11)).then_some(())?;
    let (leading, mut cursor) = scalar::decode_in_surface_row_lane(body, 1, cache)?;
    (leading.is_finite() && body.get(cursor) == Some(&0x13)).then_some(())?;
    cursor += 1;
    let mut values = [0.0; 7];
    for value in &mut values {
        let (decoded, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    let reversed = if cursor == body.len() {
        false
    } else if body.get(cursor..) == Some(&[0xf7, 0x19]) {
        true
    } else {
        return None;
    };
    let (signed_length, auxiliary) = if reversed {
        (leading, values[0])
    } else {
        (values[0], leading)
    };
    (signed_length.is_finite()
        && signed_length != 0.0
        && reversed == signed_length.is_sign_negative()
        && auxiliary.abs() < signed_length.abs())
    .then_some(())?;

    let first_radial = [values[1], values[2]];
    let second_radial = [values[4], values[5]];
    let radial_spans =
        std::array::from_fn::<_, 2, _>(|index| (second_radial[index] - first_radial[index]).abs());
    let scale = values
        .iter()
        .chain([leading, signed_length].iter())
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    let (diameter_index, radius_index) = match (
        close(radial_spans[0], 2.0 * radial_spans[1]),
        close(radial_spans[1], 2.0 * radial_spans[0]),
    ) {
        (true, false) => (0, 1),
        (false, true) => (1, 0),
        _ => return None,
    };
    let radius = radial_spans[radius_index];
    (radius > 1e-12 * scale).then_some(())?;

    let axial_end = values[6];
    let axial_start = axial_end - signed_length.abs();
    let axial_sample = values[3];
    (axial_sample >= axial_start - 1e-9 * scale && axial_sample <= axial_end + 1e-9 * scale)
        .then_some(())?;
    let mut origin = [0.0; 3];
    origin[diameter_index] =
        f64::midpoint(first_radial[diameter_index], second_radial[diameter_index]);
    origin[radius_index] = second_radial[radius_index];
    origin[2] = if reversed { axial_end } else { axial_start };
    let mut axis = [0.0; 3];
    axis[2] = if reversed { -1.0 } else { 1.0 };
    let mut ref_direction = [0.0; 3];
    ref_direction[diameter_index] =
        axis[2] * (second_radial[diameter_index] - first_radial[diameter_index]).signum();
    Some(PositionalCylinderFrame {
        origin,
        axis,
        ref_direction,
        radius,
        length: Some(signed_length.abs()),
    })
}

fn decode_precise_center_edge_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    (body.first() == Some(&0x18)).then_some(())?;
    let (control, mut cursor) = scalar::decode_in_surface_row_lane(body, 2, cache)?;
    (control.is_finite() && cursor == 9).then_some(())?;
    let mut values = [0.0; 7];
    for value in &mut values {
        let (decoded, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
        decoded.is_finite().then_some(())?;
        *value = decoded;
        cursor = next;
    }
    (body.get(cursor..) == Some(&[0xf7, 0x19])).then_some(())?;

    let signed_length = values[0];
    (signed_length.is_finite() && signed_length != 0.0).then_some(())?;
    let first: [f64; 3] = values[1..4].try_into().ok()?;
    let second: [f64; 3] = values[4..7].try_into().ok()?;
    let spans = std::array::from_fn::<_, 3, _>(|index| (second[index] - first[index]).abs());
    let scale = values.iter().map(|value| value.abs()).fold(1.0, f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    let candidates = [(0, 1, 2), (0, 2, 1), (1, 2, 0)]
        .into_iter()
        .filter_map(|(first_radial, second_radial, axis_index)| {
            (close(spans[first_radial], spans[second_radial])
                && spans[first_radial] > 1e-12 * scale
                && spans[axis_index] > spans[first_radial])
                .then_some((first_radial, second_radial, axis_index))
        })
        .collect::<Vec<_>>();
    let [(first_radial, second_radial, axis_index)] = candidates.as_slice() else {
        return None;
    };
    let radius = spans[*first_radial];
    let origin_axial = second[*axis_index] + signed_length;
    let lower = origin_axial.min(second[*axis_index]);
    let upper = origin_axial.max(second[*axis_index]);
    (first[*axis_index] >= lower - 1e-9 * scale
        && first[*axis_index] <= upper + 1e-9 * scale
        && (first[*axis_index] - origin_axial).abs() <= radius + 1e-9 * scale)
        .then_some(())?;

    let mut origin = first;
    origin[*axis_index] = origin_axial;
    let mut axis = [0.0; 3];
    axis[*axis_index] = -signed_length.signum();
    let reference_index = (*first_radial).max(*second_radial);
    let mut ref_direction = [0.0; 3];
    ref_direction[reference_index] = (second[reference_index] - first[reference_index]).signum();
    Some(PositionalCylinderFrame {
        origin,
        axis,
        ref_direction,
        radius,
        length: Some(signed_length.abs()),
    })
}

fn decode_precise_held_center_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    (body.first() == Some(&0x18)).then_some(())?;
    let (control, mut cursor) = scalar::decode_in_surface_row_lane(body, 3, cache)?;
    (control.is_finite() && cursor == 10).then_some(())?;
    let decode = |cursor| {
        let (value, next) = scalar::decode_in_surface_row_lane(body, cursor, cache)?;
        value.is_finite().then_some((value, next))
    };
    let (signed_length, next) = decode(cursor)?;
    cursor = next;
    let (first_axial, next) = decode(cursor)?;
    cursor = next;
    let (held_center, next) = decode(cursor)?;
    cursor = next;
    (body.get(cursor) == Some(&0xe4)).then_some(())?;
    let (first_radius, next) = decode(cursor)?;
    cursor = next;
    let (second_axial, next) = decode(cursor)?;
    cursor = next;
    let (radial_edge, next) = decode(cursor)?;
    cursor = next;
    (body.get(cursor) == Some(&0xe4)).then_some(())?;
    let (second_radius, next) = decode(cursor)?;
    (body.get(next..) == Some(&[0xf7, 0x19])).then_some(())?;

    let scale = [
        signed_length,
        first_axial,
        held_center,
        first_radius,
        second_axial,
        radial_edge,
        second_radius,
    ]
    .into_iter()
    .map(f64::abs)
    .fold(1.0, f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    (signed_length != 0.0
        && first_radius > 1e-12 * scale
        && close(first_radius, second_radius)
        && close((radial_edge - held_center).abs(), first_radius))
    .then_some(())?;
    let origin_axial = first_axial - signed_length;
    let lower = first_axial.min(origin_axial);
    let upper = first_axial.max(origin_axial);
    (second_axial >= lower - 1e-9 * scale
        && second_axial <= upper + 1e-9 * scale
        && (second_axial - origin_axial).abs() <= first_radius + 1e-9 * scale)
        .then_some(())?;
    Some(PositionalCylinderFrame {
        origin: [origin_axial, held_center, held_center],
        axis: [signed_length.signum(), 0.0, 0.0],
        ref_direction: [0.0, 0.0, (radial_edge - held_center).signum()],
        radius: first_radius,
        length: Some(signed_length.abs()),
    })
}

fn decode_local_system_suffix_cylinder_frame(
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<PositionalCylinderFrame> {
    let radius_starts = (1..body.len())
        .filter_map(|start| {
            let (radius, end) = scalar::decode(body, start)?;
            (end == body.len() && radius.is_finite() && radius > 0.0).then_some((start, radius))
        })
        .collect::<Vec<_>>();
    let [(radius_start, radius)] = radius_starts.as_slice() else {
        return None;
    };
    let frames = (0..*radius_start)
        .filter_map(|start| {
            scalar::decode_positional_cylinder_local_system_slots(
                body.get(start..*radius_start)?,
                cache,
            )
        })
        .filter(|slots| {
            let first: [f64; 3] = slots[0..3].try_into().expect("three support slots");
            let second: [f64; 3] = slots[3..6].try_into().expect("three support slots");
            let first_magnitude = first.iter().map(|value| value * value).sum::<f64>().sqrt();
            let second_magnitude = second.iter().map(|value| value * value).sum::<f64>().sqrt();
            let scale = first_magnitude.max(second_magnitude).max(1.0);
            first_magnitude > 0.0
                && (first_magnitude - second_magnitude).abs() <= 1e-9 * scale
                && first
                    .iter()
                    .zip(second)
                    .map(|(left, right)| left * right)
                    .sum::<f64>()
                    .abs()
                    <= 1e-9 * scale
        })
        .collect::<Vec<_>>();
    let [slots] = frames.as_slice() else {
        return None;
    };
    let normalize = |vector: [f64; 3]| {
        let magnitude = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
        (magnitude.is_finite() && magnitude > 0.0).then(|| vector.map(|value| value / magnitude))
    };
    let first = normalize(slots[0..3].try_into().ok()?)?;
    let second = normalize(slots[3..6].try_into().ok()?)?;
    let scale = slots
        .iter()
        .chain(std::iter::once(radius))
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    (first
        .iter()
        .zip(second)
        .map(|(left, right)| left * right)
        .sum::<f64>()
        .abs()
        <= 1e-9 * scale)
        .then_some(())?;
    let axis = normalize([
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ])?;
    Some(PositionalCylinderFrame {
        origin: slots[9..12].try_into().ok()?,
        axis,
        ref_direction: first,
        radius: *radius,
        length: None,
    })
}

fn decode_zero_support_cylinder_origin_radius(
    body: &[u8],
    start: usize,
    zero_support: &[u8],
    cache: &scalar::ScalarCache,
) -> Option<([f64; 3], f64)> {
    let radius_start = (start..body.len()).find(|candidate| {
        scalar::decode(body, *candidate)
            .is_some_and(|(value, end)| end == body.len() && value.is_finite() && value > 0.0)
    })?;
    let (radius, _) = scalar::decode(body, radius_start)?;
    let origins = (start + zero_support.len()..radius_start)
        .filter_map(|origin_start| {
            (body.get(origin_start - zero_support.len()..origin_start) == Some(zero_support)).then(
                || decode_positional_cylinder_origin(body, origin_start, radius_start, cache),
            )?
        })
        .collect::<Vec<_>>();
    let [origin] = origins.as_slice() else {
        return None;
    };
    Some((*origin, radius))
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

pub(super) fn decode_tabulated_cylinder_frame(
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
        let (value, next) = if matches!(slot, 1 | 4) && body.get(cursor) == Some(&0x18) {
            (0.0, cursor + 1)
        } else if matches!(slot, 0 | 3)
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
    const OUTLINE_PAIR_SEPARATOR: &[u8] = &[0x00, 0x0c, 0x98];

    let separator_close = payload
        .get(start..end)?
        .windows(OUTLINE_PAIR_SEPARATOR.len() + 1)
        .position(|window| {
            window.starts_with(OUTLINE_PAIR_SEPARATOR)
                && window.last() == Some(&psb::token::COMPOUND_CLOSE)
        })
        .map(|offset| start + offset + OUTLINE_PAIR_SEPARATOR.len());
    for token in psb::tokens(payload.get(start..end)?) {
        match token.kind {
            psb::TokenKind::CompoundClose => {
                return Some(
                    separator_close.map_or(start + token.offset, |separator_close| {
                        separator_close.min(start + token.offset)
                    }),
                );
            }
            psb::TokenKind::NamedRecord => return separator_close,
            _ => {}
        }
    }
    separator_close
}

fn named_spline_scalar_slots(
    name: &str,
    body: &[u8],
    count: usize,
    cache: &scalar::ScalarCache,
) -> Vec<ScalarTokenSlot> {
    let mut slots = Vec::with_capacity(count);
    let mut cursor = psb::Cursor::new(body);
    let mut continued_tuple = false;
    while slots.len() < count {
        if matches!(name, "i_pnts" | "i_points")
            && cursor.take_slice_if(&[psb::token::SCALAR_BODY, 0x00])
        {
            continued_tuple = true;
            continue;
        }
        let start = cursor.pos();
        let Some(value) =
            cursor.take_with(|data, pos| named_spline_scalar_slot(name, data, pos, cache))
        else {
            break;
        };
        slots.push((value, body[start..cursor.pos()].to_vec()));
    }
    if matches!(name, "i_pnts" | "i_points")
        && continued_tuple
        && cursor.pos() == body.len()
        && slots.len() + 1 == count
    {
        slots.push((Some(0.0), Vec::new()));
    }
    slots.resize_with(count, || (None, Vec::new()));
    slots
}

fn counted_parameter_scalar_slots(
    body: &[u8],
    count: usize,
    cache: &scalar::ScalarCache,
) -> Option<Vec<ScalarTokenSlot>> {
    let mut slots = Vec::with_capacity(count);
    let mut cursor = 0;
    while cursor < body.len() && slots.len() < count {
        let run = match body[cursor] {
            0xe5 => 2,
            0xe6 => 3,
            _ => 0,
        };
        if run > 0 {
            (slots.len().checked_add(run)? <= count).then_some(())?;
            slots.push((Some(0.0), vec![body[cursor]]));
            slots.extend(std::iter::repeat_n((Some(0.0), Vec::new()), run - 1));
            cursor += 1;
            continue;
        }
        let start = cursor;
        let (value, next) = named_spline_scalar_slot("params", body, cursor, cache)?;
        (next > cursor).then_some(())?;
        slots.push((value, body[start..next].to_vec()));
        cursor = next;
    }
    (slots.len() == count && cursor == body.len()).then_some(slots)
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

fn plane_envelope_scalar_slots_with_tokens_and_end(
    body: &[u8],
    count: usize,
    cache: &scalar::ScalarCache,
) -> (Vec<ScalarTokenSlot>, usize) {
    let mut slots = Vec::with_capacity(count);
    let mut cursor = 0;
    while cursor < body.len() && slots.len() < count {
        if body[cursor] == 0x0e {
            slots.push((Some(0.5), vec![0x0e]));
            cursor += 1;
        } else if body[cursor] == 0x18 && cursor + 1 == body.len() {
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

fn complete_plane_envelope_slots(
    body: &[u8],
    count: usize,
    cache: &scalar::ScalarCache,
) -> Option<Vec<ScalarTokenSlot>> {
    let (slots, consumed) = plane_envelope_scalar_slots_with_tokens_and_end(body, count, cache);
    (consumed == body.len()
        && slots
            .iter()
            .all(|(value, token)| value.is_some() && !token.is_empty())
        && slots.iter().map(|(_, token)| token.len()).sum::<usize>() == consumed)
        .then_some(slots)
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

fn sequential_named_local_system_slots(
    body: &[u8],
    count: usize,
    cache: &scalar::ScalarCache,
) -> Option<Vec<Option<f64>>> {
    let mut slots = Vec::with_capacity(count);
    let mut cursor = 0;
    while cursor < body.len() && slots.len() < count {
        if body.get(cursor) == Some(&0xe7) {
            let (inherited_count, next) = compact_int(body, cursor + 1);
            let inherited_count = usize::try_from(inherited_count).ok()?;
            (next > cursor + 1
                && inherited_count > 0
                && slots.len().checked_add(inherited_count)? <= count)
                .then_some(())?;
            slots.extend(std::iter::repeat_n(None, inherited_count));
            cursor = next;
            continue;
        }
        if body[cursor] == 0x18 && cursor + 1 == body.len() {
            slots.push(Some(0.0));
            cursor += 1;
            continue;
        }
        if body.get(cursor..cursor + 2) == Some(&[0x18, 0xe5]) {
            (slots.len() + 3 <= count).then_some(())?;
            slots.extend([Some(0.0), Some(1.0), Some(0.0)]);
            cursor += 2;
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
            cursor += 1;
            continue;
        }
        if body.get(cursor) == Some(&0x10) {
            slots.push(Some(0.0));
            cursor += 1;
            continue;
        }
        if let Some((value, next)) =
            scalar::decode_named_local_system_coordinate(body, cursor, slots.len(), cache)
        {
            slots.push(Some(value));
            cursor = next;
        } else {
            return None;
        }
    }
    (cursor == body.len()).then_some(())?;
    slots.resize(count, None);
    Some(slots)
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
    let supports = [first, middle, third];
    let magnitudes = supports.map(|support| {
        support
            .iter()
            .map(|value| value * value)
            .sum::<f64>()
            .sqrt()
    });
    let nonzero = supports
        .into_iter()
        .zip(magnitudes)
        .filter(|(_, magnitude)| *magnitude > 1e-6)
        .collect::<Vec<_>>();
    let [(first, first_magnitude), (second, second_magnitude)] = nonzero.as_slice() else {
        return PlaneFrame {
            origin,
            u_axis: None,
            normal: None,
        };
    };
    if magnitudes
        .into_iter()
        .filter(|magnitude| *magnitude <= 1e-6)
        .any(|magnitude| magnitude > 1e-9)
    {
        return PlaneFrame {
            origin,
            u_axis: None,
            normal: None,
        };
    }
    let scale = first_magnitude.max(*second_magnitude);
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
    let u_axis = (*first_magnitude > 1e-6).then(|| {
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
    if let Some(prefix) = frame_body.strip_suffix(&[0x00, 0x0c, 0x98]) {
        let mut normalized = Vec::with_capacity(prefix.len() + 1);
        normalized.extend_from_slice(prefix);
        normalized.push(0x0f);
        return scalar::decode_plane_support_local_system_slots(&normalized, cache);
    }
    scalar::decode_plane_support_local_system_slots(frame_body, cache)
}

/// Decode the e3-bounded local-system chunk following each plane envelope.
pub fn plane_local_systems(payload: &[u8]) -> Vec<PlaneLocalSystem> {
    plane_local_systems_for_rows(payload, &rows(payload))
}

/// Decode plane local-system chunks from a DEPDB cross-section namespace.
#[must_use]
pub fn cross_section_plane_local_systems(payload: &[u8]) -> Vec<PlaneLocalSystem> {
    plane_local_systems_for_rows(payload, &cross_section_rows(payload))
}

fn plane_local_systems_for_rows(payload: &[u8], rows: &[SurfaceRow]) -> Vec<PlaneLocalSystem> {
    let cache = scalar::ScalarCache::from_section(payload);
    let headers = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row.kind == SurfaceKind::Plane)
        .filter_map(|(index, row)| {
            positional_body_start(payload, row).map(|body_start| {
                let row_end = rows
                    .get(index + 1)
                    .map_or(payload.len(), |next| next.offset);
                (row, body_start, row_end)
            })
        })
        .collect::<Vec<_>>();
    let mut systems = Vec::new();
    for (row, envelope_start, row_end) in headers {
        let Some(envelope_close) = first_compound_close(payload, envelope_start, row_end) else {
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
        .enumerate()
        .filter(|(_, row)| row.kind == SurfaceKind::Plane)
        .filter_map(|(index, row)| {
            positional_body_start(payload, row).map(|body_start| {
                let row_end = all_rows
                    .get(index + 1)
                    .map_or(payload.len(), |next| next.offset);
                (row, body_start, row_end)
            })
        })
        .collect::<Vec<_>>();
    let mut envelopes = Vec::new();
    for (row, body_start, row_end) in headers {
        let Some(body) = payload.get(body_start..row_end) else {
            continue;
        };
        let Some(body_end) = surface_body_compound_close(SurfaceKind::Plane, body, &cache)
            .map(|relative| body_start + relative)
        else {
            continue;
        };
        let body = payload[body_start..body_end].to_vec();
        let scalar_tokens;
        let (envelope, corner_coordinate_equal) = if body.first() == Some(&0x0e) {
            let Some(slots) = complete_plane_envelope_slots(&body[1..], 9, &cache) else {
                continue;
            };
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
            let Some(slots) = complete_plane_envelope_slots(&body, 10, &cache) else {
                continue;
            };
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
            offset: body_start,
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
            .map_or(row_end, |relative| {
                let prototype = row.offset + relative;
                prototype
                    .checked_sub(2)
                    .filter(|start| {
                        payload.get(*start..prototype) == Some(&[psb::token::NAMED_RECORD, 0x00])
                    })
                    .unwrap_or(prototype)
            });
        let Some(relative) = payload[row.offset..named_end]
            .windows(NAMED_OUTLINE.len())
            .position(|window| window == NAMED_OUTLINE)
        else {
            continue;
        };
        let outline = row.offset + relative;
        let scalar_start = outline + NAMED_OUTLINE.len();
        let field_end = named_record_boundary(
            SurfaceKind::Plane,
            &payload[scalar_start..named_end],
            &cache,
        )
        .map_or(named_end, |relative| scalar_start + relative);
        let (slots, consumed) =
            scalar_slots_with_tokens_and_end(&payload[scalar_start..field_end], 6, &cache);
        if slots
            .iter()
            .any(|slot| slot.0.is_none() || slot.1.is_empty())
            || consumed != field_end - scalar_start
            || slots.iter().map(|slot| slot.1.len()).sum::<usize>() != consumed
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
    if complete_plane_envelope_slots(body, 10, cache).is_some() {
        return None;
    }
    let tokens = scalar_tokens(SurfaceKind::Plane, body, cache);
    let frames = scalar_frames(&tokens);
    let frame = terminal_scalar_frame(body, &frames)?;
    (frame.offset > 0 && frame.slots.len() == 9).then_some(())?;
    let slots = complete_plane_envelope_slots(&body[frame.offset..], 9, cache)?;
    slots
        .into_iter()
        .map(|(value, raw)| Some((Some(value?), raw)))
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
    fn plane_records_end_at_the_next_surface_family() {
        let payload = [
            7, 0x22, 4, 0x01, 0, 0, // plane row
            0xe4, 0xe4, 0xe4, 0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0xe4, 0xe3, // envelope
            8, 0x24, 4, 0x01, 0, 0, // cylinder row
            0x18, 0xe4, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x18, 0xe6, 0xe3,
        ];
        let rows = rows(&payload);

        assert!(matches!(
            rows.as_slice(),
            [
                SurfaceRow {
                    kind: SurfaceKind::Plane,
                    ..
                },
                SurfaceRow {
                    kind: SurfaceKind::Cylinder,
                    ..
                }
            ]
        ));
        assert_eq!(plane_envelopes_for_rows(&payload, &rows).len(), 1);
        assert!(plane_local_systems_for_rows(&payload, &rows).is_empty());
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
    fn counted_parameters_expand_compact_zero_runs() {
        let body = [0xe4, 0xe5, 0x0f, 0xe6];

        assert_eq!(
            counted_parameter_scalar_slots(&body, 7, &scalar::ScalarCache::default()),
            Some(vec![
                (Some(1.0), vec![0xe4]),
                (Some(0.0), vec![0xe5]),
                (Some(0.0), vec![]),
                (Some(0.0), vec![0x0f]),
                (Some(0.0), vec![0xe6]),
                (Some(0.0), vec![]),
                (Some(0.0), vec![]),
            ])
        );
    }

    #[test]
    fn counted_parameters_require_exact_zero_run_cardinality() {
        let body = [0xe4, 0xe5, 0x0f, 0xe6];

        assert_eq!(
            counted_parameter_scalar_slots(&body, 6, &scalar::ScalarCache::default()),
            None
        );
        assert_eq!(
            counted_parameter_scalar_slots(&body, 8, &scalar::ScalarCache::default()),
            None
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
    fn tabulated_cylinder_zero_sweep_bound_does_not_consume_the_next_slot() {
        let body = [
            0x18, 0xe4, 0x0f, 0x00, 0x0c, 0x9a, 0x46, 0x15, 0x64, 0x7b, 0x0d, 0xc3, 0x21, 0xe2,
            0x42, 0xb9, 0x99, 0x78, 0x6b, 0xf6, 0xdd, 0x26, 0xcc, 0x10, 0x4a, 0x14, 0x70, 0xf7,
            0x8b, 0x00, 0x00, 0x18, 0x7b, 0x59, 0x2f, 0x66, 0xa2, 0x53, 0xc6,
        ];

        let (frame, end) = decode_tabulated_cylinder_frame(&body, &scalar::ScalarCache::default())
            .expect("complete zero-bound frame");

        assert_eq!(frame.prefixes, [0x46, 0x42, 0x78, 0x4a, 0x18, 0x7b]);
        assert_eq!(frame.values[4], 0.0);
        assert_eq!(end, body.len());
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
        assert!((frame.origin[1] + 13.769_563_324_412_964).abs() < 1e-12);
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

        let signed_zero_support = [
            17, 72, 32, 0, 19, 24, 47, 39, 128, 72, 16, 0, 67, 232, 0, 47, 42, 128, 47, 16, 0, 25,
            154, 121, 57, 76, 158, 138, 10, 247, 25, 227, 16, 24, 230, 15, 24, 15, 24, 47, 41, 0,
            47, 16, 0, 24, 42, 232, 0,
        ];
        let frame =
            decode_positional_cylinder_frame(&signed_zero_support, &scalar::ScalarCache::default())
                .expect("complete signed zero-support positional cylinder");
        assert_eq!(frame.origin, [12.5, 4.0, 0.0]);
        assert_eq!(frame.axis, [0.0, -1.0, 0.0]);
        assert_eq!(frame.ref_direction, [-1.0, 0.0, 0.0]);
        assert_eq!(frame.radius, 0.75);
        assert_eq!(frame.length, Some(8.0));

        let mut inconsistent_signed_length = signed_zero_support;
        inconsistent_signed_length[1..4].copy_from_slice(&[72, 33, 0]);
        assert!(decode_positional_cylinder_frame(
            &inconsistent_signed_length,
            &scalar::ScalarCache::default()
        )
        .is_none());

        let mut inconsistent_signed_origin = signed_zero_support;
        inconsistent_signed_origin[39..42].copy_from_slice(&[47, 40, 0]);
        assert!(decode_positional_cylinder_frame(
            &inconsistent_signed_origin,
            &scalar::ScalarCache::default()
        )
        .is_none());

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
        assert!((frame.length.expect("axial extent") - 6.528_189_135_889_739).abs() < 1e-12);

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
        assert!((frame.length.expect("axial extent") - 6.527_254_503_477_945).abs() < 1e-12);

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
    fn positional_cylinder_frame_decodes_signed_radial_envelopes() {
        let cache = scalar::ScalarCache::default();
        let outer_left = [
            17, 72, 40, 0, 19, 72, 33, 0, 72, 49, 0, 47, 54, 0, 47, 4, 0, 72, 42, 0, 47, 56, 0, 47,
            24, 0, 247, 25,
        ];
        assert_eq!(
            decode_positional_cylinder_frame(&outer_left, &cache),
            Some(PositionalCylinderFrame {
                origin: [-15.0, 24.0, 6.0],
                axis: [0.0, 0.0, -1.0],
                ref_direction: [-1.0, 0.0, 0.0],
                radius: 2.0,
                length: Some(12.0),
            })
        );

        let middle_left = [
            17, 72, 33, 0, 19, 24, 72, 50, 128, 47, 52, 128, 71, 23, 255, 72, 39, 0, 47, 56, 0, 47,
            4, 0, 247, 25,
        ];
        assert_eq!(
            decode_positional_cylinder_frame(&middle_left, &cache),
            Some(PositionalCylinderFrame {
                origin: [-15.0, 24.0, 2.5],
                axis: [0.0, 0.0, -1.0],
                ref_direction: [-1.0, 0.0, 0.0],
                radius: 3.5,
                length: Some(8.5),
            })
        );

        let outer_right = [
            17, 47, 33, 0, 19, 47, 40, 0, 47, 42, 0, 47, 54, 0, 47, 4, 0, 47, 49, 0, 47, 56, 0, 47,
            24, 0,
        ];
        assert_eq!(
            decode_positional_cylinder_frame(&outer_right, &cache),
            Some(PositionalCylinderFrame {
                origin: [15.0, 24.0, -6.0],
                axis: [0.0, 0.0, 1.0],
                ref_direction: [1.0, 0.0, 0.0],
                radius: 2.0,
                length: Some(12.0),
            })
        );

        assert!(
            decode_positional_cylinder_frame(&outer_left[..outer_left.len() - 2], &cache).is_none()
        );
        let mut inconsistent_radius = outer_right;
        inconsistent_radius[17..20].copy_from_slice(&[47, 50, 0]);
        assert!(decode_positional_cylinder_frame(&inconsistent_radius, &cache).is_none());
    }

    #[test]
    fn positional_cylinder_frame_decodes_signed_axis_aligned_envelopes() {
        let cache = scalar::ScalarCache::default();
        let forward = [
            17, 72, 0, 0, 19, 24, 72, 55, 192, 70, 29, 255, 255, 255, 255, 255, 143, 72, 38, 0, 72,
            52, 64, 70, 21, 255, 255, 255, 255, 255, 143, 72, 34, 128,
        ];
        assert_eq!(
            decode_positional_cylinder_frame(&forward, &cache),
            Some(PositionalCylinderFrame {
                origin: [-22.0, 5.499_999_999_999_9, -9.25],
                axis: [0.0, 1.0, 0.0],
                ref_direction: [-1.0, 0.0, 0.0],
                radius: 1.75,
                length: Some(2.0),
            })
        );

        let reversed = [
            17, 72, 0, 0, 19, 24, 47, 52, 64, 70, 29, 255, 255, 255, 255, 255, 143, 72, 38, 0, 47,
            55, 192, 70, 21, 255, 255, 255, 255, 255, 143, 72, 34, 128, 247, 23,
        ];
        assert_eq!(
            decode_positional_cylinder_frame(&reversed, &cache),
            Some(PositionalCylinderFrame {
                origin: [22.0, 7.499_999_999_999_9, -9.25],
                axis: [0.0, -1.0, 0.0],
                ref_direction: [1.0, 0.0, 0.0],
                radius: 1.75,
                length: Some(2.0),
            })
        );

        let mut ambiguous_axis = forward;
        ambiguous_axis[20..23].copy_from_slice(&[72, 54, 0]);
        assert!(decode_positional_cylinder_frame(&ambiguous_axis, &cache).is_none());
        assert!(
            decode_positional_cylinder_frame(&reversed[..reversed.len() - 1], &cache).is_none()
        );
    }

    #[test]
    fn positional_cylinder_frame_decodes_xz_axis_y_radial_envelopes() {
        let cache = scalar::ScalarCache::default();
        let macro_zero = [
            32, 16, 0, 45, 48, 95, 210, 181, 75, 36, 250, 142, 178, 2, 128, 130, 232, 214, 45, 53,
            164, 168, 193, 84, 201, 136, 45, 32, 56, 227, 142, 56, 227, 144, 45, 66, 106, 9, 230,
            103, 243, 189, 52, 240, 0, 47, 34, 0, 45, 66, 170, 9, 230, 103, 243, 189, 160, 19, 88,
            48, 38, 146, 52,
        ];
        let compact_zero = [
            32, 16, 0, 45, 53, 164, 168, 193, 84, 201, 135, 142, 178, 2, 128, 130, 232, 193, 45,
            58, 233, 127, 49, 250, 214, 118, 72, 34, 0, 45, 66, 106, 9, 230, 103, 243, 190, 24, 70,
            32, 56, 227, 142, 56, 227, 144, 45, 66, 170, 9, 230, 103, 243, 189, 160, 19, 89, 194,
            152, 51, 188,
        ];
        for body in [macro_zero.as_slice(), compact_zero.as_slice()] {
            let frame = decode_positional_cylinder_frame(body, &cache)
                .expect("complete XZ-axis cylinder frame");
            assert!((frame.radius - 0.25).abs() < 1e-12);
            assert!(frame.axis[1].abs() < 1e-12);
            assert_eq!(frame.ref_direction, [0.0, -1.0, 0.0]);
            assert!(frame.length.is_some_and(|length| length > 17.0));
        }

        let mut inconsistent = compact_zero;
        inconsistent[54] = 0x18;
        assert!(decode_positional_cylinder_frame(&inconsistent, &cache).is_none());
    }

    #[test]
    fn positional_cylinder_frame_decodes_symmetric_revolution_envelopes() {
        let cache = scalar::ScalarCache::default();
        let direct = [
            21, 45, 35, 122, 225, 71, 174, 20, 124, 24, 45, 36, 28, 61, 7, 246, 190, 79, 71, 27,
            153, 70, 36, 28, 61, 7, 246, 190, 79, 24, 46, 27, 153, 70, 35, 122, 225, 71, 174, 20,
            124, 46, 27, 153, 247, 25,
        ];
        let replay = [
            23, 45, 35, 122, 225, 71, 174, 20, 124, 21, 45, 36, 28, 61, 7, 246, 190, 79, 71, 27,
            153, 70, 36, 28, 61, 7, 246, 190, 79, 71, 27, 153, 46, 27, 153, 70, 35, 122, 225, 71,
            174, 20, 124, 25, 206, 113, 206, 177, 182, 81, 242, 247, 25,
        ];
        for body in [direct.as_slice(), replay.as_slice()] {
            let frame = decode_positional_cylinder_frame(body, &cache)
                .expect("complete symmetric-revolution cylinder");
            assert_eq!(frame.origin, [0.0, 0.0, 0.0]);
            assert_eq!(frame.axis, [0.0, -1.0, 0.0]);
            assert_eq!(frame.ref_direction, [-1.0, 0.0, 0.0]);
            assert!((frame.radius - 6.9).abs() < 1e-12);
            assert!(frame
                .length
                .is_some_and(|length| (length - 19.48).abs() < 1e-12));
        }

        let mut mismatched_repetition = replay;
        mismatched_repetition[31..34].copy_from_slice(&[0x2e, 0x1b, 0x99]);
        assert!(decode_positional_cylinder_frame(&mismatched_repetition, &cache).is_none());
        let mut trailing = direct.to_vec();
        trailing.push(0x18);
        assert!(decode_positional_cylinder_frame(&trailing, &cache).is_none());
    }

    #[test]
    fn positional_cylinder_frame_decodes_axial_endpoint_radial_samples() {
        let cache = scalar::ScalarCache::default();
        let radius_three_and_half = [
            143, 30, 205, 113, 196, 112, 70, 24, 153, 33, 34, 156, 96, 224, 107, 14, 145, 174, 119,
            80, 63, 61, 215, 47, 49, 128, 210, 95, 146, 245, 61, 0, 232, 47, 12, 0, 47, 50, 0, 139,
            106, 254, 253, 38, 131, 216, 247, 25,
        ];
        let radius_three = [
            143, 30, 205, 113, 196, 112, 70, 24, 153, 33, 34, 156, 96, 224, 108, 14, 142, 112, 248,
            141, 237, 16, 111, 47, 49, 128, 207, 17, 142, 54, 177, 184, 109, 47, 8, 0, 47, 50, 0,
            135, 37, 34, 214, 139, 43, 42, 247, 25,
        ];
        for (body, expected_radius) in [
            (radius_three_and_half.as_slice(), 3.5),
            (radius_three.as_slice(), 3.0),
        ] {
            let frame = decode_positional_cylinder_frame(body, &cache)
                .expect("complete axial-endpoint radial-sample cylinder");
            assert_eq!(frame.origin, [0.0, 17.5, 0.0]);
            assert_eq!(frame.axis, [0.0, 1.0, 0.0]);
            assert_eq!(frame.ref_direction, [-1.0, 0.0, 0.0]);
            assert!((frame.radius - expected_radius).abs() < 1e-12);
            assert!(frame
                .length
                .is_some_and(|length| (length - 0.5).abs() < 1e-12));
        }

        let mut off_circle = radius_three;
        off_circle[33..36].copy_from_slice(&[0x2f, 0x0a, 0x00]);
        assert!(decode_positional_cylinder_frame(&off_circle, &cache).is_none());
        let mut trailing = radius_three_and_half.to_vec();
        trailing.push(0x18);
        assert!(decode_positional_cylinder_frame(&trailing, &cache).is_none());
    }

    #[test]
    fn positional_cylinder_frame_decodes_signed_axial_radial_envelopes() {
        let cache = scalar::ScalarCache::default();
        let positive_end = [
            17, 66, 201, 153, 19, 24, 46, 61, 204, 72, 22, 0, 228, 47, 62, 0, 25, 200, 68, 116,
            134, 59, 254, 138, 47, 22, 0, 247, 23,
        ];
        assert_eq!(
            decode_positional_cylinder_frame(&positive_end, &cache),
            Some(PositionalCylinderFrame {
                origin: [30.0, 0.0, 5.5],
                axis: [-1.0, 0.0, 0.0],
                ref_direction: [0.0, 0.0, 1.0],
                radius: 11.0,
                length: Some(0.199_999_999_999_999_98),
            })
        );

        let negative_end = [
            17, 66, 201, 153, 19, 24, 72, 62, 0, 72, 22, 0, 228, 71, 61, 204, 25, 210, 51, 87, 100,
            172, 254, 232, 47, 22, 0, 247, 23,
        ];
        assert_eq!(
            decode_positional_cylinder_frame(&negative_end, &cache),
            Some(PositionalCylinderFrame {
                origin: [-29.799_999_999_999_997, 0.0, 5.5],
                axis: [-1.0, 0.0, 0.0],
                ref_direction: [0.0, 0.0, 1.0],
                radius: 11.0,
                length: Some(0.199_999_999_999_999_98),
            })
        );

        let mut wrong_separator = positive_end;
        wrong_separator[12] = 0x10;
        assert!(decode_positional_cylinder_frame(&wrong_separator, &cache).is_none());
        assert!(
            decode_positional_cylinder_frame(&negative_end[..negative_end.len() - 2], &cache)
                .is_none()
        );
    }

    #[test]
    fn positional_cylinder_frame_decodes_precise_center_edge_envelope() {
        let body = [
            24, 44, 139, 97, 240, 181, 224, 8, 18, 45, 62, 3, 108, 62, 22, 188, 4, 72, 36, 0, 46,
            31, 255, 47, 20, 0, 72, 34, 0, 47, 67, 0, 47, 24, 0, 247, 25,
        ];
        let frame = decode_positional_cylinder_frame(&body, &scalar::ScalarCache::default())
            .expect("complete precise center-edge envelope");
        assert_eq!(frame.origin[0], -10.0);
        assert!((frame.origin[1] - 7.986_629_6).abs() < 1e-12);
        assert_eq!(frame.origin[2], 5.0);
        assert_eq!(frame.axis, [0.0, 1.0, 0.0]);
        assert_eq!(frame.ref_direction, [0.0, 0.0, 1.0]);
        assert_eq!(frame.radius, 1.0);
        assert!((frame.length.expect("axial extent") - 30.013_370_4).abs() < 1e-12);

        let mut unequal_radial_spans = body;
        unequal_radial_spans[32..35].copy_from_slice(&[47, 28, 0]);
        assert!(decode_positional_cylinder_frame(
            &unequal_radial_spans,
            &scalar::ScalarCache::default()
        )
        .is_none());

        let mut inconsistent_precise_origin = body;
        inconsistent_precise_origin[20..23].copy_from_slice(&[47, 52, 0]);
        assert!(decode_positional_cylinder_frame(
            &inconsistent_precise_origin,
            &scalar::ScalarCache::default()
        )
        .is_none());
    }

    #[test]
    fn positional_cylinder_frame_decodes_precise_held_center_envelope() {
        let body = [
            24, 40, 150, 94, 43, 46, 129, 244, 134, 18, 45, 44, 11, 47, 21, 151, 64, 252, 72, 28,
            0, 47, 20, 0, 228, 47, 28, 0, 47, 24, 0, 228, 247, 25,
        ];
        let frame = decode_positional_cylinder_frame(&body, &scalar::ScalarCache::default())
            .expect("complete precise held-center envelope");
        assert!((frame.origin[0] - 7.021_843_6).abs() < 1e-12);
        assert_eq!(frame.origin[1..], [5.0, 5.0]);
        assert_eq!(frame.axis, [-1.0, 0.0, 0.0]);
        assert_eq!(frame.ref_direction, [0.0, 0.0, 1.0]);
        assert_eq!(frame.radius, 1.0);
        assert!((frame.length.expect("axial extent") - 14.021_843_6).abs() < 1e-12);

        let mut unequal_radius_markers = body;
        unequal_radius_markers[31] = 0xe8;
        assert!(decode_positional_cylinder_frame(
            &unequal_radius_markers,
            &scalar::ScalarCache::default()
        )
        .is_none());

        let mut inconsistent_radial_edge = body;
        inconsistent_radial_edge[28..31].copy_from_slice(&[47, 28, 0]);
        assert!(decode_positional_cylinder_frame(
            &inconsistent_radial_edge,
            &scalar::ScalarCache::default()
        )
        .is_none());
    }

    #[test]
    fn positional_cylinder_frame_decodes_local_system_suffix() {
        let body = [
            90, 178, 14, 217, 114, 169, 0, 45, 53, 168, 169, 253, 44, 199, 226, 120, 172, 103, 5,
            97, 187, 80, 45, 58, 197, 27, 196, 73, 57, 170, 47, 28, 0, 47, 65, 0, 24, 45, 32, 56,
            227, 142, 56, 227, 142, 45, 66, 146, 67, 227, 143, 242, 96, 159, 113, 199, 28, 113,
            199, 32, 227, 66, 227, 51, 66, 233, 153, 24, 41, 233, 153, 66, 227, 51, 24, 229, 15,
            47, 40, 0, 47, 65, 0, 70, 53, 168, 169, 253, 44, 199, 226, 47, 20, 0,
        ];
        let frame = decode_positional_cylinder_frame(&body, &scalar::ScalarCache::default())
            .expect("complete local-system suffix");
        assert_eq!(frame.origin[0..2], [12.0, 34.0]);
        assert!((frame.origin[2] + 21.658_843_825_753_03).abs() < 1e-12);
        assert_eq!(frame.axis, [0.0, 0.0, 1.0]);
        assert!((frame.ref_direction[0] + 0.6).abs() < 1e-12);
        assert!((frame.ref_direction[1] + 0.8).abs() < 1e-12);
        assert_eq!(frame.ref_direction[2], 0.0);
        assert_eq!(frame.radius, 5.0);
        assert_eq!(frame.length, None);
        let mut payload = vec![7, 0x24, 4, 0x01, 0, 0];
        payload.extend_from_slice(&body);
        payload.push(0xe3);
        assert_eq!(
            parameter_records(&payload)[0].type24_round_radius(0x24),
            None
        );

        assert!(decode_positional_cylinder_frame(
            &body[..body.len() - 3],
            &scalar::ScalarCache::default()
        )
        .is_none());
        let mut nonorthogonal = body;
        nonorthogonal[68..71].copy_from_slice(&[41, 227, 51]);
        assert!(
            decode_positional_cylinder_frame(&nonorthogonal, &scalar::ScalarCache::default())
                .is_none()
        );
    }

    #[test]
    fn split_cylinder_outline_requires_the_exact_terminal_layout() {
        let body = [1, 2, 0x00, 0x0c, 0x98, 3, 4, 0x0d];
        let slots = [
            (-0.3125, 0, vec![1]),
            (1.3125, 1, vec![2]),
            (0.3125, 5, vec![3]),
            (1.625, 6, vec![4]),
            (-1.0, 7, vec![0x0d]),
        ]
        .into_iter()
        .map(|(value, offset, raw)| SurfaceParameterScalar {
            value: Some(value),
            raw,
            offset,
            length: 1,
        })
        .collect::<Vec<_>>();
        assert_eq!(
            split_cylinder_outline_bounds(&body, &slots),
            Some([[-0.3125, 1.3125], [0.3125, 1.625]])
        );

        let mut wrong_orientation = slots.clone();
        wrong_orientation[4].value = Some(1.0);
        assert!(split_cylinder_outline_bounds(&body, &wrong_orientation).is_none());
        let mut wrong_separator = body;
        wrong_separator[4] = 0x99;
        assert!(split_cylinder_outline_bounds(&wrong_separator, &slots).is_none());
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
    fn decodes_terminal_and_control_split_type26_five_coordinate_envelopes() {
        let bodies = [
            vec![
                0xcc, 0x4e, 0xb7, 0xaa, 0xa1, 0x3a, 0x60, 0x12, 0x41, 0x86, 0x5e, 0x2b, 0x2e, 0x79,
                0xa2, 0x91, 0x11, 0x2d, 0x1b, 0xff, 0xff, 0xff, 0xff, 0xf8, 0xf6, 0x2f, 0x14, 0x00,
                0x2f, 0x14, 0x00, 0x2f, 0x24, 0x00, 0x2d, 0x20, 0x00, 0x00, 0x00, 0x00, 0x06, 0x3c,
                0x2f, 0x18, 0x00, 0xf7, 0x1c,
            ],
            vec![
                0x28, 0x7f, 0x7d, 0xdf, 0x28, 0xe6, 0x8d, 0xaf, 0x15, 0x84, 0x41, 0x79, 0x33, 0x6d,
                0x2d, 0xaa, 0x16, 0x48, 0x24, 0x00, 0x2f, 0x14, 0x00, 0xe4, 0x4a, 0x1b, 0xff, 0xff,
                0xff, 0xff, 0xf9, 0x2d, 0x20, 0x00, 0x00, 0x00, 0x00, 0x06, 0x41, 0x2f, 0x18, 0x00,
                0xf7, 0x1c,
            ],
        ];
        let expected = [[5.0, 5.0, 10.0, -8.0, 6.0], [-10.0, 5.0, 1.0, -8.0, 6.0]];
        for (body, expected) in bodies.into_iter().zip(expected) {
            let mut payload = vec![7, 0x26, 4, 0x01, 0, 0];
            payload.extend_from_slice(&body);
            payload.push(0xe3);
            let records = parameter_records(&payload);
            let envelope = records[0]
                .type26_five_coordinate_envelope(0x26)
                .expect("terminal five-coordinate envelope");
            for (actual, expected) in envelope.values.into_iter().zip(expected) {
                assert!((actual - expected).abs() < 1e-11);
            }
        }
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
    fn decodes_complete_positional_torus_frame() {
        let body = [
            40, 141, 7, 27, 210, 101, 111, 108, 24, 148, 63, 2, 112, 22, 190, 252, 0, 18, 32, 71,
            19, 204, 70, 49, 61, 112, 163, 215, 10, 62, 71, 19, 204, 46, 19, 204, 70, 48, 189, 112,
            163, 215, 10, 62, 33, 177, 72, 10, 227, 194, 255, 45, 89, 199, 15, 241, 65, 141, 6,
            220, 32, 138, 77, 219, 24, 229, 16, 40, 141, 6, 220, 32, 138, 77, 219, 194, 255, 45,
            89, 199, 15, 241, 24, 228, 70, 48, 189, 112, 163, 215, 10, 62, 24, 46, 17, 204, 14,
        ];
        let mut payload = vec![7, 0x26, 4, 0x01, 0, 0];
        payload.extend(body);
        payload.push(0xe3);
        let record = parameter_records(&payload).remove(0);

        let frame = record
            .positional_torus_frame
            .expect("complete positional torus frame");
        assert!(frame
            .center
            .into_iter()
            .zip([1.0, 16.74, 0.0])
            .all(|(actual, expected)| (actual - expected).abs() < 1e-12));
        assert!(frame
            .axis
            .into_iter()
            .zip([0.0, 0.0, 1.0])
            .all(|(actual, expected)| (actual - expected).abs() < 1e-12));
        assert!(frame
            .ref_direction
            .into_iter()
            .zip([-0.999_899_554_583_406_1, 0.014_173_240_416_574_131, 0.0])
            .all(|(actual, expected)| (actual - expected).abs() < 1e-12));
        assert!((frame.major_radius - 4.45).abs() < 1e-12);
        assert!((frame.minor_radius - 0.5).abs() < 1e-12);

        payload[55] = 0x20;
        assert!(parameter_records(&payload)[0]
            .positional_torus_frame
            .is_none());
        payload[55] = body[49];
        payload[102] = 0x0d;
        assert!(parameter_records(&payload)[0]
            .positional_torus_frame
            .is_none());
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

        let panel = record(&panel);
        assert!(
            (panel.type24_round_radius(0x24).expect("required invariant") - 0.2).abs() < 1.0e-12
        );
        let frame = panel
            .positional_cylinder_frame
            .expect("complete repeated-diameter carrier");
        assert_eq!(frame.origin, [2.2, -22.35, -1.45]);
        assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
        assert!((frame.radius - 0.2).abs() < 1.0e-12);
        assert!(
            (frame.length.expect("required invariant") - 46.241_026_156_433_854).abs() < 1.0e-12
        );
        assert!(
            (frame.axis[1] - 46.15 / frame.length.expect("required invariant")).abs() < 1.0e-12
        );
        assert!((frame.axis[2] - 2.9 / frame.length.expect("required invariant")).abs() < 1.0e-12);
        assert!(
            (record(&prefixed_panel)
                .type24_round_radius(0x24)
                .expect("required invariant")
                - 0.2)
                .abs()
                < 1.0e-12
        );
        assert!(
            (record(&separated)
                .type24_round_radius(0x24)
                .expect("required invariant")
                - 2.0)
                .abs()
                < 1.0e-12
        );
        let replay_separated = [
            24, 45, 82, 36, 168, 193, 84, 201, 135, 18, 45, 89, 164, 168, 193, 84, 201, 135, 47,
            34, 0, 47, 32, 0, 47, 20, 0, 47, 36, 0, 47, 67, 0, 47, 24, 0, 247, 24,
        ];
        let replay_frame = record(&replay_separated)
            .positional_cylinder_frame
            .expect("replay-trailed repeated-diameter carrier");
        assert_eq!(
            replay_frame,
            PositionalCylinderFrame {
                origin: [9.0, 23.0, 5.0],
                axis: [1.0 / 2.0_f64.sqrt(), 0.0, 1.0 / 2.0_f64.sqrt()],
                ref_direction: [0.0, 1.0, 0.0],
                radius: 15.0,
                length: Some(2.0_f64.sqrt()),
            }
        );
        let compact_controls = [
            0x12, 0x2d, 0x40, 0x7a, 0x35, 0xc4, 0x3e, 0x21, 0x5b, 0x11, 0x2d, 0x44, 0xff, 0xd2,
            0xa6, 0xae, 0x74, 0x2b, 0x46, 0x65, 0x3f, 0xff, 0xff, 0xff, 0xff, 0xfc, 0x2d, 0x51,
            0xd2, 0x31, 0x1a, 0xfa, 0xb7, 0x82, 0x48, 0x28, 0x00, 0x46, 0x64, 0x1f, 0xff, 0xff,
            0xff, 0xff, 0xfc, 0x2d, 0x54, 0x14, 0xff, 0x8c, 0x32, 0xe0, 0xea, 0x48, 0x08, 0x00,
        ];
        let compact_record = record(&compact_controls);
        let compact_frame = compact_record
            .positional_cylinder_frame
            .expect("compact-control repeated-diameter carrier");
        assert!((compact_frame.radius - 4.521_925_117_895_819).abs() < 1e-12);
        assert!(compact_frame
            .axis
            .into_iter()
            .zip([
                -std::f64::consts::FRAC_1_SQRT_2,
                0.0,
                std::f64::consts::FRAC_1_SQRT_2
            ])
            .all(|(actual, expected)| (actual - expected).abs() < 1e-12));
        assert_eq!(compact_frame.ref_direction, [0.0, -1.0, 0.0]);
        let mut referenced_controls = compact_controls.to_vec();
        referenced_controls.extend_from_slice(&[0xf7, 0x40]);
        assert!(record(&referenced_controls)
            .positional_cylinder_frame
            .is_some());
        let mut invalid_control = compact_controls;
        invalid_control[0] = 0x15;
        assert!(record(&invalid_control).positional_cylinder_frame.is_none());
        invalid_control = compact_controls;
        invalid_control[9] = 0x15;
        assert!(record(&invalid_control).positional_cylinder_frame.is_none());
        let prefixed_auxiliary = [
            0x19, 0xd3, 0xae, 0x70, 0x14, 0x6d, 0xb6, 0xde, 0x2d, 0x4b, 0xc1, 0x0d, 0x60, 0xad,
            0x2a, 0x4e, 0x12, 0x2d, 0x4f, 0x01, 0x49, 0xdf, 0x84, 0xdb, 0x36, 0x48, 0x58, 0xc0,
            0x2d, 0x57, 0x75, 0x9c, 0xe9, 0x32, 0x3b, 0xfb, 0x48, 0x24, 0x00, 0x48, 0x57, 0x00,
            0x2d, 0x59, 0x15, 0xbb, 0x28, 0x9e, 0x14, 0x6f, 0x48, 0x08, 0x00, 0xf7, 0x40,
        ];
        let prefixed_frame = record(&prefixed_auxiliary)
            .positional_cylinder_frame
            .expect("selector-prefixed auxiliary repeated-diameter carrier");
        assert!((prefixed_frame.radius - 3.250_923_087_748_478).abs() < 1e-12);
        assert_eq!(prefixed_frame.ref_direction, [0.0, -1.0, 0.0]);
        assert!(prefixed_frame
            .axis
            .into_iter()
            .zip([
                std::f64::consts::FRAC_1_SQRT_2,
                0.0,
                std::f64::consts::FRAC_1_SQRT_2
            ])
            .all(|(actual, expected)| (actual - expected).abs() < 1e-12));
        let mut alternate_selector = prefixed_auxiliary;
        alternate_selector[0] = 0x32;
        assert!(record(&alternate_selector)
            .positional_cylinder_frame
            .is_some());
        let mut invalid_selector = prefixed_auxiliary;
        invalid_selector[0] = 0x18;
        assert!(record(&invalid_selector)
            .positional_cylinder_frame
            .is_none());
        let split_controls = [
            0x14, 0x2d, 0x4b, 0xc1, 0x0d, 0x60, 0xad, 0x2a, 0x4f, 0x00, 0x13, 0x1a, 0x2d, 0x4f,
            0x01, 0x49, 0xdf, 0x84, 0xdb, 0x35, 0x48, 0x58, 0xc0, 0x2d, 0x57, 0x75, 0x9c, 0xe9,
            0x32, 0x3b, 0xfc, 0x92, 0xff, 0xff, 0xff, 0xff, 0xff, 0xe8, 0x48, 0x57, 0x00, 0x2d,
            0x59, 0x15, 0xbb, 0x28, 0x9e, 0x14, 0x6e, 0x2f, 0x24, 0x00, 0xf7, 0x40,
        ];
        let split_frame = record(&split_controls)
            .positional_cylinder_frame
            .expect("split-control repeated-diameter carrier");
        assert!((split_frame.radius - 3.250_923_087_748_47).abs() < 1e-12);
        assert_eq!(split_frame.ref_direction, [0.0, -1.0, 0.0]);
        let mut invalid_split_controls = split_controls;
        invalid_split_controls[10] = 0x14;
        assert!(record(&invalid_split_controls)
            .positional_cylinder_frame
            .is_none());
        let prefixed_split_controls = [
            0x00, 0x11, 0x13, 0x2d, 0x41, 0x83, 0x08, 0x72, 0x35, 0x71, 0xa6, 0x14, 0x2d, 0x44,
            0xff, 0xd2, 0xa6, 0xae, 0x74, 0x27, 0x46, 0x64, 0x9f, 0xff, 0xff, 0xff, 0xff, 0xfc,
            0x2d, 0x52, 0x56, 0x9a, 0x71, 0xf6, 0x5f, 0xa7, 0x92, 0xff, 0xff, 0xff, 0xff, 0xff,
            0xeb, 0x46, 0x64, 0x1f, 0xff, 0xff, 0xff, 0xff, 0xfc, 0x2d, 0x54, 0x14, 0xff, 0x8c,
            0x32, 0xe0, 0xe8, 0x2f, 0x1c, 0x00, 0xf7, 0x40,
        ];
        assert!(record(&prefixed_split_controls)
            .positional_cylinder_frame
            .is_some());
        let mut invalid_prefix = prefixed_split_controls;
        invalid_prefix[2] = 0x12;
        assert!(record(&invalid_prefix).positional_cylinder_frame.is_none());
        let positive_integer_extent = [
            0x12, 0x2d, 0x41, 0x83, 0x08, 0x72, 0x35, 0x71, 0xa2, 0x00, 0x11, 0x13, 0x2d, 0x44,
            0xff, 0xd2, 0xa6, 0xae, 0x74, 0x2a, 0x46, 0x64, 0x9f, 0xff, 0xff, 0xff, 0xff, 0xfc,
            0x2d, 0x52, 0x56, 0x9a, 0x71, 0xf6, 0x5f, 0xa5, 0x48, 0x1c, 0x00, 0x46, 0x64, 0x1f,
            0xff, 0xff, 0xff, 0xff, 0xfc, 0x2d, 0x54, 0x14, 0xff, 0x8c, 0x32, 0xe0, 0xe9, 0xda,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x15,
        ];
        assert!(record(&positive_integer_extent)
            .positional_cylinder_frame
            .is_some());
        let mut invalid_integer_controls = positive_integer_extent;
        invalid_integer_controls[10] = 0x12;
        assert!(record(&invalid_integer_controls)
            .positional_cylinder_frame
            .is_none());

        let equal_span = record(&[
            24, 45, 47, 73, 81, 130, 169, 147, 32, 18, 45, 49, 164, 168, 193, 84, 201, 144, 47, 12,
            0, 47, 32, 0, 72, 24, 0, 47, 22, 0, 47, 36, 0, 72, 16, 0,
        ]);
        assert_eq!(
            equal_span.type24_scalar_frame_round_envelope(0x24),
            Some(Type24RoundEnvelope {
                diameter: 2.0,
                extent_endpoints: [[3.5, 8.0, -6.0], [5.5, 10.0, -4.0]],
            })
        );
        assert!(equal_span.positional_cylinder_frame.is_none());

        let mut inconsistent = separated;
        inconsistent[31..34].copy_from_slice(&[0x2f, 0x12, 0x00]);
        assert!(record(&inconsistent).type24_round_radius(0x24).is_none());

        let first_coordinate = [
            0x4c, 0xb7, 0x67, 0xe1, 0x01, 0x3f, 0x80, 0x2d, 0x31, 0xa4, 0xa8, 0xc1, 0x54, 0xc9,
            0x87, 0x12, 0x2d, 0x35, 0xa4, 0xa8, 0xc1, 0x54, 0xc9, 0x87, 0x2f, 0x22, 0x00, 0x2f,
            0x43, 0x00, 0x48, 0x10, 0x00, 0x2d, 0x32, 0x4e, 0xfa, 0x22, 0xce, 0x34, 0xea, 0x2d,
            0x47, 0xfc, 0xef, 0xa2, 0x2c, 0xe3, 0x4f, 0x18,
        ];
        let first_coordinate = record(&first_coordinate);
        let frame = first_coordinate
            .positional_cylinder_frame
            .expect("complete first-coordinate round carrier");
        assert_eq!(frame.origin, [9.0, 38.0, -2.0]);
        assert_eq!(frame.ref_direction, [0.0, 0.0, 1.0]);
        assert_eq!(frame.radius, 2.0);
        let length = frame.length.expect("bounded axial span");
        let expected_length = 9.308_504_271_834_785_f64.hypot(9.976_063_033_979_35);
        assert!((length - expected_length).abs() < 1.0e-12);
        assert!((frame.axis[0] - 9.308_504_271_834_785 / length).abs() < 1.0e-12);
        assert!((frame.axis[1] - 9.976_063_033_979_35 / length).abs() < 1.0e-12);
        assert_eq!(first_coordinate.type24_round_radius(0x24), Some(2.0));

        let mut wrong_close = first_coordinate.body.clone();
        wrong_close[49] = 0x19;
        assert!(record(&wrong_close).positional_cylinder_frame.is_none());

        let opposite = [
            0x4c, 0xb7, 0x67, 0xe1, 0x01, 0x3f, 0x80, 0x2d, 0x35, 0xa4, 0xa8, 0xc1, 0x54, 0xc9,
            0x87, 0x12, 0x2d, 0x39, 0xa4, 0xa8, 0xc1, 0x54, 0xc9, 0x87, 0x46, 0x32, 0x4e, 0xfa,
            0x22, 0xce, 0x34, 0xea, 0x2f, 0x43, 0x00, 0x48, 0x10, 0x00, 0x48, 0x22, 0x00, 0x2d,
            0x47, 0xfc, 0xef, 0xa2, 0x2c, 0xe3, 0x4f, 0x18,
        ];
        let opposite = record(&opposite)
            .positional_cylinder_frame
            .expect("opposite first-coordinate round carrier");
        assert_eq!(opposite.origin, [-18.308_504_271_834_785, 38.0, -2.0]);
        assert_eq!(opposite.radius, 2.0);
        assert!((opposite.length.expect("required invariant") - expected_length).abs() < 1.0e-12);

        let segmented = [
            0x18, 0x2d, 0x35, 0xa8, 0xa9, 0xfd, 0x2c, 0xc7, 0xe2, 0x70, 0xbf, 0xe3, 0x4f, 0x05,
            0x11, 0x10, 0x2d, 0x3a, 0xc5, 0x1b, 0xc4, 0x49, 0x39, 0xa9, 0x46, 0x20, 0x38, 0xe3,
            0x8e, 0x38, 0xe3, 0x8e, 0x2f, 0x41, 0x00, 0x18, 0x48, 0x1c, 0x00, 0x2d, 0x42, 0x92,
            0x43, 0xe3, 0x8f, 0xf2, 0x60, 0x9f, 0x71, 0xc7, 0x1c, 0x71, 0xc7, 0x1c, 0xf7, 0x19,
        ];
        let segmented = record(&segmented);
        let frame = segmented
            .positional_cylinder_frame
            .expect("complete segmented first-coordinate round carrier");
        let diameter = 5.111_111_111_111_111;
        assert_eq!(frame.origin, [-8.111_111_111_111_11, 34.0, 0.5 * diameter]);
        assert_eq!(frame.ref_direction, [0.0, 0.0, 1.0]);
        assert_eq!(frame.radius, 0.5 * diameter);
        let expected_length = 1.111_111_111_111_110_7_f64.hypot(3.142_696_805_273_545);
        assert!((frame.length.expect("required invariant") - expected_length).abs() < 1.0e-12);
        assert_eq!(segmented.type24_round_radius(0x24), Some(0.5 * diameter));

        let mut wrong_separator = segmented.body.clone();
        wrong_separator[9] = 0x71;
        assert!(record(&wrong_separator).positional_cylinder_frame.is_none());

        let split_coordinate = [
            24, 45, 49, 164, 168, 193, 84, 201, 133, 18, 45, 53, 164, 168, 193, 84, 201, 136, 47,
            0, 0, 47, 34, 0, 52, 240, 0, 47, 28, 0, 47, 44, 0, 47, 16, 0,
        ];
        let split_frame = record(&split_coordinate)
            .positional_cylinder_frame
            .expect("split first-coordinate round carrier");
        assert_eq!(split_frame.origin, [2.0, 9.0, 2.0]);
        assert_eq!(
            split_frame.axis,
            [1.0 / 2.0_f64.sqrt(), 1.0 / 2.0_f64.sqrt(), 0.0]
        );
        assert_eq!(split_frame.ref_direction, [0.0, 0.0, 1.0]);
        assert!((split_frame.radius - 2.0).abs() < 1.0e-12);
        assert_eq!(split_frame.length, Some(50.0_f64.sqrt()));
        assert!(
            (record(&split_coordinate)
                .type24_round_radius(0x24)
                .expect("split-coordinate rolling radius")
                - 2.0)
                .abs()
                < 1.0e-12
        );

        let opposite_split = [
            24, 45, 49, 164, 168, 193, 84, 201, 133, 18, 45, 53, 164, 168, 193, 84, 201, 136, 72,
            28, 0, 47, 34, 0, 52, 240, 0, 72, 0, 0, 47, 44, 0, 47, 16, 0,
        ];
        let opposite_frame = record(&opposite_split)
            .positional_cylinder_frame
            .expect("opposite split first-coordinate round carrier");
        assert_eq!(opposite_frame.origin, [-7.0, 9.0, 2.0]);
        assert_eq!(opposite_frame.axis, split_frame.axis);
        assert!((opposite_frame.radius - 2.0).abs() < 1.0e-12);

        let mut incomplete_split = split_coordinate;
        incomplete_split[24] = 0x18;
        assert!(record(&incomplete_split)
            .positional_cylinder_frame
            .is_none());
    }

    #[test]
    fn decodes_terminal_square_radial_type24_round_envelope() {
        let body = [
            0x32, 0x90, 0x32, 0x70, 0x63, 0x1c, 0x71, 0xa7, 0x2d, 0x4b, 0xc1, 0x0d, 0x60, 0xad,
            0x2a, 0x4c, 0x12, 0x2d, 0x4f, 0x30, 0xcb, 0xcd, 0xcc, 0x62, 0xc5, 0x48, 0x58, 0xc0,
            0x2d, 0x57, 0x75, 0x9c, 0xe9, 0x32, 0x3b, 0xfa, 0x48, 0x28, 0x00, 0x48, 0x56, 0x80,
            0x2d, 0x59, 0x2d, 0x7c, 0x1f, 0xc1, 0xd8, 0x36, 0x48, 0x08, 0x00, 0xf7, 0x40,
        ];
        let mut payload = vec![7, 0x24, 4, 0x01, 0, 0];
        payload.extend_from_slice(&body);
        payload.push(0xe3);
        let record = parameter_records(&payload).remove(0);

        let frame = record
            .positional_cylinder_frame
            .expect("complete square-radial carrier");
        assert_eq!(frame.origin, [-94.5, -93.837_702_082_688_25, -7.5]);
        assert_eq!(frame.axis, [0.0, -1.0, 0.0]);
        assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
        assert_eq!(frame.radius, 4.5);
        assert!(
            (frame.length.expect("required invariant") - 6.872_998_848_194_527).abs() < 1.0e-12
        );

        let control_terminated_body = [
            24, 45, 53, 164, 168, 193, 84, 201, 135, 18, 45, 59, 164, 168, 193, 84, 201, 135, 72,
            51, 0, 47, 67, 0, 72, 24, 0, 72, 34, 0, 47, 72, 0, 24,
        ];
        let mut control_terminated_payload = vec![7, 0x24, 4, 0x01, 0, 0];
        control_terminated_payload.extend_from_slice(&control_terminated_body);
        control_terminated_payload.push(0xe3);
        let control_terminated = parameter_records(&control_terminated_payload).remove(0);
        let frame = control_terminated
            .positional_cylinder_frame
            .expect("control-terminated square-radial carrier");
        assert!(frame
            .origin
            .into_iter()
            .zip([-27.643_2, -14.0, 43.0])
            .all(|(actual, expected)| (actual - expected).abs() < 1.0e-12));
        assert_eq!(frame.axis, [1.0, 0.0, 0.0]);
        assert_eq!(frame.ref_direction, [0.0, 1.0, 0.0]);
        assert!((frame.radius - 5.0).abs() < 1.0e-12);
        assert!((frame.length.expect("required invariant") - 21.643_2).abs() < 1.0e-12);

        let mut ambiguous = record.clone();
        ambiguous.scalar_frames[1].slots[6].value = Some(-102.837_702_082_688_25);
        assert!(ambiguous.type24_square_radial_round_frame().is_none());

        let mut unowned_tail = record;
        unowned_tail.body.push(0x00);
        assert!(unowned_tail.type24_square_radial_round_frame().is_none());

        let six_slot_body = [
            27, 244, 0, 86, 19, 73, 195, 99, 182, 160, 18, 45, 26, 98, 51, 231, 180, 183, 80, 72,
            62, 0, 45, 29, 51, 51, 51, 51, 51, 153, 71, 9, 153, 71, 61, 204, 45, 30, 0, 0, 0, 0, 0,
            101, 46, 9, 153, 247, 23,
        ];
        let mut six_slot_payload = vec![7, 0x24, 4, 0x01, 0, 0];
        six_slot_payload.extend_from_slice(&six_slot_body);
        six_slot_payload.push(0xe3);
        let six_slot = parameter_records(&six_slot_payload).remove(0);
        let frame = six_slot
            .positional_cylinder_frame
            .expect("complete six-slot square-radial carrier");
        assert!(frame
            .origin
            .into_iter()
            .zip([-29.9, -7.4, -3.2])
            .all(|(actual, expected)| (actual - expected).abs() < 1.0e-12));
        assert_eq!(frame.axis, [0.0, 0.0, 1.0]);
        assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
        assert!((frame.radius - 0.1).abs() < 1.0e-12);
        assert!((frame.length.expect("required invariant") - 6.4).abs() < 1.0e-12);

        let unbounded_body = [
            0x18, 0x2d, 0x5f, 0x25, 0xa4, 0x69, 0xd7, 0x34, 0x2d, 0x00, 0x12, 0x00, 0x2d, 0x67,
            0x06, 0x05, 0x68, 0x1e, 0xcd, 0x4a, 0x46, 0x3d, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xd0,
            0x46, 0x16, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0x5c, 0x2e, 0x1f, 0x33, 0x2e, 0x3d, 0xcc,
            0x46, 0x15, 0xff, 0xff, 0xff, 0xff, 0xff, 0x8f, 0x2f, 0x20, 0x00,
        ];
        let mut unbounded_payload = vec![7, 0x24, 4, 0x01, 0, 0];
        unbounded_payload.extend_from_slice(&unbounded_body);
        unbounded_payload.push(0xe3);
        let unbounded = parameter_records(&unbounded_payload).remove(0);
        let frame = unbounded
            .positional_cylinder_frame
            .expect("complete zero-axial square-radial carrier");
        assert!((frame.origin[0] - 29.8).abs() < 1.0e-12);
        assert!((frame.origin[1] - 5.6).abs() < 1.0e-12);
        assert!((frame.origin[2] - 7.9).abs() < 1.0e-12);
        assert_eq!(frame.axis, [1.0, 0.0, 0.0]);
        assert_eq!(frame.ref_direction, [0.0, -1.0, 0.0]);
        assert!((frame.radius - 0.1).abs() < 1.0e-12);
        assert_eq!(frame.length, None);

        let mut unequal_radials = unbounded;
        unequal_radials.scalar_frames[1].slots[6].value = Some(8.1);
        assert!(unequal_radials.type24_square_radial_round_frame().is_none());
    }

    #[test]
    fn decodes_negative_a7_repeated_diameter_round_envelope() {
        let body = [
            0x18, 0x2d, 0x45, 0x30, 0x89, 0xa0, 0x27, 0x52, 0x54, 0x12, 0x2d, 0x45, 0x7d, 0x56,
            0x6c, 0xf4, 0x1f, 0x22, 0x2d, 0x45, 0x26, 0x66, 0x66, 0x66, 0x66, 0x66, 0x2a, 0xf4,
            0x00, 0xa7, 0x33, 0x33, 0x33, 0x33, 0x33, 0x80, 0x2e, 0x45, 0x66, 0x2a, 0xfc, 0x00,
            0x5e, 0x33, 0x33, 0x33, 0x33, 0x33, 0x80,
        ];
        let mut payload = vec![7, 0x24, 4, 0x01, 0, 0];
        payload.extend_from_slice(&body);
        payload.push(0xe3);
        let frame = parameter_records(&payload)[0]
            .positional_cylinder_frame
            .expect("complete signed-DICT repeated-diameter carrier");

        assert_eq!(frame.origin, [-42.3, 1.25, 0.0]);
        assert_eq!(frame.ref_direction, [0.0, 0.0, 1.0]);
        assert!((frame.radius - 0.3).abs() < 1.0e-12);
        let length = 85.1_f64.hypot(0.5);
        assert!((frame.length.expect("required invariant") - length).abs() < 1.0e-12);
        assert!((frame.axis[0] - 85.1 / length).abs() < 1.0e-12);
        assert!((frame.axis[1] - 0.5 / length).abs() < 1.0e-12);
        assert_eq!(frame.axis[2], 0.0);
    }

    #[test]
    fn decodes_prefixed_repeated_diameter_round_envelope() {
        let body = [
            0xeb, 0xba, 0xc2, 0x1d, 0x3a, 0x2d, 0x45, 0x30, 0x89, 0xa0, 0x27, 0x52, 0x54, 0x12,
            0x2d, 0x45, 0x7d, 0x56, 0x6c, 0xf4, 0x1f, 0x22, 0x2d, 0x45, 0x26, 0x66, 0x66, 0x66,
            0x66, 0x66, 0x42, 0xfb, 0xff, 0xa7, 0x33, 0x33, 0x33, 0x33, 0x33, 0x80, 0x2e, 0x45,
            0x66, 0x42, 0xf3, 0xff, 0x5e, 0x33, 0x33, 0x33, 0x33, 0x33, 0x80,
        ];
        let record = |body: &[u8]| {
            let mut payload = vec![7, 0x24, 4, 0x01, 0, 0];
            payload.extend_from_slice(body);
            payload.push(0xe3);
            parameter_records(&payload).remove(0)
        };
        let frame = record(&body)
            .positional_cylinder_frame
            .expect("complete prefixed repeated-diameter carrier");

        assert_eq!(frame.origin[0], -42.3);
        assert!((frame.origin[1] + 1.75).abs() < 1.0e-12);
        assert_eq!(frame.origin[2], 0.0);
        assert_eq!(frame.ref_direction, [0.0, 0.0, 1.0]);
        assert!((frame.radius - 0.3).abs() < 1.0e-12);
        let length = 85.1_f64.hypot(0.5);
        assert!((frame.length.expect("required invariant") - length).abs() < 1.0e-12);
        assert!((frame.axis[0] - 85.1 / length).abs() < 1.0e-12);
        assert!((frame.axis[1] - 0.5 / length).abs() < 1.0e-12);
        assert_eq!(frame.axis[2], 0.0);

        let mut wrong_prefix = body;
        wrong_prefix[1] = 0xbb;
        assert!(record(&wrong_prefix).positional_cylinder_frame.is_none());
        let mut wrong_separator = body;
        wrong_separator[13] = 0x13;
        assert!(record(&wrong_separator).positional_cylinder_frame.is_none());
        assert!(record(&body[..body.len() - 7])
            .positional_cylinder_frame
            .is_none());
    }

    #[test]
    fn decodes_held_coordinate_type24_round_envelope() {
        let record = |body: &[u8]| {
            let mut payload = vec![7, 0x24, 4, 0x01, 0, 0];
            payload.extend_from_slice(body);
            payload.push(0xe3);
            parameter_records(&payload).remove(0)
        };
        let body = [
            0x18, 0x2d, 0x4f, 0x12, 0x6e, 0x97, 0x8d, 0x4f, 0xe0, 0x78, 0xac, 0x67, 0x05, 0x61,
            0xbb, 0x50, 0x2d, 0x54, 0x89, 0x37, 0x4b, 0xc6, 0xa7, 0xf0, 0x48, 0x24, 0x00, 0x2f,
            0x41, 0x00, 0x2f, 0x10, 0x00, 0x2f, 0x24, 0x00, 0x2f, 0x43, 0x00, 0x2f, 0x18, 0x00,
        ];
        let base_record = record(&body);
        let frame = base_record
            .positional_cylinder_frame
            .expect("complete held-coordinate round carrier");

        assert_eq!(frame.origin, [34.0, 5.0, 10.0]);
        assert_eq!(frame.axis, [1.0, 0.0, 0.0]);
        assert_eq!(frame.ref_direction, [0.0, 1.0, 0.0]);
        assert_eq!(frame.radius, 1.0);
        assert_eq!(frame.length, Some(4.0));
        assert_eq!(base_record.type24_round_radius(0x24), Some(1.0));

        let replay_body = [
            24, 45, 79, 146, 110, 151, 141, 79, 224, 120, 172, 103, 5, 97, 187, 80, 45, 84, 73, 55,
            75, 198, 167, 240, 72, 34, 0, 47, 65, 0, 47, 16, 0, 47, 34, 0, 47, 67, 0, 47, 24, 0,
            247, 24,
        ];
        let replay = record(&replay_body);
        assert_eq!(
            replay.positional_cylinder_frame,
            Some(PositionalCylinderFrame {
                origin: [34.0, 5.0, 9.0],
                axis: [1.0, 0.0, 0.0],
                ref_direction: [0.0, 1.0, 0.0],
                radius: 1.0,
                length: Some(4.0),
            })
        );
        assert_eq!(replay.type24_round_radius(0x24), Some(1.0));
        assert_eq!(
            record(&replay_body[..replay_body.len() - 2]).positional_cylinder_frame,
            replay.positional_cylinder_frame,
        );

        let mut broken_replay = replay_body;
        broken_replay[43] = 0x19;
        assert!(record(&broken_replay).positional_cylinder_frame.is_none());

        let mut wrong_control = body;
        wrong_control[25] = 0x25;
        assert!(record(&wrong_control).positional_cylinder_frame.is_none());
    }

    #[test]
    fn decodes_terminal_type24_round_radius() {
        let record = |body: &[u8]| {
            let mut payload = vec![7, 0x24, 4, 0x01, 0, 0];
            payload.extend_from_slice(body);
            payload.push(0xe3);
            parameter_records(&payload).remove(0)
        };
        let terminal = [
            0x18, 0x2d, 0x45, 0x30, 0x89, 0xa0, 0x27, 0x52, 0x54, 0x12, 0x2d, 0x45, 0x7d, 0x56,
            0x6c, 0xf4, 0x1f, 0x22, 0x2d, 0x45, 0x26, 0x66, 0x66, 0x66, 0x66, 0x66, 0x2a, 0xf4,
            0x00, 0xa7, 0x33, 0x33, 0x33, 0x33, 0x33, 0x80, 0x2e, 0x45, 0x66, 0x2a, 0xfc, 0x00,
            0x5e, 0x33, 0x33, 0x33, 0x33, 0x33, 0x80,
        ];
        assert!(
            (record(&terminal)
                .type24_round_radius(0x24)
                .expect("required invariant")
                - 0.3)
                .abs()
                < 1.0e-12
        );

        let mut replay_terminated = terminal.to_vec();
        replay_terminated.extend_from_slice(&[0xf7, 0x17]);
        assert!(
            (record(&replay_terminated)
                .type24_round_radius(0x24)
                .expect("required invariant")
                - 0.3)
                .abs()
                < 1.0e-12
        );

        let mut trailing_payload = terminal.to_vec();
        trailing_payload.push(0x18);
        assert!(record(&trailing_payload)
            .type24_round_radius(0x24)
            .is_none());
        let coordinate_terminal = [
            0x18, 0x2d, 0x45, 0x30, 0x89, 0xa0, 0x27, 0x52, 0x54, 0x12, 0x46, 0x16, 0xd9, 0xc0,
            0xeb, 0x43, 0x76, 0xac,
        ];
        assert!(record(&coordinate_terminal)
            .type24_round_radius(0x24)
            .is_none());
        assert!(record(&terminal).type24_round_radius(0x22).is_none());
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
    fn derives_plane_from_unique_six_scalar_positional_frame() {
        let slot = |value, offset| SurfaceParameterScalar {
            value: Some(value),
            raw: vec![offset as u8],
            offset,
            length: 1,
        };
        let record = SurfaceParameterRecord {
            surface_id: 41,
            body: vec![0x00, 0x0c, 0x9a],
            scalar_values: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: Vec::new(),
            scalar_frames: vec![SurfaceParameterScalarFrame {
                offset: 3,
                slots: [8.0, 2.0, -3.0, 8.0, 5.0, 4.0]
                    .into_iter()
                    .enumerate()
                    .map(|(offset, value)| slot(value, offset))
                    .collect(),
            }],
            terminal_scalar_frame: None,
            tabulated_cylinder_frame: None,
            positional_cylinder_frame: None,
            positional_torus_frame: None,
            split_cylinder_outline_bounds: None,
            positional_cone_frame: None,
            boundary: SurfaceBodyBoundary::CompoundClose,
            offset: 3,
            body_offset: 11,
        };
        let row = SurfaceRow {
            id: 41,
            type_byte: SurfaceKind::Plane.canonical_type_byte(),
            kind: SurfaceKind::Plane,
            feature_id: 17,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 3,
        };

        assert_eq!(
            positional_frame_planes(std::slice::from_ref(&record), std::slice::from_ref(&row)),
            vec![OutlinePlane {
                surface_id: 41,
                origin: [8.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
                u_axis: [0.0, 1.0, 0.0],
                offset: 14,
            }]
        );

        let mut unmarked = record.clone();
        unmarked.body[2] = 0x99;
        assert!(positional_frame_planes(&[unmarked], std::slice::from_ref(&row)).is_empty());

        let mut ambiguous = record;
        ambiguous.scalar_frames[0].slots[4].value = Some(2.0);
        assert!(positional_frame_planes(&[ambiguous], &[row]).is_empty());
    }

    #[test]
    fn derives_plane_from_auxiliary_corner_frame() {
        let slot = |value, offset, length| SurfaceParameterScalar {
            value: Some(value),
            raw: vec![0; length],
            offset,
            length,
        };
        let record = SurfaceParameterRecord {
            surface_id: 41,
            body: vec![0; 49],
            scalar_values: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: vec![
                SurfaceParameterOpaqueSpan {
                    raw: vec![0; 3],
                    offset: 0,
                    length: 3,
                },
                SurfaceParameterOpaqueSpan {
                    raw: vec![0; 8],
                    offset: 10,
                    length: 8,
                },
            ],
            scalar_frames: vec![
                SurfaceParameterScalarFrame {
                    offset: 3,
                    slots: vec![slot(0.86, 3, 7)],
                },
                SurfaceParameterScalarFrame {
                    offset: 18,
                    slots: vec![
                        slot(0.8, 18, 3),
                        slot(42.3, 21, 8),
                        slot(1.75, 29, 3),
                        slot(-0.3, 32, 3),
                        slot(37.6, 35, 8),
                        slot(1.75, 43, 3),
                        slot(0.3, 46, 3),
                    ],
                },
            ],
            terminal_scalar_frame: None,
            tabulated_cylinder_frame: None,
            positional_cylinder_frame: None,
            positional_torus_frame: None,
            split_cylinder_outline_bounds: None,
            positional_cone_frame: None,
            boundary: SurfaceBodyBoundary::CompoundClose,
            offset: 3,
            body_offset: 11,
        };
        let row = SurfaceRow {
            id: 41,
            type_byte: SurfaceKind::Plane.canonical_type_byte(),
            kind: SurfaceKind::Plane,
            feature_id: 17,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 3,
        };

        assert_eq!(
            positional_frame_planes(std::slice::from_ref(&record), std::slice::from_ref(&row)),
            vec![OutlinePlane {
                surface_id: 41,
                origin: [0.0, 1.75, 0.0],
                normal: [0.0, 1.0, 0.0],
                u_axis: [1.0, 0.0, 0.0],
                offset: 32,
            }]
        );

        let mut trailed = record.clone();
        trailed.body = vec![0; 65];
        trailed.body[63..].copy_from_slice(&[0xf7, 0x0c]);
        trailed.opaque_spans = vec![
            SurfaceParameterOpaqueSpan {
                raw: vec![0],
                offset: 0,
                length: 1,
            },
            SurfaceParameterOpaqueSpan {
                raw: vec![0; 4],
                offset: 11,
                length: 4,
            },
            SurfaceParameterOpaqueSpan {
                raw: vec![0; 2],
                offset: 16,
                length: 2,
            },
            SurfaceParameterOpaqueSpan {
                raw: vec![0xf7, 0x0c],
                offset: 63,
                length: 2,
            },
        ];
        trailed.scalar_frames = vec![
            SurfaceParameterScalarFrame {
                offset: 1,
                slots: vec![slot(0.001, 1, 7), slot(0.2, 8, 3)],
            },
            SurfaceParameterScalarFrame {
                offset: 15,
                slots: vec![slot(-1.0, 15, 1)],
            },
            SurfaceParameterScalarFrame {
                offset: 18,
                slots: vec![
                    slot(-59.8, 18, 8),
                    slot(-29.8, 26, 3),
                    slot(4.1, 29, 7),
                    slot(7.5, 36, 8),
                    slot(29.8, 44, 3),
                    slot(3.9, 47, 8),
                    slot(7.5, 55, 8),
                ],
            },
        ];
        let mut domain_prefixed = trailed.clone();
        domain_prefixed.scalar_frames.remove(1);
        domain_prefixed.scalar_frames[1].offset = 15;
        domain_prefixed.scalar_frames[1].slots.splice(
            0..0,
            [slot(-2.0, 15, 1), slot(2.0, 16, 1), slot(0.0, 17, 1)],
        );
        assert_eq!(
            positional_frame_planes(&[trailed], std::slice::from_ref(&row)),
            vec![OutlinePlane {
                surface_id: 41,
                origin: [0.0, 0.0, 7.5],
                normal: [0.0, 0.0, 1.0],
                u_axis: [1.0, 0.0, 0.0],
                offset: 37,
            }]
        );
        assert_eq!(
            positional_frame_planes(&[domain_prefixed], std::slice::from_ref(&row)),
            vec![OutlinePlane {
                surface_id: 41,
                origin: [0.0, 0.0, 7.5],
                normal: [0.0, 0.0, 1.0],
                u_axis: [1.0, 0.0, 0.0],
                offset: 37,
            }]
        );

        let mut compact_prefix = record.clone();
        compact_prefix.body = vec![
            0x18, 0x18, 0x6d, 0xeb, 0x81, 0x84, 0xcc, 0xcc, 0xd0, 0x00, 0x0c, 0x9a, 0xd5, 0xd6,
            0x25, 0xa6, 0xec, 0x06, 0x18, 0x46, 0x1a, 0xdf, 0x09, 0x9b, 0x3c, 0x32, 0xed, 0x2f,
            0x20, 0x00, 0xd5, 0xd6, 0x25, 0xa6, 0xec, 0x06, 0x18, 0x46, 0x18, 0x81, 0x99, 0x6a,
            0xa2, 0x99, 0x53, 0x2e, 0x20, 0x33, 0xf7, 0x0c,
        ];
        compact_prefix.scalar_tokens = scalar_tokens(
            SurfaceKind::Plane,
            &compact_prefix.body,
            &scalar::ScalarCache::default(),
        );
        compact_prefix.scalar_frames = scalar_frames(&compact_prefix.scalar_tokens);
        assert_eq!(
            positional_frame_planes(&[compact_prefix], std::slice::from_ref(&row)),
            vec![OutlinePlane {
                surface_id: 41,
                origin: [2.479_564_003_064_99, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
                u_axis: [0.0, 1.0, 0.0],
                offset: 23,
            }]
        );

        let mut incomplete = record;
        incomplete.opaque_spans[1].length = 7;
        assert!(
            positional_frame_planes(&[incomplete.clone()], std::slice::from_ref(&row)).is_empty()
        );

        let mut short = incomplete;
        short.scalar_frames.push(SurfaceParameterScalarFrame {
            offset: 18,
            slots: vec![slot(1.0, 18, 1)],
        });
        assert!(positional_frame_planes(&[short], &[row]).is_empty());
    }

    #[test]
    fn derives_plane_from_terminal_corner_frame() {
        let body = [
            0x37, 0x01, 0x5f, 0xff, 0xff, 0xff, 0xff, 0xf4, 0x2d, 0x4c, 0x75, 0xdb, 0x19, 0xc2,
            0x89, 0x40, 0x2e, 0x17, 0xff, 0x2d, 0x4f, 0x01, 0x49, 0xdf, 0x84, 0xdb, 0x18, 0x48,
            0x57, 0x00, 0x2d, 0x57, 0xd0, 0x03, 0xc5, 0xbc, 0xeb, 0x74, 0xda, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x11, 0x47, 0x56, 0xff, 0x2d, 0x59, 0x15, 0xbb, 0x28, 0x9e, 0x14, 0x60,
            0x2e, 0x07, 0xff, 0xf7, 0x1f,
        ];
        let mut payload = vec![7, 0x22, 4, 0x01, 0, 0];
        payload.extend_from_slice(&body);
        payload.push(0xe3);
        let record = parameter_records(&payload).remove(0);
        let row = SurfaceRow {
            id: record.surface_id,
            type_byte: SurfaceKind::Plane.canonical_type_byte(),
            kind: SurfaceKind::Plane,
            feature_id: 17,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 3,
        };

        assert_eq!(
            positional_frame_planes(std::slice::from_ref(&record), std::slice::from_ref(&row)),
            vec![OutlinePlane {
                surface_id: record.surface_id,
                origin: [-92.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
                u_axis: [0.0, 1.0, 0.0],
                offset: record.body_offset + 27,
            }]
        );

        let unprefixed_body = [
            0x48, 0x67, 0xd0, 0x46, 0x49, 0x43, 0xd8, 0x44, 0x0e, 0x17, 0x8e, 0x2f, 0x61, 0x90,
            0x2d, 0x49, 0x43, 0xd8, 0x44, 0x0e, 0x17, 0x90, 0x48, 0x67, 0xd0, 0x48, 0x14, 0x00,
            0x46, 0x49, 0x43, 0xd8, 0x44, 0x0e, 0x17, 0x8e, 0x2f, 0x61, 0x90, 0x48, 0x14, 0x00,
            0x2d, 0x49, 0x43, 0xd8, 0x44, 0x0e, 0x17, 0x90, 0xf7, 0x1f,
        ];
        let mut unprefixed_payload = vec![7, 0x22, 4, 0x01, 0, 0];
        unprefixed_payload.extend_from_slice(&unprefixed_body);
        unprefixed_payload.push(0xe3);
        let unprefixed = parameter_records(&unprefixed_payload).remove(0);
        assert_eq!(
            positional_frame_planes(
                std::slice::from_ref(&unprefixed),
                std::slice::from_ref(&row)
            ),
            vec![OutlinePlane {
                surface_id: unprefixed.surface_id,
                origin: [0.0, -5.0, 0.0],
                normal: [0.0, 1.0, 0.0],
                u_axis: [1.0, 0.0, 0.0],
                offset: unprefixed.body_offset + 22,
            }]
        );

        let mut wrong_trailer = record.clone();
        *wrong_trailer
            .body
            .last_mut()
            .expect("terminal reference id") = 0x1e;
        assert!(positional_frame_planes(&[wrong_trailer], std::slice::from_ref(&row)).is_empty());

        let mut multiple_frames = record.clone();
        multiple_frames.scalar_frames.insert(
            0,
            SurfaceParameterScalarFrame {
                offset: 0,
                slots: vec![multiple_frames.scalar_frames[0].slots[0].clone()],
            },
        );
        assert!(positional_frame_planes(&[multiple_frames], std::slice::from_ref(&row)).is_empty());

        let mut ambiguous = record;
        ambiguous.scalar_frames[0].slots[7].value = Some(-95.250_230_249_874_05);
        assert!(positional_frame_planes(&[ambiguous], &[row]).is_empty());
    }

    #[test]
    fn derives_plane_from_split_terminal_corner_frame() {
        let body = [
            0x32, 0xf7, 0xf0, 0x6c, 0x6b, 0x2d, 0x51, 0x9a, 0x2d, 0x42, 0x50, 0x4a, 0x32, 0x0f,
            0x60, 0x20, 0x2e, 0x4e, 0xff, 0x2d, 0x4e, 0x4f, 0x19, 0xda, 0x50, 0x97, 0xe8, 0x46,
            0x64, 0x1f, 0xff, 0xff, 0xff, 0xff, 0xfc, 0x2d, 0x52, 0xbd, 0x3b, 0x51, 0xe3, 0x56,
            0xe4, 0x2f, 0x1c, 0x00, 0x46, 0x58, 0xbf, 0xff, 0xff, 0xff, 0xff, 0xf8, 0x2d, 0x58,
            0xbc, 0xa3, 0x26, 0x03, 0xf2, 0xc8, 0x2f, 0x1c, 0x00, 0xf7, 0x1f,
        ];
        let mut payload = vec![7, 0x22, 4, 0x01, 0, 0];
        payload.extend_from_slice(&body);
        payload.push(0xe3);
        let record = parameter_records(&payload).remove(0);
        let row = SurfaceRow {
            id: record.surface_id,
            type_byte: SurfaceKind::Plane.canonical_type_byte(),
            kind: SurfaceKind::Plane,
            feature_id: 17,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 3,
        };

        assert_eq!(
            positional_frame_planes(std::slice::from_ref(&record), std::slice::from_ref(&row)),
            vec![OutlinePlane {
                surface_id: record.surface_id,
                origin: [0.0, 0.0, 7.0],
                normal: [0.0, 0.0, 1.0],
                u_axis: [1.0, 0.0, 0.0],
                offset: record.body_offset + 27,
            }]
        );

        let mut incomplete_controls = record.clone();
        incomplete_controls.opaque_spans[1].length -= 1;
        assert!(
            positional_frame_planes(&[incomplete_controls], std::slice::from_ref(&row)).is_empty()
        );

        let mut ambiguous = record;
        ambiguous.scalar_frames[1].slots[5].value = ambiguous.scalar_frames[1].slots[2].value;
        assert!(positional_frame_planes(&[ambiguous], &[row]).is_empty());
    }

    #[test]
    fn derives_plane_from_marker_bounded_corner_frames() {
        let body = vec![
            0x18, 0xe4, 0x28, 0xad, 0xfb, 0xcd, 0xe8, 0xf5, 0xc2, 0x80, 0x00, 0x0c, 0x9a, 0xdc,
            0x9c, 0x95, 0x35, 0x00, 0x80, 0xf8, 0x46, 0x1a, 0xdf, 0x09, 0x9b, 0x3c, 0x32, 0xed,
            0x2f, 0x20, 0x00, 0xdc, 0x9c, 0x95, 0x35, 0x00, 0x80, 0xf8, 0x46, 0x1a, 0xa3, 0x11,
            0xff, 0x6a, 0x47, 0x68, 0x2e, 0x20, 0x33, 0xf7, 0x0c,
        ];
        let tokens = scalar_tokens(SurfaceKind::Plane, &body, &scalar::ScalarCache::default());
        let frames = scalar_frames(&tokens);
        let record = SurfaceParameterRecord {
            surface_id: 41,
            scalar_values: tokens.iter().filter_map(|token| token.value).collect(),
            opaque_spans: opaque_spans(&body, &tokens),
            terminal_scalar_frame: terminal_scalar_frame(&body, &frames),
            scalar_tokens: tokens,
            scalar_frames: frames,
            tabulated_cylinder_frame: None,
            positional_cylinder_frame: None,
            split_cylinder_outline_bounds: None,
            positional_cone_frame: None,
            positional_torus_frame: None,
            body,
            boundary: SurfaceBodyBoundary::CompoundClose,
            offset: 3,
            body_offset: 11,
        };
        let row = SurfaceRow {
            id: 41,
            type_byte: SurfaceKind::Plane.canonical_type_byte(),
            kind: SurfaceKind::Plane,
            feature_id: 17,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 3,
        };

        assert_eq!(
            record
                .scalar_frames
                .last()
                .expect("reflected corner frame")
                .slots
                .len(),
            6
        );
        assert_eq!(
            positional_frame_planes(std::slice::from_ref(&record), std::slice::from_ref(&row)),
            vec![OutlinePlane {
                surface_id: 41,
                origin: [3.326_456_464_841_722_7, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
                u_axis: [0.0, 1.0, 0.0],
                offset: 24,
            }]
        );

        let mut prefixed_eight_byte = record.clone();
        prefixed_eight_byte.body = vec![
            0x18, 0xe4, 0x28, 0xc6, 0xc6, 0xa8, 0x58, 0x51, 0xeb, 0xa0, 0x00, 0x0c, 0x9a, 0x46,
            0x1e, 0x3e, 0x61, 0xf5, 0x38, 0x92, 0x68, 0x46, 0x19, 0xb0, 0xe5, 0x1d, 0x83, 0xe1,
            0x02, 0x2f, 0x20, 0x00, 0x46, 0x1e, 0x3e, 0x61, 0xf5, 0x38, 0x92, 0x68, 0x46, 0x18,
            0xfa, 0xaf, 0xda, 0xc1, 0x51, 0xa5, 0x2e, 0x20, 0x33,
        ];
        prefixed_eight_byte.scalar_tokens = scalar_tokens(
            SurfaceKind::Plane,
            &prefixed_eight_byte.body,
            &scalar::ScalarCache::default(),
        );
        prefixed_eight_byte.scalar_frames = scalar_frames(&prefixed_eight_byte.scalar_tokens);
        assert_eq!(
            positional_frame_planes(&[prefixed_eight_byte], std::slice::from_ref(&row)),
            vec![OutlinePlane {
                surface_id: 41,
                origin: [7.560_920_554_712_176, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
                u_axis: [0.0, 1.0, 0.0],
                offset: 24,
            }]
        );

        let mut prefixed_seven_byte = record.clone();
        prefixed_seven_byte.body = vec![
            0x18, 0xe4, 0x28, 0xc6, 0xc6, 0xa8, 0x58, 0x51, 0xeb, 0xa0, 0x00, 0x0c, 0x9a, 0x4a,
            0x19, 0x29, 0x8e, 0x22, 0xd2, 0x2c, 0x46, 0x19, 0xb0, 0xe5, 0x1d, 0x83, 0xe1, 0x02,
            0x2f, 0x20, 0x00, 0x4a, 0x19, 0x29, 0x8e, 0x22, 0xd2, 0x2c, 0x46, 0x18, 0xfa, 0xaf,
            0xda, 0xc1, 0x51, 0xa5, 0x2e, 0x20, 0x33,
        ];
        prefixed_seven_byte.scalar_tokens = scalar_tokens(
            SurfaceKind::Plane,
            &prefixed_seven_byte.body,
            &scalar::ScalarCache::default(),
        );
        prefixed_seven_byte.scalar_frames = scalar_frames(&prefixed_seven_byte.scalar_tokens);
        assert_eq!(
            positional_frame_planes(&[prefixed_seven_byte], std::slice::from_ref(&row)),
            vec![OutlinePlane {
                surface_id: 41,
                origin: [6.290_581_268_384_813, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
                u_axis: [0.0, 1.0, 0.0],
                offset: 24,
            }]
        );

        let mut unterminated = record.clone();
        unterminated.body.truncate(unterminated.body.len() - 2);
        unterminated.scalar_tokens = scalar_tokens(
            SurfaceKind::Plane,
            &unterminated.body,
            &scalar::ScalarCache::default(),
        );
        unterminated.scalar_frames = scalar_frames(&unterminated.scalar_tokens);
        assert_eq!(
            positional_frame_planes(&[unterminated], std::slice::from_ref(&row)),
            vec![OutlinePlane {
                surface_id: 41,
                origin: [3.326_456_464_841_722_7, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
                u_axis: [0.0, 1.0, 0.0],
                offset: 24,
            }]
        );

        let mut y_held = record.clone();
        y_held.body = vec![
            0x18, 0xe4, 0x2c, 0xbe, 0x45, 0xa8, 0x7a, 0xe1, 0x48, 0x00, 0x0c, 0x9a, 0xd1, 0xf1,
            0x60, 0x5a, 0xa4, 0xd9, 0x00, 0x46, 0x1b, 0x1c, 0x28, 0x70, 0x5d, 0x7a, 0x9b, 0x2f,
            0x20, 0x00, 0xd0, 0x0d, 0x05, 0xd2, 0xf6, 0xc4, 0x80, 0x46, 0x1b, 0x1c, 0x28, 0x70,
            0x5d, 0x7a, 0x9b, 0x2e, 0x20, 0x33, 0xf7, 0x0c,
        ];
        y_held.scalar_tokens = scalar_tokens(
            SurfaceKind::Plane,
            &y_held.body,
            &scalar::ScalarCache::default(),
        );
        y_held.scalar_frames = scalar_frames(&y_held.scalar_tokens);
        assert_eq!(
            positional_frame_planes(&[y_held], std::slice::from_ref(&row)),
            vec![OutlinePlane {
                surface_id: 41,
                origin: [0.0, 6.777_498_012_261_868, 0.0],
                normal: [0.0, 1.0, 0.0],
                u_axis: [1.0, 0.0, 0.0],
                offset: 23,
            }]
        );

        let mut mixed_width = record.clone();
        mixed_width.body = vec![
            0x18, 0xe4, 0x2c, 0xbe, 0x45, 0x9b, 0x33, 0x33, 0x33, 0x00, 0x0c, 0x9a, 0x4a, 0x19,
            0x29, 0x8e, 0x22, 0xd2, 0x2c, 0x46, 0x1a, 0x29, 0xfb, 0x8f, 0x4b, 0x8f, 0x16, 0x2f,
            0x20, 0x00, 0x46, 0x18, 0xb0, 0x77, 0xb6, 0x05, 0x5f, 0x34, 0x46, 0x1a, 0x29, 0xfb,
            0x8f, 0x4b, 0x8f, 0x16, 0x2e, 0x20, 0x33,
        ];
        mixed_width.scalar_tokens = scalar_tokens(
            SurfaceKind::Plane,
            &mixed_width.body,
            &scalar::ScalarCache::default(),
        );
        mixed_width.scalar_frames = scalar_frames(&mixed_width.scalar_tokens);
        assert_eq!(
            positional_frame_planes(&[mixed_width], std::slice::from_ref(&row)),
            vec![OutlinePlane {
                surface_id: 41,
                origin: [0.0, 6.540_998_686_777_831, 0.0],
                normal: [0.0, 1.0, 0.0],
                u_axis: [1.0, 0.0, 0.0],
                offset: 23,
            }]
        );

        let mut malformed = record;
        malformed.body[31] = 0x00;
        let tokens = scalar_tokens(
            SurfaceKind::Plane,
            &malformed.body,
            &scalar::ScalarCache::default(),
        );
        assert!(tokens.iter().all(|token| token.offset != 13));
        malformed.scalar_frames = scalar_frames(&tokens);
        assert!(positional_frame_planes(&[malformed], &[row]).is_empty());
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
    fn positional_plane_envelope_rejects_bytes_before_a_complete_standard_frame() {
        let payload = [
            7, 0x22, 4, 0x01, 0, 0, 0xfb, 0x0f, 0xe4, 0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0xe4, 0x0f,
            0xe4, 0xe3,
        ];

        assert_eq!(rows(&payload).len(), 1);
        assert!(plane_envelopes(&payload).is_empty());
    }

    #[test]
    fn plane_envelope_scalar_tokens_take_precedence_over_compound_close_bytes() {
        let body = [
            70, 32, 107, 133, 30, 184, 81, 235, 70, 47, 201, 160, 13, 107, 10, 126, 47, 32, 0, 24,
            70, 32, 107, 133, 30, 184, 81, 235, 70, 47, 201, 160, 13, 107, 10, 126, 142, 71, 174,
            20, 122, 225, 72, 47, 32, 0, 24, 142, 71, 174, 20, 122, 225, 72,
        ];
        let mut payload = vec![7, 0x22, 4, 0x01, 0, 0];
        payload.extend_from_slice(&body);
        payload.push(psb::token::COMPOUND_CLOSE);

        let envelopes = plane_envelopes(&payload);
        assert_eq!(envelopes.len(), 1);
        assert_eq!(envelopes[0].body, body);
        assert_eq!(
            envelopes[0].corner_coordinate_equal,
            [Some(false), Some(false), Some(true)]
        );
    }

    #[test]
    fn plane_envelope_coordinates_decode_compact_positive_half() {
        let body = [
            0x0f, 0xe4, 0x0d, 0x0f, 0x43, 0xe0, 0x00, 0xe4, 0x0f, 0x0e, 0xe4, 0x0f,
        ];
        let (slots, consumed) = plane_envelope_scalar_slots_with_tokens_and_end(
            &body,
            10,
            &scalar::ScalarCache::default(),
        );

        assert_eq!(consumed, body.len());
        assert_eq!(slots[4].0, Some(-0.5));
        assert_eq!(slots[7].0, Some(0.5));
        assert_eq!(slot_equality(&slots[4], &slots[7]), Some(false));
        assert_eq!(slot_equality(&slots[5], &slots[8]), Some(true));
        assert_eq!(slot_equality(&slots[6], &slots[9]), Some(true));
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
    fn named_plane_outline_rejects_bytes_between_the_wrapper_and_slots() {
        let payload = b"srf_array\0\xf8\x01\xe0\x01geom_id\0\x07\xe0\x01geom_type\0\x22\xe0\x01feat_id\0\x04\xe0\x01orient\0\x01\xe0\x01boundary_type\0\x00\xe0\x01next_geom_ptr\0\x00\xe0\x02outline\0\xf9\x02\x03\xfb\xe4\x18\xe4\xe4\xe4\x18\xe0\x00srf_prim_ptr(plane)\0\xe3";

        assert_eq!(rows(payload).len(), 1);
        assert!(plane_envelopes(payload).is_empty());
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
    fn positional_plane_frame_decodes_outline_separator_zero_suffix() {
        let first = [
            0x10, 0x18, 0xe5, 0x10, 0x18, 0xe5, 0x0f, 0x18, 0x2f, 0x05, 0x00, 0x00, 0x0c, 0x98,
        ];
        let second = [
            0x10, 0x18, 0xe5, 0x10, 0x18, 0xe5, 0x0f, 0x18, 0x2a, 0xfa, 0x00, 0x00, 0x0c, 0x98,
        ];

        assert_eq!(
            complete_plane_local_system_slots(&first, &scalar::ScalarCache::default())
                .map(|slots| [slots[9], slots[10], slots[11]]),
            Some([0.0, 2.625, 0.0])
        );
        assert_eq!(
            complete_plane_local_system_slots(&second, &scalar::ScalarCache::default())
                .map(|slots| [slots[9], slots[10], slots[11]]),
            Some([0.0, 1.625, 0.0])
        );
    }

    #[test]
    fn outline_separator_precedes_compact_integer_alias_of_compound_close() {
        let payload = [0x0f, 0x00, 0x0c, 0x98, 0xe3, 0xe0, 0x01, b'x', 0];
        assert_eq!(first_compound_close(&payload, 0, payload.len()), Some(4));
    }

    #[test]
    fn positional_plane_frame_decodes_rank_two_image_before_null_tail() {
        let body = [0x18, 0xe4, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x18, 0xe6, 0xe1];

        let slots = complete_plane_local_system_slots(&body, &scalar::ScalarCache::default())
            .expect("complete rank-two frame");
        assert_eq!(
            slots,
            [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        );
        let frame = plane_frame(&slots.map(Some));
        assert_eq!(frame.origin, Some([0.0, 0.0, 0.0]));
        assert_eq!(frame.u_axis, Some([0.0, 1.0, 0.0]));
        assert_eq!(frame.normal, Some([0.0, 0.0, -1.0]));
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
        let first_rank_zero = plane_frame(&options([
            0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -3.0, 0.0,
        ]));
        assert_eq!(first_rank_zero.origin, Some([0.0, -3.0, 0.0]));
        assert_eq!(first_rank_zero.u_axis, Some([1.0, 0.0, 0.0]));
        assert_eq!(first_rank_zero.normal, Some([0.0, -1.0, 0.0]));
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
        let slots = scalar_slots_with_tokens_and_end(&body, 3, &scalar::ScalarCache::default()).0;

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
        let slots =
            scalar_slots_with_tokens_and_end(&[0xe4, 0x18], 2, &scalar::ScalarCache::default()).0;

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
    fn named_local_system_splits_zero_before_coordinate_token() {
        let body = [
            0x41, 0xd2, 0x3c, 0xfc, 0xe9, 0x9e, 0x37, 0xb2, 0x79, 0xac, 0x53, 0x1a, 0x28, 0x66,
            0x9d, 0x18, 0x79, 0xac, 0x53, 0x1a, 0x28, 0x66, 0x9d, 0x5d, 0x3c, 0xfc, 0xe9, 0x9e,
            0x37, 0xb2, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f,
        ];

        let slots = sequential_named_local_system_slots(&body, 12, &scalar::ScalarCache::default())
            .expect("complete local system");

        assert_eq!(slots[2], Some(0.0));
        assert_eq!(slots[3], slots[1]);
        assert_eq!(slots[4], slots[0].map(|value| -value));
        assert_eq!(slots[5..], [Some(0.0); 7]);
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
    fn named_local_system_advances_across_inherited_slots() {
        let body = [
            0xe4, 0x0f, 0xe7, 0x03, 0xe4, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f,
        ];

        assert_eq!(
            sequential_named_local_system_slots(&body, 12, &scalar::ScalarCache::default()),
            Some(vec![
                Some(1.0),
                Some(0.0),
                None,
                None,
                None,
                Some(1.0),
                Some(0.0),
                Some(0.0),
                Some(0.0),
                Some(0.0),
                Some(0.0),
                Some(0.0),
            ])
        );
    }

    #[test]
    fn named_local_system_rejects_invalid_inherited_slot_transitions() {
        for body in [
            &[0xe7][..],
            &[0xe7, 0x00],
            &[0xe7, 0x0d],
            &[0xe4, 0xe7, 0x0c],
        ] {
            assert_eq!(
                sequential_named_local_system_slots(body, 12, &scalar::ScalarCache::default()),
                None
            );
        }
    }

    #[test]
    fn named_local_system_rejects_an_unknown_byte_before_complete_slots() {
        let payload = b"srf_prim_ptr(cylinder)\0\
            \xe0\x02local_sys\0\xf9\x04\x03\xfb\x18\xe5\x0f\x0f\x0f\xe4\x0f\x0f\x0f\x2f\x2e\0\x18\
            \xe0\x01radius\0\xe4";
        let records = named_prototype_records(payload);

        assert_eq!(
            records[0].field("local_sys").map(|field| &field.value),
            Some(&SurfaceNamedValue::Opaque(
                b"\xf9\x04\x03\xfb\x18\xe5\x0f\x0f\x0f\xe4\x0f\x0f\x0f\x2f\x2e\0\x18".to_vec()
            ))
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
        assert_eq!(values[6], Some(-0.070_335_614_969_227_37));
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
    fn named_prototype_radius_decodes_positive_eight_byte_form() {
        let value = 0.125_f64;
        let raw = value.to_be_bytes();
        assert_eq!(raw[0], 0x3f);
        let mut payload = b"srf_prim_ptr(cylinder)\0\xe0\x01radius\0".to_vec();
        payload.push(0x28);
        payload.extend_from_slice(&raw[1..]);

        let records = named_prototype_records(&payload);

        assert_eq!(
            records[0].field("radius").map(|field| &field.value),
            Some(&SurfaceNamedValue::ScalarSequence(vec![value]))
        );
    }

    #[test]
    fn named_prototype_radius_decodes_positive_dict_form() {
        let value = 4.5_f64;
        let raw = value.to_be_bytes();
        let prefix = u8::try_from(u16::from_be_bytes([raw[0], raw[1]]) - 0x3f75)
            .expect("synthetic value lies in the named-radius DICT lattice");
        let mut payload = b"srf_prim_ptr(cylinder)\0\xe0\x01radius\0".to_vec();
        payload.push(prefix);
        payload.extend_from_slice(&raw[2..]);

        let records = named_prototype_records(&payload);

        assert_eq!(
            records[0].field("radius").map(|field| &field.value),
            Some(&SurfaceNamedValue::ScalarSequence(vec![value]))
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
