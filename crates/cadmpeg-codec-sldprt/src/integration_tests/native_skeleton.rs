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
            dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
            source_properties: std::collections::BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
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
    write_with_losses(ir).0
}

fn write_with_losses(ir: &cadmpeg_ir::CadIr) -> (Vec<u8>, Vec<cadmpeg_ir::report::loss::LossNote>) {
    let plan = SldprtCodec
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .expect("the part plans");
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes).expect("the part writes");
    (bytes, report.losses)
}

/// The decoder's exact walk of the written part's `Config-0` and
/// `ResolvedFeatures`, and the `ResolvedFeatures` payload.
fn walk(bytes: &[u8]) -> (crate::carchive::Walk, Vec<u8>) {
    let scan = crate::test_support::container::scan(bytes);
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
    (walk, resolved.to_vec())
}

fn classes(walk: &crate::carchive::Walk) -> Vec<&str> {
    walk.trace.iter().map(|object| &*object.class).collect()
}

fn loss<'a>(
    losses: &'a [cadmpeg_ir::report::loss::LossNote],
    code: &str,
) -> &'a cadmpeg_ir::report::loss::LossNote {
    losses
        .iter()
        .find(|loss| loss.code.local_code() == code)
        .unwrap_or_else(|| panic!("{code} in {losses:#?}"))
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
    let chains = walk
        .trace
        .iter()
        .filter(|object| &*object.class == "moSketchChain_c")
        .count();
    let profiles = ir
        .model
        .sketches
        .iter()
        .map(|sketch| sketch.profiles.as_slice().len())
        .sum::<usize>();
    assert_eq!(chains, profiles, "one moSketchChain_c per profile chain");
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
        parameters::{DesignParameter, DimensionDisplay, ParameterValue},
        FeatureId, ParameterId,
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
            dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
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
            Some(cadmpeg_ir::features::parameters::ParameterValue::Length(length)) => {
                Some(length.get())
            }
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

/// A driven (reference) dimension is no native sketch dimension, which is
/// driving: the sketch leaves it out and names it.
#[test]
fn a_native_sketch_leaves_out_a_driven_dimension() {
    let mut ir = cube_with_constrained_sketches();
    for constraint in &mut ir.model.sketch_constraints {
        if constraint.id.as_str() == "synthetic:test:constraint#bore" {
            constraint.driving = Some(false);
        }
    }
    let (bytes, losses) = write_with_losses(&ir);
    let (walk, _) = walk(&bytes);
    let names = walk
        .parameters
        .iter()
        .filter_map(|parameter| parameter.name.as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"Turn"), "{names:?}");
    assert!(!names.contains(&"Bore"), "{names:?}");
    let driven = loss(&losses, "writer.driven-dimensions-omitted");
    assert_eq!(
        driven.code,
        crate::loss::SldprtLossCode::WriterDrivenDimensionsOmitted.kind()
    );
    assert!(
        driven
            .message
            .ends_with("1 driven (reference) sketch dimension(s): synthetic:test:constraint#bore"),
        "{}",
        driven.message
    );
}

/// `cube_with_sketches` with the rectangle made the cube's 10 by 10 base
/// and extruded 10 into a new body, which is the cube.
fn cube_with_extrusion() -> cadmpeg_ir::CadIr {
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};
    let mut ir = cube_with_sketches();
    let corners = [
        Point2::new(0.0, 0.0),
        Point2::new(10.0, 0.0),
        Point2::new(10.0, 10.0),
        Point2::new(0.0, 10.0),
    ];
    for (index, entity) in ir
        .model
        .sketch_entities
        .iter_mut()
        .filter(|entity| {
            entity
                .id()
                .as_str()
                .starts_with("synthetic:test:sketch-entity#Rectangle-")
        })
        .enumerate()
    {
        entity.geometry = SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: corners[index],
            end: corners[(index + 1) % 4],
        })
        .expect("geometry");
    }
    push_extrusion(
        &mut ir,
        "Boss",
        "Rectangle",
        cadmpeg_ir::features::operation_settings::BooleanOp::NewBody,
        cadmpeg_ir::features::extents::ExtrudeDirection::ProfileNormal {},
        10.0,
    );
    ir
}

/// Append a one-sided blind extrusion of `sketch` by `depth` millimetres.
fn push_extrusion(
    ir: &mut cadmpeg_ir::CadIr,
    key: &str,
    sketch: &str,
    op: cadmpeg_ir::features::operation_settings::BooleanOp,
    direction: cadmpeg_ir::features::extents::ExtrudeDirection,
    depth: f64,
) {
    use cadmpeg_ir::features::{
        extents::{ExtrudeExtent, ExtrudeSide, ExtrudeStart, LinearTermination},
        profiles::{PlanarProfileRef, ProfileRef},
        Feature, FeatureId,
    };
    let ordinal = u64::try_from(ir.model.features.len()).expect("ordinal");
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint(format!("synthetic:test:feature#{key}")).expect("id"),
        ordinal,
        name: Some(key.into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Extrude {
                profile: ProfileRef::Planar(PlanarProfileRef::Sketch(
                    cadmpeg_ir::sketches::SketchId::mint(format!("synthetic:test:sketch#{sketch}"))
                        .expect("id"),
                )),
                direction,
                start: ExtrudeStart::ProfilePlane {},
                extent: ExtrudeExtent::OneSided {
                    side: ExtrudeSide {
                        termination: LinearTermination::Blind {
                            length: cadmpeg_ir::scalar::NonZeroLength::new(depth).expect("length"),
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
            }),
        ),
        native_ref: None,
    });
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
    let items = classes
        .iter()
        .filter(|class| **class == "moHistoryFeatItemData_c")
        .count();
    assert_eq!(items, 3, "two sketches and the extrusion");
    assert!(!classes.contains(&"moBaseBody_c"));
    let depths = walk
        .parameters
        .iter()
        .filter(|parameter| parameter.name.as_str() == Some("D1"))
        .map(|parameter| parameter.value)
        .collect::<Vec<_>>();
    assert_eq!(depths, [0.01]);
    let end_spec = walk
        .trace
        .iter()
        .find(|object| &*object.class == "moEndSpec_c")
        .expect("the end specification is written");
    let direction = u32::from_le_bytes(
        resolved[end_spec.body + 10..end_spec.body + 14]
            .try_into()
            .expect("four bytes"),
    );
    assert_eq!(direction, 0, "the extrusion runs along the sketch normal");
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
                extent,
                profile,
                op,
                ..
            }) => {
                // The source's operation survives the family word 0x0140.
                assert_eq!(
                    *op,
                    cadmpeg_ir::features::operation_settings::BooleanOp::NewBody
                );
                Some((extent.clone(), profile.clone()))
            }
            _ => None,
        })
        .expect("the extrusion decodes");
    assert!(
        matches!(
            &extrude,
            (
                cadmpeg_ir::features::extents::ExtrudeExtent::OneSided {
                    side: cadmpeg_ir::features::extents::ExtrudeSide {
                        termination: cadmpeg_ir::features::extents::LinearTermination::Blind { length },
                        ..
                    }
                },
                cadmpeg_ir::features::profiles::ProfileRef::Planar(
                    cadmpeg_ir::features::profiles::PlanarProfileRef::Sketch(_)
                ),
            ) if (length.get() - 10.0).abs() < 1.0e-9
        ),
        "{extrude:?}"
    );
    assert_eq!(decoded.ir().geometry.bodies.len(), 1);
}

#[test]
fn a_part_whose_write_leaves_out_a_feature_keeps_its_stored_body() {
    use cadmpeg_ir::features::{Feature, FeatureId};
    let mut ir = cube_with_extrusion();
    let ordinal = u64::try_from(ir.model.features.len()).expect("ordinal");
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint("other:model:feature#insert").expect("id"),
        ordinal,
        name: Some("Insert".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "Component Insert".into(),
                parameters: std::collections::BTreeMap::new(),
            }),
        ),
        native_ref: None,
    });
    let (bytes, losses) = write_with_losses(&ir);
    let (walk, _) = walk(&bytes);
    let classes = classes(&walk);
    assert!(classes.contains(&"moBaseBody_c"), "{classes:?}");
    assert!(!classes.contains(&"moExtrusion_c"), "{classes:?}");
    let summary = loss(&losses, "writer.native-history-summary");
    assert!(
        summary.message.contains(
            "the tree writes no extrusion natively because the write leaves out source feature \
             other:model:feature#insert, which records no body it makes or changes"
        ),
        "{}",
        summary.message
    );
    assert!(losses
        .iter()
        .all(|loss| loss.code.local_code() != "writer.native-body-identity-omitted"));
}

#[test]
fn a_second_extrusion_keeps_the_stored_body() {
    use cadmpeg_ir::features::{extents::ExtrudeDirection, operation_settings::BooleanOp};
    for op in [BooleanOp::Join, BooleanOp::Cut] {
        let mut ir = cube_with_extrusion();
        push_extrusion(
            &mut ir,
            "Second",
            "Arcs",
            op,
            ExtrudeDirection::ProfileNormal {},
            2.0,
        );
        let (bytes, losses) = write_with_losses(&ir);
        let (walk, _) = walk(&bytes);
        let classes = classes(&walk);
        assert!(classes.contains(&"moBaseBody_c"), "{op:?}: {classes:?}");
        assert!(
            !classes.contains(&"moExtrusion_c") && !classes.contains(&"moCut_c"),
            "{op:?}: {classes:?}"
        );
        assert_eq!(
            walk.sketch_profiles.len(),
            2,
            "{op:?}: both sketches stay native"
        );
        let summary = loss(&losses, "writer.native-history-summary");
        assert!(
            summary.message.contains(
                "extrusion synthetic:test:feature#Second follows extrusion \
                 synthetic:test:feature#Boss"
            ),
            "{op:?}: {}",
            summary.message
        );
    }
}

/// A part the tree cannot write the extrusion of keeps the native sketches,
/// the skeleton and the stored body, and names why.
fn assert_stored_body_because(ir: &cadmpeg_ir::CadIr, reason: &str) {
    let (bytes, losses) = write_with_losses(ir);
    assert!(
        losses
            .iter()
            .all(|loss| loss.code.local_code() != "writer.native-skeleton-omitted"),
        "{losses:#?}"
    );
    let (walk, _) = walk(&bytes);
    let classes = classes(&walk);
    assert!(classes.contains(&"moBaseBody_c"), "{classes:?}");
    assert!(!classes.contains(&"moExtrusion_c"), "{classes:?}");
    assert_eq!(walk.sketch_profiles.len(), 2, "both sketches stay native");
    let summary = loss(&losses, "writer.native-history-summary");
    assert!(summary.message.contains(reason), "{}", summary.message);
}

#[test]
fn an_extrusion_before_its_sketch_in_the_source_order_keeps_the_stored_body() {
    let mut ir = cube_with_extrusion();
    for feature in &mut ir.model.features {
        match feature.id.as_str() {
            "synthetic:test:feature#Rectangle" => feature.ordinal = 2,
            "synthetic:test:feature#Boss" => feature.ordinal = 0,
            _ => {}
        }
    }
    assert_stored_body_because(
        &ir,
        "extrusion synthetic:test:feature#Boss does not follow its sketch \
         synthetic:test:sketch#Rectangle in the source order",
    );
}

#[test]
fn a_suppressed_extrusion_keeps_the_stored_body() {
    let mut ir = cube_with_extrusion();
    for feature in &mut ir.model.features {
        if feature.id.as_str() == "synthetic:test:feature#Boss" {
            feature.suppressed = Some(true);
        }
    }
    assert_stored_body_because(&ir, "extrusion synthetic:test:feature#Boss is suppressed");
}

/// An extrusion along the normal of a sketch whose normal opposes its
/// plane's runs against the plane's normal.
#[test]
fn an_extrusion_against_its_plane_normal_writes_the_reversed_direction_flag() {
    use cadmpeg_ir::math::{Point3, Vector3};
    let mut ir = cube_with_sketches();
    let rectangle = ir
        .model
        .sketches
        .iter_mut()
        .find(|sketch| sketch.name.as_deref() == Some("Rectangle"))
        .expect("the rectangle sketch");
    rectangle.placement = cadmpeg_ir::sketches::SketchPlacement::try_resolved(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, -1.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .expect("placement");
    push_extrusion(
        &mut ir,
        "Boss",
        "Rectangle",
        cadmpeg_ir::features::operation_settings::BooleanOp::NewBody,
        cadmpeg_ir::features::extents::ExtrudeDirection::ProfileNormal {},
        5.0,
    );
    let (bytes, losses) = write_with_losses(&ir);
    let (walk, resolved) = walk(&bytes);
    assert!(classes(&walk).contains(&"moExtrusion_c"), "{losses:#?}");
    let end_spec = walk
        .trace
        .iter()
        .find(|object| &*object.class == "moEndSpec_c")
        .expect("the end specification is written");
    let direction = u32::from_le_bytes(
        resolved[end_spec.body + 10..end_spec.body + 14]
            .try_into()
            .expect("four bytes"),
    );
    assert_eq!(direction, 1, "the extrusion runs against the sketch normal");
}

#[test]
fn a_part_whose_only_sketch_is_spatial_keeps_the_generated_lane() {
    use cadmpeg_ir::features::{Feature, FeatureId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::sketches::{
        SpatialSketch, SpatialSketchEntity, SpatialSketchEntityId, SpatialSketchGeometry,
        SpatialSketchGeometryDefinition, SpatialSketchId,
    };
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let sketch = SpatialSketchId::mint("synthetic:test:spatial-sketch#path").expect("id");
    ir.model.spatial_sketches.push(SpatialSketch {
        id: sketch.clone(),
        name: Some("Path".into()),
        configuration: Some("0".into()),
        visible: None,
        profiles: Vec::new(),
        native_ref: None,
    });
    let (start, end) = (Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0));
    ir.model
        .spatial_sketch_entities
        .push(SpatialSketchEntity::new(
            SpatialSketchEntityId::mint("synthetic:test:spatial-sketch-entity#line").expect("id"),
            sketch.clone(),
            SpatialSketchGeometry::try_from(SpatialSketchGeometryDefinition::Line { start, end })
                .expect("geometry"),
        ));
    ir.model.features.push(Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:feature#path").expect("id"),
        ordinal: 0,
        name: Some("Path".into()),
        suppressed: Some(false),
        dependencies: cadmpeg_ir::features::selections::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::SpatialSketch {
                sketch: Some(sketch),
            }),
        ),
        native_ref: None,
    });
    let (bytes, losses) = write_with_losses(&ir);
    let lane = loss(&losses, "writer.native-sketch-lane-orphan");
    assert!(
        lane.message
            .contains("spatial sketch synthetic:test:spatial-sketch#path"),
        "{}",
        lane.message
    );
    let decoded = SldprtCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("the part decodes");
    let entities = &decoded.ir().model.spatial_sketch_entities;
    assert_eq!(entities.len(), 1, "{entities:?}");
    assert!(
        matches!(
            *entities[0].geometry.definition(),
            SpatialSketchGeometryDefinition::Line { start: read_start, end: read_end }
                if cadmpeg_ir::compare::floats_agree(read_start.x, start.x)
                    && cadmpeg_ir::compare::floats_agree(read_end.z, end.z)
        ),
        "{entities:?}"
    );
}
