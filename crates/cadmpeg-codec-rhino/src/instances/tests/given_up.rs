// SPDX-License-Identifier: Apache-2.0

use super::support::{install_definitions, static_definition};
use crate::chunks::ArchiveVersion;
use crate::test_support::test_dump::{
    instance_reference_payload, object_record_with_payload, point_payload, scan_with_objects,
    set_identity, transform, INSTANCE_REFERENCE_CLASS, MESH_CLASS, POINT_CLASS,
};

/// One member of one definition, at source order 0, and two references to
/// that definition.
fn member_and_two_references(member: Vec<u8>) -> crate::container::Scan<'static> {
    let archive = ArchiveVersion::V5;
    let member_id = [0x5a; 16];
    let definition_id = [0x6a; 16];
    let reference = |x| {
        object_record_with_payload(
            archive,
            0x1000,
            INSTANCE_REFERENCE_CLASS,
            &instance_reference_payload(definition_id, transform(1.0, [x, 0.0, 0.0])),
        )
    };
    let mut scan = scan_with_objects(&[member, reference(10.0), reference(20.0)]);
    for (source_order, id) in [member_id, [0x7a; 16], [0x7b; 16]].into_iter().enumerate() {
        set_identity(
            &mut scan,
            source_order,
            id,
            &format!("object-{source_order}"),
            None,
            true,
        );
    }
    install_definitions(
        &mut scan,
        vec![static_definition(definition_id, &[member_id])],
    );
    scan
}

fn unreadable(result: &cadmpeg_ir::codec::DecodeResult) -> Vec<&str> {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind())
        .map(|loss| loss.message.as_str())
        .collect()
}

#[test]
fn a_definition_member_that_does_not_decode_is_given_up_once() {
    let member = object_record_with_payload(
        ArchiveVersion::V5,
        0x20,
        MESH_CLASS,
        &crate::test_support::test_archive::mesh_payload(3, 5, true, false),
    );
    let scan = member_and_two_references(member);
    let offset = scan.objects[0].range().start;

    let result = crate::decode::decode_for_test(&scan);
    assert!(result.ir().model.tessellations.is_empty());
    assert!(result.ir().geometry.bodies.is_empty());
    assert_eq!(unreadable(&result).len(), 1, "{:?}", result.report().losses);
    crate::container::tests::assert_part_given_up(
        result.ir(),
        result.source_fidelity(),
        &result.report().losses,
        ("object-record", offset),
        Some("rhino:object:record#000000"),
        "rhino:object:record#000000",
        cadmpeg_ir::unknown::part::Cause::Unreadable,
    );
    let native = result
        .ir()
        .native
        .namespace("rhino")
        .expect("Rhino namespace");
    assert_eq!(native.arenas()["product_occurrences"].len(), 2);
}

#[test]
fn a_member_the_instance_entity_cap_refuses_is_not_given_up() {
    let member = object_record_with_payload(
        ArchiveVersion::V5,
        1,
        POINT_CLASS,
        &point_payload([1.0, 2.0, 3.0]),
    );
    let scan = member_and_two_references(member);
    crate::decode::with_expand(&scan, |expand| {
        let mut context = crate::decode::DecodeContext::new(&scan, expand).expect("transaction");
        context.set_expansion_limits([16, 16, 0]);
        context
            .decode_geometry()
            .expect("references over the entity cap are retained");
        assert_eq!(expand.ctx().resource_refusal(), None);
        let result =
            crate::decode::seal_for_test(context.commit().expect("test decode commit"), false);
        assert!(result.ir().geometry.points.is_empty());
        assert!(
            unreadable(&result).is_empty(),
            "{:?}",
            result.report().losses
        );
    });
}
