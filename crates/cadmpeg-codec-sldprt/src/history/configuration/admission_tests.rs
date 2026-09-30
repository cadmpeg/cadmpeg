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
