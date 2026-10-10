// SPDX-License-Identifier: Apache-2.0
use super::{
    fixture, native_fallback_annotation, native_fallback_group, native_fallback_null_pair,
    native_fallback_pair,
};
use crate::design::dimensions::{
    bind_dimension_loci, retain_planar_dimension_constraints, DimensionLocusInputs,
};
use cadmpeg_core::decode::{DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::{
    SketchConstraint, SketchConstraintDefinition, SketchConstraintDefinitionInput,
    SketchConstraintId, SketchId,
};

const SCRATCH_LIMIT: u64 = 1 << 20;

fn point() -> crate::records::sketch_geometry::SketchPoint {
    crate::records::sketch_geometry::SketchPoint::try_from(
        crate::records::sketch_geometry::SketchPointDraft {
            id: "f3d:Design/BulkStream.dat:sketch-point#30".into(),
            record_index: 30,
            owner_reference: None,
            class_tag: "300".to_owned().try_into().unwrap(),
            byte_offset: 0,
            coordinate_offset: 0,
            companion: crate::records::sketch_geometry::SketchPointCompanion {
                incident_curves: Vec::new(),
            },
            record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
                30,
                crate::records::sketch_geometry::SketchPointClosure::Selector0State0,
                None,
                0.0,
            ),
            paired_reference: 0,
            coordinates: cadmpeg_ir::math::Point2::new(0.0, 0.0),
        },
    )
    .unwrap()
}

fn run_binding(ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
    let fixture = fixture();
    let pair = native_fallback_pair();
    let null_pair = native_fallback_null_pair();
    let group = native_fallback_group();
    let frame = native_fallback_annotation();
    let mut points = [point()];
    let mut curve = fixture.curve.clone();
    curve.owner_reference = None;
    let mut curves = [curve];
    bind_dimension_loci(
        ctx,
        DimensionLocusInputs {
            placements: std::slice::from_ref(&fixture.placement),
            owners: std::slice::from_ref(&fixture.owner),
            pairs: std::slice::from_ref(&pair),
            groups: std::slice::from_ref(&group),
            annotation_frames: std::slice::from_ref(&frame),
            null_pairs: std::slice::from_ref(&null_pair),
        },
        &mut points,
        &mut curves,
    )?;
    assert_eq!(points[0].owner_reference, Some(7));
    assert_eq!(curves[0].owner_reference, Some(7));
    Ok(())
}

fn binding_refusal(operation: &'static str) {
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        operation,
        0,
        run_binding,
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::WorkUnits && limit.operation == operation)
    );
}

#[test]
fn dimension_binding_releases_all_scratch_storage() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = SCRATCH_LIMIT;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        run_binding(ctx).unwrap();
        let _released = ctx
            .reserve_scoped(SCRATCH_LIMIT, "released dimension binding storage")
            .unwrap();
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn dimension_binding_conflicting_owners_keep_the_first_error() {
    crate::test_support::with_decode_context(|ctx| {
        let mut storage = ctx
            .reserve_scoped(0, "dimension binding conflict storage")
            .unwrap();
        let mut bindings = std::collections::HashMap::new();
        super::super::super::insert_dimension_binding(
            ctx,
            &mut storage,
            &mut bindings,
            "native",
            20,
            7,
        )
        .unwrap();
        super::super::super::insert_dimension_binding(
            ctx,
            &mut storage,
            &mut bindings,
            "native",
            20,
            7,
        )
        .unwrap();
        let error = super::super::super::insert_dimension_binding(
            ctx,
            &mut storage,
            &mut bindings,
            "native",
            20,
            8,
        )
        .unwrap_err();
        let CodecError::Malformed(message) = error else {
            panic!("expected owner conflict");
        };
        assert_eq!(
            message,
            "Fusion dimensional geometry record 20 belongs to multiple sketches"
        );
    });
}

fn constraint(sketch: SketchId, name: &str) -> SketchConstraint {
    SketchConstraint {
        id: SketchConstraintId::mint(format!("synthetic:test:constraint#{name}")).unwrap(),
        sketch,
        definition: SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::Disabled {},
        )
        .unwrap(),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    }
}

fn planar_refusal(operation: &'static str, dimension: ResourceDimension) {
    let fixture = fixture();
    let constraints = vec![constraint(fixture.entity.sketch.clone(), "spatial")];
    let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
        retain_planar_dimension_constraints(
            ctx,
            std::slice::from_ref(&fixture.placement),
            std::slice::from_ref(&fixture.spatial),
            constraints.clone(),
        )
    });
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == dimension && limit.operation == operation)
    );
}

#[test]
fn planar_exclusion_keeps_input_order_and_releases_index_storage() {
    let fixture = fixture();
    let excluded = constraint(fixture.entity.sketch.clone(), "spatial");
    let first = constraint(
        SketchId::mint("synthetic:test:sketch#first").unwrap(),
        "first",
    );
    let second = constraint(
        SketchId::mint("synthetic:test:sketch#second").unwrap(),
        "second",
    );
    let placements = vec![fixture.placement.clone(); 128];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = SCRATCH_LIMIT;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let output = retain_planar_dimension_constraints(
            ctx,
            &placements,
            std::slice::from_ref(&fixture.spatial),
            vec![first.clone(), excluded, second.clone()],
        )
        .unwrap();
        assert_eq!(output, [first, second]);
        drop(output);
        let _released = ctx
            .reserve_scoped(SCRATCH_LIMIT, "released planar exclusion storage")
            .unwrap();
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn planar_unmatched_placement_keeps_the_constraint() {
    let fixture = fixture();
    let kept = constraint(
        SketchId::mint("synthetic:test:sketch#unmatched").unwrap(),
        "unmatched",
    );
    let output = crate::test_support::with_decode_context(|ctx| {
        retain_planar_dimension_constraints(
            ctx,
            std::slice::from_ref(&fixture.placement),
            std::slice::from_ref(&fixture.spatial),
            vec![kept.clone()],
        )
    })
    .unwrap();
    assert_eq!(output, [kept]);
}

#[test]
fn planar_exclusion_index_refuses_materialized_limit() {
    planar_refusal(
        "f3d planar excluded sketch index",
        ResourceDimension::MaterializedBytes,
    );
}

#[test]
fn dimension_binding_placement_source_refuses_work_limit() {
    binding_refusal("f3d dimension binding placement source");
}

#[test]
fn dimension_binding_owner_source_refuses_work_limit() {
    binding_refusal("f3d dimension binding owner source");
}

#[test]
fn dimension_binding_pair_source_refuses_work_limit() {
    binding_refusal("f3d dimension binding pair source");
}

#[test]
fn dimension_binding_group_source_refuses_work_limit() {
    binding_refusal("f3d dimension binding group source");
}

#[test]
fn dimension_binding_locus_source_refuses_work_limit() {
    binding_refusal("f3d dimension binding locus source");
}

#[test]
fn dimension_binding_annotation_source_refuses_work_limit() {
    binding_refusal("f3d dimension binding annotation source");
}

#[test]
fn dimension_binding_operand_source_refuses_work_limit() {
    binding_refusal("f3d dimension binding operand source");
}

#[test]
fn dimension_binding_null_pair_source_refuses_work_limit() {
    binding_refusal("f3d dimension binding null pair source");
}

#[test]
fn dimension_binding_point_source_refuses_work_limit() {
    binding_refusal("f3d dimension binding point source");
}

#[test]
fn dimension_binding_curve_source_refuses_work_limit() {
    binding_refusal("f3d dimension binding curve source");
}

#[test]
fn dimension_binding_companion_lookup_refuses_work_limit() {
    binding_refusal("f3d dimension binding companion lookup");
}

#[test]
fn dimension_binding_placement_lookup_refuses_work_limit() {
    binding_refusal("f3d dimension binding placement lookup");
}

#[test]
fn dimension_binding_record_lookup_refuses_work_limit() {
    binding_refusal("f3d dimension binding record lookup");
}

#[test]
fn planar_spatial_source_refuses_work_limit() {
    planar_refusal(
        "f3d planar spatial sketch source",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn planar_placement_source_refuses_work_limit() {
    planar_refusal("f3d planar placement source", ResourceDimension::WorkUnits);
}

#[test]
fn planar_spatial_lookup_refuses_work_limit() {
    planar_refusal(
        "f3d planar spatial sketch lookup",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn planar_excluded_lookup_refuses_work_limit() {
    planar_refusal(
        "f3d planar excluded sketch lookup",
        ResourceDimension::WorkUnits,
    );
}

fn relation(stream: &str, offset: u64) -> crate::records::sketch_relations::SketchRelation {
    use crate::records::sketch_relations::{
        SketchRelationDefinition, SketchRelationDraft, SketchRelationMember,
        SketchRelationReference, SketchRelationReturnMember,
    };
    crate::records::sketch_relations::SketchRelation::try_new(SketchRelationDraft {
        id: format!("f3d:{stream}:sketch-relation#{offset}"),
        record_index: 30,
        class_tag: "286".to_owned().try_into().unwrap(),
        byte_offset: offset,
        state_offset: 66,
        owner_reference: 7,
        owner_entity_id: None,
        auxiliary_references: crate::records::identity::ReferenceRun::located(Vec::new()),
        rectangular_counted_reference_count: None,
        members: [30, 31]
            .into_iter()
            .map(|record| SketchRelationMember {
                reference: SketchRelationReference::Index(record),
                offset: 25,
                relation_ordinal: Some(0),
            })
            .collect::<Vec<_>>()
            .try_into()
            .unwrap(),
        owner_reference_offset: 56,
        definition: SketchRelationDefinition::new(0, None).unwrap(),
        entity_genesis: None,
        return_members: [30, 31]
            .into_iter()
            .map(|record| SketchRelationReturnMember {
                reference: SketchRelationReference::Index(record),
                offset: 79,
            })
            .collect::<Vec<_>>()
            .try_into()
            .unwrap(),
        raw_bytes: [0; 96].to_vec(),
    })
    .unwrap()
}

fn frame_refusal(operation: &'static str, dimension: ResourceDimension) {
    let pair = native_fallback_pair();
    let group = native_fallback_group();
    let null_pair = native_fallback_null_pair();
    let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
        let mut relations = vec![relation("Design/BulkStream.dat", 0)];
        crate::design::dimensions::remove_dimension_frame_relations(
            ctx,
            &mut relations,
            std::slice::from_ref(&pair),
            std::slice::from_ref(&group),
            std::slice::from_ref(&null_pair),
        )
    });
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == dimension && limit.operation == operation)
    );
}

#[test]
fn dimension_frame_pair_source_refuses_work_limit() {
    frame_refusal(
        "f3d dimension relation pair source",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn dimension_frame_group_source_refuses_work_limit() {
    frame_refusal(
        "f3d dimension relation group source",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn dimension_frame_null_pair_source_refuses_work_limit() {
    frame_refusal(
        "f3d dimension relation null pair source",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn dimension_frame_relation_source_refuses_work_limit() {
    frame_refusal(
        "f3d dimension relation source",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn dimension_frame_relation_lookup_refuses_work_limit() {
    frame_refusal(
        "f3d dimension frame relation lookup",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn dimension_frame_index_refuses_materialized_limit() {
    frame_refusal(
        "f3d dimension frame relation index",
        ResourceDimension::MaterializedBytes,
    );
}

#[test]
fn dimension_frame_filter_keeps_order_and_releases_scratch() {
    let group = native_fallback_group();
    let first = relation("other", 0);
    let second = relation("Design/BulkStream.dat", 1);
    let mut relations = vec![
        first.clone(),
        relation("Design/BulkStream.dat", 0),
        second.clone(),
    ];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = SCRATCH_LIMIT;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        crate::design::dimensions::remove_dimension_frame_relations(
            ctx,
            &mut relations,
            &[],
            std::slice::from_ref(&group),
            &[],
        )
        .unwrap();
        assert_eq!(relations, [first, second]);
        let _released = ctx
            .reserve_scoped(SCRATCH_LIMIT, "released dimension frame storage")
            .unwrap();
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn dimension_frame_unscoped_records_pay_original_source_work() {
    let mut group = native_fallback_group();
    group.id = "invalid".into();
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "f3d dimension relation group source",
        0,
        |ctx| {
            crate::design::dimensions::remove_dimension_frame_relations(
                ctx,
                &mut Vec::new(),
                &[],
                std::slice::from_ref(&group),
                &[],
            )
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "f3d dimension relation group source")
    );
}

#[test]
fn dimension_binding_conflict_diagnostic_stays_retained() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        u64::try_from("Fusion dimensional geometry record 20 belongs to multiple sketches".len())
            .unwrap()
            - 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut storage = ctx
            .reserve_scoped(0, "dimension binding diagnostic storage")
            .unwrap();
        let mut bindings = std::collections::HashMap::new();
        super::super::super::insert_dimension_binding(
            ctx,
            &mut storage,
            &mut bindings,
            "native",
            20,
            7,
        )
        .unwrap();
        let error = super::super::super::insert_dimension_binding(
            ctx,
            &mut storage,
            &mut bindings,
            "native",
            20,
            8,
        )
        .unwrap_err();
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "f3d Design diagnostic")
        );
    });
}
