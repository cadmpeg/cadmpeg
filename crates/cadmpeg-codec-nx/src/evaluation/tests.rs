mod body_operations;
mod history;
mod patterns;

use std::collections::BTreeMap;

use cadmpeg_ir::features::{
    BodyRetentionMode, BodyTrimSide, ChamferGroup, ChamferSpec, ConfigurationBodies,
    ConfigurationFeatureState, ConfigurationId, CurveProjectionDirection,
    CurveProjectionDirectionState, DesignConfiguration, EdgeSelection, ExtrudeDirection,
    ExtrudeExtent, ExtrudeSide, ExtrudeStart, FaceSelection, Feature, FilletGroup, HoleKind,
    HolePlacement, Length, LinearTermination, PathRef, PatternKind, ProfileRef, RadiusSpec,
    RevolveConstruction, RibConstruction, RibDraft, SurfaceExtension, SweepMode, SweepSection,
    ThickenSide, TrimRegion,
};
use cadmpeg_ir::ids::{CurveId, FaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Body, BodyKind};

use super::*;

fn model_body(id: &str) -> Body {
    Body {
        id: BodyId::mint(id.to_string()).expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    }
}

fn complete_block_ir() -> CadIr {
    let mut ir = CadIr::empty();
    let body = BodyId::mint("test:model:entity#body".to_string()).expect("identity grammar");
    ir.model.bodies.push(model_body(body.as_str()));
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:id#block".to_string()).expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            FeatureDefinition::Block {
                dimensions: Some([
                    cadmpeg_ir::features::PositiveLength::new(1.0).unwrap(),
                    cadmpeg_ir::features::PositiveLength::new(2.0).unwrap(),
                    cadmpeg_ir::features::PositiveLength::new(3.0).unwrap(),
                ]),
                placement: Some(cadmpeg_ir::features::FeatureRigidPlacement::identity()),
                op: BooleanOp::NewBody,
            },
            vec![body],
        )
        .unwrap(),
        native_ref: None,
    });
    ir
}

fn attach_complete_active_configuration(ir: &mut CadIr) {
    let feature_states = ir
        .model
        .features
        .iter()
        .map(|feature| {
            (
                feature.id.clone(),
                ConfigurationFeatureState {
                    evaluation: cadmpeg_ir::features::ConfigurationEvaluation::Active {
                        outputs: (feature.evaluation.outputs().clone()).try_into().unwrap(),
                    },
                    dependencies: feature.dependencies.clone(),
                    definition: feature.evaluation.definition().clone(),
                },
            )
        })
        .collect();
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("synthetic:test:id#active".to_string())
            .expect("identity grammar"),
        ordinal: 0,
        active: true,
        source_index: Some(0),
        name: "Model".into(),
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        bodies: ConfigurationBodies::Resolved(
            (ir.model
                .bodies
                .iter()
                .map(|body| body.id.clone())
                .collect::<Vec<_>>())
            .try_into()
            .unwrap(),
        ),
        parameter_values: BTreeMap::new(),
        feature_states,
        native_ref: None,
    });
}

fn complete_hole(body: BodyId) -> Feature {
    Feature {
        id: FeatureId::mint("synthetic:test:id#hole".to_string()).expect("identity grammar"),
        ordinal: 1,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            FeatureDefinition::Hole {
                profile: None,
                profile_filter: None,
                face: None,
                direction: None,
                placements: Some(vec![HolePlacement::Directed {
                    position: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                        .unwrap(),
                    direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::new(
                        0.0, 0.0, 1.0,
                    ))
                    .unwrap(),
                }]),
                shape: cadmpeg_ir::features::HoleShape::new(
                    cadmpeg_ir::features::HoleConstruction::form(HoleKind::Simple),
                    None,
                    Some(cadmpeg_ir::features::PositiveLength::new(0.5).unwrap()),
                )
                .unwrap(),

                extent: Some(LinearTermination::ThroughAll),
                bottom: None,
                taper_angle: None,
                allow_multi_profile_faces: None,
            },
            vec![body],
        )
        .unwrap(),
        native_ref: None,
    }
}

fn body_preserving_feature(
    id: &str,
    ordinal: u64,
    body: BodyId,
    definition: FeatureDefinition,
) -> Feature {
    Feature {
        id: FeatureId::mint(format!("synthetic:test:id#{id}")).expect("identity grammar"),
        ordinal,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(definition, vec![body]).unwrap(),
        native_ref: None,
    }
}

fn body_neutral_feature(id: &str, ordinal: u64, definition: FeatureDefinition) -> Feature {
    Feature {
        id: FeatureId::mint(format!("synthetic:test:id#{id}")).expect("identity grammar"),
        ordinal,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: None,
    }
}

fn complete_extrude_feature(
    id: &str,
    ordinal: u64,
    profile: FeatureId,
    outputs: Vec<BodyId>,
    op: BooleanOp,
) -> Feature {
    let mut feature = body_neutral_feature(
        id,
        ordinal,
        FeatureDefinition::Extrude {
            profile: ProfileRef::Feature(profile.clone()),
            direction: ExtrudeDirection::ProfileNormal,
            start: ExtrudeStart::ProfilePlane,
            extent: ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::features::NonZeroLength::new(1.0).unwrap(),
                    },
                    draft: None,
                },
            },
            op,
            solid: Some(true),
            face_maker: None,
            inner_wire_taper: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        },
    );
    feature.dependencies.insert(profile);
    feature.evaluation.set_outputs(outputs).unwrap();
    feature
}
