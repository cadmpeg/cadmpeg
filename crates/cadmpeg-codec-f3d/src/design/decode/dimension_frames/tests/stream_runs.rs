// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn dimension_geometry_runs_commit_only_surviving_columns() {
    let point = crate::records::sketch_geometry::SketchPoint::try_from(
        crate::records::sketch_geometry::SketchPointDraft {
            id: "f3d:Design/BulkStream.dat:sketch-point#0".into(),
            record_index: 20,
            owner_reference: None,
            class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                .unwrap(),
            byte_offset: 0,
            coordinate_offset: 89,
            companion: crate::records::sketch_geometry::SketchPointCompanion {
                incident_curves: Vec::new(),
            },
            record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
                20,
                crate::records::sketch_geometry::SketchPointClosure::Selector0State0,
                None,
                0.0,
            ),
            paired_reference: 0,
            coordinates: cadmpeg_ir::math::Point2::new(1.0, 2.0),
        },
    )
    .unwrap();
    let surviving = u64::try_from(
        4 * std::mem::size_of::<&str>() + 4 * std::mem::size_of::<u32>(),
    )
    .unwrap();
    for retained in [surviving, surviving - 1] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = retained;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let (runs, storage) = super::super::dimension_geometry_runs(
            &ctx,
            std::slice::from_ref(&point),
            &[],
        )
        .unwrap();
        assert_eq!(runs.streams, ["f3d:Design/BulkStream.dat"]);
        assert_eq!(runs.indices, [20]);
        assert_eq!(runs.streams.capacity(), 4);
        assert_eq!(runs.indices.capacity(), 4);
        let result = storage.commit_value(runs);
        if retained == surviving {
            assert!(result.is_ok());
        } else {
            let CodecError::ResourceLimit(limit) = result.err().unwrap() else {
                panic!("expected original retained refusal");
            };
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(limit.used, 0);
            assert_eq!(limit.additional, surviving);
            assert_eq!(limit.limit, retained);
            assert_eq!(limit.operation, "f3d dimension geometry indices");
            assert_eq!(ctx.resource_refusal(), Some(limit));
        }
    }
}

#[test]
fn dimension_sketch_entity_runs_commit_only_surviving_columns() {
    let entity = crate::records::entity_header::DesignEntityHeader {
        id: "f3d:Design/BulkStream.dat:design-entity#0".into(),
        byte_offset: 0,
        entity_id: crate::records::identity::DesignEntityId::try_from("0_20".to_owned())
            .unwrap(),
        class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
            .unwrap(),
        optional_slot_present: false,
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some("MSketch".to_owned()),
            None,
            crate::records::identity::ReferenceRun::unlocated(Vec::new()),
        )
        .unwrap(),
    };
    let surviving = u64::try_from(
        4 * std::mem::size_of::<&str>() + 4 * std::mem::size_of::<u32>(),
    )
    .unwrap();
    for retained in [surviving, surviving - 1] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = retained;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let (runs, storage) = super::super::dimension_sketch_entity_runs(
            &ctx,
            std::slice::from_ref(&entity),
        )
        .unwrap();
        assert_eq!(runs.streams, ["f3d:Design/BulkStream.dat"]);
        assert_eq!(runs.indices, [20]);
        assert_eq!(runs.streams.capacity(), 4);
        assert_eq!(runs.indices.capacity(), 4);
        let result = storage.commit_value(runs);
        if retained == surviving {
            assert!(result.is_ok());
        } else {
            let CodecError::ResourceLimit(limit) = result.err().unwrap() else {
                panic!("expected original retained refusal");
            };
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(limit.used, 0);
            assert_eq!(limit.additional, surviving);
            assert_eq!(limit.limit, retained);
            assert_eq!(limit.operation, "f3d dimension sketch entities");
            assert_eq!(ctx.resource_refusal(), Some(limit));
        }
    }
}
