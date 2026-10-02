// SPDX-License-Identifier: Apache-2.0

use crate::native::attach::AnnotationBuilder;
use crate::native::attach::CadIr;
use crate::native::attach::SketchGeometryDefinition;
use cadmpeg_ir::annotations::StreamHandle;

#[test]
fn sketch_coordinate_pairs_are_retained_as_native_entities_without_roles() {
    let label = crate::native::features::FeatureOperationLabel {
        id: "nx:feature-history:operation-label#section-9".to_string(),
        section_link: "section".to_string(),
        ordinal: 9,
        value: "SKETCH".to_string(),
        objects: crate::om::header_references::HeaderReferences([None; 4]),
        stable_identity: None,
        source_offset: 40,
    };
    let pair = crate::native::features::FeaturePayloadScalarPair {
        id: "nx:feature-history:sketch-payload-coordinate-pair#section-9-0000000000".to_string(),
        operation_label: label.id.clone(),
        payload: crate::native::features::FeatureScalarPairPayload::Construction {
            construction_payload: "payload".to_string(),
            frame: crate::om::binary64_pair::Binary64Pair::new(
                crate::om::binary64_pair::SketchBinary64PairForm::Object(
                    crate::om::binary64_pair::ObjectPairForm::Short,
                ),
                12,
                [12.5_f64, -3.0_f64].map(|value| {
                    let mut raw = value.to_be_bytes();
                    raw[0] -= 0x10;
                    crate::om::scalar::ShiftedBinary64::try_from(raw).unwrap()
                }),
            )
            .unwrap(),
        },
        ordinal: 0,
        value_source_offsets: [59, 67],
        source_offset: 51,
    };
    let coordinate_pairs = [&pair];
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let stream = StreamHandle::new(&cadmpeg_test_support::service_decode_context(), cadmpeg_ir::stream_name!("nx:container"), "fixture stream handle").unwrap();

    crate::test_support::with_decode_context(|ctx| {
        let sketch = super::super::attach_sketch_graph(
            ctx,
            &mut ir,
            &label,
            &super::super::SketchSources {
                point_uses: &[],
                point_groups: &[],
                points: &[],
                payload_scalars: &[],
                fixed_points: &[],
                coordinate_pairs: &coordinate_pairs,
            },
            &mut annotations,
            &stream,
        )
        .unwrap()
        .expect("one complete coordinate pair retains a native sketch graph");

        assert_eq!(ir.model.sketches[0].id, sketch);
        assert!(matches!(
            ir.model.sketches[0].placement,
            cadmpeg_ir::sketches::SketchPlacement::Unresolved {}
        ));
        assert_eq!(ir.model.sketch_entities.len(), 1);
        assert_eq!(
            ir.model.sketch_entities[0].id().as_str(),
            "nx:feature-history:sketch-entity#coordinate-pair-section-9-0000000000"
        );
        assert!(cadmpeg_ir::ids::is_valid_identity(
            ir.model.sketch_entities[0].id().as_str()
        ));
        assert_eq!(
            ir.model.sketch_entities[0].native_ref.as_deref(),
            Some(pair.id.as_str())
        );
        assert!(matches!(ir.model.sketch_entities[0].geometry.definition(),
            SketchGeometryDefinition::Native { native_kind } if native_kind == "nx-coordinate-pair"
        ));
    });
}

#[test]
fn sketch_fixed_points_are_retained_as_native_entities_without_roles() {
    let label = crate::native::features::FeatureOperationLabel {
        id: "nx:feature-history:operation-label#section-11".to_string(),
        section_link: "section".to_string(),
        ordinal: 11,
        value: "SKETCH".to_string(),
        objects: crate::om::header_references::HeaderReferences([None; 4]),
        stable_identity: None,
        source_offset: 80,
    };
    let point = crate::native::features::FeatureSketchFixedPoint {
        id: "nx:feature-history:sketch-fixed-point#section-11-0000000000".to_string(),
        operation_label: label.id.clone(),
        named_record: "named-record".to_string(),
        name: "Point1".to_string(),
        fixed_pair: "fixed-pair".to_string(),
        values: [0.25, -0.5].map(|value| cadmpeg_ir::scalar::FiniteReal::new(value).unwrap()),
        source_offset: 91,
    };
    let fixed_points = [&point];
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let stream = StreamHandle::new(&cadmpeg_test_support::service_decode_context(), cadmpeg_ir::stream_name!("nx:container"), "fixture stream handle").unwrap();

    crate::test_support::with_decode_context(|ctx| {
        let sketch = super::super::attach_sketch_graph(
            ctx,
            &mut ir,
            &label,
            &super::super::SketchSources {
                point_uses: &[],
                point_groups: &[],
                points: &[],
                payload_scalars: &[],
                fixed_points: &fixed_points,
                coordinate_pairs: &[],
            },
            &mut annotations,
            &stream,
        )
        .unwrap()
        .expect("one complete fixed point retains a native sketch graph");

        assert_eq!(ir.model.sketches[0].id, sketch);
        assert!(matches!(
            ir.model.sketches[0].placement,
            cadmpeg_ir::sketches::SketchPlacement::Unresolved {}
        ));
        assert_eq!(ir.model.sketch_entities.len(), 1);
        assert_eq!(
            ir.model.sketch_entities[0].id().as_str(),
            "nx:feature-history:sketch-entity#fixed-point-section-11-0000000000"
        );
        assert_eq!(
            ir.model.sketch_entities[0].native_ref.as_deref(),
            Some(point.id.as_str())
        );
        assert!(matches!(ir.model.sketch_entities[0].geometry.definition(),
            SketchGeometryDefinition::Native { native_kind } if native_kind == "nx-fixed-point"
        ));
    });
}

fn fixed_point_sketch_with_limit(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> Result<Option<cadmpeg_ir::sketches::SketchId>, cadmpeg_core::CodecError> {
    let label = crate::native::features::FeatureOperationLabel {
        id: "nx:feature-history:operation-label#section-11".into(),
        section_link: "section".into(),
        ordinal: 11,
        value: "SKETCH".into(),
        objects: crate::om::header_references::HeaderReferences([None; 4]),
        stable_identity: None,
        source_offset: 80,
    };
    let point = crate::native::features::FeatureSketchFixedPoint {
        id: "nx:feature-history:sketch-fixed-point#section-11-0000000000".into(),
        operation_label: label.id.clone(),
        named_record: "named-record".into(),
        name: "Point1".into(),
        fixed_pair: "fixed-pair".into(),
        values: [0.25, -0.5].map(|value| cadmpeg_ir::scalar::FiniteReal::new(value).unwrap()),
        source_offset: 91,
    };

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            let mut ir = CadIr::empty();
            let mut annotations = AnnotationBuilder::new();
            let stream = StreamHandle::new(&cadmpeg_test_support::service_decode_context(), cadmpeg_ir::stream_name!("nx:container"), "fixture stream handle").unwrap();
            super::super::attach_sketch_graph(
                ctx,
                &mut ir,
                &label,
                &super::super::SketchSources {
                    point_uses: &[],
                    point_groups: &[],
                    points: &[],
                    payload_scalars: &[],
                    fixed_points: &[&point],
                    coordinate_pairs: &[],
                },
                &mut annotations,
                &stream,
            )
        },
    )
}

#[test]
fn sketch_graph_refuses_collection_limit() {
    let error =
        fixed_point_sketch_with_limit(|policy| policy.limits.max_collection_items = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn sketch_graph_refuses_retained_limit() {
    let error =
        fixed_point_sketch_with_limit(|policy| policy.limits.max_retained_bytes = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn sketch_graph_refuses_scoped_limit() {
    let error = fixed_point_sketch_with_limit(|policy| policy.limits.max_materialized_bytes = 0)
        .unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn sketch_graph_refuses_work_limit() {
    let error =
        fixed_point_sketch_with_limit(|policy| policy.limits.max_work_units = 0).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}
