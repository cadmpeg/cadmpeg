// SPDX-License-Identifier: Apache-2.0

use super::project_configuration_sketch_states;
use crate::history::tests::{design_configuration, feature_input_lane};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::geometry::sampled::PolygonalSurface;
use cadmpeg_ir::geometry::{PlacedSurface, SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::{CodecFormat, SourceObjectAssociation};

fn carrier_model() -> cadmpeg_ir::CadIr {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let polygonal = PolygonalSurface::new(
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
        vec![[0, 1, 2]], 0.01,
    ).unwrap();
    let mut placed = SolvedSurfaceGeometry::Polygonal(polygonal);
    for _ in 0..2 {
        placed = SolvedSurfaceGeometry::Transformed(PlacedSurface::try_new(
            Box::new(placed), cadmpeg_ir::transform::Transform::identity(),
        ).unwrap());
    }
    ir.model.surfaces.push(Surface {
        id: cadmpeg_ir::ids::SurfaceId::mint("synthetic:test:id#polygonal").unwrap(),
        geometry: SurfaceGeometry::Solved(placed),
        source_object: Some(SourceObjectAssociation {
            format: CodecFormat::Sldprt,
            object_id: cadmpeg_core::text::NonBlankString::new("carrier").unwrap(),
            name: Some("Retained surface".into()), color: None, visible: Some(true),
            layer: Some("Layer".into()), instance_path: vec!["Outer".into(), "Inner".into()],
        }),
    });
    for rational in [false, true] {
        let axis = || NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false);
        let poles = vec![
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
            vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)],
        ];
        let weights = rational.then(|| vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
        let geometry = NurbsSurface::from_lanes(axis(), axis(), NurbsSurfaceLanes::new(poles, weights), false).unwrap();
        ir.model.surfaces.push(Surface {
            id: cadmpeg_ir::ids::SurfaceId::mint(format!("synthetic:test:id#nurbs-{rational}")).unwrap(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(geometry)),
            source_object: None,
        });
    }
    ir.model.surfaces.push(Surface {
        id: cadmpeg_ir::ids::SurfaceId::mint("synthetic:test:id#procedural").unwrap(),
        geometry: SurfaceGeometry::Procedural {
            construction: cadmpeg_ir::ids::ProceduralSurfaceId::mint("synthetic:test:id#construction").unwrap(),
            cache: Some(SolvedSurfaceGeometry::Unknown {
                record: Some(cadmpeg_ir::ids::UnknownId::mint("synthetic:test:id#record").unwrap()),
            }),
        },
        source_object: None,
    });
    let mut configuration = design_configuration("carriers", 0, Some(0), None);
    configuration.bodies = None;
    ir.model.configurations.push(configuration);
    ir
}

fn run(policy: &DecodePolicy) -> Result<(), CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut ir = carrier_model();
    let expected = ir.clone();
    let losses = project_configuration_sketch_states(
        &ctx, &mut ir, &[], &[feature_input_lane("lane", Some("0"))],
        &mut cadmpeg_ir::Annotations::default(),
    )?;
    assert!(losses.is_empty());
    assert_eq!(ir, expected);
    Ok(())
}

fn set_limit(policy: &mut DecodePolicy, dimension: ResourceDimension, limit: u64) {
    match dimension {
        ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
        ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
        ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = limit,
        ResourceDimension::WorkUnits => policy.limits.max_work_units = limit,
        _ => panic!("unexpected test dimension"),
    }
}

fn assert_projection_refusal(dimension: ResourceDimension, project: fn(&DecodePolicy) -> Result<(), CodecError>) {
    let mut policy = DecodePolicy::service();
    project(&policy).unwrap();
    let mut lower = 0;
    let mut upper = 1;
    loop {
        set_limit(&mut policy, dimension, upper);
        match project(&policy) {
            Ok(()) => break,
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, dimension);
                upper = upper.checked_mul(2).unwrap();
            }
            Err(error) => panic!("unexpected route error: {error}"),
        }
    }
    while lower < upper {
        let midpoint = lower + (upper - lower) / 2;
        set_limit(&mut policy, dimension, midpoint);
        match project(&policy) {
            Ok(()) => upper = midpoint,
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, dimension);
                lower = midpoint + 1;
            }
            Err(error) => panic!("unexpected route error: {error}"),
        }
    }
    assert!(upper > 0);
    set_limit(&mut policy, dimension, upper);
    project(&policy).unwrap();
    set_limit(&mut policy, dimension, upper - 1);
    assert!(matches!(project(&policy), Err(CodecError::ResourceLimit(limit)) if limit.dimension == dimension));
}

#[test]
fn configuration_sketch_projection_refuses_carrier_collection_limit() {
    assert_projection_refusal(ResourceDimension::CollectionItems, run);
}

#[test]
fn configuration_sketch_projection_refuses_carrier_retained_limit() {
    assert_projection_refusal(ResourceDimension::RetainedBytes, run);
}

#[test]
fn configuration_sketch_projection_refuses_carrier_nesting_limit() {
    assert_projection_refusal(ResourceDimension::RecursionDepth, run);
}

#[test]
fn configuration_sketch_projection_refuses_carrier_work_limit() {
    assert_projection_refusal(ResourceDimension::WorkUnits, run);
}


fn datum_model() -> cadmpeg_ir::CadIr {
    use cadmpeg_ir::features::{
        ConfigurationEvaluation, ConfigurationFeatureState, DatumPlaneReference,
        Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    };
    let plane = FeatureId::mint("synthetic:test:id#plane").unwrap();
    let first = FeatureId::mint("synthetic:test:id#offset-first").unwrap();
    let second = FeatureId::mint("synthetic:test:id#offset-second").unwrap();
    let definitions = [
        FeatureDefinition::Operation(FeatureOperation::DatumPlane {
            frame: cadmpeg_ir::features::FeatureDatumPlaneFrame::new(
                Point3::new(0.0, 0.0, 0.0), cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            ).unwrap(),
        }),
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature { feature: plane.clone() }),
            distance: cadmpeg_ir::scalar::Length::new(2.0).unwrap(),
        }),
        FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: Some(DatumPlaneReference::Feature { feature: first.clone() }),
            distance: cadmpeg_ir::scalar::Length::new(3.0).unwrap(),
        }),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty();
    for ((id, ordinal, dependencies), definition) in [
        (plane.clone(), 0, Vec::new()),
        (first, 1, vec![plane.clone()]),
        (second.clone(), 2, vec![FeatureId::mint("synthetic:test:id#offset-first").unwrap()]),
    ].into_iter().zip(definitions) {
        ir.model.features.push(Feature {
            id, ordinal, name: None, suppressed: Some(false),
            dependencies: dependencies.try_into().unwrap(),
            source_properties: std::collections::BTreeMap::new(), source_tag: None,
            source_text: None, source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(definition), native_ref: None,
        });
    }
    let mut configuration = design_configuration("datum", 0, Some(0), None);
    configuration.feature_states.insert(second, ConfigurationFeatureState {
        evaluation: ConfigurationEvaluation::Active { outputs: cadmpeg_ir::features::DistinctMembers::default() },
        dependencies: vec![plane].try_into().unwrap(),
        definition: FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
            reference: None, distance: cadmpeg_ir::scalar::Length::new(7.0).unwrap(),
        }),
    });
    ir.model.configurations.push(configuration);
    ir
}

fn run_datum(policy: &DecodePolicy) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{DatumPlaneReference, FeatureDefinition, FeatureId, FeatureOperation};
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut ir = datum_model();
    let mut expected = ir.clone();
    let state = expected.model.configurations[0].feature_states
        .get_mut(&FeatureId::mint("synthetic:test:id#offset-second").unwrap()).unwrap();
    state.definition = FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::Feature {
            feature: FeatureId::mint("synthetic:test:id#offset-first").unwrap(),
        }),
        distance: cadmpeg_ir::scalar::Length::new(7.0).unwrap(),
    });
    state.dependencies = vec![
        FeatureId::mint("synthetic:test:id#plane").unwrap(),
        FeatureId::mint("synthetic:test:id#offset-first").unwrap(),
    ].try_into().unwrap();
    super::inherit_configuration_reference_plane_states(&ctx, &mut ir)?;
    assert_eq!(ir, expected);
    Ok(())
}

#[test]
fn configuration_datum_state_projection_refuses_collection_limit() {
    assert_projection_refusal(ResourceDimension::CollectionItems, run_datum);
}

#[test]
fn configuration_datum_state_projection_refuses_retained_limit() {
    assert_projection_refusal(ResourceDimension::RetainedBytes, run_datum);
}

#[test]
fn configuration_datum_state_projection_refuses_nesting_limit() {
    assert_projection_refusal(ResourceDimension::RecursionDepth, run_datum);
}

#[test]
fn configuration_datum_state_projection_refuses_work_limit() {
    assert_projection_refusal(ResourceDimension::WorkUnits, run_datum);
}

fn run_design(policy: &DecodePolicy) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{ConfigurationEvaluation, FeatureDefinition, FeatureId, FeatureOperation, ParameterId, ParameterValue};
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut feature = crate::history::tests::feature("feature", None, 0);
    feature.parameters.insert(cadmpeg_core::nonblank_literal!("Length"), "12mm".into());
    let histories = [crate::records::FeatureHistory {
        id: "history".into(), part_name: None, properties: std::collections::BTreeMap::new(),
        content: Vec::new(), configurations: Vec::new(), features: vec![feature],
    }];
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.configurations.push(design_configuration("design", 0, Some(0), None));
    super::project_configuration_design_states(
        &ctx, &mut ir, &histories, &[feature_input_lane("lane", Some("0"))], &[], None,
    )?;
    let configuration = &ir.model.configurations[0];
    assert_eq!(configuration.parameter_values, std::collections::BTreeMap::from([(
        ParameterId::mint("sldprt:model:parameter#feature:0").unwrap(),
        ParameterValue::Length(cadmpeg_ir::scalar::Length::new(12.0).unwrap()),
    )]));
    assert_eq!(configuration.feature_states.len(), 1);
    let state = &configuration.feature_states[&FeatureId::mint("sldprt:model:feature#feature").unwrap()];
    assert_eq!(state.evaluation, ConfigurationEvaluation::Active { outputs: cadmpeg_ir::features::DistinctMembers::default() });
    assert!(state.dependencies.is_empty());
    assert_eq!(state.definition, FeatureDefinition::Operation(FeatureOperation::Native {
        kind: "Custom".into(), parameters: std::collections::BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("Length"), "12mm".into(),
        )]),
    }));
    Ok(())
}

#[test]
fn configuration_design_projection_refuses_collection_limit() {
    assert_projection_refusal(ResourceDimension::CollectionItems, run_design);
}

#[test]
fn configuration_design_projection_refuses_work_limit() {
    assert_projection_refusal(ResourceDimension::WorkUnits, run_design);
}

fn run_unscoped_datum(policy: &DecodePolicy) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{DatumPlaneReference, FeatureDefinition, FeatureId, FeatureOperation};
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let mut ir = datum_model();
    let mut expected = ir.clone();
    let state = expected.model.configurations[0].feature_states
        .get_mut(&FeatureId::mint("synthetic:test:id#offset-second").unwrap()).unwrap();
    state.definition = FeatureDefinition::Operation(FeatureOperation::DatumOffsetPlane {
        reference: Some(DatumPlaneReference::Feature {
            feature: FeatureId::mint("synthetic:test:id#offset-first").unwrap(),
        }),
        distance: cadmpeg_ir::scalar::Length::new(7.0).unwrap(),
    });
    state.dependencies = vec![
        FeatureId::mint("synthetic:test:id#plane").unwrap(),
        FeatureId::mint("synthetic:test:id#offset-first").unwrap(),
    ].try_into().unwrap();
    let losses = project_configuration_sketch_states(
        &ctx, &mut ir, &[], &[], &mut cadmpeg_ir::Annotations::default(),
    )?;
    assert!(losses.is_empty());
    assert_eq!(ir, expected);
    Ok(())
}

#[test]
fn configuration_sketch_projection_refuses_unscoped_datum_collection_limit() {
    assert_projection_refusal(ResourceDimension::CollectionItems, run_unscoped_datum);
}

#[test]
fn configuration_sketch_projection_refuses_unscoped_datum_retained_limit() {
    assert_projection_refusal(ResourceDimension::RetainedBytes, run_unscoped_datum);
}

#[test]
fn configuration_sketch_projection_refuses_unscoped_datum_work_limit() {
    assert_projection_refusal(ResourceDimension::WorkUnits, run_unscoped_datum);
}

fn run_spatial_ownership(policy: &DecodePolicy) -> Result<(), CodecError> {
    use cadmpeg_ir::features::{ConfigurationEvaluation, ConfigurationFeatureState, Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation};
    use cadmpeg_ir::sketches::{SpatialSketch, SpatialSketchId};
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let sketch = SpatialSketchId::mint("sldprt:model:spatial-sketch#scoped").unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut configuration = design_configuration("spatial", 0, Some(0), None);
    for (ordinal, name, definition) in [
        (0, "scoped", FeatureDefinition::Operation(FeatureOperation::SpatialSketch { sketch: None })),
        (1, "alias", FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Unresolved,
        })),
    ] {
        let id = FeatureId::mint(format!("sldprt:model:feature#{name}")).unwrap();
        ir.model.features.push(Feature {
            id: id.clone(), ordinal, name: None, suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::new(), source_tag: None,
            source_text: None, source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::SpatialSketch { sketch: Some(sketch.clone()) },
            )), native_ref: None,
        });
        configuration.feature_states.insert(id, ConfigurationFeatureState {
            evaluation: ConfigurationEvaluation::Active { outputs: cadmpeg_ir::features::DistinctMembers::default() },
            dependencies: cadmpeg_ir::features::DistinctMembers::default(), definition,
        });
    }
    ir.model.configurations.push(configuration);
    ir.model.spatial_sketches.push(SpatialSketch {
        id: sketch.clone(), name: None, configuration: Some("0".into()), visible: None,
        profiles: Vec::new(), native_ref: Some("lane".into()),
    });
    let mut expected = ir.clone();
    for state in expected.model.configurations[0].feature_states.values_mut() {
        state.definition = FeatureDefinition::Operation(FeatureOperation::SpatialSketch { sketch: Some(sketch.clone()) });
    }
    let losses = project_configuration_sketch_states(
        &ctx, &mut ir, &[], &[feature_input_lane("lane", Some("0"))],
        &mut cadmpeg_ir::Annotations::default(),
    )?;
    assert!(losses.is_empty());
    assert_eq!(ir, expected);
    Ok(())
}

#[test]
fn configuration_sketch_projection_refuses_ownership_collection_limit() {
    assert_projection_refusal(ResourceDimension::CollectionItems, run_spatial_ownership);
}

#[test]
fn configuration_sketch_projection_refuses_ownership_retained_limit() {
    assert_projection_refusal(ResourceDimension::RetainedBytes, run_spatial_ownership);
}

#[test]
fn configuration_sketch_projection_refuses_ownership_work_limit() {
    assert_projection_refusal(ResourceDimension::WorkUnits, run_spatial_ownership);
}

fn run_hole_construction(policy: &DecodePolicy) -> Result<(), CodecError> {
    use cadmpeg_ir::features::holes::{HoleConstruction, HoleKind, HoleShape, HoleSpecification, HoleThreadDepth, ThreadHand};
    use cadmpeg_ir::features::{ConfigurationEvaluation, ConfigurationFeatureState, Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation, LinearTermination};
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let clearance = HoleSpecification::Clearance {
        standard: cadmpeg_core::nonblank_literal!("ISO"), designation: Some("M6".into()),
        fit: Some("Close".into()), modeled: false, cosmetic: true, hand: ThreadHand::Right,
        depth: HoleThreadDepth::HoleDepth, clearance: Some(cadmpeg_ir::scalar::Length::new(0.1).unwrap()),
    };
    let threaded = HoleSpecification::Threaded {
        standard: cadmpeg_core::nonblank_literal!("ISO"), designation: Some("M6x1".into()),
        class: Some("6H".into()), modeled: true, cosmetic: false,
        pitch: Some(cadmpeg_ir::scalar::PositiveLength::new(1.0).unwrap()),
        major_diameter: Some(cadmpeg_ir::scalar::PositiveLength::new(6.0).unwrap()),
        hand: ThreadHand::Left, depth: HoleThreadDepth::TappedStandard, clearance: None,
    };
    let hole = |specification, complete: bool| FeatureDefinition::Operation(FeatureOperation::Hole {
        profile: None, profile_filter: None, face: None, direction: None, placements: None,
        shape: HoleShape::new(HoleConstruction::Form { kind: HoleKind::Simple, specification }, None,
            complete.then(|| cadmpeg_ir::scalar::PositiveLength::new(5.0).unwrap())).unwrap(),
        extent: complete.then(|| LinearTermination::Blind {
            length: cadmpeg_ir::scalar::NonZeroLength::new(12.0).unwrap(),
        }), bottom: None, taper_angle: None, allow_multi_profile_faces: None,
    });
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut configuration = design_configuration("hole", 0, Some(0), None);
    for (ordinal, name, base_specification, local_specification) in [
        (0, "clearance", Some(Box::new(clearance.clone())), None),
        (1, "threaded", None, Some(Box::new(threaded.clone()))),
    ] {
        let id = FeatureId::mint(format!("synthetic:test:id#{name}")).unwrap();
        ir.model.features.push(Feature {
            id: id.clone(), ordinal, name: None, suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::new(), source_tag: None,
            source_text: None, source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(hole(base_specification, true)), native_ref: None,
        });
        configuration.feature_states.insert(id, ConfigurationFeatureState {
            evaluation: ConfigurationEvaluation::Active { outputs: cadmpeg_ir::features::DistinctMembers::default() },
            dependencies: cadmpeg_ir::features::DistinctMembers::default(), definition: hole(local_specification, false),
        });
    }
    ir.model.configurations.push(configuration);
    let mut expected = ir.clone();
    for (name, specification) in [("clearance", clearance), ("threaded", threaded)] {
        expected.model.configurations[0].feature_states
            .get_mut(&FeatureId::mint(format!("synthetic:test:id#{name}")).unwrap()).unwrap()
            .definition = hole(Some(Box::new(specification)), true);
    }
    let losses = project_configuration_sketch_states(
        &ctx, &mut ir, &[], &[], &mut cadmpeg_ir::Annotations::default(),
    )?;
    assert!(losses.is_empty());
    assert_eq!(ir, expected);
    Ok(())
}

#[test]
fn configuration_sketch_projection_refuses_hole_construction_collection_limit() {
    assert_projection_refusal(ResourceDimension::CollectionItems, run_hole_construction);
}

#[test]
fn configuration_sketch_projection_refuses_hole_construction_retained_limit() {
    assert_projection_refusal(ResourceDimension::RetainedBytes, run_hole_construction);
}

#[test]
fn configuration_sketch_projection_refuses_hole_construction_work_limit() {
    assert_projection_refusal(ResourceDimension::WorkUnits, run_hole_construction);
}

fn run_hole_operands(policy: &DecodePolicy) -> Result<(), CodecError> {
    use cadmpeg_ir::features::holes::{HoleConstruction, HoleKind, HolePlacement, HoleShape};
    use cadmpeg_ir::features::{ConfigurationEvaluation, ConfigurationFeatureState, FaceSelection, Feature, FeatureDefinition, FeatureDirection3, FeatureEvaluation, FeatureId, FeatureOperation, FinitePoint3};
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let hole = |operands: bool| FeatureDefinition::Operation(FeatureOperation::Hole {
        profile: None, profile_filter: None,
        face: operands.then(|| FaceSelection::Resolved {
            faces: vec![cadmpeg_ir::ids::FaceId::mint("synthetic:test:id#face-first").unwrap(),
                cadmpeg_ir::ids::FaceId::mint("synthetic:test:id#face-second").unwrap()],
            native: "retained face selection".into(),
        }), direction: None,
        placements: operands.then(|| vec![
            HolePlacement::Directed {
                position: FinitePoint3::new(Point3::new(1.0, 2.0, 3.0)).unwrap(),
                direction: FeatureDirection3::new(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)).unwrap(),
            },
            HolePlacement::Axis {
                origin: FinitePoint3::new(Point3::new(4.0, 5.0, 6.0)).unwrap(),
                axis: FeatureDirection3::new(cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)).unwrap(),
            },
        ]),
        shape: HoleShape::new(HoleConstruction::form(HoleKind::Simple), None, None).unwrap(),
        extent: None, bottom: None, taper_angle: None, allow_multi_profile_faces: None,
    });
    let id = FeatureId::mint("synthetic:test:id#hole-operands").unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features.push(Feature {
        id: id.clone(), ordinal: 0, name: None, suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(), source_tag: None,
        source_text: None, source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(hole(true)), native_ref: None,
    });
    let mut configuration = design_configuration("hole-operands", 0, Some(0), None);
    configuration.feature_states.insert(id.clone(), ConfigurationFeatureState {
        evaluation: ConfigurationEvaluation::Active { outputs: cadmpeg_ir::features::DistinctMembers::default() },
        dependencies: cadmpeg_ir::features::DistinctMembers::default(), definition: hole(false),
    });
    ir.model.configurations.push(configuration);
    let mut expected = ir.clone();
    expected.model.configurations[0].feature_states.get_mut(&id).unwrap().definition = hole(true);
    let losses = project_configuration_sketch_states(
        &ctx, &mut ir, &[], &[], &mut cadmpeg_ir::Annotations::default(),
    )?;
    assert!(losses.is_empty());
    assert_eq!(ir, expected);
    Ok(())
}

#[test]
fn configuration_sketch_projection_refuses_hole_operand_collection_limit() {
    assert_projection_refusal(ResourceDimension::CollectionItems, run_hole_operands);
}

#[test]
fn configuration_sketch_projection_refuses_hole_operand_retained_limit() {
    assert_projection_refusal(ResourceDimension::RetainedBytes, run_hole_operands);
}

#[test]
fn configuration_sketch_projection_refuses_hole_operand_work_limit() {
    assert_projection_refusal(ResourceDimension::WorkUnits, run_hole_operands);
}

fn profile_termination_operands() -> (Vec<cadmpeg_ir::features::PlanarProfileRef>, Vec<cadmpeg_ir::features::LinearTermination>) {
    use cadmpeg_ir::features::{FaceSelection, FeatureId, GeneratedCurveRef, GeneratedVertexRef, LinearTermination, PlanarProfileRef, SketchProfileBoundaryUse, SketchProfileRegion, VertexSelection};
    use cadmpeg_ir::ids::{FaceId, FeatureInputTopologyId, HistoricalFaceId, HistoricalVertexId};
    use cadmpeg_ir::sketches::{SketchEntityId, SketchId};
    let sketch = SketchId::mint("synthetic:test:id#profile-sketch").unwrap();
    let entity = SketchEntityId::mint("synthetic:test:id#profile-entity").unwrap();
    let owner = FeatureId::mint("synthetic:test:id#profile-owner").unwrap();
    let state = FeatureInputTopologyId::mint("synthetic:test:id#profile-state").unwrap();
    let faces = vec![FaceId::mint("synthetic:test:id#profile-face").unwrap()];
    let boundary = SketchProfileBoundaryUse {
        entity: entity.clone(), parameter_range: cadmpeg_ir::geometry::DirectedParameterRange::new([1.0, 0.0]).unwrap(),
        reversed: true,
    };
    let planar = vec![
        PlanarProfileRef::Unresolved("unresolved profile".into()),
        PlanarProfileRef::Native("native profile".into()),
        PlanarProfileRef::Sketch(sketch.clone()),
        PlanarProfileRef::SketchProfiles { sketch: sketch.clone(), profiles: vec![0, 2].try_into().unwrap() },
        PlanarProfileRef::SketchRegions { sketch: sketch.clone(), regions: vec![
            SketchProfileRegion::loops(0, vec![1, 3]).unwrap(),
            SketchProfileRegion::trimmed(vec![boundary.clone()], vec![vec![boundary.clone()], vec![boundary]]).unwrap(),
        ].try_into().unwrap() },
        PlanarProfileRef::SketchEntities { sketch: sketch.clone(), entities: vec![entity].try_into().unwrap() },
        PlanarProfileRef::SketchSelection { sketch, selections: vec!["native first".into(), "native second".into()].try_into().unwrap() },
        PlanarProfileRef::HistoricalFaces { state: state.clone(), faces: vec![HistoricalFaceId::mint("synthetic:test:id#historical-face").unwrap()].try_into().unwrap(),
            native: vec!["historical first".into(), "historical second".into()].try_into().unwrap() },
        PlanarProfileRef::Feature(owner.clone()),
        PlanarProfileRef::Generated { curves: vec![
            GeneratedCurveRef { feature: owner.clone(), local_id: "curve first".to_string().try_into().unwrap() },
            GeneratedCurveRef { feature: owner.clone(), local_id: "curve second".to_string().try_into().unwrap() },
        ].try_into().unwrap(), native: "generated profile".to_string().try_into().unwrap() },
        PlanarProfileRef::Faces(faces.clone()),
    ];
    let terminations = vec![
        LinearTermination::Unresolved {},
        LinearTermination::Blind { length: cadmpeg_ir::scalar::NonZeroLength::new(12.0).unwrap() },
        LinearTermination::ThroughAll {}, LinearTermination::ThroughNext {},
        LinearTermination::ToFirst {}, LinearTermination::ToLast {},
        LinearTermination::ToFace { face: FaceSelection::Resolved { faces, native: "terminating face".into() }, offset: Some(cadmpeg_ir::scalar::Length::new(-2.0).unwrap()) },
        LinearTermination::OffsetFromFace { face: FaceSelection::Native("offset face".into()), offset: cadmpeg_ir::scalar::PositiveLength::new(3.0).unwrap() },
        LinearTermination::ToShape { target: FaceSelection::Native("target shape".into()) },
        LinearTermination::ToVertex { vertex: VertexSelection::Unresolved },
        LinearTermination::ToVertex { vertex: VertexSelection::generated(GeneratedVertexRef {
            feature: owner, local_id: "generated vertex".to_string().try_into().unwrap(),
        }, "vertex native".into()).unwrap() },
        LinearTermination::ToVertex { vertex: VertexSelection::historical(state, HistoricalVertexId::mint("synthetic:test:id#historical-vertex").unwrap(), "historical vertex".into()).unwrap() },
        LinearTermination::ToVertex { vertex: VertexSelection::native("native vertex".into()).unwrap() },
    ];
    (planar, terminations)
}

fn run_hole_profile_termination(policy: &DecodePolicy) -> Result<(), CodecError> {
    use cadmpeg_ir::features::holes::{HoleConstruction, HoleKind, HoleShape};
    use cadmpeg_ir::features::{ConfigurationEvaluation, ConfigurationFeatureState, Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation};
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let (profiles, terminations) = profile_termination_operands();
    let hole = |profile, extent| FeatureDefinition::Operation(FeatureOperation::Hole {
        profile, profile_filter: None, face: None, direction: None, placements: None,
        shape: HoleShape::new(HoleConstruction::form(HoleKind::Simple), None, None).unwrap(),
        extent, bottom: None, taper_angle: None, allow_multi_profile_faces: None,
    });
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut configuration = design_configuration("profile-termination", 0, Some(0), None);
    for index in 0..profiles.len().max(terminations.len()) {
        let id = FeatureId::mint(format!("synthetic:test:id#hole-profile-{index}")).unwrap();
        ir.model.features.push(Feature {
            id: id.clone(), ordinal: u64::try_from(index).unwrap(), name: None, suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::new(), source_tag: None,
            source_text: None, source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(hole(
                Some(profiles[index % profiles.len()].clone()), Some(terminations[index % terminations.len()].clone()),
            )), native_ref: None,
        });
        configuration.feature_states.insert(id, ConfigurationFeatureState {
            evaluation: ConfigurationEvaluation::Active { outputs: cadmpeg_ir::features::DistinctMembers::default() },
            dependencies: cadmpeg_ir::features::DistinctMembers::default(), definition: hole(None, None),
        });
    }
    ir.model.configurations.push(configuration);
    let mut expected = ir.clone();
    for feature in &expected.model.features {
        expected.model.configurations[0].feature_states.get_mut(&feature.id).unwrap()
            .definition = feature.evaluation.definition().clone();
    }
    let losses = project_configuration_sketch_states(
        &ctx, &mut ir, &[], &[], &mut cadmpeg_ir::Annotations::default(),
    )?;
    assert!(losses.is_empty());
    assert_eq!(ir, expected);
    Ok(())
}

#[test]
fn configuration_sketch_projection_refuses_hole_profile_termination_collection_limit() {
    assert_projection_refusal(ResourceDimension::CollectionItems, run_hole_profile_termination);
}

#[test]
fn configuration_sketch_projection_refuses_hole_profile_termination_retained_limit() {
    assert_projection_refusal(ResourceDimension::RetainedBytes, run_hole_profile_termination);
}

#[test]
fn configuration_sketch_projection_refuses_hole_profile_termination_work_limit() {
    assert_projection_refusal(ResourceDimension::WorkUnits, run_hole_profile_termination);
}
