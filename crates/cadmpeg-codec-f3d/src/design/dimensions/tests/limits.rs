// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::dimensions::{
    container_only_dimension_companions, project_dimension_constraints,
    project_spatial_dimension_constraints, retain_planar_dimension_constraints,
    DimensionConstraintInputs,
};
use crate::design::test_support::parameter_record;
use crate::records::parameters::{
    DesignCompanionPayload, DesignParameter, DesignParameterCompanion, DesignParameterOwner,
    DesignParameterOwnerWire,
};
use crate::records::dimensions::{
    DesignDimensionAnnotationOperand, DesignDimensionLocusPair, DesignDimensionLocusPairDraft,
};
use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::sketch_placement::{DesignSketchFrame, DesignSketchFrameForm, DesignSketchPlacement};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SpatialSketch,
    SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry,
};

struct Fixture {
    placement: DesignSketchPlacement,
    parameter: DesignParameter,
    owner: DesignParameterOwner,
    curve: SketchCurveIdentity,
    entity: SketchEntity,
    spatial: SpatialSketch,
}

impl Fixture {
    fn inputs(&self) -> DimensionConstraintInputs<'_> {
        DimensionConstraintInputs {
            placements: std::slice::from_ref(&self.placement),
            parameters: std::slice::from_ref(&self.parameter),
            owners: std::slice::from_ref(&self.owner),
            pairs: &[], groups: &[], annotation_frames: &[], null_pairs: &[],
            companions: &[], recipe_records: &[], points: &[],
            curves: std::slice::from_ref(&self.curve),
            entities: std::slice::from_ref(&self.entity),
        }
    }

    fn spatial_entity(&self) -> SpatialSketchEntity {
        SpatialSketchEntity::new(
            SpatialSketchEntityId::mint("synthetic:test:id#spatial-dimension-curve").unwrap(),
            self.spatial.id.clone(),
            SpatialSketchGeometry::try_line_from_parts(
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0)).unwrap(),
            ).unwrap(),
        ).with_native_ref(Some(self.curve.id.clone()))
    }
}

fn fixture() -> Fixture {
    let stream = "f3d:Design/BulkStream.dat";
    let placement = DesignSketchPlacement {
        frame: DesignSketchFrame::new(0, DesignSketchFrameForm::ScopeCompact).unwrap(),
        id: format!("{stream}:placement#7"),
        scope_record_index: Some(10),
        entity_id: crate::records::identity::DesignEntityId::try_from("Sketch_7".to_owned()).unwrap(),
        visibility: None,
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        record_index: 7,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("260".to_owned()).unwrap(),
    };
    let mut parameter = parse_design_parameter_record(&parameter_record(
        Some(21), "1 mm", "Linear Dimension", Some("mm"), "d1", 0.1,
    )).unwrap();
    parameter.id = format!("{stream}:design-parameter#20");
    parameter.record_index = 20;
    let owner = DesignParameterOwner::try_from(DesignParameterOwnerWire {
        id: format!("{stream}:design-parameter-owner#21"),
        byte_offset: 0,
        frame_length: 104,
        class_tag: crate::records::references::DesignClassTag::try_from("292".to_owned()).unwrap(),
        record_index: 21,
        scope_record_index: 10,
        local_ordinal: 0,
        evaluated_value: 0.1,
        evaluated_value_offset: 40,
        parameter_record_index: 20,
        owned_ordinal: 0,
        variant: Some(0),
        companion_record_index: 22,
    }).unwrap();
    let curve = SketchCurveIdentity {
        id: format!("{stream}:sketch-curve#30"),
        record_index: 30,
        owner_reference: Some(10),
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
        byte_offset: 0,
        geometry_offset: 0,
        entity_genesis: None,
        primary_id: std::num::NonZeroU64::new(30).unwrap(),
        secondary_id: 1,
        geometry: None,
    };
    let entity = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#dimension-curve").unwrap(),
        crate::ids::neutral_sketch_id(&placement),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(0.0, 0.0), end: Point2::new(1.0, 0.0),
        }).unwrap(),
    ).with_native_ref(Some(curve.id.clone()));
    let spatial = SpatialSketch {
        id: crate::ids::neutral_spatial_sketch_id(&placement),
        name: None, configuration: None, visible: None,
        profiles: Vec::new(), native_ref: Some(placement.id.clone()),
    };
    Fixture { placement, parameter, owner, curve, entity, spatial }
}

fn parameter_companion() -> DesignParameterCompanion {
    DesignParameterCompanion::unbound(
        "f3d:Design/BulkStream.dat:design-parameter-companion#22".to_owned(),
        0,
        crate::records::references::DesignClassTag::try_from("408".to_owned()).unwrap(),
        22,
        21,
        std::num::NonZeroU64::new(1).unwrap(),
        42,
    ).bound(DesignCompanionPayload::new(58, 0, Vec::new()))
}

fn assert_refusal(operation: &'static str) {
    let fixture = fixture();
    for limit in 0..32 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match project_dimension_constraints(Some(&ctx), &fixture.inputs(),
            std::slice::from_ref(&fixture.spatial), 1.0e-6) {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == operation
                    && failure.dimension == ResourceDimension::CollectionItems => return,
            Err(CodecError::ResourceLimit(_)) => {},
            Ok(_) => panic!("expected {operation} refusal, got success"),
            Err(error) => panic!("expected {operation} refusal: {error}"),
        }
    }
    panic!("no {operation} refusal");
}

fn assert_spatial_index_refusal(operation: &'static str) {
    let fixture = fixture();
    for limit in 0..64 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match project_spatial_dimension_constraints(Some(&ctx), &fixture.inputs(),
            std::slice::from_ref(&fixture.spatial), &[], 1.0e-6) {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == operation
                    && failure.dimension == ResourceDimension::CollectionItems => return,
            Err(CodecError::ResourceLimit(_)) => {},
            Ok(_) => panic!("expected {operation} refusal, got success"),
            Err(error) => panic!("expected {operation} refusal: {error}"),
        }
    }
    panic!("no {operation} refusal");
}

#[test]
fn spatial_planar_sketch_index_refuses_collection_limit() {
    assert_spatial_index_refusal("f3d spatial planar sketch index");
}

#[test]
fn spatial_scope_sketch_index_refuses_collection_limit() {
    assert_spatial_index_refusal("f3d spatial scope sketch index");
}

#[test]
fn spatial_native_record_index_refuses_collection_limit() {
    assert_spatial_index_refusal("f3d spatial native record index");
}

#[test]
fn spatial_projected_record_index_refuses_collection_limit() {
    let fixture = fixture();
    let entity = fixture.spatial_entity();
    for limit in 0..64 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = project_spatial_dimension_constraints(Some(&ctx), &fixture.inputs(),
            std::slice::from_ref(&fixture.spatial), std::slice::from_ref(&entity), 1.0e-6);
        match result {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d spatial projected record index"
                    && failure.dimension == ResourceDimension::CollectionItems => return,
            Err(CodecError::ResourceLimit(_)) => {},
            Ok(_) => panic!("expected spatial projected record index refusal"),
            Err(error) => panic!("expected spatial projected record index refusal: {error}"),
        }
    }
    panic!("no spatial projected record index refusal");
}

#[test]
fn spatial_parameter_length_index_refuses_collection_limit() {
    assert_spatial_index_refusal("f3d spatial parameter length index");
}

#[test]
fn spatial_parameter_index_refuses_collection_limit() {
    assert_spatial_index_refusal("f3d spatial parameter index");
}

#[test]
fn spatial_parameter_count_index_refuses_collection_limit() {
    let fixture = fixture();
    let companion = parameter_companion();
    let mut inputs = fixture.inputs();
    inputs.companions = std::slice::from_ref(&companion);
    for limit in 0..128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = project_spatial_dimension_constraints(Some(&ctx), &inputs,
            std::slice::from_ref(&fixture.spatial), &[], 1.0e-6);
        match result {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d spatial parameter count index"
                    && failure.dimension == ResourceDimension::CollectionItems => return,
            Err(CodecError::ResourceLimit(_)) => {},
            Ok(_) => panic!("expected spatial parameter count index refusal"),
            Err(error) => panic!("expected spatial parameter count index refusal: {error}"),
        }
    }
    panic!("no spatial parameter count index refusal");
}

#[test]
fn spatial_scope_sketch_id_refuses_retained_limit() {
    let fixture = fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = project_spatial_dimension_constraints(Some(&ctx), &fixture.inputs(),
        std::slice::from_ref(&fixture.spatial), &[], 1.0e-6);
    assert!(matches!(result, Err(CodecError::ResourceLimit(failure))
        if failure.operation == "f3d spatial scope sketch id"
            && failure.dimension == ResourceDimension::RetainedBytes));
}

#[test]
fn spatial_parameter_count_id_refuses_retained_limit() {
    let fixture = fixture();
    let companion = parameter_companion();
    let mut inputs = fixture.inputs();
    inputs.companions = std::slice::from_ref(&companion);
    for limit in 0..512 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = project_spatial_dimension_constraints(Some(&ctx), &inputs,
            std::slice::from_ref(&fixture.spatial), &[], 1.0e-6);
        match result {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d spatial parameter count id"
                    && failure.dimension == ResourceDimension::RetainedBytes => return,
            Err(CodecError::ResourceLimit(_)) => {},
            Ok(_) => panic!("expected spatial parameter count ID refusal"),
            Err(error) => panic!("expected spatial parameter count ID refusal: {error}"),
        }
    }
    panic!("no spatial parameter count ID refusal");
}

#[test]
fn spatial_owner_record_index_refuses_collection_limit() {
    assert_spatial_index_refusal("f3d spatial owner record index");
}

fn assert_spatial_companion_collection_refusal(operation: &'static str) {
    let fixture = fixture();
    let entity = fixture.spatial_entity();
    let companion = parameter_companion().bound(DesignCompanionPayload::new(58, 1, Vec::new()));
    let mut inputs = fixture.inputs();
    inputs.companions = std::slice::from_ref(&companion);
    inputs.entities = &[];
    for limit in 0..128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = project_spatial_dimension_constraints(Some(&ctx), &inputs,
            std::slice::from_ref(&fixture.spatial), std::slice::from_ref(&entity), 1.0e-6);
        match result {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == operation
                    && failure.dimension == ResourceDimension::CollectionItems => return,
            Err(CodecError::ResourceLimit(_)) => {},
            Ok(_) => panic!("expected {operation} refusal"),
            Err(error) => panic!("expected {operation} refusal: {error}"),
        }
    }
    panic!("no {operation} refusal");
}

#[test]
fn retained_spatial_parameter_index_refuses_collection_limit() {
    assert_spatial_companion_collection_refusal("f3d retained spatial parameter index");
}

#[test]
fn spatial_companion_record_index_refuses_collection_limit() {
    assert_spatial_companion_collection_refusal("f3d spatial companion record index");
}

#[test]
fn spatial_source_parameter_index_refuses_collection_limit() {
    assert_spatial_companion_collection_refusal("f3d spatial source parameter index");
}

#[test]
fn projected_spatial_dimension_output_refuses_collection_limit() {
    assert_spatial_companion_collection_refusal("f3d projected spatial dimension output");
}

fn assert_spatial_companion_retained_refusal(operation: &'static str) {
    let fixture = fixture();
    let entity = fixture.spatial_entity();
    let companion = parameter_companion().bound(DesignCompanionPayload::new(58, 1, Vec::new()));
    let mut inputs = fixture.inputs();
    inputs.companions = std::slice::from_ref(&companion);
    inputs.entities = &[];
    for limit in 0..512 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_retained_bytes = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = project_spatial_dimension_constraints(Some(&ctx), &inputs,
            std::slice::from_ref(&fixture.spatial), std::slice::from_ref(&entity), 1.0e-6);
        match result {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == operation
                    && failure.dimension == ResourceDimension::RetainedBytes => return,
            Err(CodecError::ResourceLimit(_)) => {},
            Ok(_) => panic!("expected {operation} refusal"),
            Err(error) => panic!("expected {operation} refusal: {error}"),
        }
    }
    panic!("no {operation} refusal");
}

#[test]
fn retained_spatial_parameter_id_refuses_retained_limit() {
    assert_spatial_companion_retained_refusal("f3d retained spatial parameter id");
}

#[test]
fn projected_spatial_sketch_id_refuses_retained_limit() {
    assert_spatial_companion_retained_refusal("f3d projected spatial sketch id");
}

#[test]
fn spatial_source_parameter_id_refuses_retained_limit() {
    assert_spatial_companion_retained_refusal("f3d spatial source parameter id");
}

fn assert_missing_spatial_refusal(
    operation: &'static str,
    dimension: ResourceDimension,
) {
    let fixture = fixture();
    let companion = parameter_companion();
    let mut inputs = fixture.inputs();
    inputs.companions = std::slice::from_ref(&companion);
    for limit in 0..1024 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            _ => panic!("unsupported dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = project_spatial_dimension_constraints(Some(&ctx), &inputs,
            std::slice::from_ref(&fixture.spatial), &[], 1.0e-6);
        match result {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == operation && failure.dimension == dimension => return,
            Err(CodecError::ResourceLimit(_)) => {},
            Ok(_) => panic!("expected {operation} refusal"),
            Err(error) => panic!("expected {operation} refusal: {error}"),
        }
    }
    panic!("no {operation} refusal");
}

#[test]
fn missing_spatial_parameter_refuses_collection_limit() {
    assert_missing_spatial_refusal("f3d missing spatial parameter",
        ResourceDimension::CollectionItems);
}

#[test]
fn missing_spatial_constraint_output_refuses_collection_limit() {
    assert_missing_spatial_refusal("f3d missing spatial constraint output",
        ResourceDimension::CollectionItems);
}

#[test]
fn missing_spatial_sketch_id_refuses_retained_limit() {
    assert_missing_spatial_refusal("f3d missing spatial sketch id",
        ResourceDimension::RetainedBytes);
}

#[test]
fn missing_spatial_operand_native_id_refuses_retained_limit() {
    assert_missing_spatial_refusal("f3d missing spatial operand native id",
        ResourceDimension::RetainedBytes);
}

#[test]
fn missing_spatial_constraint_native_id_refuses_retained_limit() {
    assert_missing_spatial_refusal("f3d missing spatial constraint native id",
        ResourceDimension::RetainedBytes);
}

#[test]
fn missing_spatial_output_parameter_id_refuses_retained_limit() {
    assert_missing_spatial_refusal("f3d missing spatial output parameter id",
        ResourceDimension::RetainedBytes);
}

#[test]
fn planar_spatial_sketch_index_refuses_collection_limit() {
    assert_refusal("f3d planar spatial sketch index");
}

#[test]
fn dimension_sketch_index_refuses_collection_limit() {
    assert_refusal("f3d dimension sketch index");
}

#[test]
fn dimension_scope_sketch_index_refuses_collection_limit() {
    assert_refusal("f3d dimension scope sketch index");
}

#[test]
fn dimension_parameter_index_refuses_collection_limit() {
    assert_refusal("f3d dimension parameter index");
}

#[test]
fn dimension_companion_index_refuses_collection_limit() {
    assert_refusal("f3d dimension companion index");
}

#[test]
fn dimension_native_geometry_index_refuses_collection_limit() {
    assert_refusal("f3d dimension native geometry index");
}

#[test]
fn dimension_native_reference_index_refuses_collection_limit() {
    assert_refusal("f3d dimension native reference index");
}

#[test]
fn dimension_projected_entity_index_refuses_collection_limit() {
    assert_refusal("f3d dimension projected entity index");
}

#[test]
fn dimension_curve_secondary_index_refuses_collection_limit() {
    assert_refusal("f3d dimension curve secondary index");
}

#[test]
fn planar_dimension_output_refuses_collection_limit() {
    let constraint = cadmpeg_ir::sketches::SketchConstraint {
        id: cadmpeg_ir::sketches::SketchConstraintId::mint(
            "synthetic:test:id#dimension-limit-constraint").unwrap(),
        sketch: cadmpeg_ir::sketches::SketchId::mint(
            "synthetic:test:id#dimension-limit-sketch").unwrap(),
        definition: cadmpeg_ir::sketches::SketchConstraintDefinition::try_from(
            cadmpeg_ir::sketches::SketchConstraintDefinitionInput::Disabled {},
        ).unwrap(),
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
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = retain_planar_dimension_constraints(Some(&ctx), &[], &[],
        vec![constraint.clone()]).unwrap_err();
    assert!(matches!(error, CodecError::ResourceLimit(failure)
        if failure.operation == "f3d planar dimension output"
            && failure.dimension == ResourceDimension::CollectionItems));
    let output = retain_planar_dimension_constraints(None, &[], &[], vec![constraint.clone()]).unwrap();
    assert_eq!(output, [constraint]);
}

fn companion_pair() -> DesignDimensionLocusPair {
    DesignDimensionLocusPair::try_new(DesignDimensionLocusPairDraft {
        id: "f3d:Design/BulkStream.dat:design-dimension-locus-pair#31".to_owned(),
        companion_record_index: 30,
        governing_companion_record_index: 99,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("423".to_owned()).unwrap(),
        record_index: 31,
        frame_length: 100,
        opaque_index: None,
        loci: [
            DesignDimensionAnnotationOperand {
                geometry_record_index: None,
                geometry_reference_offset: 25,
                role: 14,
                role_offset: 35,
            },
            DesignDimensionAnnotationOperand {
                geometry_record_index: std::num::NonZeroU32::new(40),
                geometry_reference_offset: 40,
                role: 3,
                role_offset: 50,
            },
        ],
        paired_class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned()).unwrap(),
        paired_byte_offset: 100,
    }).unwrap()
}

fn assert_companion_refusal(limit: u64, operation: &'static str) {
    let pair = companion_pair();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let pairs = [pair];
    let result = container_only_dimension_companions(
        Some(&ctx), &pairs, &[], &[], &[], &[],
    );
    assert!(matches!(result, Err(CodecError::ResourceLimit(failure))
        if failure.operation == operation
            && failure.dimension == ResourceDimension::CollectionItems));
}

#[test]
fn physical_dimension_companion_refuses_collection_limit() {
    assert_companion_refusal(0, "f3d physical dimension companion");
}

#[test]
fn governed_dimension_companion_refuses_collection_limit() {
    assert_companion_refusal(1, "f3d governed dimension companion");
}

#[test]
fn container_only_dimension_companion_refuses_collection_limit() {
    assert_companion_refusal(2, "f3d container-only dimension companion");
}
