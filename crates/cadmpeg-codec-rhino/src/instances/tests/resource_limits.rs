// SPDX-License-Identifier: Apache-2.0

use crate::chunks::{chunk_at, ArchiveVersion, BoundedReader, FramingError};
use crate::container::Record;
use crate::loss::Diagnostics;
use crate::test_support::test_dump::{
    anonymous_chunk, class_userdata, definition_record, definition_record_with_userdata,
    file_reference as file_reference_bytes, long_chunk, v5_definition_payload,
    v6_definition_payload,
};

fn with_collection_limit<T>(
    limit: u64,
    f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    f(&ctx)
}

fn with_retained_limit<T>(
    limit: u64,
    f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    f(&ctx)
}

fn assert_definition_retained_refusal(
    archive: ArchiveVersion,
    data: &[u8],
    limit: u64,
    operation: &str,
) {
    let chunk = chunk_at(data, 0, data.len(), archive, false).expect("definition record");
    let record = Record::long(chunk.typecode, chunk.range(), chunk.body());
    with_retained_limit(limit, |ctx| {
        let error = crate::instances::parse_definitions(
            ctx,
            data,
            std::slice::from_ref(&record),
            archive,
            0x1000_0021,
        )
        .expect_err("retained string exceeds limit");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.operation == operation),
            "expected resource operation {operation}"
        );
    });
}

fn assert_resource(error: &FramingError, operation: &str) {
    assert!(
        matches!(error, FramingError::Resource(refusal) if refusal.operation == operation),
        "expected resource operation {operation}, got {error:?}"
    );
}

#[test]
fn sha1_text_refuses_retained_limit() {
    with_retained_limit(39, |ctx| {
        let error = crate::instances::hex(ctx, &[0x5a; 20], "Rhino test SHA-1")
            .expect_err("forty hex bytes exceed retained limit");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(refusal)
                if refusal.operation == "Rhino test SHA-1"
        ));
    });
    with_retained_limit(40, |ctx| {
        assert_eq!(
            crate::instances::hex(ctx, &[0x5a; 20], "Rhino test SHA-1")
                .expect("digest text fits retained limit"),
            "5a".repeat(20)
        );
    });
}

#[test]
fn sha256_text_refuses_retained_limit() {
    with_retained_limit(63, |ctx| {
        let error = crate::instances::hex(ctx, &[0x5a; 32], "Rhino test SHA-256")
            .expect_err("sixty-four hex bytes exceed retained limit");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(refusal)
                if refusal.operation == "Rhino test SHA-256"
        ));
    });
    with_retained_limit(64, |ctx| {
        assert_eq!(
            crate::instances::hex(ctx, &[0x5a; 32], "Rhino test SHA-256")
                .expect("digest text fits retained limit"),
            "5a".repeat(32)
        );
    });
}

#[test]
fn definition_diagnostic_loss_refuses_retained_limit() {
    let diagnostic = crate::instances::DefinitionDiagnostic {
        diagnostic: crate::loss::RhinoDiagnostic {
            code: None,
            message: "fixture warning".to_owned(),
        },
        source_range: 0..1,
    };
    with_retained_limit(0, |ctx| {
        let error = diagnostic
            .to_loss(ctx)
            .expect_err("loss text exceeds retained limit");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(refusal)
                if refusal.operation == "Rhino instance-definition loss text"
        ));
    });
    let loss = diagnostic
        .to_loss(&cadmpeg_test_support::service_decode_context())
        .expect("service profile admits loss text");
    assert!(loss.message.contains("fixture warning"));
}

#[test]
fn reference_object_array_ranges_refuse_collection_limit() {
    let archive = ArchiveVersion::V8;
    let mut data = 1_i32.to_le_bytes().to_vec();
    data.extend(long_chunk(archive, 0x4000_8000, &[]));
    with_collection_limit(0, |ctx| {
        let mut reader = BoundedReader::new(&data, 0, data.len()).expect("bounded array");
        let error =
            crate::instances::skip_object_array(ctx, &data, &mut reader, archive, &mut Vec::new())
                .expect_err("array range exceeds collection limit");
        assert_resource(&error, "Rhino reference object ranges");
    });
}

#[test]
fn reference_parent_layer_range_refuses_collection_limit() {
    let archive = ArchiveVersion::V8;
    let mut implementation_body = 0_i32.to_le_bytes().to_vec();
    implementation_body.extend(0_i32.to_le_bytes());
    implementation_body.push(1);
    implementation_body.extend(long_chunk(archive, 0x4000_0000, &[]));
    let implementation = anonymous_chunk(archive, 0, &implementation_body);
    let mut body = vec![1];
    body.extend(implementation);
    let data = anonymous_chunk(archive, 0, &body);
    with_collection_limit(0, |ctx| {
        let mut reader = BoundedReader::new(&data, 0, data.len()).expect("bounded settings");
        let error = crate::instances::reference_settings(
            ctx,
            &data,
            &mut reader,
            archive,
            &mut Diagnostics::new(),
        )
        .expect_err("parent range exceeds collection limit");
        assert_resource(&error, "Rhino reference parent layer range");
    });
}

#[test]
fn definition_checksum_children_refuse_collection_limit_without_degradation() {
    let archive = ArchiveVersion::V8;
    let payload = v6_definition_payload(archive, [7; 16], &[], 2, true, true);
    let data = definition_record(archive, &payload);
    let chunk = chunk_at(&data, 0, data.len(), archive, false).expect("definition record");
    let record = Record::long(chunk.typecode, chunk.range(), chunk.body());
    for (limit, operation) in [
        (0, "Rhino instance definition checksum children"),
        (2, "Rhino linked definition checksum children"),
        (3, "Rhino linked definition checksum children"),
        (4, "Rhino instance definition checksum children"),
    ] {
        with_collection_limit(limit, |ctx| {
            let error = crate::instances::parse_definitions(
                ctx,
                &data,
                std::slice::from_ref(&record),
                archive,
                0x1000_0021,
            )
            .expect_err("checksum range exceeds collection limit");
            assert!(
                matches!(&error, cadmpeg_core::CodecError::ResourceLimit(refusal) if refusal.operation == operation),
                "expected resource operation {operation}, got {error:?}"
            );
        });
    }
}

#[test]
fn definition_userdata_refuses_collection_limit_without_opaque_fallback() {
    let archive = ArchiveVersion::V8;
    let payload = v6_definition_payload(archive, [7; 16], &[], 0, false, false);
    let userdata = class_userdata(archive, [2; 16], [3; 16], "source.3dm", false);
    let data = definition_record_with_userdata(archive, &payload, &userdata);
    let chunk = chunk_at(&data, 0, data.len(), archive, false).expect("definition record");
    let record = Record::long(chunk.typecode, chunk.range(), chunk.body());
    with_collection_limit(0, |ctx| {
        let error = crate::instances::parse_definitions(
            ctx,
            &data,
            std::slice::from_ref(&record),
            archive,
            0x1000_0021,
        )
        .expect_err("class userdata exceeds collection limit");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(refusal)
                if refusal.operation == "Rhino class userdata"
        ));
    });
}

#[test]
fn definition_member_uuids_refuse_collection_limit_without_opaque_fallback() {
    let archive = ArchiveVersion::V5;
    let payload = v5_definition_payload(archive, 6, [7; 16], &[[8; 16]], false);
    let data = definition_record(archive, &payload);
    let chunk = chunk_at(&data, 0, data.len(), archive, false).expect("definition record");
    let record = Record::long(chunk.typecode, chunk.range(), chunk.body());
    with_collection_limit(0, |ctx| {
        let error = crate::instances::parse_definitions(
            ctx,
            &data,
            std::slice::from_ref(&record),
            archive,
            0x1000_0021,
        )
        .expect_err("one member UUID exceeds collection limit");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(refusal)
                if refusal.operation == "Rhino instance member UUIDs"
        ));
    });
}

#[test]
fn malformed_definition_diagnostic_and_index_refuse_collection_limits() {
    let record = Record::short(0x2000_807b, 0..0, 0);
    for (limit, operation) in [
        (0, "Rhino instance definition diagnostics"),
        (1, "Rhino opaque instance definition indexes"),
    ] {
        with_collection_limit(limit, |ctx| {
            let refusal = crate::instances::parse_definitions(
                ctx,
                &[],
                std::slice::from_ref(&record),
                ArchiveVersion::V5,
                0x1000_0021,
            )
            .expect_err("diagnostic and index exceed the collection limit");
            assert!(
                matches!(refusal, cadmpeg_core::CodecError::ResourceLimit(item) if item.operation == operation)
            );
        });
    }
    let parsed = crate::instances::parse_definitions(
        &cadmpeg_test_support::service_decode_context(),
        &[],
        &[record],
        ArchiveVersion::V5,
        0x1000_0021,
    )
    .expect("service profile admits malformed definition report");
    assert_eq!(parsed.scan.diagnostics().len(), 1);
    assert_eq!(parsed.opaque_records.len(), 1);
}

fn v5_definition_with_path(linked: bool) -> Vec<u8> {
    let archive = ArchiveVersion::V5;
    let payload = v5_definition_payload(archive, 6, [7; 16], &[], linked);
    definition_record(archive, &payload)
}

#[test]
fn v5_definition_name_refuses_retained_limit() {
    let data = v5_definition_with_path(false);
    assert_definition_retained_refusal(ArchiveVersion::V5, &data, 0, "Rhino instance name");
}

#[test]
fn definition_description_refuses_retained_limit() {
    let data = v5_definition_with_path(false);
    assert_definition_retained_refusal(
        ArchiveVersion::V5,
        &data,
        "v5 definition".len() as u64,
        "Rhino instance description",
    );
}

#[test]
fn definition_url_refuses_retained_limit() {
    let data = v5_definition_with_path(false);
    assert_definition_retained_refusal(
        ArchiveVersion::V5,
        &data,
        ("v5 definition".len() + "description".len()) as u64,
        "Rhino instance URL",
    );
}

#[test]
fn definition_url_tag_refuses_retained_limit() {
    let data = v5_definition_with_path(false);
    assert_definition_retained_refusal(
        ArchiveVersion::V5,
        &data,
        ("v5 definition".len() + "description".len() + "https://example.test".len()) as u64,
        "Rhino instance URL tag",
    );
}

#[test]
fn v5_linked_path_refuses_retained_limit() {
    let data = v5_definition_with_path(true);
    assert_definition_retained_refusal(
        ArchiveVersion::V5,
        &data,
        ("v5 definition".len() + "description".len() + "https://example.test".len() + "tag".len())
            as u64,
        "Rhino instance linked path",
    );
}

#[test]
fn v6_component_name_refuses_retained_limit() {
    let archive = ArchiveVersion::V8;
    let payload = v6_definition_payload(archive, [7; 16], &[], 1, false, false);
    let data = definition_record(archive, &payload);
    assert_definition_retained_refusal(archive, &data, 0, "Rhino instance component name");
}

#[test]
fn unit_detail_name_refuses_retained_limit() {
    let archive = ArchiveVersion::V5;
    let mut body = 2_u32.to_le_bytes().to_vec();
    body.extend(0.5_f64.to_le_bytes());
    body.extend(crate::test_support::test_dump::utf16_bytes("retained name"));
    let data = anonymous_chunk(archive, 0, &body);
    with_retained_limit(0, |ctx| {
        let mut reader = BoundedReader::new(&data, 0, data.len()).expect("bounded units");
        let error = crate::instances::unit_detail(
            ctx,
            &data,
            &mut reader,
            archive,
            &mut Diagnostics::new(),
        )
        .expect_err("unit name exceeds retained limit");
        assert_resource(&error, "Rhino instance unit name");
    });
}

fn file_reference_refusal(limit: u64) -> FramingError {
    let archive = ArchiveVersion::V8;
    let data = file_reference_bytes(archive, "/full/source.3dm", "source.3dm");
    with_retained_limit(limit, |ctx| {
        let mut reader = BoundedReader::new(&data, 0, data.len()).expect("bounded reference");
        crate::instances::file_reference(ctx, &data, &mut reader, archive, &mut Diagnostics::new())
            .expect_err("file-reference string exceeds retained limit")
    })
}

#[test]
fn file_reference_full_path_refuses_retained_limit() {
    assert_resource(&file_reference_refusal(0), "Rhino file reference full path");
}

#[test]
fn file_reference_relative_path_refuses_retained_limit() {
    assert_resource(
        &file_reference_refusal("/full/source.3dm".len() as u64),
        "Rhino file reference relative path",
    );
}
