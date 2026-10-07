// SPDX-License-Identifier: Apache-2.0

use crate::chunks::FramingError;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn legacy_rdk_early_searches_propagate_work_refusal() {
    let xml = "<!--root--><xml><other/><render-content-manager-data><other/><material xmlns:other=\"urn:other\" other:instance-id=\"AABBCCDD-EEFF-0011-2233-445566778899\" other=\"value\" instance-id=\"00000000-0000-0000-0000-000000000001\"/></render-content-manager-data><unused/></xml>";
    let bytes = super::legacy_rdk_payload(xml, false, &[]);
    for operation in [
        "Rhino RDK root search",
        "Rhino RDK render data search",
        "Rhino RDK material search",
        "Rhino RDK instance attribute search",
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
                let result = crate::presentation::parse_legacy_rdk_material_instance_id(
                    &ctx,
                    &bytes,
                    0..bytes.len(),
                )
                .map_err(|error| match error {
                    FramingError::Resource(limit) => CodecError::ResourceLimit(limit),
                    other => panic!("valid RDK XML failed: {other:?}"),
                });
                if let Err(CodecError::ResourceLimit(limit)) = &result {
                    assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
                }
                result
            },
        );
    }
    assert_eq!(
        crate::presentation::parse_legacy_rdk_material_instance_id(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            0..bytes.len()
        )
        .expect("RDK UUID"),
        Some(crate::wire::Uuid::from_canonical([
            0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77,
            0x88, 0x99
        ]))
    );
}

#[test]
fn group_memberships_without_a_unique_group_do_not_format_output_links() {
    use crate::chunks::ArchiveVersion;
    use crate::test_support::test_dump::{
        class_wrapper, crc_chunk, minimal_document, object_record_with_attribute_userdata,
        set_test_units, table, tagged_attributes, utf16_bytes, POINT_CLASS,
    };
    let archive = ArchiveVersion::V5;
    let mut group = vec![0x1f];
    group.extend(7_i32.to_le_bytes());
    group.extend(utf16_bytes("fixtures"));
    group.extend([0x44; 16]);
    let group = crc_chunk(
        archive,
        0x2000_8073,
        &class_wrapper(archive, crate::presentation::GROUP.to_wire(), &group),
    );
    let mut membership = 1_i32.to_le_bytes().to_vec();
    membership.extend(7_i32.to_le_bytes());
    let attributes = tagged_attributes(&[(18, membership)], 0);
    let object = object_record_with_attribute_userdata(archive, 1, POINT_CLASS, &attributes, &[]);
    for groups in [vec![], vec![group.clone(), group]] {
        let bytes = minimal_document(
            "50",
            &[
                table(archive, 0x1000_0014, &[]),
                table(archive, 0x1000_0015, &[]),
                table(archive, 0x1000_0018, &groups),
                table(archive, 0x1000_0013, std::slice::from_ref(&object)),
            ],
        );
        let mut scan = crate::container::scan_owned(bytes).expect("group membership scan");
        set_test_units(&mut scan, 1.0);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(scan.data, &arena, &policy).unwrap();
        // Arm the removed allocation: this is an absence check, not a limit boundary.
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::RetainedBytes,
            "Rhino group member link",
            None,
        );
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        crate::presentation::install(&ctx, &scan, &mut ir)
            .expect("no unused member link is formatted");
        let records = &ir.native.namespace("rhino").unwrap().arenas()["groups"];
        assert_eq!(records.len(), groups.len());
        for record in records {
            assert_eq!(record.field("links"), Some(serde_json::json!([])));
        }
    }
}

#[test]
fn dimension_child_digest_admits_source_bytes_before_hashing() {
    use crate::chunks::{ArchiveVersion, BoundedReader};
    for size in [1, 137] {
        let bytes = crate::test_support::test_dump::anonymous_chunk(ArchiveVersion::V8, 0, &vec![0x55; size]);
        let error = cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "Rhino dimension child SHA-256", |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
            let mut reader = BoundedReader::new(&bytes, 0, bytes.len()).unwrap();
            let result = crate::presentation::named_child(&ctx, &bytes, &mut reader, ArchiveVersion::V8).map_err(|error| match error {
                FramingError::Resource(limit) => CodecError::ResourceLimit(limit),
                other => panic!("valid child failed: {other:?}"),
            });
            if let Err(CodecError::ResourceLimit(limit)) = &result { assert_eq!(ctx.resource_refusal().as_ref(), Some(limit)); }
            result
        });
        let CodecError::ResourceLimit(limit) = error else { panic!("digest work refusal"); };
        assert_eq!(limit.additional, u64::try_from(bytes.len()).unwrap());
    }
}
