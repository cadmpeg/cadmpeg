// SPDX-License-Identifier: Apache-2.0
//! SLDPRT display-list tessellation emission.

use std::borrow::Cow;
use cadmpeg_core::convert::f32_from_f64;
use cadmpeg_core::decode::{index_from_u32, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;

pub(super) fn tessellation_payload(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    length_scale: f64,
) -> Result<Vec<u8>, CodecError> {
    type AuxiliaryWriter = fn(&mut Vec<u8>, &[u32]) -> Result<(), CodecError>;
    let meshes = ir
        .model
        .tessellations
        .iter()
        .map(sequential_tessellation)
        .collect::<Result<Vec<_>, _>>()?;
    let mut out = b"uoTempBodyTessData_c".to_vec();
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(b"uoTempFaceTessData_c");
    let (triangle_count, strip_count) = match meshes.first() {
        Some(mesh) => (
            u32::try_from(mesh.triangle_count()).map_err(|_| {
                CodecError::NotImplemented("tessellation triangle count overflow".into())
            })?,
            u32::try_from(mesh.strip_lengths().len()).map_err(|_| {
                CodecError::NotImplemented("tessellation strip count overflow".into())
            })?,
        ),
        None => (0, 0),
    };
    out.extend_from_slice(&triangle_count.to_le_bytes());
    out.extend_from_slice(&strip_count.to_le_bytes());
    for mesh in &meshes {
        let strips = mesh
            .strip_lengths()
            .iter()
            .flat_map(|run| run.to_le_bytes())
            .collect::<Vec<_>>();
        descriptor(&mut out, 4, 8, 2, mesh.strip_lengths().len(), &strips)?;
        let mut positions = Vec::with_capacity(mesh.vertex_count() * 12);
        for point in mesh.vertices() {
            for value in [point.x, point.y, point.z] {
                positions.extend_from_slice(
                    &tessellation_f32(value * length_scale, "position")?.to_le_bytes(),
                );
            }
        }
        descriptor(&mut out, 12, 100, 2, mesh.vertex_count(), &positions)?;
        let mut normals = Vec::with_capacity(mesh.vertex_normals().len() * 12);
        for normal in mesh.vertex_normals() {
            for value in [normal.x, normal.y, normal.z] {
                normals.extend_from_slice(&tessellation_f32(value, "normal")?.to_le_bytes());
            }
        }
        descriptor(&mut out, 12, 100, 2, mesh.vertex_normals().len(), &normals)?;
        // `has_core_tessellation_channels` matches at least three channels, so
        // the remainder after the core three is the complete auxiliary run.
        // The display-list table carries exactly three auxiliary channels; the
        // consistency check below refuses any other number rather than
        // truncating the run the mesh states.
        let auxiliary_start = usize::from(has_core_tessellation_channels(mesh.channels())) * 3;
        let auxiliary = &mesh.channels()[auxiliary_start..];
        let strip_lengths = mesh
            .strip_lengths()
            .iter()
            .map(|run| usize::try_from(*run))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| CodecError::Malformed("tessellation strip length overflow".into()))?;
        if !auxiliary.is_empty()
            && !crate::tessellation::auxiliary_channels_are_consistent(
                ctx,
                &strip_lengths,
                auxiliary,
            )?
        {
            return Err(CodecError::InvalidInput(
                "tessellation channels: the SLDPRT display-list table carries exactly three \
                 auxiliary channels after the strip, position and normal channels"
                    .into(),
            ));
        }
        for channel in auxiliary {
            descriptor(
                &mut out,
                channel.item_size(),
                channel.kind(),
                channel.flags(),
                index_from_u32(channel.count()),
                channel.data(),
            )?;
        }
        let list_c = mesh
            .strip_lengths()
            .iter()
            .map(|run| {
                run.checked_mul(2)
                    .and_then(|value| value.checked_sub(2))
                    .ok_or_else(|| {
                        CodecError::NotImplemented(
                            "tessellation strip is too short or too large".into(),
                        )
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let append: [AuxiliaryWriter; 3] = [
            |out, _| descriptor(out, 4, 8, 2, 0, &[]),
            |out, list_c| {
                let data = list_c
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect::<Vec<_>>();
                descriptor(out, 4, 8, 2, list_c.len(), &data)
            },
            |out, _| descriptor(out, 1, 8, 2, 0, &[]),
        ];
        // The run is empty or the exact three the table carries, so the
        // synthesized defaults fill in only for a mesh that states none.
        for append in append.iter().skip(auxiliary.len()) {
            append(&mut out, &list_c)?;
        }
    }
    Ok(out)
}

fn has_core_tessellation_channels(
    channels: &[cadmpeg_ir::tessellation::TessellationChannel],
) -> bool {
    matches!(channels, [strips, positions, normals, ..]
        if (strips.item_size(), strips.kind()) == (4, 8)
            && (positions.item_size(), positions.kind()) == (12, 100)
            && (normals.item_size(), normals.kind()) == (12, 100))
}

pub(super) fn sequential_tessellation(
    mesh: &cadmpeg_ir::tessellation::Tessellation,
) -> Result<Cow<'_, cadmpeg_ir::tessellation::Tessellation>, CodecError> {
    if !mesh.triangle_groups().is_empty() || !mesh.texture_assignments().is_empty() {
        return Err(CodecError::NotImplemented(
            "SLDPRT display tessellation triangle groups and texture assignments are not writable"
                .into(),
        ));
    }
    if !mesh.feature_edges().is_empty() {
        return Err(CodecError::NotImplemented(
            "SLDPRT display tessellation feature edges are not writable".into(),
        ));
    }
    if matches!(
        mesh.mesh(),
        cadmpeg_ir::tessellation::TessellationMesh::Strips { .. }
            | cadmpeg_ir::tessellation::TessellationMesh::ShadedStrips { .. }
    ) {
        return Ok(Cow::Borrowed(mesh));
    }
    let source_vertices = mesh.vertices();
    let source_normals = mesh.vertex_normals();
    let corner_normals = mesh.per_corner_normals();
    let triangles = mesh.triangles();
    let indices = triangles
        .iter()
        .flat_map(|triangle| triangle.iter().copied())
        .map(|index| {
            usize::try_from(index)
                .map_err(|_| CodecError::Malformed("tessellation index is out of bounds".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let positions = indices
        .iter()
        .map(|index| source_vertices[*index])
        .collect::<Vec<_>>();
    let normals = if corner_normals.is_empty() {
        if source_normals.is_empty() {
            Vec::new()
        } else {
            indices
                .iter()
                .map(|index| source_normals[*index])
                .collect::<Vec<_>>()
        }
    } else {
        corner_normals
    };
    let mut channels = Vec::new();
    for channel in mesh.channels() {
        if usize::try_from(channel.count()).ok() != Some(source_vertices.len()) {
            channels.push(channel.clone());
            continue;
        }
        let item_size = usize::try_from(channel.item_size())
            .map_err(|_| CodecError::Malformed("tessellation channel item size overflow".into()))?;
        let data = indices
            .iter()
            .flat_map(|index| {
                let start = index * item_size;
                channel.data()[start..start + item_size].iter().copied()
            })
            .collect();
        channels.push(
            cadmpeg_ir::tessellation::TessellationChannel::new(
                channel.addressing().clone(),
                channel.item_size(),
                channel.kind(),
                channel.flags(),
                data,
            )
            .map_err(|err| CodecError::Malformed(err.to_string()))?,
        );
    }
    // One triangle per strip: the SLDPRT display list states strips, so a
    // triangle list is written as three-vertex runs.
    let rows = if normals.is_empty() {
        strip_rows(&positions)?.map_or(
            cadmpeg_ir::tessellation::TessellationMesh::List {
                vertices: Vec::new(),
                triangles: Vec::new(),
            },
            |strips| cadmpeg_ir::tessellation::TessellationMesh::Strips { strips },
        )
    } else {
        let shaded = positions
            .into_iter()
            .zip(normals)
            .map(|(position, normal)| cadmpeg_ir::tessellation::ShadedVertex { position, normal })
            .collect::<Vec<_>>();
        strip_rows(&shaded)?.map_or(
            cadmpeg_ir::tessellation::TessellationMesh::List {
                vertices: Vec::new(),
                triangles: Vec::new(),
            },
            |strips| cadmpeg_ir::tessellation::TessellationMesh::ShadedStrips { strips },
        )
    };
    mesh.clone()
        .with_mesh(rows, channels)
        .map(Cow::Owned)
        .map_err(|err| CodecError::Malformed(err.to_string()))
}

/// Cut a corner-expanded vertex run into one three-vertex strip per triangle.
fn strip_rows<V: Clone>(
    vertices: &[V],
) -> Result<Option<cadmpeg_ir::tessellation::Strips<V>>, CodecError> {
    let mut strips = Vec::with_capacity(vertices.len() / 3);
    for corners in vertices.chunks(3) {
        strips.push(
            cadmpeg_ir::tessellation::Strip::new(corners.to_vec()).ok_or_else(|| {
                CodecError::Malformed("tessellation triangle spans fewer than three corners".into())
            })?,
        );
    }
    Ok(cadmpeg_ir::tessellation::Strips::new(strips))
}

fn tessellation_f32(value: f64, role: &str) -> Result<f32, CodecError> {
    f32_from_f64(value).ok_or_else(|| {
        CodecError::malformed(format_args!("SLDPRT tessellation {role} exceeds f32 range"))
    })
}

fn descriptor(
    out: &mut Vec<u8>,
    item_size: u32,
    kind: u32,
    flags: u32,
    count: usize,
    data: &[u8],
) -> Result<(), CodecError> {
    let count = u32::try_from(count).map_err(|_| {
        CodecError::NotImplemented("SLDPRT display-list channel count exceeds u32".into())
    })?;
    out.extend_from_slice(&item_size.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(data);
    Ok(())
}

#[cfg(test)]
mod conversion_correction_tests {
    use super::tessellation_f32;

    const RELATIVE_F32_OVERFLOW: f64 = 1e-10;

    #[test]
    fn tessellation_f32_refuses_a_value_above_f32_max_that_the_cast_rounded_down() {
        // A value a hair above `f32::MAX` rounded to `f32::MAX` under `as`; the
        // sum with `1.0` would not do, since `1.0` is below the spacing of
        // `f64` at that magnitude.
        let just_above = f64::from(f32::MAX) * (1.0 + RELATIVE_F32_OVERFLOW);
        assert!(just_above > f64::from(f32::MAX));
        assert!(tessellation_f32(just_above, "position").is_err());
        assert_eq!(
            tessellation_f32(f64::from(f32::MAX), "position").expect("exact maximum"),
            f32::MAX
        );
        assert!(tessellation_f32(f64::NAN, "position").is_err());
        assert!(tessellation_f32(f64::INFINITY, "position").is_err());
    }
}

