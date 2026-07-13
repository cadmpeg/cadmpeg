// SPDX-License-Identifier: Apache-2.0
//! Surface namespace rows and prototype parameters.
//!
//! A [`SurfaceRow`] identifies a surface family and its feature, orientation,
//! boundary, and namespace links. A [`SurfacePrototype`] contains named template
//! parameters. Prototype values do not locate a surface instance in model space.

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
    /// `geom_type = 0x29`: fillet or spline surface family; the split
    /// between the two is not decoded (see the open-items list).
    FilletOrSpline,
    /// `geom_type = 0x2a` or `0x2c`: a `surface_of_extrusion` linear
    /// extrusion.
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
            0x29 => Some(Self::FilletOrSpline),
            0x2a | 0x2c => Some(Self::Extrusion),
            _ => None,
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
    /// Spline or fillet prototype.
    Spline,
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
            "spline" | "fillet" => Self::Spline,
            "surface_of_extrusion" | "extrusion" => Self::Extrusion,
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
    /// Structural form that bounded the body.
    pub boundary: SurfaceBodyBoundary,
    /// Byte offset of the positional surface row in the original stream.
    pub offset: usize,
    /// Byte offset of the first parameter-body byte in the original stream.
    pub body_offset: usize,
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
        let Some(first) = corners[0]
            .map(|value| value)
            .into_iter()
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Some(second) = corners[1]
            .map(|value| value)
            .into_iter()
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let scale = first
            .iter()
            .chain(&second)
            .map(|value| value.abs())
            .fold(1.0, f64::max);
        let held = (0..3)
            .filter(|axis| (first[*axis] - second[*axis]).abs() <= 1e-9 * scale)
            .collect::<Vec<_>>();
        if held.len() != 1 {
            continue;
        }
        let axis = held[0];
        let mut origin = [0.0; 3];
        origin[axis] = first[axis];
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
        let kind = find_in(payload, b"geom_type\0", start, end)
            .and_then(|at| payload.get(at + b"geom_type\0".len()))
            .and_then(|byte| SurfaceKind::from_byte(*byte));
        if let (Some((id, id_offset)), Some(kind), Some((feature_id, _)), Some((next_surface, _))) = (
            value(b"geom_id\0"),
            kind,
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
];

fn named_surface_value(name: &str, body: &[u8], cache: &scalar::ScalarCache) -> SurfaceNamedValue {
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
            if values.len() == usize::try_from(count).unwrap_or(usize::MAX) {
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
            values: scalar_slots(&body[3..], slot_count, cache),
        };
    }
    let mut values = Vec::new();
    let mut cursor = 0;
    while cursor < body.len() {
        if matches!(body[cursor], 0xe0..=0xe3 | 0xf1 | 0xf7 | 0xfb) {
            break;
        }
        let Some((value, next)) = scalar::decode_in_lane(body, cursor, cache) else {
            values.clear();
            break;
        };
        values.push(value);
        cursor = next;
    }
    if !values.is_empty() {
        return SurfaceNamedValue::ScalarSequence(values);
    }
    let scalar_field = matches!(name, "radius" | "radius1" | "radius2" | "half_angle");
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
        let family = String::from_utf8_lossy(&payload[family_start..close]);
        let mut record_end = find(payload, b"srf_prim_ptr(", close + 2).unwrap_or(payload.len());
        for marker in [b"crv_array\0".as_slice(), b"lo_array\0", b"qlt_array\0"] {
            if let Some(at) = find(payload, marker, close + 2) {
                record_end = record_end.min(at);
            }
        }
        let tokens = psb::tokens(&payload[close + 2..record_end]);
        let named = tokens
            .iter()
            .enumerate()
            .filter(|(_, token)| token.kind == psb::TokenKind::NamedRecord)
            .collect::<Vec<_>>();
        let mut parameters = Vec::new();
        for (position, (_, token)) in named.iter().enumerate() {
            let token_offset = close + 2 + token.offset;
            let name_start = token_offset + 2;
            let name_end = token_offset + token.length - 1;
            let name = String::from_utf8_lossy(&payload[name_start..name_end]);
            if !PROTOTYPE_PARAMETER_NAMES.contains(&name.as_ref()) {
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
            family: SurfacePrototypeFamily::from_name(&family),
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

fn scalar_values(body: &[u8], cache: &scalar::ScalarCache) -> Vec<f64> {
    let mut values = Vec::new();
    let mut cursor = 0;
    while cursor < body.len() {
        if let Some((value, next)) = scalar::decode_in_row_lane(body, cursor, cache) {
            values.push(value);
            cursor = next;
        } else {
            cursor += 1;
        }
    }
    values
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
        if let Some(relative) = payload[*body_start..body_end]
            .iter()
            .position(|byte| *byte == 0xe3)
        {
            body_end = body_start + relative;
            boundary = SurfaceBodyBoundary::CompoundClose;
        }
        if let Some(relative) = payload[*body_start..body_end]
            .iter()
            .position(|byte| *byte == psb::token::NAMED_RECORD)
        {
            let candidate = body_start + relative;
            if payload
                .get(candidate + 2..body_end)
                .is_some_and(|bytes| bytes.contains(&0))
            {
                body_end = candidate;
                boundary = SurfaceBodyBoundary::NamedRecord;
            }
        }
        let body = payload[*body_start..body_end].to_vec();
        records.push(SurfaceParameterRecord {
            surface_id: row.id,
            scalar_values: scalar_values(&body, &cache),
            body,
            boundary,
            offset: row.offset,
            body_offset: *body_start,
        });
    }
    records
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

fn row_scalar_slots(body: &[u8], count: usize, cache: &scalar::ScalarCache) -> Vec<Option<f64>> {
    let mut slots = Vec::with_capacity(count);
    let mut cursor = 0;
    while cursor < body.len() && slots.len() < count {
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
        if let Some((value, next)) = scalar::decode_in_row_lane(body, cursor, cache) {
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
        .filter(|row| row.kind == SurfaceKind::Plane && row.boundary_type == 0)
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
    let cache = scalar::ScalarCache::from_section(payload);
    let headers = rows(payload)
        .into_iter()
        .filter(|row| row.kind == SurfaceKind::Plane && row.boundary_type == 0)
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
        let envelope = if body.first() == Some(&0x0e) {
            let slots = scalar_slots(&body[1..], 9, &cache);
            PlaneEnvelope::Compact {
                prefix: [slots[0], slots[1], slots[2]],
                corners_3d: [
                    [slots[3], slots[4], slots[5]],
                    [slots[6], slots[7], slots[8]],
                ],
            }
        } else {
            let slots = scalar_slots(&body, 10, &cache);
            PlaneEnvelope::Standard {
                bounds_2d: [[slots[0], slots[1]], [slots[2], slots[3]]],
                corners_3d: [
                    [slots[4], slots[5], slots[6]],
                    [slots[7], slots[8], slots[9]],
                ],
            }
        };
        envelopes.push(PlaneEnvelopeRecord {
            surface_id: row.id,
            body,
            envelope,
            row_offset: row.offset,
            offset: *body_start,
        });
    }
    envelopes
}

/// Decode fully specified scalar fields in labeled `srf_prim_ptr` prototype
/// records. A prototype is emitted only when its named kind is known.
pub fn prototypes(payload: &[u8]) -> Vec<SurfacePrototype> {
    let mut prototypes = Vec::new();
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
        prototypes.push(SurfacePrototype {
            kind,
            radius: scalar_at(b"radius\0"),
            radius2: scalar_at(b"radius2\0"),
            half_angle: scalar_at(b"half_angle\0"),
            offset: record,
        });
    }
    prototypes
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
            7, 0x22, 4, 0x01, 0, 8, // plane id 7 -> 8
            0x80, 0x80, 0x24, 0x81, 0x01, 0xf6, 0x06, 0x80, 0x81,
        ]; // cylinder id 128, feature 257, reversed -> 129
        assert_eq!(
            rows(&payload),
            vec![
                SurfaceRow {
                    id: 7,
                    kind: SurfaceKind::Plane,
                    feature_id: 4,
                    reversed: false,
                    boundary_type: 0,
                    next_surface: 8,
                    offset: 0,
                },
                SurfaceRow {
                    id: 128,
                    kind: SurfaceKind::Cylinder,
                    feature_id: 257,
                    reversed: true,
                    boundary_type: 6,
                    next_surface: 129,
                    offset: 6,
                },
            ]
        );
    }

    #[test]
    fn rejects_rows_without_the_fixed_discriminators() {
        assert!(rows(&[7, 0x22, 4, 0x02, 0, 8]).is_empty());
        assert!(rows(&[7, 0x22, 4, 0x01, 0x20, 8]).is_empty());
    }

    #[test]
    fn decodes_named_prototype_scalars_without_promoting_them_to_instances() {
        let payload = b"srf_prim_ptr\0geom_type\0\x24radius\0\x2a\xf4\0\
                        srf_prim_ptr\0geom_type\0\x25half_angle\0\x29\xe8\0";
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
                    half_angle: Some(0.75),
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
    fn withholds_incomplete_scalar_field_from_compact_integer_fallback() {
        let payload = b"srf_prim_ptr(torus)\0\xe0\x01radius1\0\x18\xe3";
        let records = named_prototype_records(payload);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].parameters.len(), 1);
        assert_eq!(
            records[0].parameters[0].value,
            SurfaceNamedValue::Opaque(vec![0x18])
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
            row_offset: 10,
            offset: 20,
        }];
        assert!(outline_planes(&records).is_empty());
    }
}
