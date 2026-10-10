// SPDX-License-Identifier: Apache-2.0

use super::{
    container, decode_mesh_container_charged, CodecError, GUID, TRIANGLE_CORNERS, TRIANGLE_VERTICES,
};
use cadmpeg_core::decode::{DecodePolicy, View};

#[test]
fn paramesh_chunk_errors_release_the_live_name_table_storage() {
    let bytes = container(GUID, &TRIANGLE_VERTICES, &TRIANGLE_CORNERS);
    let count =
        usize::try_from(View::u64_le_at(&bytes, super::super::PROTOBUF_COUNT_AT).unwrap()).unwrap();
    let table_start = super::super::PROTOBUF_AT + count;
    let table_count = usize::try_from(View::u64_le_at(&bytes, table_start).unwrap()).unwrap();
    let table_end = table_start + 12 + table_count;
    let mut repeated = bytes[..table_end].to_vec();
    repeated.extend_from_slice(&bytes[table_start..table_end]);
    let mut undefined = bytes[..table_end].to_vec();
    undefined.extend_from_slice(&0u64.to_le_bytes());
    undefined.extend_from_slice(&5u32.to_le_bytes());
    let mut truncated = bytes[..table_end].to_vec();
    truncated.extend_from_slice(&1u64.to_le_bytes());
    truncated.extend_from_slice(&super::CHUNK_STREAM.to_le_bytes());
    for (bytes, expected) in [
        (repeated, "paramesh container repeats its name table"),
        (
            undefined,
            "paramesh container holds an undefined chunk kind",
        ),
        (truncated, "paramesh chunk body is truncated"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 16 * 1024;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_decompressed_bytes_total = 0;
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let result = ctx.with_scoped_storage("test malformed paramesh candidate", || {
                decode_mesh_container_charged(ctx, &bytes)
            });
            assert!(matches!(result, Err(CodecError::Malformed(message)) if message == expected));
            assert_eq!(ctx.resource_refusal(), None);
            // Every temporary registry/name-table/index backing is gone.
            let storage = ctx
                .reserve_scoped(
                    policy.limits.max_materialized_bytes,
                    "reuse released paramesh temporary backing",
                )
                .unwrap();
            drop(storage);
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
}

#[test]
fn paramesh_semantic_field_refusal_admits_one_reached_field() {
    use super::super::{
        channel_group, channel_streams, mesh_registry, registry_channel, registry_property,
        MeshAttributeDomain,
    };
    use cadmpeg_core::decode::ResourceDimension;
    type Reader = fn(&cadmpeg_core::decode::DecodeContext<'_>, &[u8]) -> Result<(), CodecError>;
    let readers: [(Reader, &str); 5] = [
        (
            |ctx, bytes| channel_streams(ctx, bytes).map(|_| ()),
            "read paramesh protobuf entry fields",
        ),
        (
            |ctx, bytes| channel_group(ctx, bytes).map(|_| ()),
            "read paramesh protobuf entry fields",
        ),
        (
            |ctx, bytes| registry_channel(ctx, bytes, MeshAttributeDomain::Triangle).map(|_| ()),
            "read paramesh protobuf entry fields",
        ),
        (
            |ctx, bytes| registry_property(ctx, bytes).map(|_| ()),
            "read paramesh registry property fields",
        ),
        (
            |ctx, bytes| mesh_registry(ctx, bytes).map(|_| ()),
            "read paramesh protobuf message fields",
        ),
    ];
    for (read, operation) in readers {
        let mut bytes = super::varint_field(99, 0);
        for _ in 0..128 {
            bytes.extend(super::varint_field(1, 0));
        }
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| read(ctx, &bytes),
        );
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 1 && limit.operation == operation)
        );
    }
}

#[test]
fn paramesh_lzma_refuses_work_before_library_decoder() {
    use cadmpeg_core::decode::ResourceDimension;
    let chunk = super::stream_chunk(&[0x80], &[7; 1024]);
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "paramesh LZMA work",
        0,
        |ctx| super::inflate_stream_charged(ctx, &chunk[12..]),
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.operation == "paramesh LZMA work")
    );
}

#[test]
fn paramesh_index_validation_refuses_one_reached_position() {
    use cadmpeg_core::decode::ResourceDimension;
    // The terminal value is 100; zero deltas resolve every value to 100.
    let mut stream = vec![0; 128 * 4];
    stream[127 * 4..].copy_from_slice(&100_u32.to_le_bytes());
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "validate paramesh index positions",
        0,
        |ctx| super::super::decode_index_positions(ctx, &stream, 128, 0, 3),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 1));
}

#[test]
fn paramesh_message_pack_readers_admit_one_reached_entry() {
    use cadmpeg_core::decode::ResourceDimension;
    let mut names = vec![0xde, 0, 128, 0xa0, 0];
    names.extend(std::iter::repeat_n([0xa1, b'A', 0], 127).flatten());
    let mut descriptor = vec![0x8f, 0];
    descriptor.extend(std::iter::repeat_n([0xa1, b'D', 0], 14).flatten());
    for (bytes, operation, name_table) in [
        (names, "visit paramesh stream-name entries", true),
        (
            descriptor,
            "visit paramesh stream descriptor entries",
            false,
        ),
    ] {
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| {
                if name_table {
                    super::super::message_pack_name_table(ctx, &bytes).map(|_| ())
                } else {
                    super::super::stream_descriptor(ctx, &bytes).map(|_| ())
                }
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 1));
    }
}

#[test]
fn paramesh_corner_selector_validation_admits_one_reached_row() {
    use super::super::{
        FloatWidth, MeshAttribute, MeshAttributeAddressing, MeshElements, UniqueFaceGroups,
    };
    use cadmpeg_core::decode::ResourceDimension;
    for overrides in [false, true] {
        let attribute = MeshAttribute {
            role: 0,
            resource_guid: None,
            authored_name: None,
            groups: UniqueFaceGroups::default(),
            elements: MeshElements::Float {
                width: FloatWidth::Pair,
                values: vec![0; if overrides { 131 * 8 } else { 3 * 8 }],
            },
            addressing: if overrides {
                MeshAttributeAddressing::Corner(vec![384; 128])
            } else {
                MeshAttributeAddressing::Vertex
            },
        };
        let triangles = vec![if overrides { [0, 1, 2] } else { [3, 0, 0] }; 128];
        let operation = if overrides {
            "validate paramesh corner override positions"
        } else {
            "validate paramesh corner selectors"
        };
        let error = crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| attribute.corner_selectors(ctx, 3, &triangles),
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 1));
        crate::test_support::with_decode_context(|ctx| {
            assert!(attribute
                .corner_selectors(ctx, 3, &triangles)
                .unwrap()
                .is_none());
        });
    }
}

#[test]
fn paramesh_duplicate_name_ids_and_face_channels_leave_the_tail_unread() {
    use super::super::{
        MeshAttribute, MeshAttributeAddressing, MeshElements, TriangleDeltaStream,
        UniqueFaceGroups, UniqueStreamNames,
    };
    use cadmpeg_core::decode::ResourceDimension;
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "validate paramesh stream names",
        1,
        |ctx| {
            UniqueStreamNames::new(ctx, (0..128).map(|i| (format!("stream{i}"), 0)).collect())
                .map(|_| ())
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 1));
    let attributes: Vec<_> = (0..128)
        .map(|_| MeshAttribute {
            role: 0,
            resource_guid: None,
            authored_name: None,
            groups: UniqueFaceGroups::default(),
            elements: MeshElements::TriangleDelta(TriangleDeltaStream {
                values: Vec::new(),
                decoded: Vec::new(),
            }),
            addressing: MeshAttributeAddressing::Triangle,
        })
        .collect();
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "select paramesh face-group channel",
        1,
        |ctx| super::super::registry_triangle_groups(ctx, &attributes, 0),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 1));
}

#[test]
fn paramesh_face_group_membership_admits_one_reached_triangle() {
    use super::super::{
        MeshAttribute, MeshAttributeAddressing, MeshElements, TriangleDeltaStream, UniqueFaceGroups,
    };
    use cadmpeg_core::decode::ResourceDimension;
    let attributes = [MeshAttribute {
        role: 0,
        resource_guid: None,
        authored_name: None,
        groups: UniqueFaceGroups(vec![(0, GUID.to_owned())]),
        elements: MeshElements::TriangleDelta(TriangleDeltaStream {
            values: vec![0; 128 * 4],
            decoded: vec![9; 128],
        }),
        addressing: MeshAttributeAddressing::Triangle,
    }];
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "assign paramesh face groups",
        0,
        |ctx| super::super::registry_triangle_groups(ctx, &attributes, 1),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 1));
}

#[test]
fn paramesh_missing_attribute_resource_admits_one_reached_name() {
    use super::super::{AttributeNameKind, RegisteredAttributeName};
    use cadmpeg_core::decode::ResourceDimension;
    let names = (0..128)
        .map(|i| {
            (
                format!("00000000-0000-0000-0000-{i:012x}"),
                RegisteredAttributeName {
                    kind: AttributeNameKind::Color,
                    authored_name: String::new(),
                },
            )
        })
        .collect();
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "validate paramesh attribute resources",
        0,
        |ctx| {
            super::super::registry_attributes(
                ctx,
                &[],
                &std::collections::HashMap::new(),
                &names,
                0,
                0,
            )
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 1));
}
