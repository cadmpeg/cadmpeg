// SPDX-License-Identifier: Apache-2.0
//! Configuration-local hole inheritance and placement tests.

use crate::history::configuration::inherit_configuration_hole_semantics;
use crate::history::configuration::inherit_configuration_shared_semantics;
use crate::history::configuration::project_configuration_sketch_states;
use crate::history::tests::design_configuration;
use crate::history::tests::feature_input_lane;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use std::collections::BTreeMap;

#[test]
fn configuration_hole_inherits_shared_construction_and_placement() {
    use cadmpeg_ir::features::{
        holes::{HoleKind, HolePlacement},
        FeatureDefinition, FeatureId, FeatureOperation, LinearTermination,
    };

    let id = FeatureId::mint("test:model:feature#hole").expect("identity grammar");
    let base = cadmpeg_ir::features::Feature {
        id: id.clone(),
        ordinal: 0,
        name: Some("Hole".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Hole {
                profile: None,
                profile_filter: None,
                face: None,
                direction: None,
                placements: Some(vec![HolePlacement::Axis {
                    origin: cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                        1.0, 2.0, 3.0,
                    ))
                    .unwrap(),
                    axis: cadmpeg_ir::features::FeatureDirection3::new(
                        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                    )
                    .unwrap(),
                }]),
                shape: cadmpeg_ir::features::holes::HoleShape::new(
                    cadmpeg_ir::features::holes::HoleConstruction::form(HoleKind::Counterbore {
                        diameter: cadmpeg_ir::scalar::PositiveLength::new(8.0).unwrap(),
                        depth: cadmpeg_ir::scalar::PositiveLength::new(4.0).unwrap(),
                    }),
                    None,
                    Some(cadmpeg_ir::scalar::PositiveLength::new(5.0).unwrap()),
                )
                .unwrap(),

                extent: Some(LinearTermination::Blind {
                    length: cadmpeg_ir::scalar::NonZeroLength::new(12.0).unwrap(),
                }),
                bottom: None,
                taper_angle: None,
                allow_multi_profile_faces: None,
            }),
        ),
        native_ref: None,
    };
    let mut configured = base.clone();
    configured
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Hole {
            profile: None,
            profile_filter: None,
            face: None,
            direction: None,
            placements: None,
            shape: cadmpeg_ir::features::holes::HoleShape::new(
                cadmpeg_ir::features::holes::HoleConstruction::form(HoleKind::Simple),
                None,
                None,
            )
            .unwrap(),

            extent: None,
            bottom: None,
            taper_angle: None,
            allow_multi_profile_faces: None,
        }));

    configured.evaluation.edit(|definition, _| {
        inherit_configuration_shared_semantics(&cadmpeg_test_support::service_decode_context(), definition, base.evaluation.definition()).unwrap();
    });

    assert_eq!(
        configured.evaluation.definition(),
        base.evaluation.definition()
    );
}

#[test]
fn configuration_lane_inherits_hole_construction_without_replacing_positions() {
    use cadmpeg_ir::features::{
        holes::{HoleKind, HolePlacement},
        FeatureDefinition, FeatureOperation, LinearTermination,
    };

    let placement = HolePlacement::Axis {
        origin: cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
            9.0, 8.0, 7.0,
        ))
        .unwrap(),
        axis: cadmpeg_ir::features::FeatureDirection3::new(cadmpeg_ir::math::Vector3::new(
            0.0, 1.0, 0.0,
        ))
        .unwrap(),
    };
    let base = FeatureDefinition::Operation(FeatureOperation::Hole {
        profile: None,
        profile_filter: None,
        face: None,
        direction: None,
        placements: Some(vec![HolePlacement::Axis {
            origin: cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                1.0, 2.0, 3.0,
            ))
            .unwrap(),
            axis: cadmpeg_ir::features::FeatureDirection3::new(cadmpeg_ir::math::Vector3::new(
                1.0, 0.0, 0.0,
            ))
            .unwrap(),
        }]),
        shape: cadmpeg_ir::features::holes::HoleShape::new(
            cadmpeg_ir::features::holes::HoleConstruction::form(HoleKind::Counterbore {
                diameter: cadmpeg_ir::scalar::PositiveLength::new(8.0).unwrap(),
                depth: cadmpeg_ir::scalar::PositiveLength::new(4.0).unwrap(),
            }),
            None,
            Some(cadmpeg_ir::scalar::PositiveLength::new(5.0).unwrap()),
        )
        .unwrap(),

        extent: Some(LinearTermination::Blind {
            length: cadmpeg_ir::scalar::NonZeroLength::new(12.0).unwrap(),
        }),
        bottom: None,
        taper_angle: None,
        allow_multi_profile_faces: None,
    });
    let mut local = FeatureDefinition::Operation(FeatureOperation::Hole {
        profile: None,
        profile_filter: None,
        face: None,
        direction: None,
        placements: Some(vec![placement.clone()]),
        shape: cadmpeg_ir::features::holes::HoleShape::new(
            cadmpeg_ir::features::holes::HoleConstruction::form(HoleKind::Simple),
            None,
            None,
        )
        .unwrap(),

        extent: None,
        bottom: None,
        taper_angle: None,
        allow_multi_profile_faces: None,
    });

    inherit_configuration_hole_semantics(&cadmpeg_test_support::service_decode_context(), &mut local, &base, false).unwrap();

    let FeatureDefinition::Operation(FeatureOperation::Hole {
        placements,
        shape,

        extent,
        ..
    }) = local
    else {
        panic!("hole definition changed variant");
    };
    let construction = shape.construction();
    let diameter = shape.diameter();
    assert_eq!(placements, Some(vec![placement]));
    assert!(matches!(
        construction,
        cadmpeg_ir::features::holes::HoleConstruction::Form {
            kind: HoleKind::Counterbore {
                diameter: actual_diameter,
                depth: actual_depth,
            },
            ..
        } if actual_diameter.get() == 8.0 && actual_depth.get() == 4.0
    ));
    assert_eq!(
        diameter,
        Some(cadmpeg_ir::scalar::PositiveLength::new(5.0).unwrap())
    );
    assert_eq!(
        extent,
        Some(LinearTermination::Blind {
            length: cadmpeg_ir::scalar::NonZeroLength::new(12.0).unwrap(),
        })
    );
}

#[test]
fn configuration_lane_does_not_inherit_shared_hole_semantics() {
    use cadmpeg_ir::features::{
        holes::HoleKind, ConfigurationFeatureState, Feature as NeutralFeature, FeatureDefinition,
        FeatureId, FeatureOperation, LinearTermination,
    };

    let id = FeatureId::mint("test:model:feature#hole-lane").expect("identity grammar");
    let base_definition = FeatureDefinition::Operation(FeatureOperation::Hole {
        profile: None,
        profile_filter: None,
        face: None,
        direction: None,
        placements: Some(vec![cadmpeg_ir::features::holes::HolePlacement::Axis {
            origin: cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                1.0, 2.0, 3.0,
            ))
            .unwrap(),
            axis: cadmpeg_ir::features::FeatureDirection3::new(cadmpeg_ir::math::Vector3::new(
                0.0, 0.0, 1.0,
            ))
            .unwrap(),
        }]),
        shape: cadmpeg_ir::features::holes::HoleShape::new(
            cadmpeg_ir::features::holes::HoleConstruction::form(HoleKind::Counterbore {
                diameter: cadmpeg_ir::scalar::PositiveLength::new(8.0).unwrap(),
                depth: cadmpeg_ir::scalar::PositiveLength::new(4.0).unwrap(),
            }),
            None,
            Some(cadmpeg_ir::scalar::PositiveLength::new(5.0).unwrap()),
        )
        .unwrap(),

        extent: Some(LinearTermination::Blind {
            length: cadmpeg_ir::scalar::NonZeroLength::new(12.0).unwrap(),
        }),
        bottom: None,
        taper_angle: None,
        allow_multi_profile_faces: None,
    });
    let local_definition = FeatureDefinition::Operation(FeatureOperation::Hole {
        profile: None,
        profile_filter: None,
        face: None,
        direction: None,
        placements: None,
        shape: cadmpeg_ir::features::holes::HoleShape::new(
            cadmpeg_ir::features::holes::HoleConstruction::form(HoleKind::Simple),
            None,
            None,
        )
        .unwrap(),

        extent: None,
        bottom: None,
        taper_angle: None,
        allow_multi_profile_faces: None,
    });
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features.push(NeutralFeature {
        id: id.clone(),
        ordinal: 0,
        name: Some("Hole".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::new(
            base_definition,
            (Vec::new()).try_into().unwrap(),
        ),
        native_ref: None,
    });
    let mut configuration = design_configuration("configuration", 0, Some(0), None);
    configuration.active = true;
    configuration.feature_states.insert(
        id.clone(),
        ConfigurationFeatureState {
            evaluation: cadmpeg_ir::features::ConfigurationEvaluation::Active {
                outputs: cadmpeg_ir::features::DistinctMembers::default(),
            },
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            definition: local_definition,
        },
    );
    ir.model.configurations.push(configuration);

    let mut annotations = cadmpeg_ir::Annotations::default();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    project_configuration_sketch_states(
        &ctx,
        &mut ir,
        &[],
        &[feature_input_lane("lane", Some("0"))],
        &mut annotations,
    )
    .unwrap();

    assert!(matches!(
    &ir.model.configurations[0].feature_states[&id].definition, FeatureDefinition::Operation(FeatureOperation::Hole {
        placements,
        shape,

        extent: None,
        ..
    }) if matches!((shape.construction(), &shape.diameter(),), (construction, None,) if placements.is_none()
        && matches!(construction, cadmpeg_ir::features::holes::HoleConstruction::Form {
            kind: HoleKind::Simple,
            ..
        }))));
}

