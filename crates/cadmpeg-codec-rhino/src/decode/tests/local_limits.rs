// SPDX-License-Identifier: Apache-2.0

use super::{with_expand, CandidateError, DecodeContext};
use crate::chunks::ArchiveVersion;
use crate::test_support::test_dump::{
    object_record_with_payload, point_payload, scan_with_objects, set_test_units, POINT_CLASS,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Point;

#[test]
fn point_commit_skips_the_object_at_the_local_entity_limit() {
    let object = object_record_with_payload(
        ArchiveVersion::V5,
        1,
        POINT_CLASS,
        &point_payload([2.0, 0.0, 0.0]),
    );
    let mut scan = scan_with_objects(&[object]);
    set_test_units(&mut scan, 1.0);
    with_expand(&scan, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction");
        context.set_expansion_limits([16, 16, 4]);
        context
            .decode_geometry()
            .expect("five point entities exceeding four skip the point");
        assert!(context.session.document().model.points.is_empty());
        assert!(context
            .report
            .phase_warnings
            .messages()
            .any(|message| message.contains("document instance entity budget exceeded")));
        assert_eq!(expand.ctx().resource_refusal(), None);
    });
}

#[test]
fn candidate_is_rejected_at_the_local_entity_limit() {
    let scan = scan_with_objects(&[]);
    with_expand(&scan, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction");
        context.set_expansion_limits([16, 16, 0]);
        let error = context
            .validate_candidate(|ir, _| {
                ir.model.points.push(Point::new(
                    "rhino:test:point#one".try_into().expect("id"),
                    cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                        .expect("point"),
                    Some(cadmpeg_ir::SourceObjectAssociation {
                        geometry_role: None,
                        format: cadmpeg_ir::CodecFormat::Rhino,
                        object_id: cadmpeg_core::text::NonBlankString::new("one")
                            .expect("source identity"),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                ));
            })
            .expect_err("candidate exceeds zero entities");
        assert!(matches!(&error, CandidateError::Admission(message)
            if message == "document instance entity budget exceeded"));
        assert!(context.session.document().model.points.is_empty());
        assert_eq!(expand.ctx().resource_refusal(), None);
    });
}

#[test]
fn brep_commit_skips_the_object_at_the_local_entity_limit() {
    let object = object_record_with_payload(
        ArchiveVersion::V5,
        0x10,
        crate::brep::ON_BREP.to_wire(),
        &crate::test_support::test_archive::brep_payload(false),
    );
    let mut scan = scan_with_objects(&[object]);
    set_test_units(&mut scan, 1.0);
    with_expand(&scan, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction");
        context.set_expansion_limits([16, 16, 0]);
        context
            .decode_geometry()
            .expect("a Brep exceeding zero entities is skipped");
        assert!(context.session.document().model.bodies.is_empty());
        assert!(context
            .report
            .phase_warnings
            .messages()
            .any(|message| message.contains(
                "Brep draft rejected before commit: document instance entity budget exceeded"
            )));
        assert_eq!(expand.ctx().resource_refusal(), None);
    });
}

#[test]
fn instance_selection_key_joins_path_and_member() {
    let selection = super::super::InstanceSelection::new(
        0,
        &["root".to_string(), "child".to_string()],
        crate::wire::Uuid::from_wire([0x51; 16]),
    )
    .expect("valid selection key");
    assert_eq!(selection.path, ["root", "child"]);
    assert_eq!(
        selection.key.as_str(),
        "root.child.51515151-5151-5151-5151-515151515151"
    );
}

#[test]
fn instance_selection_rejects_invalid_identity_key() {
    {
        let error = super::super::InstanceSelection::new(
            0,
            &["bad path".to_string()],
            crate::wire::Uuid::from_wire([0x51; 16]),
        )
        .expect_err("whitespace cannot enter an identity key");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    };
}

#[test]
fn point_cloud_commit_skips_the_object_at_the_local_entity_limit() {
    let scan = scan_with_objects(&[crate::test_support::test_dump::object_record(
        ArchiveVersion::V5,
        1,
        POINT_CLASS,
    )]);
    with_expand(&scan, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("transaction");
        context.set_expansion_limits([16, 16, 6]);
        let point =
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).expect("point");
        let committed = context
            .commit_geometry(
                0,
                crate::curves::DecodedGeometry::PointCloud(crate::curves::PointCloud {
                    points: vec![point, point],
                    scaled: false,
                    warnings: crate::loss::Diagnostics::new(),
                }),
            )
            .expect("seven cloud entities exceeding six skip the cloud");
        assert!(!committed);
        assert!(context.session.document().model.bodies.is_empty());
        assert!(context.session.document().model.vertices.is_empty());
        assert!(context
            .report
            .phase_warnings
            .messages()
            .any(|message| message.contains("document instance entity budget exceeded")));
        assert_eq!(expand.ctx().resource_refusal(), None);
    });
}
