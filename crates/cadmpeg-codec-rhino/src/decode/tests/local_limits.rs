// SPDX-License-Identifier: Apache-2.0

use super::{with_expand, with_transaction_limits, CandidateError, DecodeContext};
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

#[test]
fn instance_selection_path_refuses_scoped_storage_before_copy() {
    let scan = scan_with_objects(&[]);
    with_transaction_limits(&scan, 100, None, Some(0), |expand| {
        let error = super::super::InstanceSelection::new(expand.ctx(), 0,
            &["root".to_string()], crate::wire::Uuid::from_wire([0x51; 16]))
            .expect_err("path copy needs scoped storage");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
                && refusal.operation == "Rhino instance selection scratch"));
    });
}

#[test]
fn instance_selection_key_refuses_scoped_storage_before_formatting() {
    let scan = scan_with_objects(&[]);
    let path_bytes = std::mem::size_of::<String>() + "root".len();
    with_transaction_limits(&scan, 100, None, Some(u64::try_from(path_bytes).expect("size")), |expand| {
        let error = super::super::InstanceSelection::new(expand.ctx(), 0,
            &["root".to_string()], crate::wire::Uuid::from_wire([0x51; 16]))
            .expect_err("key needs storage beyond the copied path");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
                && refusal.operation == "Rhino instance selection scratch"
                && refusal.used == u64::try_from(path_bytes).expect("size")));
    });
    with_expand(&scan, |expand| {
        let (selection, _bytes) = super::super::InstanceSelection::new(expand.ctx(), 0,
            &["root".to_string(), "child".to_string()], crate::wire::Uuid::from_wire([0x51; 16]))
            .expect("selection admitted");
        assert_eq!(selection.path, ["root", "child"]);
        assert_eq!(selection.key.as_str(), "root.child.51515151-5151-5151-5151-515151515151");
    });
}

#[test]
fn instance_selection_rejects_invalid_identity_key() {
    let scan = scan_with_objects(&[]);
    with_expand(&scan, |expand| {
        let error = super::super::InstanceSelection::new(expand.ctx(), 0,
            &["bad path".to_string()], crate::wire::Uuid::from_wire([0x51; 16]))
            .expect_err("whitespace cannot enter an identity key");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    });
}

#[test]
fn instance_path_segment_refuses_scoped_storage_before_formatting() {
    let scan = scan_with_objects(&[crate::test_support::test_dump::object_record(ArchiveVersion::V5, 1, POINT_CLASS)]);
    with_transaction_limits(&scan, 100, None, Some(0), |expand| {
        let context = DecodeContext::new(&scan, expand).expect("transaction");
        let mut scratch = expand.ctx().reserve_scoped(0, "Rhino instance traversal scratch").expect("empty scope");
        let error = context.reference_segment(0, scan.objects[0].identity().expect("identity"), &mut scratch)
            .expect_err("path UUID needs scoped storage");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
                && refusal.operation == "Rhino instance traversal scratch"));
    });
}

#[test]
fn transformed_instance_links_refuse_scoped_slots_before_copy() {
    let scan = scan_with_objects(&[]);
    with_transaction_limits(&scan, 100, None, Some(0), |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction");
        let before = cadmpeg_ir::draft::ModelCheckpoint::capture(&context.ir.model);
        context.ir.model.bodies.push(cadmpeg_ir::topology::Body {
            id: "rhino:test:body#one".try_into().expect("id"),
            name: None,
            kind: cadmpeg_ir::topology::BodyKind::Solid,
            regions: Vec::new(),
            color: None,
            visible: None,
            transform: None,
        });
        let mut scratch = expand.ctx().reserve_scoped(0, "Rhino instance link scratch").expect("empty scope");
        let error = context.transform_new_entities(&before, cadmpeg_ir::transform::Transform::identity(), &mut scratch)
            .expect_err("link slot needs scoped storage");
        assert!(matches!(error, super::ReferenceFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
            if refusal.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
                && refusal.operation == "Rhino instance link scratch"));
    });
}

#[test]
fn transformed_instance_identity_refuses_scoped_text_before_copy() {
    let scan = scan_with_objects(&[]);
    let slot_bytes = u64::try_from(std::mem::size_of::<String>()).expect("size");
    with_transaction_limits(&scan, 100, None, Some(slot_bytes), |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction");
        let before = cadmpeg_ir::draft::ModelCheckpoint::capture(&context.ir.model);
        context.ir.model.curves.push(cadmpeg_ir::geometry::Curve {
            id: "rhino:test:curve#one".try_into().expect("id"),
            geometry: cadmpeg_ir::geometry::CurveGeometry::Solved(cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(super::line_nurbs(0.0, 1.0, false))),
            source_object: None,
        });
        let mut scratch = expand.ctx().reserve_scoped(0, "Rhino instance link scratch").expect("empty scope");
        let error = context.transform_new_entities(&before, cadmpeg_ir::transform::Transform::identity(), &mut scratch)
            .expect_err("identity needs storage beyond its slot");
        assert!(matches!(error, super::ReferenceFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
            if refusal.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
                && refusal.operation == "Rhino instance link scratch" && refusal.used == slot_bytes));
    });
}

#[test]
fn transformed_instance_annotation_ids_refuse_scoped_slots_before_copy() {
    let scan = scan_with_objects(&[]);
    with_transaction_limits(&scan, 100, None, Some(0), |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction");
        let before = cadmpeg_ir::draft::ModelCheckpoint::capture(&context.ir.model);
        context.ir.model.points.push(Point::new("rhino:test:point#one".try_into().expect("id"),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("point"), None));
        let mut scratch = expand.ctx().reserve_scoped(0, "Rhino instance annotation scratch").expect("empty scope");
        let error = context.transform_new_entities(&before, cadmpeg_ir::transform::Transform::identity(), &mut scratch)
            .expect_err("annotation identity slot needs scoped storage");
        assert!(matches!(error, super::ReferenceFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(refusal))
            if refusal.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
                && refusal.operation == "Rhino instance annotation scratch"));
    });
}

#[test]
fn point_cloud_commit_propagates_local_entity_limit() {
    let scan = scan_with_objects(&[crate::test_support::test_dump::object_record(ArchiveVersion::V5, 1, POINT_CLASS)]);
    with_expand(&scan, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction");
        context.set_expansion_limits([16, 16, 6]);
        let point = cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).expect("point");
        let error = context.commit_geometry(0, crate::curves::DecodedGeometry::PointCloud(crate::curves::PointCloud {
            points: vec![point, point], scaled: false, warnings: crate::loss::Diagnostics::new(),
        })).expect_err("seven cloud entities exceed six");
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(refusal)
            if refusal.operation == "Rhino instance entity limit" && refusal.limit == 6 && refusal.additional == 1));
        assert!(context.ir.model.bodies.is_empty());
        assert!(context.ir.model.vertices.is_empty());
        assert!(context.report.phase_warnings.is_empty());
    });
}
