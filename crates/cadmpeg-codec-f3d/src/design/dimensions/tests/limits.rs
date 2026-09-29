// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::dimensions::{
    project_dimension_constraints, retain_planar_dimension_constraints, DimensionConstraintInputs,
};
use crate::design::test_support::parameter_record;
use crate::records::parameters::{DesignParameter, DesignParameterOwner, DesignParameterOwnerWire};
use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::sketch_placement::{DesignSketchFrame, DesignSketchFrameForm, DesignSketchPlacement};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SpatialSketch,
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
