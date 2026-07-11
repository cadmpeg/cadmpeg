// SPDX-License-Identifier: Apache-2.0
//! Surface namespace rows and prototype parameters.
//!
//! A [`SurfaceRow`] identifies a surface family and its feature, orientation,
//! boundary, and namespace links. A [`SurfacePrototype`] contains named template
//! parameters. Prototype values do not locate a surface instance in model space.

use crate::psb::compact_int;
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

const BOUNDARY_TYPES: &[u8] = &[0x00, 0x01, 0x06, 0xf6];

/// Discover positional rows from every `srf_array` namespace in `payload`.
/// The scan anchors on the surface-kind byte and validates both adjacent
/// compact-integer fields and the orientation/boundary discriminators. This
/// retains only byte-backed rows; a link target never inherits a kind.
pub fn rows(payload: &[u8]) -> Vec<SurfaceRow> {
    let mut result = Vec::new();
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
}
