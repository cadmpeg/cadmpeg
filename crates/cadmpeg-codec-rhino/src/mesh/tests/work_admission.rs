// SPDX-License-Identifier: Apache-2.0
use super::{chunk, compressed_mesh, with_expand};
use crate::chunks::{ArchiveVersion, BoundedReader};
use crate::loss::Diagnostics;
use crate::mesh::{decode, read_ngons, MeshBudget, MeshDecodeOptions, MeshId};
use crate::settings::MillimeterScale;

#[test]
fn ordinary_mesh_has_no_proxy_fingerprint() {
    let bytes = compressed_mesh();
    let mesh = with_expand(&bytes, |expand| {
        decode(
            expand,
            &bytes,
            0..bytes.len(),
            ArchiveVersion::V5,
            MeshDecodeOptions {
                writer_version: None,
                association: None,
                id: MeshId::Ready(
                    cadmpeg_ir::tessellation::TessellationId::mint(
                        "synthetic:test:tessellation#ordinary",
                    )
                    .unwrap(),
                ),
                scale: MillimeterScale::IDENTITY,
                userdata: &[],
            },
            &mut MeshBudget::new(),
        )
    })
    .unwrap();
    assert!(mesh.proxy_fingerprint.is_none());
}

#[test]
fn mesh_proxy_fingerprint_counts_faces_and_vertices() {
    let faces = [[0, 1, 2, 2]];
    let vertices = [[cadmpeg_ir::scalar::FiniteBinary32::new(0.0).unwrap(); 3]; 3];
    let fingerprint = crate::mesh::native_proxy_fingerprint(&faces, &vertices);
    assert_eq!(fingerprint.face_count, 1);
    assert_eq!(fingerprint.vertex_count, 3);
}

#[test]
fn current_mesh_ngon_records_read_one_ngon() {
    let mut body = Vec::new();
    for value in [1_u32, 0, 1, 3, 1, 0, 1, 2, 0] {
        body.extend(value.to_le_bytes());
    }
    let bytes = chunk(&body);
    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
    assert_eq!(
        read_ngons(
            &mut reader,
            ArchiveVersion::V5,
            3,
            1,
            &mut Diagnostics::new()
        )
        .unwrap(),
        1
    );
}
