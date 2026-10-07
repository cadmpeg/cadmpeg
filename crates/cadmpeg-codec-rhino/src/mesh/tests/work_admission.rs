// SPDX-License-Identifier: Apache-2.0
use super::{buffer, chunk, compressed_mesh, with_expand, with_expand_policy};
use crate::chunks::{ArchiveVersion, BoundedReader};
use crate::curves::GeometryError;
use crate::loss::Diagnostics;
use crate::mesh::{
    decode, read_buffer, read_ngons, MeshBudget, MeshBufferSpec, MeshDecodeOptions, MeshId,
};
use crate::settings::MillimeterScale;
use cadmpeg_core::{
    decode::{DecodePolicy, ResourceDimension},
    CodecError,
};

fn codec_error(error: GeometryError) -> CodecError {
    match error {
        GeometryError::Codec(error) => error,
        other => panic!("unexpected geometry error: {other}"),
    }
}

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
fn mesh_proxy_fingerprint_refuses_hash_work_before_hashing() {
    let faces = [[0, 1, 2, 2]];
    let vertices = [[cadmpeg_ir::scalar::FiniteBinary32::new(0.0).unwrap(); 3]; 3];
    // One face and three vertices require four record visits. Each record has a fixed field count.
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "Rhino mesh proxy SHA-1", |cap| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        with_expand_policy(&[], policy, |expand| {
            let result = crate::mesh::native_proxy_fingerprint(&faces, &vertices, expand.ctx());
            if let Err(CodecError::ResourceLimit(limit)) = &result {
                assert_eq!(limit.additional, 1);
                assert_eq!(expand.ctx().resource_refusal().as_ref(), Some(limit));
            }
            result
        })
    });
    with_expand(&[], |expand| {
        let fingerprint =
            crate::mesh::native_proxy_fingerprint(&faces, &vertices, expand.ctx()).unwrap();
        assert_eq!(fingerprint.face_count, 1);
        assert_eq!(fingerprint.vertex_count, 3);
    });
}

#[test]
fn mesh_buffer_crc_refuses_its_own_scan_work() {
    let bytes = buffer(&[1, 2, 3, 4], 0);
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 4;
    with_expand_policy(&bytes, policy, |expand| {
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
        let error = read_buffer(
            expand,
            &mut reader,
            MeshBufferSpec {
                expected: 4,
                name: "fixture",
            },
            &mut Diagnostics::new(),
            &mut MeshBudget::new(),
            ArchiveVersion::V5,
        )
        .unwrap_err();
        assert!(
            matches!(error, GeometryError::Codec(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "Rhino mesh buffer checksum bytes"
                && limit.used == 4 && limit.additional == 4)
        );
        assert!(expand.ctx().resource_refusal().is_some());
    });
}

#[test]
fn compressed_mesh_chunk_crc_propagates_work_refusal() {
    let bytes = buffer(&[1, 2, 3, 4], 1);
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "Rhino chunk checksum bytes",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            with_expand_policy(&bytes, policy, |expand| {
                let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
                read_buffer(
                    expand,
                    &mut reader,
                    MeshBufferSpec {
                        expected: 4,
                        name: "fixture",
                    },
                    &mut Diagnostics::new(),
                    &mut MeshBudget::new(),
                    ArchiveVersion::V5,
                )
                .map(|_| ())
                .map_err(codec_error)
            })
        },
    );
}

#[test]
fn current_mesh_ngon_records_and_indices_refuse_work() {
    let mut body = Vec::new();
    for value in [1_u32, 0, 1, 3, 1, 0, 1, 2, 0] {
        body.extend(value.to_le_bytes());
    }
    let bytes = chunk(&body);
    for operation in [
        "Rhino current mesh ngon records",
        "Rhino current mesh ngon indices",
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                with_expand_policy(&bytes, policy, |expand| {
                    let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
                    read_ngons(
                        expand.ctx(),
                        &mut reader,
                        ArchiveVersion::V5,
                        3,
                        1,
                        &mut Diagnostics::new(),
                    )
                    .map(|_| ())
                    .map_err(codec_error)
                })
            },
        );
    }
    with_expand(&bytes, |expand| {
        let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
        assert_eq!(
            read_ngons(
                expand.ctx(),
                &mut reader,
                ArchiveVersion::V5,
                3,
                1,
                &mut Diagnostics::new()
            )
            .unwrap(),
            1
        );
    });
}

#[test]
fn compressed_mesh_source_equality_preserves_work_refusal() {
    let bytes = buffer(&[1, 2, 3, 4], 1);
    let independent_bytes = bytes.clone();
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "Rhino compressed mesh source equality",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            with_expand_policy(&bytes, policy, |expand| {
                let mut reader = BoundedReader::new(&independent_bytes, 0, independent_bytes.len()).unwrap();
                let result = read_buffer(
                    expand,
                    &mut reader,
                    MeshBufferSpec {
                        expected: 4,
                        name: "test",
                    },
                    &mut Diagnostics::new(),
                    &mut MeshBudget::new(),
                    ArchiveVersion::V5,
                )
                .map(|_| ())
                .map_err(codec_error);
                if let Err(CodecError::ResourceLimit(refusal)) = &result {
                    assert_eq!(expand.ctx().resource_refusal().as_ref(), Some(refusal));
                }
                result
            })
        },
    );
}
