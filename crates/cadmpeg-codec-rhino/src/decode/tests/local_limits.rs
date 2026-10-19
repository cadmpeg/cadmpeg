// SPDX-License-Identifier: Apache-2.0
//! Local carriers use the document resource budget once, without instance copies.

use super::{with_entity_limit, CandidateError, DecodeContext};
use crate::test_support::test_dump::scan_with_objects;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Point;

#[test]
fn candidate_entity_limit_is_a_resource_refusal_and_preserves_the_document() {
    let scan = scan_with_objects(&[]);
    with_entity_limit(&scan, 0, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("empty transaction");
        let error = context
            .validate_candidate(|ir, _| {
                ir.model.points.push(Point::new(
                    "rhino:test:point#one".try_into().expect("id"),
                    cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                        .expect("finite point"),
                    None,
                ));
            })
            .expect_err("one entity exceeds the document limit");
        assert!(
            matches!(error,CandidateError::Codec(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation=="rhino_object_entities")
        );
        assert!(context.session.document().model.points.is_empty());
    });
}

#[test]
fn point_cloud_resource_refusal_keeps_all_geometry_arenas_empty() {
    let scan = scan_with_objects(&[crate::test_support::test_dump::object_record(
        crate::chunks::ArchiveVersion::V5,
        1,
        crate::test_support::test_dump::POINT_CLASS,
    )]);
    with_entity_limit(&scan, 6, |expand| {
        let mut context = DecodeContext::new(&scan, expand).expect("empty transaction");
        let point = cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 2.0, 3.0))
            .expect("finite point");
        let error = context
            .commit_geometry(
                0,
                crate::curves::DecodedGeometry::PointCloud(crate::curves::PointCloud {
                    points: vec![point, point],
                    scaled: false,
                    warnings: crate::loss::Diagnostics::new(),
                }),
            )
            .expect_err("seven point-cloud entities exceed six");
        assert!(
            matches!(error,cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation=="rhino_object_entities")
        );
        let model = &context.session.document().model;
        assert!(model.bodies.is_empty());
        assert!(model.regions.is_empty());
        assert!(model.shells.is_empty());
        assert!(model.vertices.is_empty());
        assert!(model.points.is_empty());
    });
}
