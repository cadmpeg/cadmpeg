// SPDX-License-Identifier: Apache-2.0

use super::{seal_for_test, with_expand, DecodeContext};
use crate::chunks::ArchiveVersion;
use crate::test_support::test_dump::{
    object_record_with_payload, scan_with_objects, MESH_CLASS, REV_SURFACE_CLASS,
};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::topology::{Body, BodyKind};
use cadmpeg_ir::unknown::part::Cause;

const HOLDER: &str = "rhino:object:record#000000";

/// Decodes the one object of `scan` after `occupy` puts into the document a
/// record that holds an id the object's decode takes.
fn decode_against(
    scan: &crate::container::Scan<'_>,
    occupy: impl FnOnce(&mut DecodeContext<'_>, cadmpeg_ir::ids::IdentityKey),
) -> cadmpeg_ir::codec::DecodeResult {
    with_expand(scan, |expand| {
        let mut context = DecodeContext::new(scan, expand).expect("transaction");
        let identity = scan.objects[0].identity().expect("framed object");
        let key = context.checked_object_key(identity, 0).expect("object key");
        occupy(&mut context, key);
        context
            .decode_geometry()
            .expect("the refused object is skipped");
        seal_for_test(context.commit().expect("test decode commit"), false)
    })
}

#[test]
fn a_brep_draft_the_document_refuses_is_given_up() {
    let object = object_record_with_payload(
        ArchiveVersion::V5,
        0x10,
        crate::brep::ON_BREP.to_wire(),
        &crate::test_support::test_archive::brep_payload(false),
    );
    let scan = scan_with_objects(&[object]);
    let result = decode_against(&scan, |context, key| {
        context.ir_mut().geometry.bodies.push(Body {
            id: cadmpeg_ir::ids::BodyId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "body"),
                key,
            ),
            kind: BodyKind::Sheet,
            transform: None,
            name: None,
            color: None,
            visible: None,
            native_ref: None,
            unstated: None,
        });
    });
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    let given_up = crate::container::tests::assert_part_given_up(
        result.ir(),
        result.source_fidelity(),
        &result.report().losses,
        ("object-record", scan.objects[0].range().start),
        Some(HOLDER),
        HOLDER,
        Cause::Undecoded,
    );
    assert!(
        given_up
            .message
            .contains("Brep draft rejected before commit"),
        "{}",
        given_up.message
    );
}

#[test]
fn a_rejected_unknown_surface_fallback_is_given_up() {
    let object = object_record_with_payload(ArchiveVersion::V5, 8, REV_SURFACE_CLASS, &[0]);
    let scan = scan_with_objects(&[object]);
    let result = decode_against(&scan, |context, key| {
        context.ir_mut().geometry.surfaces.push(Surface {
            id: cadmpeg_ir::ids::SurfaceId::compose(
                &cadmpeg_ir::identity_namespace!("rhino", "object", "surface"),
                key,
            ),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
            native_ref: None,
        });
    });
    assert_eq!(result.ir().geometry.surfaces.len(), 1);
    let given_up = crate::container::tests::assert_part_given_up(
        result.ir(),
        result.source_fidelity(),
        &result.report().losses,
        ("object-record", scan.objects[0].range().start),
        Some(HOLDER),
        HOLDER,
        Cause::Undecoded,
    );
    assert!(
        given_up
            .message
            .contains("unknown surface validation rejected candidate"),
        "{}",
        given_up.message
    );
}

#[test]
fn a_given_up_record_past_the_codec_retention_cap_keeps_its_bytes_in_its_part() {
    let object = object_record_with_payload(
        ArchiveVersion::V5,
        0x20,
        MESH_CLASS,
        &crate::test_support::test_archive::mesh_payload(3, 5, true, false),
    );
    let scan = scan_with_objects(&[object]);
    let range = scan.objects[0].range();
    let result = with_expand(&scan, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction");
        context
            .set_retention_limits(0, 0)
            .expect("test retention limits");
        context
            .decode_geometry()
            .expect("the malformed mesh is skipped");
        seal_for_test(context.commit().expect("test decode commit"), false)
    });
    let fidelity = result.source_fidelity();
    assert_eq!(
        fidelity.retained_record(HOLDER).expect("holder").data(),
        None
    );
    let head = format!("part object-record stream=\"\" offset={} ", range.start);
    let charges = result
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code == cadmpeg_ir::unknown::part::record_unreadable_kind()
                && loss.message.starts_with(&head)
        })
        .collect::<Vec<_>>();
    assert_eq!(charges.len(), 1, "{:?}", result.report().losses);
    assert!(!charges[0].message.contains("max_retained_bytes"));
    let stub = charges[0]
        .message
        .split_once(" record=")
        .and_then(|(_, rest)| rest.split_once(": "))
        .map(|(stub, _)| stub)
        .expect("the charge names its stub");
    assert_eq!(
        fidelity.retained_record(stub).expect("part").data(),
        Some(&scan.data[range])
    );
}
