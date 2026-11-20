// SPDX-License-Identifier: Apache-2.0
//! Point clouds.
//!
//! A point cloud is scanned points kept in the design as a reference. In a
//! container its points are a `SCPNTS01` blob named by `points_blob`: the
//! magic, the point count and a colour flag (u64 little-endian each), then
//! x, y, z per point (f32) and, when flagged, r, g, b per point (u8). In
//! plain JSON `points` and `colors` hold the same bytes as base64. The points
//! are millimetres from the cloud's `origin`, and the cloud's `transform` is
//! a rigid move of them into the design. The neutral cloud holds the points
//! placed in the design. Segments, triangles, file properties, filters, and
//! display settings stay in the retained `point_clouds` section.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{IdentityKey, PointCloudId, ProductDefinitionId};
use cadmpeg_ir::point_cloud::PointCloud;
use cadmpeg_ir::report::loss::LossNote;
use serde_json::{json, Value};

use crate::loss::SolidcraftLossCode;
use crate::package::Package;

/// The blob magic of a point cloud.
const MAGIC: &[u8; 8] = b"SCPNTS01";
/// Most points one cloud may hold, as the application limits it.
const MAX_POINTS: usize = 20_000_000;

/// A cloud's points and colours.
struct Raw {
    positions: Vec<[f32; 3]>,
    colors: Vec<[u8; 3]>,
}

/// The points of a `SCPNTS01` blob.
fn blob(bytes: &[u8]) -> Option<Raw> {
    let mut view = View::over_retained(bytes);
    if view.take(MAGIC.len())? != MAGIC {
        return None;
    }
    let count = usize::try_from(view.u64_le()?)
        .ok()
        .filter(|count| *count <= MAX_POINTS)?;
    let colored = match view.u64_le()? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let coordinates = count.checked_mul(12)?;
    let rest = coordinates.checked_add(if colored { count.checked_mul(3)? } else { 0 })?;
    if view.remaining() != rest {
        return None;
    }
    let positions = positions(view.take(coordinates)?)?;
    Some(Raw {
        positions,
        colors: if colored {
            colors(view.unread())
        } else {
            Vec::new()
        },
    })
}

fn positions(bytes: &[u8]) -> Option<Vec<[f32; 3]>> {
    if !bytes.len().is_multiple_of(12) {
        return None;
    }
    let mut view = View::over_retained(bytes);
    let mut out = Vec::with_capacity(bytes.len() / 12);
    while view.remaining() > 0 {
        let point = [view.f32_le()?, view.f32_le()?, view.f32_le()?];
        if !point.iter().all(|coordinate| coordinate.is_finite()) {
            return None;
        }
        out.push(point);
    }
    Some(out)
}

fn colors(bytes: &[u8]) -> Vec<[u8; 3]> {
    bytes
        .chunks_exact(3)
        .map(|rgb| [rgb[0], rgb[1], rgb[2]])
        .collect()
}

/// Standard base64, with or without padding.
pub(crate) fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut bits = 0_u32;
    let mut held = 0_u8;
    for byte in text.bytes().filter(|byte| *byte != b'=') {
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(digit);
        held += 6;
        if held >= 8 {
            held -= 8;
            out.push(u8::try_from((bits >> held) & 0xff).ok()?);
        }
    }
    Some(out)
}

/// The points of the cloud record `record`.
fn raw<'a>(
    ctx: &DecodeContext<'a>,
    package: Option<&Package<'a>>,
    record: &Value,
) -> Result<Option<Raw>, CodecError> {
    if let Some(name) = record.get("points_blob").and_then(Value::as_str) {
        let Some(package) = package.filter(|package| package.contains(name)) else {
            return Ok(None);
        };
        return Ok(blob(package.member(ctx, name)?.window()));
    }
    let Some(points) = record
        .get("points")
        .and_then(Value::as_str)
        .and_then(base64)
        .and_then(|bytes| positions(&bytes))
    else {
        return Ok(None);
    };
    let colors = record
        .get("colors")
        .and_then(Value::as_str)
        .and_then(base64)
        .map(|bytes| colors(&bytes))
        .unwrap_or_default();
    Ok(Some(Raw {
        positions: points,
        colors,
    }))
}

fn vector(value: Option<&Value>) -> Option<[f64; 3]> {
    let value = value?;
    if let Some(array) = value.as_array() {
        return Some([
            array.first()?.as_f64()?,
            array.get(1)?.as_f64()?,
            array.get(2)?.as_f64()?,
        ]);
    }
    Some([
        value.get("x")?.as_f64()?,
        value.get("y")?.as_f64()?,
        value.get("z")?.as_f64()?,
    ])
}

/// The rows of the cloud's rigid move, identity when it has none.
fn transform(record: &Value) -> Option<[[f64; 4]; 3]> {
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    let Some(rows) = record.get("transform").and_then(Value::as_array) else {
        return Some(identity);
    };
    let mut out = identity;
    for (row, target) in rows.iter().take(3).zip(&mut out) {
        for (index, slot) in target.iter_mut().enumerate() {
            *slot = row.as_array()?.get(index)?.as_f64()?;
        }
    }
    Some(out)
}

/// The point clouds of `document`, placed in the design.
pub(crate) fn clouds<'a>(
    ctx: &DecodeContext<'a>,
    package: Option<&Package<'a>>,
    document: &serde_json::Map<String, Value>,
    ir: &mut CadIr,
) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = Vec::new();
    let namespace = cadmpeg_ir::identity_namespace!("solidcraft", "design", "point-cloud");
    // The design is the root definition, whose frame holds the points.
    let root = ProductDefinitionId::compose(
        &cadmpeg_ir::identity_namespace!("solidcraft", "design", "product"),
        IdentityKey::encode_segment("0"),
    );
    let Some(records) = document.get("point_clouds").and_then(Value::as_array) else {
        return Ok(losses);
    };
    for (index, record) in records.iter().enumerate() {
        let key = record
            .get("id")
            .and_then(Value::as_u64)
            .map_or_else(|| index.to_string(), |id| id.to_string());
        let name = record.get("name").and_then(Value::as_str).unwrap_or("");
        let placed = raw(ctx, package, record)?;
        let (Some(raw), Some(origin), Some(rows)) =
            (placed, vector(record.get("origin")), transform(record))
        else {
            losses.push(SolidcraftLossCode::DocumentSectionUndecoded.note(format!(
                "the points of cloud {name:?} do not decode, so it stays in the retained section"
            )));
            continue;
        };
        ctx.charge_entities(u64_from_index(raw.positions.len()), "decode point cloud")?;
        let placed: Vec<Value> = raw
            .positions
            .iter()
            .map(|point| {
                let local = [
                    origin[0] + f64::from(point[0]),
                    origin[1] + f64::from(point[1]),
                    origin[2] + f64::from(point[2]),
                ];
                let at = |row: usize| {
                    rows[row][0] * local[0]
                        + rows[row][1] * local[1]
                        + rows[row][2] * local[2]
                        + rows[row][3]
                };
                json!({"x": at(0), "y": at(1), "z": at(2)})
            })
            .collect();
        let colors: Vec<Value> = raw
            .colors
            .iter()
            .map(|[r, g, b]| {
                json!({
                    "r": f64::from(*r) / 255.0,
                    "g": f64::from(*g) / 255.0,
                    "b": f64::from(*b) / 255.0,
                    "a": 1.0,
                })
            })
            .collect();
        let id = PointCloudId::compose(&namespace, IdentityKey::encode_segment(&key));
        let wire = json!({
            "id": id,
            "definition": root,
            "name": name,
            "positions": placed,
            "normals": [],
            "colors": colors,
            "channels": [],
        });
        match serde_json::from_value::<PointCloud>(wire) {
            Ok(cloud) => ir.model.point_clouds.push(cloud),
            Err(error) => losses.push(
                SolidcraftLossCode::DocumentSectionUndecoded
                    .note(format!("cloud {name:?} has no neutral form: {error}")),
            ),
        }
    }
    Ok(losses)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blob_and_base64_decode_to_the_same_points() {
        let mut bytes = Vec::from(*MAGIC);
        bytes.extend_from_slice(&2_u64.to_le_bytes());
        bytes.extend_from_slice(&1_u64.to_le_bytes());
        for value in [1.0_f32, 2.0, 3.0, 4.0, 5.0, 6.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&[255, 0, 0, 0, 255, 0]);
        let raw = blob(&bytes).unwrap();
        assert_eq!(raw.positions, [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]);
        assert_eq!(raw.colors, [[255, 0, 0], [0, 255, 0]]);
        assert_eq!(base64("AQID").unwrap(), [1, 2, 3]);
        assert_eq!(base64("AQI=").unwrap(), [1, 2]);
        assert!(blob(&bytes[..bytes.len() - 1]).is_none());
        assert!(base64("A*ID").is_none());
    }
}
