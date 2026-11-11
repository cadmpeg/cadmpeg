// SPDX-License-Identifier: Apache-2.0

use super::super::{bind_loft_and_revolve_sketch_selections, SketchProfileResolution};
use super::group;
use crate::ids::neutral_sketch_id;
use crate::records::sketch_placement::DesignSketchPlacement;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::features::{
    BooleanOp, Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    LoftGuidance, LoftSection, PlanarProfileRef, ProfileRef,
};
use cadmpeg_ir::sketches::{Sketch, SketchPlacement};

const STREAM: &str = "f3d:FusionAssetName[Active]/Design1/BulkStream.dat";

fn loft_binding(
    flags: [u8; 2],
    tail_length: usize,
) -> (
    Vec<cadmpeg_ir::report::loss::LossNote>,
    crate::records::topology::construction::DesignConstructionOperandGroup,
    Feature,
    cadmpeg_ir::sketches::SketchId,
) {
    loft_binding_with_regions(flags, tail_length, None, DesignOperandRole::PROFILE)
}

fn loft_binding_with_regions(
    flags: [u8; 2],
    tail_length: usize,
    selected: Option<&[(u64, u64, bool)]>,
    role: DesignOperandRole,
) -> (
    Vec<cadmpeg_ir::report::loss::LossNote>,
    crate::records::topology::construction::DesignConstructionOperandGroup,
    Feature,
    cadmpeg_ir::sketches::SketchId,
) {
    let (archive, _, headers, entities) =
        crate::test_support::multiple_profiles::fixture_with_curve_regions(
            1,
            Some((flags, tail_length)),
            selected,
        );
    let mut profile_group = group();
    profile_group.id = format!("{STREAM}:construction-group#9");
    profile_group.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(role);
    profile_group
        .try_set_members(vec![crate::records::identity::Located {
            value: 100,
            offset: 0,
        }])
        .unwrap();
    let placement = DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
        )
        .unwrap(),
        id: format!("{STREAM}:placement#20"),
        scope_record_index: Some(7),
        entity_id: crate::records::identity::DesignEntityId::try_from("0_172".to_owned()).unwrap(),
        visibility: None,
        class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned()).unwrap(),
        record_index: 20,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned())
            .unwrap(),
    };
    let sketch_id = neutral_sketch_id(&placement);
    let sketch_entities: Vec<_> = [(11, 115, 9.0), (12, 116, 2.0)]
        .into_iter()
        .map(|(id, secondary, radius)| {
            cadmpeg_ir::sketches::SketchEntity::new(
                crate::ids::neutral_sketch_curve_id(&sketch_id, id, secondary),
                sketch_id.clone(),
                cadmpeg_ir::sketches::SketchGeometry::try_from(
                    cadmpeg_ir::sketches::SketchGeometryDefinition::Circle {
                        center: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                        radius: cadmpeg_ir::scalar::Length::new(radius).unwrap(),
                    },
                )
                .unwrap(),
            )
        })
        .collect();
    let mut curves = [super::curve(31, 11, 115), super::curve(32, 12, 116)];
    for curve in &mut curves {
        curve.id = format!("{STREAM}:curve-{}", curve.primary_id);
        curve.owner_reference = Some(172);
    }
    let sketches = [Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(
            sketch_entities
                .iter()
                .map(|entity| {
                    vec![cadmpeg_ir::sketches::SketchEntityUse {
                        entity: entity.id().clone(),
                        reversed: false,
                    }]
                })
                .collect::<Vec<_>>(),
        )
        .unwrap(),
        native_ref: None,
    }];
    let placements = [placement];
    let profile_curves = crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::design::decode::operands::decode_profile_curve_selections(
            &cadmpeg_test_support::service_decode_context(),
            scan,
            std::slice::from_ref(&profile_group),
            &headers,
        )
        .unwrap()
    });
    assert_eq!(profile_curves.len(), usize::from(selected.is_some()));
    let resolution = SketchProfileResolution {
        profile_curves: &profile_curves,
        entities: &entities,
        entity_selection_operands: &[],
        placements: &placements,
        curve_identities: &curves,
        sketches: &sketches,
        sketch_entities: &sketch_entities,
        spatial_sketches: &[],
        spatial_sketch_entities: &[],
        linear_tolerance: 1.0e-7,
        angular_tolerance: 1.0e-9,
    };
    let mut features = vec![Feature {
        product_definition: None,
        id: FeatureId::mint("test:model:feature#loft").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Loft {
                sections: vec![LoftSection::Profile(ProfileRef::Planar(
                    PlanarProfileRef::Native(profile_group.id.clone()),
                ))],
                guidance: LoftGuidance::default(),
                op: BooleanOp::NewBody,
                closed: false,
                solid: true,
                ruled: false,
                linearize: false,
                max_degree: None,
                allow_multi_profile_faces: None,
            },
        )),
        native_ref: None,
    }];
    let mut revolve = features[0].clone();
    revolve.id = FeatureId::mint("test:model:feature#revolve").unwrap();
    revolve.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Revolve {
            construction: cadmpeg_ir::features::RevolveConstruction::Unresolved(
                cadmpeg_ir::features::PartialRevolveConstruction::Axis {
                    profile: PlanarProfileRef::Native(profile_group.id.clone()),
                    extent: None,
                    solid: None,
                    face_maker: None,
                    fuse_order: None,
                    allow_multi_profile_faces: None,
                },
            ),
            op: BooleanOp::NewBody,
        },
    ));
    features.push(revolve);
    let losses = crate::test_support::zip_test::with_scan(&archive, |scan| {
        bind_loft_and_revolve_sketch_selections(
            &cadmpeg_test_support::service_decode_context(),
            &super::super::ProfileGeometryWork::new(),
            scan,
            std::slice::from_mut(&mut profile_group),
            &headers,
            &resolution,
            &mut features,
        )
        .unwrap()
    });
    let [feature, revolve] = <[Feature; 2]>::try_from(features).unwrap();
    let FeatureDefinition::Operation(FeatureOperation::Revolve { construction, .. }) =
        revolve.evaluation.definition()
    else {
        panic!("revolve definition");
    };
    let LoftSection::Profile(ProfileRef::Planar(profile)) = loft_section(&feature) else {
        panic!("planar loft section");
    };
    assert_eq!(construction.profile(), Some(profile));
    (losses, profile_group, feature, sketch_id)
}

fn loft_section(feature: &Feature) -> &LoftSection {
    let FeatureDefinition::Operation(FeatureOperation::Loft { sections, .. }) =
        feature.evaluation.definition()
    else {
        panic!("loft definition");
    };
    &sections[0]
}

#[test]
fn loft_compact_profile_flags_are_retained_and_report_the_assumption() {
    for tail_length in [89, 93] {
        let (losses, profile_group, feature, sketch_id) = loft_binding([0, 1], tail_length);
        assert_eq!(
            loft_section(&feature),
            &LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Sketch(sketch_id)))
        );
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code.local_code(),
            "feature.sketch-profile-flags-assumed"
        );
        let profile = profile_group.sketch_profile.unwrap();
        assert_eq!(profile.record_index, 100);
        assert_eq!(profile.compact_tail_flags, Some([false, true]));
    }
}

#[test]
fn loft_compact_profile_zero_flags_bind_without_an_assumption_loss() {
    let (losses, profile_group, feature, sketch_id) = loft_binding([0, 0], 93);
    assert_eq!(
        loft_section(&feature),
        &LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Sketch(sketch_id)))
    );
    assert!(losses.is_empty());
    assert_eq!(
        profile_group.sketch_profile.unwrap().compact_tail_flags,
        Some([false, false])
    );
}

#[test]
fn a_role_0x5_loft_section_of_planar_sketch_curves_binds_as_sketch_entities() {
    use crate::ids::neutral_sketch_curve_id;
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::sketches::{SketchEntity, SketchGeometry, SketchGeometryDefinition};

    let placement = super::placement();
    let sketch = neutral_sketch_id(&placement);
    let curves = [super::curve(30, 100, 101), super::curve(31, 200, 201)];
    let line = |start: (f64, f64), end: (f64, f64)| {
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(start.0, start.1),
            end: Point2::new(end.0, end.1),
        })
        .unwrap()
    };
    let sketch_entities = [
        SketchEntity::new(
            neutral_sketch_curve_id(&sketch, 100, 101),
            sketch.clone(),
            line((0.0, 0.0), (1.0, 0.0)),
        ),
        SketchEntity::new(
            neutral_sketch_curve_id(&sketch, 200, 201),
            sketch.clone(),
            line((1.0, 0.0), (1.0, 1.0)),
        ),
    ];
    let sketches = [Sketch {
        id: sketch.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    }];
    let mut section_group = group();
    section_group.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::ROLE_0X5,
        );
    let operands = [super::operand(10, 0, 100), super::operand(11, 1, 200)];
    let placements = [placement];
    let resolution =
        super::planar_resolution(&operands, &placements, &curves, &sketches, &sketch_entities);
    let mut features = vec![Feature {
        product_definition: None,
        id: FeatureId::mint("test:model:feature#loft").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Loft {
                sections: vec![LoftSection::Profile(ProfileRef::Planar(
                    PlanarProfileRef::Native(section_group.id.clone()),
                ))],
                guidance: LoftGuidance::default(),
                op: BooleanOp::NewBody,
                closed: false,
                solid: true,
                ruled: false,
                linearize: false,
                max_degree: None,
                allow_multi_profile_faces: None,
            },
        )),
        native_ref: None,
    }];
    let (archive, _, headers, _) =
        crate::test_support::multiple_profiles::fixture_with_flags(1, None);
    let losses = crate::test_support::zip_test::with_scan(&archive, |scan| {
        bind_loft_and_revolve_sketch_selections(
            &cadmpeg_test_support::service_decode_context(),
            &super::super::ProfileGeometryWork::new(),
            scan,
            std::slice::from_mut(&mut section_group),
            &headers,
            &resolution,
            &mut features,
        )
        .unwrap()
    });
    assert!(losses.is_empty(), "{losses:?}");
    assert_eq!(
        loft_section(&features[0]),
        &LoftSection::Profile(ProfileRef::Planar(
            PlanarProfileRef::sketch_entities(
                sketch.clone(),
                vec![
                    neutral_sketch_curve_id(&sketch, 100, 101),
                    neutral_sketch_curve_id(&sketch, 200, 201),
                ],
            )
            .unwrap()
        ))
    );
}

#[test]
fn loft_and_revolve_non_outer_region_loops_stay_native_with_loss() {
    let (losses, group, feature, _) = loft_binding_with_regions(
        [0, 0],
        93,
        Some(&[(11, 115, true), (12, 0, false)]),
        DesignOperandRole::ROLE_0X43,
    );
    assert_eq!(
        loft_section(&feature),
        &LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Native(group.id)))
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code.local_code(),
        "feature.profile-selection-native"
    );
}

#[test]
fn loft_planar_region_selection_preserves_inner_disc() {
    let (losses, _, feature, sketch) = loft_binding_with_regions(
        [0, 0],
        93,
        Some(&[(12, 116, true)]),
        DesignOperandRole::PROFILE,
    );
    assert!(losses.is_empty());
    assert_eq!(
        loft_section(&feature),
        &LoftSection::Profile(ProfileRef::Planar(
            PlanarProfileRef::sketch_regions(
                sketch,
                vec![cadmpeg_ir::features::SketchProfileRegion::loops(1, vec![]).unwrap(),]
            )
            .unwrap(),
        ))
    );
}

#[test]
fn loft_unresolved_planar_region_selection_stays_native_with_loss() {
    let (losses, group, feature, _) = loft_binding_with_regions(
        [0, 0],
        93,
        Some(&[(99, 117, true)]),
        DesignOperandRole::ROLE_0X43,
    );
    assert_eq!(
        loft_section(&feature),
        &LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Native(group.id),))
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code.local_code(),
        "feature.profile-selection-native"
    );
}

#[test]
fn loft_and_revolve_single_outer_loop_selects_the_outer_profile() {
    let (losses, _, feature, sketch) = loft_binding_with_regions(
        [0, 0],
        93,
        Some(&[(11, 115, true)]),
        DesignOperandRole::PROFILE,
    );
    assert!(losses.is_empty());
    assert_eq!(
        loft_section(&feature),
        &LoftSection::Profile(ProfileRef::Planar(
            PlanarProfileRef::sketch_regions(
                sketch,
                vec![cadmpeg_ir::features::SketchProfileRegion::loops(0, vec![]).unwrap()],
            )
            .unwrap(),
        )),
    );
}
