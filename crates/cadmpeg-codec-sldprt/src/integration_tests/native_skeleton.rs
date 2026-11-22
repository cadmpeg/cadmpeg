// SPDX-License-Identifier: Apache-2.0
//! A generated part carries the native skeleton: its object streams walk to
//! their end, and its default tree and stored body decode as a native part's
//! do.

use std::io::Cursor;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

use crate::SldprtCodec;

fn written_cube() -> Vec<u8> {
    let ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let plan = SldprtCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .expect("the cube plans");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("the cube writes");
    bytes
}

#[test]
fn generated_part_object_streams_walk_to_their_end() {
    let bytes = written_cube();
    let scan = crate::test_support::container::scan(&bytes);
    let payload = |name: &str| {
        scan.sections()
            .find(|section| section.name() == Some(name))
            .map_or_else(
                || panic!("{name} is written"),
                crate::container::Section::payload,
            )
    };
    let config = payload("Contents/Config-0");
    let resolved = payload("Contents/Config-0-ResolvedFeatures");
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let version =
        crate::carchive::walk_version(crate::container::declared_sw_version(&scan), config);
    let walk = crate::carchive::walk_configuration(&ctx, config, resolved, version)
        .expect("walk")
        .expect("the Config-0 class map reaches the ResolvedFeatures count");
    assert_eq!(walk.status, crate::carchive::Status::End);
    for stream in [
        "swXmlContents/KeyWords",
        "swXmlContents/Features",
        "Contents/OleItems",
        "Contents/eModelLic",
    ] {
        payload(stream);
    }
    assert!(scan
        .sections()
        .all(|section| section.name() != Some("Contents/SolidWorks")));
}

#[test]
fn generated_part_decodes_its_default_tree_and_stored_body() {
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(written_cube()), &DecodeOptions::default())
        .expect("the written cube decodes");
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.local_code() != "feature.input-archive-walk-stopped"));
    let features = &decoded.ir().model.features;
    let named = |name: &str| {
        features
            .iter()
            .find(|feature| feature.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("{name} decodes"))
    };
    for plane in ["Front Plane", "Top Plane", "Right Plane", "Origin"] {
        named(plane);
    }
    assert!(matches!(
        named("Imported1").evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
    ));
}

/// The unit cube with two sketches: a 10 by 6 rectangle on the XY plane and,
/// on the XZ plane, a quarter disc of radius 3 about (5, 5) and a circle of
/// radius 2 about (-4, 1), all in millimetres.
fn cube_with_sketches() -> cadmpeg_ir::CadIr {
    use cadmpeg_ir::features::{Feature, FeatureId};
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::sketches::{
        Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
        SketchGeometryDefinition, SketchId, SketchPlacement, SketchProfiles,
    };
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let add = |ir: &mut cadmpeg_ir::CadIr,
               key: &str,
               normal: Vector3,
               profiles: Vec<Vec<SketchGeometryDefinition>>| {
        let sketch = SketchId::mint(format!("synthetic:test:sketch#{key}")).expect("id");
        let mut uses = Vec::new();
        for (profile_index, profile) in profiles.into_iter().enumerate() {
            let mut chain = Vec::new();
            for (index, definition) in profile.into_iter().enumerate() {
                let entity = SketchEntityId::mint(format!(
                    "synthetic:test:sketch-entity#{key}-{profile_index}-{index}"
                ))
                .expect("id");
                ir.model.sketch_entities.push(SketchEntity::new(
                    entity.clone(),
                    sketch.clone(),
                    SketchGeometry::try_from(definition).expect("geometry"),
                ));
                chain.push(SketchEntityUse {
                    entity,
                    reversed: false,
                });
            }
            uses.push(chain);
        }
        ir.model.sketches.push(Sketch {
            id: sketch.clone(),
            name: Some(key.into()),
            configuration: None,
            visible: None,
            placement: SketchPlacement::try_resolved(
                Point3::new(0.0, 0.0, 0.0),
                normal,
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("placement"),
            profiles: SketchProfiles::try_from(uses).expect("profiles"),
            native_ref: None,
        });
        let ordinal = u64::try_from(ir.model.features.len()).expect("ordinal");
        ir.model.features.push(Feature {
            product_definition: None,
            id: FeatureId::mint(format!("synthetic:test:feature#{key}")).expect("id"),
            ordinal,
            name: Some(key.into()),
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(Some(sketch)),
                }),
            ),
            native_ref: None,
        });
    };
    let line = |start: Point2, end: Point2| SketchGeometryDefinition::Line { start, end };
    let corners = [
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
        Point2::new(10.0, 6.0),
        Point2::new(0.0, 6.0),
    ];
    let rectangle = (0..4)
        .map(|index| line(corners[index], corners[(index + 1) % 4]))
        .collect();
    add(
        &mut ir,
        "Rectangle",
        Vector3::new(0.0, 0.0, 1.0),
        vec![rectangle],
    );
    let length = |value: f64| cadmpeg_ir::scalar::Length::new(value).expect("length");
    let angle = |value: f64| cadmpeg_ir::scalar::Angle::new(value).expect("angle");
    add(
        &mut ir,
        "Arcs",
        Vector3::new(0.0, 1.0, 0.0),
        vec![
            vec![
                SketchGeometryDefinition::Arc {
                    center: Point2::new(5.0, 5.0),
                    radius: length(3.0),
                    start_angle: angle(0.0),
                    end_angle: angle(std::f64::consts::FRAC_PI_2),
                },
                line(Point2::new(5.0, 8.0), Point2::new(5.0, 5.0)),
                line(Point2::new(5.0, 5.0), Point2::new(8.0, 5.0)),
            ],
            vec![SketchGeometryDefinition::Circle {
                center: Point2::new(-4.0, 1.0),
                radius: length(2.0),
            }],
        ],
    );
    ir
}

fn write(ir: &cadmpeg_ir::CadIr) -> Vec<u8> {
    let plan = SldprtCodec
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .expect("the part plans");
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).expect("the part writes");
    bytes
}

/// The model-space points of a decoded sketch entity's loci.
fn model_points(ir: &cadmpeg_ir::CadIr, name: &str) -> Vec<[f64; 3]> {
    use cadmpeg_ir::sketches::SketchGeometryDefinition;
    let sketch = ir
        .model
        .sketches
        .iter()
        .find(|sketch| sketch.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("sketch {name} decodes"));
    let (origin, normal, u_axis) = sketch.resolved_placement().expect("placement");
    let (origin, normal, u_axis) = (origin.get(), normal.get(), u_axis.get());
    let v_axis = normal.cross(u_axis);
    let lift = |u: f64, v: f64| {
        [
            origin.x + u * u_axis.x + v * v_axis.x,
            origin.y + u * u_axis.y + v * v_axis.y,
            origin.z + u * u_axis.z + v * v_axis.z,
        ]
    };
    let mut points = Vec::new();
    for entity in ir
        .model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id)
    {
        match entity.geometry.definition() {
            SketchGeometryDefinition::Line { start, end } => {
                points.push(lift(start.get().u, start.get().v));
                points.push(lift(end.get().u, end.get().v));
            }
            SketchGeometryDefinition::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                let (c, r) = (center.get(), radius.get());
                for angle in [start_angle.get(), end_angle.get()] {
                    points.push(lift(c.u + r * angle.cos(), c.v + r * angle.sin()));
                }
                points.push(lift(c.u, c.v));
            }
            SketchGeometryDefinition::Circle { center, .. } => {
                points.push(lift(center.get().u, center.get().v));
            }
            _ => {}
        }
    }
    points
}

#[test]
fn generated_sketches_are_native_profile_features_that_decode_back() {
    let ir = cube_with_sketches();
    let bytes = write(&ir);

    let scan = crate::test_support::container::scan(&bytes);
    let payload = |name: &str| {
        scan.sections()
            .find(|section| section.name() == Some(name))
            .map_or_else(
                || panic!("{name} is written"),
                crate::container::Section::payload,
            )
    };
    let config = payload("Contents/Config-0");
    let resolved = payload("Contents/Config-0-ResolvedFeatures");
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let walk = crate::carchive::walk_configuration(&ctx, config, resolved, Some(14000))
        .expect("walk")
        .expect("the class map reconciles");
    assert_eq!(walk.status, crate::carchive::Status::End);
    assert_eq!(walk.sketch_profiles.len(), 2);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the part decodes");
    assert!(decoded
        .report()
        .losses
        .iter()
        .all(|loss| loss.code.local_code() != "feature.input-archive-walk-stopped"));
    let close = |left: &[f64; 3], right: &[f64; 3]| {
        left.iter()
            .zip(right)
            .all(|(left, right)| (left - right).abs() < 1.0e-9)
    };
    let expect = |name: &str, expected: &[[f64; 3]]| {
        let points = model_points(decoded.ir(), name);
        for point in expected {
            assert!(
                points.iter().any(|candidate| close(candidate, point)),
                "{name}: {point:?} in {points:?}"
            );
        }
    };
    expect(
        "Rectangle",
        &[
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 6.0, 0.0],
            [0.0, 6.0, 0.0],
        ],
    );
    // The XZ sketch's v axis is the normal (+Y) cross u (+X): -Z.
    expect(
        "Arcs",
        &[
            [8.0, 0.0, -5.0],
            [5.0, 0.0, -8.0],
            [5.0, 0.0, -5.0],
            [-4.0, 0.0, -1.0],
        ],
    );
}

/// `cube_with_sketches` with the rectangle's first line horizontal, its second
/// vertical, the arc's radius dimensioned 3 and the circle's diameter
/// dimensioned 4.
fn cube_with_constrained_sketches() -> cadmpeg_ir::CadIr {
    use cadmpeg_ir::features::{
        DesignParameter, DimensionDisplay, FeatureId, ParameterId, ParameterValue,
    };
    use cadmpeg_ir::sketches::{
        SketchConstraint, SketchConstraintDefinition, SketchConstraintDefinitionInput,
        SketchConstraintId, SketchEntityId, SketchId,
    };
    let mut ir = cube_with_sketches();
    let entity = |key: &str| SketchEntityId::mint(format!("synthetic:test:sketch-entity#{key}"));
    let line0 = entity("Rectangle-0-0").expect("id");
    let line1 = entity("Rectangle-0-1").expect("id");
    let arc = entity("Arcs-0-0").expect("id");
    let circle = entity("Arcs-1-0").expect("id");
    let parameter = |ir: &mut cadmpeg_ir::CadIr,
                     key: &str,
                     owner: &str,
                     display: DimensionDisplay,
                     expression: &str,
                     value: f64| {
        let id = ParameterId::mint(format!("synthetic:test:parameter#{key}")).expect("id");
        let ordinal = u32::try_from(ir.model.parameters.len()).expect("ordinal");
        ir.model.parameters.push(DesignParameter {
            product_definition: None,
            id: id.clone(),
            owner: Some(FeatureId::mint(format!("synthetic:test:feature#{owner}")).expect("id")),
            ordinal,
            name: key.into(),
            expression: expression.into(),
            display: Some(display),
            value: Some(ParameterValue::Length(
                cadmpeg_ir::scalar::Length::new(value).expect("length"),
            )),
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            properties: std::collections::BTreeMap::new(),
            pmi: None,
            native_ref: None,
        });
        id
    };
    let turn = parameter(
        &mut ir,
        "Turn",
        "Arcs",
        DimensionDisplay::Radius,
        "R3mm",
        3.0,
    );
    let bore = parameter(
        &mut ir,
        "Bore",
        "Arcs",
        DimensionDisplay::Diameter,
        "<MOD-DIAM>4mm",
        4.0,
    );
    let constraints = [
        (
            "Rectangle",
            "horizontal",
            SketchConstraintDefinitionInput::Horizontal { entity: line0 },
        ),
        (
            "Rectangle",
            "vertical",
            SketchConstraintDefinitionInput::Vertical { entity: line1 },
        ),
        (
            "Arcs",
            "turn",
            SketchConstraintDefinitionInput::Radius {
                entity: arc,
                parameter: turn,
            },
        ),
        (
            "Arcs",
            "bore",
            SketchConstraintDefinitionInput::Diameter {
                entity: circle,
                parameter: bore,
            },
        ),
    ];
    for (sketch, key, definition) in constraints {
        ir.model.sketch_constraints.push(SketchConstraint {
            id: SketchConstraintId::mint(format!("synthetic:test:constraint#{key}")).expect("id"),
            sketch: SketchId::mint(format!("synthetic:test:sketch#{sketch}")).expect("id"),
            definition: SketchConstraintDefinition::try_from(definition).expect("constraint"),
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
        });
    }
    ir
}

#[test]
fn native_sketch_relations_and_dimensions_decode_back() {
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Def;
    let bytes = write(&cube_with_constrained_sketches());

    let decoded = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the part decodes");
    // The relation-table rows decode as native relations of their codes:
    // horizontal `4` and vertical `5`.
    let codes = decoded
        .ir()
        .model
        .sketch_constraints
        .iter()
        .filter_map(|constraint| match constraint.definition.kind() {
            Def::Native { native_kind, .. } => Some(native_kind.as_str().to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    for code in ["sldprt:sketch-relation:4", "sldprt:sketch-relation:5"] {
        assert!(codes.iter().any(|kind| kind == code), "{code} in {codes:?}");
    }
    let lengths = decoded
        .ir()
        .model
        .parameters
        .iter()
        .filter_map(|parameter| match parameter.value {
            Some(cadmpeg_ir::features::ParameterValue::Length(length)) => Some(length.get()),
            _ => None,
        })
        .collect::<Vec<_>>();
    for expected in [3.0, 4.0] {
        assert!(
            lengths
                .iter()
                .any(|length| (length - expected).abs() < 1.0e-9),
            "{expected} in {lengths:?}"
        );
    }
}

/// `cube_with_sketches` with the rectangle extruded 5 into a new body.
fn cube_with_extrusion() -> cadmpeg_ir::CadIr {
    use cadmpeg_ir::features::{
        BooleanOp, ExtrudeDirection, ExtrudeExtent, ExtrudeSide, ExtrudeStart, Feature, FeatureId,
        LinearTermination, PlanarProfileRef, ProfileRef,
    };
    let mut ir = cube_with_sketches();
    let ordinal = u64::try_from(ir.model.features.len()).expect("ordinal");
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:feature#Boss").expect("id"),
        ordinal,
        name: Some("Boss".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile: ProfileRef::Planar(PlanarProfileRef::Sketch(
                    cadmpeg_ir::sketches::SketchId::mint("synthetic:test:sketch#Rectangle")
                        .expect("id"),
                )),
                direction: ExtrudeDirection::ProfileNormal {},
                start: ExtrudeStart::ProfilePlane {},
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: cadmpeg_ir::scalar::NonZeroLength::new(5.0).expect("length"),
                        },
                        draft: None,
                    },
                },
                op: BooleanOp::NewBody,
                solid: Some(true),
                face_maker: None,
                inner_wire_taper: None,
                length_along_profile_normal: None,
                allow_multi_profile_faces: None,
            }),
        ),
        native_ref: None,
    });
    ir
}

#[test]
fn a_blind_extrusion_is_a_native_feature_in_place_of_the_stored_body() {
    let bytes = write(&cube_with_extrusion());
    let scan = crate::test_support::container::scan(&bytes);
    let payload = |name: &str| {
        scan.sections()
            .find(|section| section.name() == Some(name))
            .map_or_else(
                || panic!("{name} is written"),
                crate::container::Section::payload,
            )
    };
    let config = payload("Contents/Config-0");
    let resolved = payload("Contents/Config-0-ResolvedFeatures");
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let walk = crate::carchive::walk_configuration(&ctx, config, resolved, Some(14000))
        .expect("walk")
        .expect("the class map reconciles");
    assert_eq!(walk.status, crate::carchive::Status::End);
    let classes = walk
        .trace
        .iter()
        .map(|object| &*object.class)
        .collect::<Vec<_>>();
    assert!(classes.contains(&"moExtrusion_c"));
    assert!(!classes.contains(&"moBaseBody_c"));
    let depths = walk
        .parameters
        .iter()
        .filter(|parameter| parameter.name.as_str() == Some("D1"))
        .map(|parameter| parameter.value)
        .collect::<Vec<_>>();
    assert_eq!(depths, [0.005]);
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the part decodes");
    let extrude = decoded
        .ir()
        .model
        .features
        .iter()
        .find_map(|feature| match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                extent, profile, ..
            }) => Some((extent.clone(), profile.clone())),
            _ => None,
        })
        .expect("the extrusion decodes");
    assert!(
        matches!(
            &extrude,
            (
                cadmpeg_ir::features::ExtrudeExtent::OneSided {
                    side: cadmpeg_ir::features::ExtrudeSide {
                        termination: cadmpeg_ir::features::LinearTermination::Blind { length },
                        ..
                    }
                },
                cadmpeg_ir::features::ProfileRef::Planar(
                    cadmpeg_ir::features::PlanarProfileRef::Sketch(_)
                ),
            ) if (length.get() - 5.0).abs() < 1.0e-9
        ),
        "{extrude:?}"
    );
    assert_eq!(decoded.ir().geometry.bodies.len(), 1);
}
