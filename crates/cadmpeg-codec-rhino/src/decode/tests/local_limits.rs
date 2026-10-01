// SPDX-License-Identifier: Apache-2.0

use super::{with_expand, CandidateError, DecodeContext};
use crate::chunks::ArchiveVersion;
use crate::test_support::test_dump::{object_record_with_payload, point_payload, scan_with_objects, set_test_units, POINT_CLASS};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Point;

#[test]
fn point_commit_propagates_local_entity_limit() {
    let object = object_record_with_payload(ArchiveVersion::V5, 1, POINT_CLASS,
        &point_payload([2.0, 0.0, 0.0]));
    let mut scan = scan_with_objects(&[object]);
    set_test_units(&mut scan, 1.0);
    with_expand(&scan, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction");
        context.set_expansion_limits([16, 16, 4]);
        let error = context.decode_geometry().expect_err("five point entities exceed four");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.operation == "Rhino instance entity limit" && refusal.limit == 4 && refusal.additional == 1));
        assert!(context.ir.model.points.is_empty());
    });
}

#[test]
fn candidate_propagates_local_entity_limit() {
    let scan = scan_with_objects(&[]);
    with_expand(&scan, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction");
        context.set_expansion_limits([16, 16, 0]);
        let error = context.validate_candidate(|ir, _| {
            ir.model.points.push(Point::new("rhino:test:point#one".try_into().expect("id"),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("point"), Some(cadmpeg_ir::SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Rhino,
                    object_id: cadmpeg_core::text::NonBlankString::new("one").expect("source identity"),
                    name: None, color: None, visible: None, layer: None, instance_path: Vec::new(),
                })));
        }).expect_err("candidate exceeds zero entities");
        assert!(matches!(error, CandidateError::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
            if refusal.operation == "Rhino instance entity limit" && refusal.limit == 0 && refusal.additional == 1));
        assert!(context.ir.model.points.is_empty());
    });
}

#[test]
fn brep_commit_propagates_local_entity_limit() {
    let object = object_record_with_payload(ArchiveVersion::V5, 0x10,
        crate::brep::ON_BREP.to_wire(), &crate::test_support::test_archive::brep_payload(false));
    let mut scan = scan_with_objects(&[object]);
    set_test_units(&mut scan, 1.0);
    with_expand(&scan, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction");
        context.set_expansion_limits([16, 16, 0]);
        let error = context.decode_geometry().expect_err("Brep exceeds zero entities");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.operation == "Rhino instance entity limit" && refusal.limit == 0));
        assert!(context.ir.model.bodies.is_empty());
    });
}
