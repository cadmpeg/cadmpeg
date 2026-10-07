// SPDX-License-Identifier: Apache-2.0
use super::{
    buffer, chunk, compressed_mesh, v5_double_userdata_descriptor, with_expand, with_expand_policy,
};
use crate::chunks::{ArchiveVersion, BoundedReader, FramingError};
use crate::curves::GeometryError;
use crate::loss::Diagnostics;
use crate::mesh::{
    decode, read_buffer, read_v5_double_vertices, MeshBudget, MeshBufferSpec, MeshDecodeOptions,
    MeshId,
};
use crate::settings::MillimeterScale;
use cadmpeg_core::{decode::DecodePolicy, CodecError};

#[test]
fn mesh_per_buffer_ceiling_fuses_and_propagates_resource_refusal() {
    let bytes = buffer(&[0; 36], 1);
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_per_expand = 32;
    with_expand_policy(&bytes, policy, |expand| {
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
        let error = read_buffer(
            expand,
            &mut reader,
            MeshBufferSpec {
                expected: 36,
                name: "vertices",
            },
            &mut Diagnostics::new(),
            &mut MeshBudget::new(),
            ArchiveVersion::V5,
            None,
        )
        .unwrap_err();
        let GeometryError::Codec(CodecError::ResourceLimit(limit)) = error else {
            panic!("expected unchanged codec resource refusal");
        };
        assert_eq!(limit.operation, "Rhino mesh buffer output bytes");
        assert_eq!(limit.limit, 32);
        assert_eq!(limit.used + limit.additional, 36);
        assert_eq!(expand.ctx().resource_refusal(), Some(limit));
        assert_eq!(reader.position(), 4);
    });
}

#[test]
fn mesh_per_buffer_refusal_reaches_full_decode_with_other_geometry() {
    let mesh = compressed_mesh();
    let bytes = crate::test_support::test_archive::archive(&[
        crate::test_support::test_archive::object_record(
            0x20,
            crate::mesh::ON_MESH.to_wire(),
            &mesh,
        ),
        crate::test_support::test_archive::object_record(
            1,
            crate::test_support::test_dump::POINT_CLASS,
            &crate::test_support::test_dump::point_payload([2.0, 3.0, 4.0]),
        ),
    ]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_per_expand = 32;
    let result = cadmpeg_test_support::decode::full(&crate::RhinoCodec, &bytes, &policy);
    assert!(
        matches!(result, Err(cadmpeg_ir::codec::DecodeFailure::Codec(CodecError::ResourceLimit(limit)))
        if limit.operation == "Rhino mesh buffer output bytes")
    );
}

#[test]
fn document_mesh_ceiling_refuses_before_copy_and_preserves_usage() {
    let bytes = buffer(&[1, 2, 3, 4], 0);
    with_expand(&bytes, |expand| {
        let mut budget = MeshBudget::with_limit(7);
        let mut first = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
        read_buffer(
            expand,
            &mut first,
            MeshBufferSpec {
                expected: 4,
                name: "first",
            },
            &mut Diagnostics::new(),
            &mut budget,
            ArchiveVersion::V5,
            None,
        )
        .unwrap();
        assert_eq!(budget.used(), 4);
        let mut second = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
        let error = read_buffer(
            expand,
            &mut second,
            MeshBufferSpec {
                expected: 4,
                name: "second",
            },
            &mut Diagnostics::new(),
            &mut budget,
            ArchiveVersion::V5,
            None,
        )
        .unwrap_err();
        let GeometryError::Codec(CodecError::ResourceLimit(limit)) = error else {
            panic!("expected unchanged codec resource refusal");
        };
        assert_eq!(limit.operation, "Rhino document mesh buffer bytes");
        assert_eq!(limit.limit, 7);
        assert_eq!(limit.used + limit.additional, 8);
        assert_eq!(expand.ctx().resource_refusal(), Some(limit));
        assert_eq!(budget.used(), 4);
        assert_eq!(second.position(), 4);
    });
}

#[test]
fn independent_mesh_channels_each_use_the_per_expansion_ceiling() {
    for method in [0, 1] {
        let mut bytes = buffer(&[1; 12], method);
        bytes.extend(buffer(&[2; 12], method));
        let mut policy = DecodePolicy::service();
        policy.limits.max_decompressed_bytes_per_expand = 16;
        with_expand_policy(&bytes, policy, |expand| {
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
            let mut budget = MeshBudget::new();
            for value in [1_u8, 2] {
                let result = read_buffer(
                    expand,
                    &mut reader,
                    MeshBufferSpec {
                        expected: 12,
                        name: "channel",
                    },
                    &mut Diagnostics::new(),
                    &mut budget,
                    ArchiveVersion::V5,
                    None,
                )
                .unwrap()
                .unwrap();
                assert_eq!(result.as_ref(), [value; 12]);
            }
            assert_eq!(budget.used(), 24);
            assert_eq!(reader.remaining(), 0);
            assert!(expand.ctx().resource_refusal().is_none());
        });
    }
}

#[test]
fn truncated_v5_double_vertices_are_malformed_before_collection_admission() {
    let mut body = Vec::new();
    for value in [1_u32, 0, 3, 3, 0, 0, 100_000] {
        body.extend(value.to_le_bytes());
    }
    let bytes = chunk(&body);
    let descriptor = v5_double_userdata_descriptor(0..bytes.len());
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    with_expand_policy(&bytes, policy, |expand| {
        let result = read_v5_double_vertices(
            expand.ctx(),
            &bytes,
            descriptor.known().unwrap(),
            ArchiveVersion::V5,
            &[],
        );
        assert!(matches!(
            result,
            Err(GeometryError::Malformed(FramingError::Truncated {
                needed: 2_400_000,
                ..
            }))
        ));
        assert!(expand.ctx().resource_refusal().is_none());
    });
}

#[test]
fn truncated_v5_double_userdata_retains_valid_float_mesh() {
    let mut bytes = compressed_mesh();
    let payload_end = bytes.len();
    let mut body = Vec::new();
    for value in [1_u32, 0, 3, 3, 0, 0, 100_000] {
        body.extend(value.to_le_bytes());
    }
    bytes.extend(chunk(&body));
    let descriptor = v5_double_userdata_descriptor(payload_end..bytes.len());
    let mesh = with_expand(&bytes, |expand| {
        decode(
            expand,
            &bytes,
            0..payload_end,
            ArchiveVersion::V5,
            MeshDecodeOptions {
                writer_version: None,
                association: None,
                id: MeshId::Ready(
                    cadmpeg_ir::tessellation::TessellationId::mint(
                        "synthetic:test:tessellation#truncated-double",
                    )
                    .unwrap(),
                ),
                scale: MillimeterScale::IDENTITY,
                userdata: std::slice::from_ref(&descriptor),
            },
            &mut MeshBudget::new(),
        )
    })
    .unwrap();
    assert_eq!(mesh.tessellation.vertices().len(), 3);
    assert!(mesh.warnings.iter().any(|warning| warning
        .contains("V5 mesh double-precision userdata")
        && warning.contains("dropped")));
}
