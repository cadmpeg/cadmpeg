//! Axial hole-profile role tests.

use super::{lane_with_position_reference, model_hole, native_history, profile_line};
use std::collections::{BTreeMap, HashMap};

use cadmpeg_ir::features::{
    FeatureDefinition, FeatureId, FeatureOperation, HoleBottom, HoleKind, LinearTermination,
};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};

use super::super::*;
use crate::records::FeatureSource;

#[test]
fn axial_profile_resolves_counterbore_roles() {
    let mut profile = native_history().features.remove(0);
    profile.parameters = [
        ("a".into(), "118°".into()),
        ("b".into(), "5.7".into()),
        ("c".into(), "<MOD-DIAM>10".into()),
        ("d".into(), "15".into()),
        ("e".into(), "<MOD-DIAM>5.5".into()),
    ]
    .into_iter()
    .collect();
    profile.content = ["a", "b", "c", "d", "e"]
        .into_iter()
        .map(|name| crate::records::FeatureContent::Dimension(name.into()))
        .collect();
    profile.parameters.insert("display".into(), "101.6".into());
    let sketch = SketchId::mint("synthetic:test:id#profile").unwrap();
    let drill_length = 2.75 / (118_f64.to_radians() / 2.0).tan();
    let entities = [
        profile_line(&sketch, 0, Point2::new(0.0, 5.0), Point2::new(-5.7, 5.0)),
        profile_line(&sketch, 1, Point2::new(-5.7, 5.0), Point2::new(-5.7, 2.75)),
        profile_line(
            &sketch,
            2,
            Point2::new(-5.7, 2.75),
            Point2::new(-15.0, 2.75),
        ),
        profile_line(
            &sketch,
            3,
            Point2::new(-15.0, 2.75),
            Point2::new(-15.0 - drill_length, 0.0),
        ),
    ];

    let construction =
        profiled_hole_construction(&profile, &sketch, &entities).expect("exact profile");
    assert_eq!(
        construction.diameter,
        cadmpeg_ir::scalar::PositiveLength::new(5.5).unwrap()
    );
    assert_eq!(
        construction.extent,
        LinearTermination::Blind {
            length: cadmpeg_ir::scalar::NonZeroLength::new(15.0).unwrap()
        }
    );
    assert!(matches!(
        construction.kind,
        HoleKind::CounterboreDrilled {
            diameter: actual_diameter,
            depth: actual_depth,
            drill_point_angle: angle,
        } if ((angle.get() - 118_f64.to_radians()).abs() < 1.0e-12) && actual_diameter.get() == 10.0 && actual_depth.get() == 5.7
    ));
    assert_eq!(construction.bottom, None);

    let mut translated_entities = entities.clone();
    for entity in &mut translated_entities {
        entity
            .geometry
            .edit(|definition| {
                let SketchGeometryDefinition::Line { start, end } = definition else {
                    unreachable!();
                };
                start.u += 42.0;
                start.v -= 17.0;
                end.u += 42.0;
                end.v -= 17.0;
            })
            .unwrap();
    }
    let translated = profiled_hole_construction(&profile, &sketch, &translated_entities)
        .expect("translated exact profile");
    assert_eq!(translated.diameter, construction.diameter);
    assert_eq!(translated.extent, construction.extent);
    assert_eq!(translated.kind, construction.kind);
    assert_eq!(translated.bottom, construction.bottom);
    assert_eq!(translated.taper_angle, construction.taper_angle);

    let mut independently_translated_entities = entities.clone();
    for (ordinal, entity) in independently_translated_entities.iter_mut().enumerate() {
        entity
            .geometry
            .edit(|definition| {
                let SketchGeometryDefinition::Line { start, end } = definition else {
                    unreachable!();
                };
                let offset = (ordinal + 1) as f64 * 100.0;
                start.u += offset;
                start.v -= offset;
                end.u += offset;
                end.v -= offset;
            })
            .unwrap();
    }
    assert!(
        profiled_hole_construction(&profile, &sketch, &independently_translated_entities).is_none()
    );

    profile.parameters.insert("a".into(), "180°".into());
    let construction =
        profiled_hole_construction(&profile, &sketch, &entities[..3]).expect("flat-bottom profile");
    assert_eq!(
        construction.extent,
        LinearTermination::Blind {
            length: cadmpeg_ir::scalar::NonZeroLength::new(15.0).unwrap()
        }
    );
    assert_eq!(
        construction.kind,
        HoleKind::Counterbore {
            diameter: cadmpeg_ir::scalar::PositiveLength::new(10.0).unwrap(),
            depth: cadmpeg_ir::scalar::PositiveLength::new(5.7).unwrap(),
        }
    );
    assert_eq!(construction.bottom, Some(HoleBottom::Flat));
}

#[test]
fn axial_profile_resolves_counterdrill_roles() {
    let mut profile = native_history().features.remove(0);
    profile.parameters = [
        ("a".into(), "<MOD-DIAM>2.9".into()),
        ("b".into(), "15".into()),
        ("c".into(), "<MOD-DIAM>5.5".into()),
        ("d".into(), "2.9".into()),
        ("e".into(), "<MOD-DIAM>5.55".into()),
        ("f".into(), "90°".into()),
    ]
    .into_iter()
    .collect();
    let sketch = SketchId::mint("synthetic:test:id#profile").unwrap();
    let profile_point = |ordinal: usize, position| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#profile-point-{ordinal}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point { position }).unwrap(),
        )
    };
    let entities = [
        profile_point(0, Point2::new(0.0, 2.775)),
        profile_point(1, Point2::new(-0.025, 2.75)),
        profile_line(
            &sketch,
            2,
            Point2::new(-0.025, 2.75),
            Point2::new(-2.9, 2.75),
        ),
        profile_point(3, Point2::new(-2.9, 2.75)),
        profile_point(4, Point2::new(-2.9, 1.45)),
        profile_line(
            &sketch,
            5,
            Point2::new(-2.9, 1.45),
            Point2::new(-15.0, 1.45),
        ),
    ];

    let construction =
        profiled_hole_construction(&profile, &sketch, &entities).expect("exact profile");
    assert_eq!(
        construction.diameter,
        cadmpeg_ir::scalar::PositiveLength::new(2.9).unwrap()
    );
    assert_eq!(construction.extent, LinearTermination::ThroughAll {});
    assert_eq!(
        construction.kind,
        HoleKind::Counterdrill {
            diameters: cadmpeg_ir::features::CounterdrillDiameters::new(
                cadmpeg_ir::scalar::PositiveLength::new(5.5).unwrap(),
                Some(cadmpeg_ir::scalar::PositiveLength::new(5.55).unwrap())
            )
            .unwrap(),

            depth: cadmpeg_ir::scalar::PositiveLength::new(2.9).unwrap(),
            angle: cadmpeg_ir::scalar::InteriorAngle::new(std::f64::consts::FRAC_PI_2).unwrap(),
        }
    );

    let mut translated = entities.clone();
    for entity in &mut translated {
        entity
            .geometry
            .edit(|definition| match definition {
                SketchGeometryDefinition::Point { position } => {
                    position.u -= 11.0;
                    position.v += 7.0;
                }
                SketchGeometryDefinition::Line { start, end } => {
                    start.u -= 11.0;
                    start.v += 7.0;
                    end.u -= 11.0;
                    end.v += 7.0;
                }
                _ => unreachable!(),
            })
            .unwrap();
    }
    assert_eq!(
        profiled_hole_construction(&profile, &sketch, &translated)
            .expect("translated exact profile")
            .kind,
        construction.kind
    );

    assert!(profiled_hole_construction(&profile, &sketch, &entities[..5]).is_none());
}

#[test]
fn single_diameter_axial_profile_resolves_flat_and_drilled_holes() {
    let mut profile = native_history().features.remove(0);
    profile.parameters = [
        ("diameter".into(), "<MOD-DIAM>14.5".into()),
        ("depth".into(), "15".into()),
    ]
    .into_iter()
    .collect();
    let sketch = SketchId::mint("synthetic:test:id#profile").unwrap();

    let flat = profiled_hole_construction(&profile, &sketch, &[]).expect("exact flat profile");
    assert_eq!(
        flat.diameter,
        cadmpeg_ir::scalar::PositiveLength::new(14.5).unwrap()
    );
    assert_eq!(
        flat.extent,
        LinearTermination::Blind {
            length: cadmpeg_ir::scalar::NonZeroLength::new(15.0).unwrap()
        }
    );
    assert_eq!(flat.kind, HoleKind::Simple);
    assert_eq!(flat.bottom, Some(HoleBottom::Flat));
    assert_eq!(flat.taper_angle, None);
    assert!(profiled_hole_construction_with_evidence(
        &profile,
        &sketch,
        &[],
        ProfileEvidence::AxialTopology,
    )
    .is_none());
    let radius = 14.5 / 2.0;
    let entities = [
        profile_line(&sketch, 0, Point2::new(0.0, 0.0), Point2::new(0.0, radius)),
        profile_line(
            &sketch,
            1,
            Point2::new(0.0, radius),
            Point2::new(-15.0, radius),
        ),
        profile_line(
            &sketch,
            2,
            Point2::new(-15.0, radius),
            Point2::new(-15.0, 0.0),
        ),
        profile_line(&sketch, 3, Point2::new(-15.0, 0.0), Point2::new(0.0, 0.0)),
    ];
    let topology_proven = profiled_hole_construction_with_evidence(
        &profile,
        &sketch,
        &entities,
        ProfileEvidence::AxialTopology,
    )
    .expect("axial rectangle");
    assert_eq!(topology_proven.diameter, flat.diameter);
    assert_eq!(topology_proven.extent, flat.extent);

    profile.parameters.insert("point".into(), "118°".into());
    let drilled =
        profiled_hole_construction(&profile, &sketch, &[]).expect("exact drilled profile");
    assert!(matches!(
        drilled.kind,
        HoleKind::SimpleDrilled {
            drill_point_angle: angle,
        } if (angle.get() - 118_f64.to_radians()).abs() < 1.0e-12
    ));
    assert_eq!(
        drilled.bottom,
        Some(HoleBottom::Angled {
            included_angle: cadmpeg_ir::scalar::InteriorAngle::new(118_f64.to_radians()).unwrap(),
            depth_to_tip: false,
        })
    );
}

#[test]
fn closed_tapered_axial_profile_resolves_conical_hole() {
    let mut profile = native_history().features.remove(0);
    profile.parameters = [
        ("entry".into(), "<MOD-DIAM>12.2".into()),
        ("terminal".into(), "<MOD-DIAM>13.66623".into()),
        ("depth".into(), "42".into()),
    ]
    .into_iter()
    .collect();
    let sketch = SketchId::mint("synthetic:test:id#profile").unwrap();
    let entry_radius = 6.1;
    let terminal_radius = 6.833_115;
    let terminal_geometry_radius = 6.833_112_73;
    let entities = [
        profile_line(
            &sketch,
            0,
            Point2::new(0.0, 0.0),
            Point2::new(0.0, entry_radius),
        ),
        profile_line(
            &sketch,
            1,
            Point2::new(0.0, entry_radius),
            Point2::new(-42.0, terminal_geometry_radius),
        ),
        profile_line(
            &sketch,
            2,
            Point2::new(-42.0, terminal_geometry_radius),
            Point2::new(-42.0, 0.0),
        ),
        profile_line(&sketch, 3, Point2::new(-42.0, 0.0), Point2::new(0.0, 0.0)),
    ];

    let construction =
        profiled_hole_construction(&profile, &sketch, &entities).expect("exact taper");
    assert_eq!(
        construction.diameter,
        cadmpeg_ir::scalar::PositiveLength::new(12.2).unwrap()
    );
    assert_eq!(
        construction.extent,
        LinearTermination::Blind {
            length: cadmpeg_ir::scalar::NonZeroLength::new(42.0).unwrap()
        }
    );
    assert_eq!(construction.kind, HoleKind::Simple);
    assert_eq!(construction.bottom, Some(HoleBottom::Flat));
    let included_angle = construction
        .taper_angle
        .expect("included taper angle")
        .get();
    assert!(
        (included_angle - 2.0 * ((terminal_radius - entry_radius) / 42.0_f64).atan()).abs()
            < 1.0e-12
    );
}

#[test]
fn tapered_profile_reconstructs_missing_edges_from_endpoint_points() {
    let mut profile = native_history().features.remove(0);
    profile.parameters = [
        ("entry".into(), "<MOD-DIAM>12.2".into()),
        ("terminal".into(), "<MOD-DIAM>13.66623".into()),
        ("depth".into(), "42".into()),
    ]
    .into_iter()
    .collect();
    let sketch = SketchId::mint("synthetic:test:id#profile").unwrap();
    let point = |ordinal: usize, position| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#profile-point-{ordinal}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point { position }).unwrap(),
        )
    };
    let entities = [
        point(0, Point2::new(0.0, 0.0)),
        point(1, Point2::new(-42.0, 0.0)),
        point(2, Point2::new(-42.0, 6.833_112_73)),
        point(3, Point2::new(0.0, 6.1)),
        profile_line(&sketch, 0, Point2::new(0.0, 0.0), Point2::new(-42.0, 0.0)),
        profile_line(
            &sketch,
            1,
            Point2::new(-42.0, 0.0),
            Point2::new(-42.0, 6.833_112_73),
        ),
    ];

    let construction =
        profiled_hole_construction(&profile, &sketch, &entities).expect("endpoint proof");
    assert_eq!(
        construction.diameter,
        cadmpeg_ir::scalar::PositiveLength::new(12.2).unwrap()
    );
    assert_eq!(
        construction.extent,
        LinearTermination::Blind {
            length: cadmpeg_ir::scalar::NonZeroLength::new(42.0).unwrap()
        }
    );
    assert_eq!(construction.kind, HoleKind::Simple);
    assert_eq!(construction.bottom, Some(HoleBottom::Flat));
    let taper_angle = construction.taper_angle.expect("taper angle").get();
    assert!((taper_angle - 2.0 * ((6.833_115 - 6.1) / 42.0_f64).atan()).abs() < 1.0e-12);
}

#[test]
fn axial_profile_resolves_countersink_and_drill_point_roles() {
    let mut profile = native_history().features.remove(0);
    profile.parameters = [
        ("a".into(), "120°".into()),
        ("b".into(), "5".into()),
        ("c".into(), "<MOD-DIAM>4.134".into()),
        ("d".into(), "<MOD-DIAM>5".into()),
        ("e".into(), "90°".into()),
    ]
    .into_iter()
    .collect();
    let sketch = SketchId::mint("synthetic:test:id#profile").unwrap();
    let point = |ordinal: usize, position| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#profile-point-{ordinal}")).unwrap(),
            sketch.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point { position }).unwrap(),
        )
    };
    let entities = [
        point(0, Point2::new(0.0, 2.5)),
        point(1, Point2::new(-0.433, 2.067)),
        profile_line(
            &sketch,
            2,
            Point2::new(-0.433, 2.067),
            Point2::new(-5.0, 2.067),
        ),
        profile_line(
            &sketch,
            3,
            Point2::new(-5.0, 2.067),
            Point2::new(-6.193_383_012, 0.0),
        ),
    ];

    let construction =
        profiled_hole_construction(&profile, &sketch, &entities).expect("exact profile");
    assert_eq!(
        construction.diameter,
        cadmpeg_ir::scalar::PositiveLength::new(4.134).unwrap()
    );
    assert_eq!(
        construction.extent,
        LinearTermination::Blind {
            length: cadmpeg_ir::scalar::NonZeroLength::new(5.0).unwrap()
        }
    );
    assert!(matches!(
        construction.kind,
        HoleKind::Countersink {
            diameter: actual_diameter,
            angle,
        } if ((angle.get() - 90_f64.to_radians()).abs() < 1.0e-12) && actual_diameter.get() == 5.0
    ));
    assert_eq!(
        construction.bottom,
        Some(HoleBottom::Angled {
            included_angle: cadmpeg_ir::scalar::InteriorAngle::new(120_f64.to_radians()).unwrap(),
            depth_to_tip: false,
        })
    );

    let mut translated_entities = entities.clone();
    for entity in &mut translated_entities {
        entity
            .geometry
            .edit(|definition| match definition {
                SketchGeometryDefinition::Point { position } => {
                    position.u += 21.0;
                    position.v -= 33.0;
                }
                SketchGeometryDefinition::Line { start, end } => {
                    start.u += 21.0;
                    start.v -= 33.0;
                    end.u += 21.0;
                    end.v -= 33.0;
                }
                _ => unreachable!(),
            })
            .unwrap();
    }
    let translated = profiled_hole_construction(&profile, &sketch, &translated_entities)
        .expect("translated exact profile");
    assert_eq!(translated.diameter, construction.diameter);
    assert_eq!(translated.extent, construction.extent);
    assert_eq!(translated.kind, construction.kind);
    assert_eq!(translated.bottom, construction.bottom);

    let insufficient = [
        point(0, Point2::new(0.0, 2.5)),
        point(1, Point2::new(-0.433, 2.067)),
        point(2, Point2::new(-5.0, 2.067)),
        profile_line(
            &sketch,
            3,
            Point2::new(-5.0, 2.067),
            Point2::new(-6.193_383_012, 0.0),
        ),
    ];
    assert!(profiled_hole_construction(&profile, &sketch, &insufficient).is_none());
}

#[test]
fn axial_profile_resolves_open_countersink_with_optional_terminal_overrun() {
    let mut profile = native_history().features.remove(0);
    profile.parameters = [
        ("a".into(), "6".into()),
        ("b".into(), "<MOD-DIAM>6.4".into()),
        ("c".into(), "<MOD-DIAM>13.2".into()),
        ("d".into(), "90°".into()),
    ]
    .into_iter()
    .collect();
    let sketch = SketchId::mint("synthetic:test:id#profile").unwrap();
    let entities = |terminal, mirror_wall: bool| {
        let wall_radius = if mirror_wall { -3.2 } else { 3.2 };
        [
            profile_line(&sketch, 0, Point2::new(0.0, 6.6), Point2::new(-3.4, 3.2)),
            profile_line(
                &sketch,
                1,
                Point2::new(-3.4, wall_radius),
                Point2::new(terminal, wall_radius),
            ),
        ]
    };

    for (terminal, mirror_wall) in [(-6.0, false), (-6.000_05, false), (-6.001, true)] {
        let exact_entities = entities(terminal, mirror_wall);
        let construction =
            profiled_hole_construction(&profile, &sketch, &exact_entities).expect("exact profile");
        assert_eq!(
            construction.diameter,
            cadmpeg_ir::scalar::PositiveLength::new(6.4).unwrap()
        );
        assert_eq!(construction.extent, LinearTermination::ThroughAll {});
        assert_eq!(
            construction.kind,
            HoleKind::Countersink {
                diameter: cadmpeg_ir::scalar::PositiveLength::new(13.2).unwrap(),
                angle: cadmpeg_ir::scalar::InteriorAngle::new(std::f64::consts::FRAC_PI_2).unwrap(),
            }
        );
        assert_eq!(construction.bottom, None);

        let mut translated_entities = exact_entities;
        for entity in &mut translated_entities {
            entity
                .geometry
                .edit(|definition| {
                    let SketchGeometryDefinition::Line { start, end } = definition else {
                        unreachable!();
                    };
                    start.u += 20.0;
                    start.v += 30.0;
                    end.u += 20.0;
                    end.v += 30.0;
                })
                .unwrap();
        }
        assert_eq!(
            profiled_hole_construction(&profile, &sketch, &translated_entities)
                .expect("translated exact profile")
                .kind,
            construction.kind
        );
    }
    assert!(profiled_hole_construction(&profile, &sketch, &entities(-6.002, true)).is_none());

    let mut independently_translated = entities(-6.0, false);
    for (index, entity) in independently_translated.iter_mut().enumerate() {
        entity
            .geometry
            .edit(|definition| {
                let SketchGeometryDefinition::Line { start, end } = definition else {
                    unreachable!();
                };
                let offset = (index + 1) as f64 * 20.0;
                start.u += offset;
                start.v += offset;
                end.u += offset;
                end.v += offset;
            })
            .unwrap();
    }
    assert!(profiled_hole_construction(&profile, &sketch, &independently_translated).is_none());
}

#[test]
fn incomplete_axial_profile_does_not_assign_dimension_roles() {
    let mut profile = native_history().features.remove(0);
    profile.parameters = [
        ("a".into(), "8.6".into()),
        ("b".into(), "<MOD-DIAM>15".into()),
        ("c".into(), "23".into()),
        ("d".into(), "<MOD-DIAM>9".into()),
    ]
    .into_iter()
    .collect();
    let sketch = SketchId::mint("synthetic:test:id#profile").unwrap();
    let entities = [
        profile_line(&sketch, 0, Point2::new(0.0, 7.5), Point2::new(-8.6, 7.5)),
        profile_line(&sketch, 1, Point2::new(-8.6, 4.5), Point2::new(-23.0, 4.5)),
    ];

    assert!(profiled_hole_construction(&profile, &sketch, &entities).is_none());
}

#[test]
fn unique_axial_profile_resolves_the_unique_incomplete_hole() {
    let mut history = native_history();
    history.features[0]
        .properties
        .insert("DissectableChildren".into(), "6,9".into());
    let mut profile = history.features[0].clone();
    profile.id = "native-profile".into();
    profile.source_id = FeatureSource::from_value(9);
    profile.ordinal = 1;
    profile.xml_tag = "Sketch".into();
    profile.kind = "Sketch".into();
    profile.input_class = Some("moProfileFeature_c".into());
    profile.parameters = [
        ("a".into(), "8.6".into()),
        ("b".into(), "<MOD-DIAM>15".into()),
        ("c".into(), "23".into()),
        ("d".into(), "<MOD-DIAM>9".into()),
    ]
    .into_iter()
    .collect();
    history.features.push(profile);
    let mut position = history.features[0].clone();
    position.id = "native-position".into();
    position.source_id = FeatureSource::from_value(6);
    position.ordinal = 2;
    position.xml_tag = "Sketch".into();
    position.kind = "Sketch".into();
    position.input_class = Some("moProfileFeature_c".into());
    position.parameters = [("D1".into(), "50".into()), ("D2".into(), "35".into())]
        .into_iter()
        .collect();
    history.features.push(position);

    let sketch = SketchId::mint("synthetic:test:id#profile").unwrap();
    let entities = [
        profile_line(&sketch, 0, Point2::new(0.0, 7.5), Point2::new(-8.6, 7.5)),
        profile_line(&sketch, 1, Point2::new(-8.6, 7.5), Point2::new(-8.6, 4.5)),
        profile_line(&sketch, 2, Point2::new(-8.6, 4.5), Point2::new(-23.0, 4.5)),
    ];
    let sketch_feature = cadmpeg_ir::features::Feature {
        id: FeatureId::mint("synthetic:test:id#profile-feature").expect("identity grammar"),
        ordinal: 1,
        name: Some("Profile".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
            }),
        ),
        native_ref: Some("native-profile".into()),
    };
    let position_feature = cadmpeg_ir::features::Feature {
        id: FeatureId::mint("synthetic:test:id#position-feature").expect("identity grammar"),
        ordinal: 2,
        name: Some("Position".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                    SketchId::mint("synthetic:test:id#position").unwrap(),
                )),
            }),
        ),
        native_ref: Some("native-position".into()),
    };
    let mut features = vec![model_hole(), sketch_feature, position_feature];
    let lane = lane_with_position_reference(6);
    let model_sketches = features
        .iter()
        .filter_map(|feature| {
            let FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
                ..
            }) = feature.evaluation.definition()
            else {
                return None;
            };
            Some((feature.native_ref.clone()?, sketch.clone()))
        })
        .collect::<HashMap<_, _>>();
    let histories = [history.clone()];
    assert_eq!(
        direct_hole_position_feature(
            &histories[0].features[0],
            &histories,
            &model_sketches,
            &entities,
        )
        .map(|feature| feature.id.as_str()),
        Some("native-position")
    );

    let mut single_child_history = history.clone();
    single_child_history.features[0]
        .properties
        .insert("DissectableChildren".into(), "9".into());
    single_child_history.features[1].ordinal = 2;
    single_child_history.features[2].ordinal = 1;
    assert_eq!(
        direct_hole_position_feature(
            &single_child_history.features[0],
            std::slice::from_ref(&single_child_history),
            &model_sketches,
            &entities,
        )
        .map(|feature| feature.id.as_str()),
        Some("native-position")
    );
    single_child_history.features[0]
        .properties
        .remove("DissectableChildren");
    assert_eq!(
        direct_hole_position_feature(
            &single_child_history.features[0],
            std::slice::from_ref(&single_child_history),
            &model_sketches,
            &entities,
        )
        .map(|feature| feature.id.as_str()),
        Some("native-position")
    );

    project_profiled_hole_constructions(&mut features, &entities, &[history], &[lane]).unwrap();

    assert!(matches!(
        features[0].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole {
            shape,
            extent: Some(LinearTermination::ThroughAll {}),

            ..
        }) if matches!((&shape.diameter(), shape.construction(),), (Some(actual_diameter), cadmpeg_ir::features::HoleConstruction::Form {
                kind: HoleKind::Counterbore {
                    diameter: actual_diameter_2,
                    depth: actual_depth,
                },
                ..
            },) if actual_diameter.get() == 9.0 && actual_diameter_2.get() == 15.0 && actual_depth.get() == 8.6)));
}

#[test]
fn ordered_profile_fallback_excludes_claimed_profiles() {
    let mut history = native_history();
    let mut second_hole = history.features[0].clone();
    second_hole.id = "second-hole".into();
    second_hole.source_id = FeatureSource::from_value(8);
    second_hole.ordinal = 1;
    let mut claimed_hole = history.features[0].clone();
    claimed_hole.id = "claimed-hole".into();
    claimed_hole.source_id = FeatureSource::from_value(11);
    claimed_hole.ordinal = 2;
    claimed_hole
        .properties
        .insert("DissectableChildren".into(), "9".into());
    let profile = |id: &str, source: &str, ordinal, diameter: &str, depth: &str| {
        let mut profile = history.features[0].clone();
        profile.id = id.into();
        profile.source_id = Some(FeatureSource::try_from(source).expect("test feature source id"));
        profile.ordinal = ordinal;
        profile.xml_tag = "Sketch".into();
        profile.kind = "Sketch".into();
        profile.input_class = Some("moProfileFeature_c".into());
        profile.parameters = [
            ("diameter".into(), format!("<MOD-DIAM>{diameter}")),
            ("depth".into(), depth.into()),
        ]
        .into();
        profile
    };
    let claimed_profile = profile("claimed-profile", "9", 3, "15", "23");
    let first_profile = profile("first-profile", "10", 4, "4.2", "6.8");
    let second_profile = profile("second-profile", "12", 5, "6", "14");
    history.features.extend([
        second_hole,
        claimed_hole,
        claimed_profile,
        first_profile,
        second_profile,
    ]);

    let model_sketch = |id: &str, sketch: &str, ordinal| cadmpeg_ir::features::Feature {
        id: FeatureId::mint(format!("synthetic:test:id#{id}-feature")).expect("identity grammar"),
        ordinal,
        name: None,
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(
                    SketchId::mint(sketch).unwrap(),
                )),
            }),
        ),
        native_ref: Some(id.into()),
    };
    let mut second_model_hole = model_hole();
    second_model_hole.id =
        FeatureId::mint("synthetic:test:id#second-model-hole").expect("identity grammar");
    second_model_hole.ordinal = 1;
    second_model_hole.native_ref = Some("second-hole".into());
    let mut features = vec![
        model_hole(),
        second_model_hole,
        model_sketch("claimed-profile", "synthetic:test:id#claimed-sketch", 1),
        model_sketch("first-profile", "synthetic:test:id#first-sketch", 2),
        model_sketch("second-profile", "synthetic:test:id#second-sketch", 3),
    ];
    let axial_rectangle = |sketch: &str, radius: f64, depth: f64, first_ordinal| {
        let sketch = SketchId::mint(sketch).unwrap();
        [
            profile_line(
                &sketch,
                first_ordinal,
                Point2::new(0.0, 0.0),
                Point2::new(0.0, radius),
            ),
            profile_line(
                &sketch,
                first_ordinal + 1,
                Point2::new(0.0, radius),
                Point2::new(-depth, radius),
            ),
            profile_line(
                &sketch,
                first_ordinal + 2,
                Point2::new(-depth, radius),
                Point2::new(-depth, 0.0),
            ),
            profile_line(
                &sketch,
                first_ordinal + 3,
                Point2::new(-depth, 0.0),
                Point2::new(0.0, 0.0),
            ),
        ]
    };
    let entities = [
        axial_rectangle("synthetic:test:id#first-sketch", 2.1, 6.8, 0),
        axial_rectangle("synthetic:test:id#second-sketch", 3.0, 14.0, 4),
    ]
    .concat();

    project_profiled_hole_constructions(&mut features, &entities, &[history], &[]).unwrap();

    assert!(matches!(
        features[0].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole {
            shape,
            extent: Some(LinearTermination::Blind {
                length: actual_length
            }),
            ..
        }) if matches!((&shape.diameter(),), (Some(actual_diameter),) if actual_diameter.get() == 4.2 && actual_length.get() == 6.8)));
    assert!(matches!(
        features[1].evaluation.definition(), FeatureDefinition::Operation(FeatureOperation::Hole {
            shape,
            extent: Some(LinearTermination::Blind {
                length: actual_length
            }),
            ..
        }) if matches!((&shape.diameter(),), (Some(actual_diameter),) if actual_diameter.get() == 6.0 && actual_length.get() == 14.0)));
}
