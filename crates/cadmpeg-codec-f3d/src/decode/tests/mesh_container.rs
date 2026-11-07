// SPDX-License-Identifier: Apache-2.0
//! Unjoined `ParaMesh` data remain typed in container coordinates.

use std::io::{Cursor, Write};

use cadmpeg_ir::codec::{Codec, DecodeOptions};

#[test]
fn unjoined_paramesh_retains_its_decoded_local_geometry() {
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/ParaMeshGeometry.BlobParts/ParaMeshGeometry.11111111-2222-4333-8444-555555555555.paramesh", stored).unwrap();
    zip.write_all(&crate::test_support::paramesh::triangle())
        .unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    let result = crate::F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let native = crate::test_support::native_test::f3d_native(result.ir());
    let [mesh] = native.mesh_containers.as_slice() else {
        panic!("one unjoined mesh container")
    };
    assert_eq!(mesh.fusion_uuid, "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE");
    assert_eq!(mesh.container_vertices.len(), 3);
    assert_eq!(mesh.container_vertices[1].get().x, 1.0);
    assert_eq!(mesh.container_vertices[2].get().y, 1.0);
    assert_eq!(mesh.triangles, [[0, 1, 2]]);
    assert!(mesh.attributes.is_empty());
    assert!(result.ir().model.tessellations.is_empty());
    let wire = serde_json::to_value(mesh).unwrap();
    assert_eq!(
        serde_json::from_value::<crate::records::mesh_container::MeshContainerRecord>(wire)
            .unwrap(),
        *mesh
    );
}

#[test]
fn unjoined_paramesh_retains_decoded_channel_metadata() {
    use crate::paramesh::{
        FloatWidth, MeshAttribute, MeshAttributeAddressing, MeshElements, UniqueFaceGroups,
    };
    let bytes = crate::test_support::paramesh::triangle();
    let mut mesh = crate::test_support::with_decode_context(|ctx| {
        let mut parts =
            cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::F3d, ctx);
        crate::paramesh::decode_mesh_container(ctx, &mut parts, "Synthetic.paramesh", &bytes)
            .unwrap()
    });
    let uv = [0.0_f32, 0.0, 1.0, 0.0, 0.0, 1.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    mesh.attributes.push(MeshAttribute {
        role: 7,
        resource_guid: Some("AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE".into()),
        authored_name: Some("uv".into()),
        groups: UniqueFaceGroups::default(),
        elements: MeshElements::Float {
            width: FloatWidth::Pair,
            values: uv.clone(),
        },
        addressing: MeshAttributeAddressing::Vertex,
    });
    let mut unresolved = std::collections::BTreeMap::new();
    let (record, untyped) = crate::test_support::with_decode_context(|ctx| {
        super::super::unjoined_mesh_container(ctx, "mesh.paramesh", mesh, &mut unresolved).unwrap()
    });
    assert!(!untyped);
    assert!(unresolved.is_empty());
    let [attribute] = record.attributes.as_slice() else {
        panic!("one channel")
    };
    assert_eq!(attribute.authored_name.as_deref(), Some("uv"));
    let crate::records::mesh_container::MeshContainerValues::Typed { channel } = &attribute.values
    else {
        panic!("typed channel")
    };
    assert_eq!(channel.kind(), 7);
    assert_eq!(channel.flags(), 2);
    assert_eq!(channel.data(), uv);
}

#[test]
fn unjoined_paramesh_marks_unknown_channel_values_untyped() {
    use crate::paramesh::{MeshAttribute, MeshAttributeAddressing, MeshElements, UniqueFaceGroups};
    let bytes = crate::test_support::paramesh::triangle();
    let mut mesh = crate::test_support::with_decode_context(|ctx| {
        let mut parts =
            cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::F3d, ctx);
        crate::paramesh::decode_mesh_container(ctx, &mut parts, "Synthetic.paramesh", &bytes)
            .unwrap()
    });
    mesh.attributes.push(MeshAttribute {
        role: 9,
        resource_guid: None,
        authored_name: Some("unknown".into()),
        groups: UniqueFaceGroups::default(),
        elements: MeshElements::Opaque {
            code: 99,
            values: vec![1, 2, 3],
        },
        addressing: MeshAttributeAddressing::UndecodedCorner,
    });
    let mut unresolved = std::collections::BTreeMap::new();
    let (record, untyped) = crate::test_support::with_decode_context(|ctx| {
        super::super::unjoined_mesh_container(ctx, "mesh.paramesh", mesh, &mut unresolved).unwrap()
    });
    assert!(untyped);
    let [attribute] = record.attributes.as_slice() else {
        panic!("one channel")
    };
    assert!(matches!(
        attribute.values,
        crate::records::mesh_container::MeshContainerValues::Untyped {
            role: 9,
            element_code: 99,
            ..
        }
    ));
    assert_eq!(unresolved.values().sum::<usize>(), 1);
}
