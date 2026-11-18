// SPDX-License-Identifier: Apache-2.0
//! Mesh body slices (`SCMESH01`): an imported STL or 3MF body as triangles.
//!
//! A slice is the signature, a 32-bit little-endian header length, and the
//! JSON header (`positions`, `triangles`, and `corner_normals` counts, the
//! `color`, and the `paint`). Then come each position as three binary64,
//! each triangle as three 32-bit vertex indices, and, when the header counts
//! them, three corner normals of three binary64 per triangle, all
//! little-endian. The counts account for every byte after the header.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::tessellation::{ShadedTriangle, TessellationMesh};
use serde_json::Value;

/// The signature of a mesh body slice.
pub(super) const SIGNATURE: &[u8; 8] = b"SCMESH01";
/// Most bytes of a mesh slice header.
const MAX_HEADER: usize = 16 << 20;
/// Bytes of one position.
const POSITION_BYTES: usize = 24;
/// Bytes of one triangle.
const TRIANGLE_BYTES: usize = 12;
/// Bytes of the corner normals of one triangle.
const NORMALS_BYTES: usize = 72;

/// A decoded mesh body.
pub(super) struct MeshBody {
    /// The triangles, with corner normals when the slice carries them.
    pub(super) mesh: TessellationMesh<FinitePoint3, FiniteVector3>,
    /// The body colour, red, green, and blue in `[0, 1]`.
    pub(super) color: Option<[f64; 3]>,
}

fn malformed(what: &str) -> CodecError {
    CodecError::malformed(format_args!("mesh body slice: {what}"))
}

fn header_count(header: &Value, key: &str) -> Result<usize, CodecError> {
    header
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|count| usize::try_from(count).ok())
        .ok_or_else(|| malformed(key))
}

fn point(view: &mut View<'_>) -> Result<[f64; 3], CodecError> {
    let mut point = [0.0; 3];
    for slot in &mut point {
        *slot = view.f64_le().ok_or_else(|| malformed("truncated"))?;
    }
    Ok(point)
}

/// Reads a mesh body slice.
pub(super) fn read(ctx: &DecodeContext<'_>, slice: View<'_>) -> Result<MeshBody, CodecError> {
    let mut view = slice;
    if view.take(SIGNATURE.len()) != Some(SIGNATURE.as_slice()) {
        return Err(malformed("the slice does not start with SCMESH01"));
    }
    let length = view.u32_le().ok_or_else(|| malformed("truncated"))?;
    let length = usize::try_from(length)
        .ok()
        .filter(|length| *length <= MAX_HEADER)
        .ok_or_else(|| malformed("header length"))?;
    let header = view
        .take(length)
        .ok_or_else(|| malformed("truncated header"))?;
    let header = std::str::from_utf8(header).map_err(|_| malformed("the header is not UTF-8"))?;
    let header = ctx.parse_json_value(header, "SolidCraft mesh body header")?;
    let positions = header_count(&header, "positions")?;
    let triangles = header_count(&header, "triangles")?;
    let normals = match header.get("corner_normals") {
        None | Some(Value::Null) => 0,
        Some(_) => header_count(&header, "corner_normals")?,
    };
    if normals != 0 && normals != triangles {
        return Err(malformed("the corner normals do not cover the triangles"));
    }
    // The counts must account for every byte, so nothing is allocated for
    // data that is not there.
    let expected = positions
        .checked_mul(POSITION_BYTES)
        .and_then(|bytes| bytes.checked_add(triangles.checked_mul(TRIANGLE_BYTES)?))
        .and_then(|bytes| bytes.checked_add(normals.checked_mul(NORMALS_BYTES)?))
        .ok_or_else(|| malformed("counts"))?;
    if expected != view.remaining() {
        return Err(malformed("the counts do not account for the slice"));
    }
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(positions + triangles),
        "solidcraft mesh body",
    )?;
    let mut vertices = Vec::with_capacity(positions);
    for _ in 0..positions {
        let [x, y, z] = point(&mut view)?;
        vertices.push(
            FinitePoint3::new(Point3::new(x, y, z))
                .ok_or_else(|| malformed("a position is not finite"))?,
        );
    }
    let mut corners = Vec::with_capacity(triangles);
    for _ in 0..triangles {
        let mut triangle = [0_u32; 3];
        for slot in &mut triangle {
            let index = view.u32_le().ok_or_else(|| malformed("truncated"))?;
            if usize::try_from(index).map_or(true, |index| index >= positions) {
                return Err(malformed("a triangle names a vertex past the positions"));
            }
            *slot = index;
        }
        corners.push(triangle);
    }
    let mesh = if normals == 0 {
        TessellationMesh::List {
            vertices,
            triangles: corners,
        }
    } else {
        let mut shaded = Vec::with_capacity(triangles);
        for triangle in corners {
            let mut corner_normals = [FiniteVector3::ZERO; 3];
            for slot in &mut corner_normals {
                let [x, y, z] = point(&mut view)?;
                *slot = FiniteVector3::new(Vector3::new(x, y, z))
                    .ok_or_else(|| malformed("a normal is not finite"))?;
            }
            shaded.push(ShadedTriangle {
                corners: triangle,
                normals: corner_normals,
            });
        }
        TessellationMesh::CornerShadedList {
            vertices,
            triangles: shaded,
        }
    };
    let color = header
        .get("color")
        .and_then(Value::as_array)
        .and_then(|channels| {
            let channels: Vec<f64> = channels.iter().filter_map(Value::as_f64).collect();
            <[f64; 3]>::try_from(channels).ok()
        });
    Ok(MeshBody { mesh, color })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slice(
        header: &str,
        positions: &[[f64; 3]],
        triangles: &[[u32; 3]],
        normals: &[[f64; 3]],
    ) -> Vec<u8> {
        let mut bytes = SIGNATURE.to_vec();
        bytes.extend(u32::try_from(header.len()).unwrap().to_le_bytes());
        bytes.extend(header.as_bytes());
        for p in positions {
            for x in p {
                bytes.extend(x.to_le_bytes());
            }
        }
        for t in triangles {
            for i in t {
                bytes.extend(i.to_le_bytes());
            }
        }
        for n in normals {
            for x in n {
                bytes.extend(x.to_le_bytes());
            }
        }
        bytes
    }

    fn read_bytes(bytes: &[u8]) -> Result<MeshBody, CodecError> {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, root) = DecodeContext::from_root_bytes(
            bytes,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::default(),
        )
        .unwrap();
        read(&ctx, root)
    }

    const TRIANGLE: [[f64; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

    #[test]
    fn a_triangle_with_corner_normals_reads_back() {
        let header = r#"{"color":[1.0,0.5,0.0],"corner_normals":1,"paint":null,"positions":3,"triangles":1}"#;
        let bytes = slice(header, &TRIANGLE, &[[0, 1, 2]], &[[0.0, 0.0, 1.0]; 3]);
        let body = read_bytes(&bytes).unwrap();
        assert_eq!(body.color, Some([1.0, 0.5, 0.0]));
        assert!(
            matches!(body.mesh, TessellationMesh::CornerShadedList { ref triangles, .. } if triangles.len() == 1)
        );
    }

    #[test]
    fn counts_that_miss_the_bytes_or_indices_past_the_positions_are_refused() {
        let header = r#"{"positions":3,"triangles":2}"#;
        assert!(read_bytes(&slice(header, &TRIANGLE, &[[0, 1, 2]], &[])).is_err());
        let header = r#"{"positions":3,"triangles":1}"#;
        assert!(read_bytes(&slice(header, &TRIANGLE, &[[0, 1, 3]], &[])).is_err());
        assert!(read_bytes(&slice(header, &TRIANGLE, &[[0, 1, 2]], &[])).is_ok());
    }
}
