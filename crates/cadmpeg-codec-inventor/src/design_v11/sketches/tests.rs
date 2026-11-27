// SPDX-License-Identifier: Apache-2.0
use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::design::ops::datum::DatumElement;
use cadmpeg_ir::design::ops::sketch::{PlanarCurve, SketchRelation};
use cadmpeg_ir::design::selection::{PickTarget, Picks};
use cadmpeg_ir::features::datums::{DatumPointConstruction, SketchPointSelection};
use cadmpeg_ir::features::evaluation::FeatureEvaluation;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{
    Feature, FeatureContent, FeatureDefinition, FeatureId, SketchFeatureBinding,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::check::Check;
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{SketchEntityId, SketchId};

fn feature(operation: LegacyOperation, name: &str) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint(format!("inventor:design:feature#{name}")).expect("id"),
        ordinal: 0,
        name: Some(name.into()),
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(operation)),
        native_ref: Some(format!("inventor:pmdc:{name}#test")),
    }
}

fn sketch_id() -> SketchId {
    SketchId::mint("inventor:design:sketch#test").expect("id")
}

fn entity_id(name: &str) -> SketchEntityId {
    SketchEntityId::mint(format!("inventor:design:entity#{name}")).expect("id")
}

fn fixture() -> CadIr {
    let mut ir = crate::test_support::root_part_ir();
    ir.model.features.push(feature(
        LegacyOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(Some(sketch_id())),
        },
        "sketch",
    ));
    ir.model.sketches.push(cadmpeg_ir::sketches::Sketch {
        id: sketch_id(),
        name: Some("Profile".into()),
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("placement"),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: Some("inventor:pmdc:sketch#test".into()),
    });
    ir
}

fn add_entity(ir: &mut CadIr, name: &str, geometry: Geometry, construction: bool) {
    ir.model.sketch_entities.push(
        cadmpeg_ir::sketches::SketchEntity::new(
            entity_id(name),
            sketch_id(),
            geometry.try_into().expect("geometry"),
        )
        .with_construction(construction)
        .with_native_ref(Some(format!("inventor:pmdc:entity#{name}"))),
    );
}

fn run(ir: &mut CadIr) {
    let model = ir.model.clone();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    crate::design_v11::project_history(&ctx, ir, &BTreeMap::default(), false).expect("history");
    project(&ctx, ir).expect("sketches");
    crate::design_v11::datums::project(&ctx, ir).expect("datums");
    assert_eq!(ir.model, model);
    assert_eq!(
        ir.design
            .features
            .iter()
            .find(|item| item.id == model.features[0].id)
            .expect("feature")
            .definition
            .operation,
        FeatureOperation::Sketch(sketch_id())
    );
    assert_eq!(ir.design.sketches[0].owner, model.features[0].id);
    assert_eq!(
        ir.design.sketches[0].native_ref,
        model.sketches[0].native_ref
    );
}

#[test]
fn rectangle_on_principal_plane_keeps_frame_and_construction_line() {
    let mut ir = fixture();
    for (name, start, end) in [
        ("a", [0.0, 0.0], [4.0, 0.0]),
        ("b", [4.0, 0.0], [4.0, 3.0]),
        ("c", [4.0, 3.0], [0.0, 3.0]),
        ("d", [0.0, 3.0], [0.0, 0.0]),
    ] {
        add_entity(
            &mut ir,
            name,
            Geometry::Line {
                start: Point2::new(start[0], start[1]),
                end: Point2::new(end[0], end[1]),
            },
            name == "d",
        );
    }
    ir.model.sketch_constraints.push(
        json::from_value(cadmpeg_core::json!({
            "id":"inventor:design:constraint#horizontal", "sketch":sketch_id(),
            "definition":{"kind":"horizontal", "entity":entity_id("a")}
        }))
        .expect("constraint"),
    );
    run(&mut ir);
    assert_eq!(ir.design.sketch_constraints.len(), 1);
    assert_eq!(
        ir.design.sketch_constraints[0].id,
        ir.model.sketch_constraints[0].id
    );
    assert_eq!(ir.design.sketch_constraints[0].sketch, sketch_id());
    assert_eq!(
        ir.design.sketch_constraints[0].relation,
        SketchRelation::Horizontal {
            a: cadmpeg_ir::design::ops::sketch::Operand::Entity(entity_id("a")),
            b: None,
        }
    );
    assert_eq!(ir.design.sketch_entities.len(), 4);
    for (source, target) in ir
        .model
        .sketch_entities
        .iter()
        .zip(&ir.design.sketch_entities)
    {
        assert_eq!(&target.id, source.id());
        assert_eq!(target.sketch, sketch_id());
        let Geometry::Line { start, end } = source.geometry.definition() else {
            panic!("line")
        };
        assert_eq!(
            target.definition,
            EntityDefinition::Free(PlanarCurve::Line {
                start: *start,
                end: *end
            })
        );
        assert_eq!(target.construction, Some(source.construction));
    }
    let SketchPlane::Placed(frame) = &ir.design.sketches[0].plane else {
        panic!("placed")
    };
    let frame = frame.resolved.as_ref().expect("frame");
    assert_eq!(frame.origin.get(), Point3::new(0.0, 0.0, 0.0));
    assert_eq!(*frame.x_axis.as_raw(), Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(*frame.y_axis.as_raw(), Vector3::new(0.0, 1.0, 0.0));
    assert_eq!(*frame.z_axis.as_raw(), Vector3::new(0.0, 0.0, 1.0));
    assert!(ir.design.sketches[0].solve.state.is_none());
}

#[test]
fn arc_circle_and_point_keep_geometry_and_sketch_point_datum() {
    let mut ir = fixture();
    add_entity(
        &mut ir,
        "arc",
        Geometry::Arc {
            center: Point2::new(1.0, 2.0),
            radius: Length::new(3.0).expect("length"),
            start_angle: Angle::new(0.0).expect("angle"),
            end_angle: Angle::new(std::f64::consts::PI).expect("angle"),
        },
        false,
    );
    add_entity(
        &mut ir,
        "circle",
        Geometry::Circle {
            center: Point2::new(4.0, 5.0),
            radius: Length::new(2.0).expect("length"),
        },
        false,
    );
    add_entity(
        &mut ir,
        "point",
        Geometry::Point {
            position: Point2::new(2.0, 3.0),
        },
        false,
    );
    ir.model.features.push(feature(
        LegacyOperation::DatumPoint {
            position: FinitePoint3::new(Point3::new(2.0, 3.0, 0.0)).expect("point"),
            construction: Some(Box::new(DatumPointConstruction::SketchPoint {
                point: SketchPointSelection::Planar {
                    sketch: sketch_id(),
                    point: entity_id("point"),
                    native: "inventor:pmdc:entity#point".into(),
                },
            })),
        },
        "datum",
    ));
    run(&mut ir);
    let EntityDefinition::Free(PlanarCurve::Arc {
        center,
        radius,
        start,
        end,
    }) = &ir.design.sketch_entities[0].definition
    else {
        panic!("arc")
    };
    assert_eq!(center.get(), Point2::new(1.0, 2.0));
    assert_eq!(radius.get(), 3.0);
    assert_eq!(start.get(), 0.0);
    assert_eq!(end.get(), std::f64::consts::PI);
    let EntityDefinition::Free(PlanarCurve::Circle { center, radius }) =
        &ir.design.sketch_entities[1].definition
    else {
        panic!("circle")
    };
    assert_eq!(center.get(), Point2::new(4.0, 5.0));
    assert_eq!(radius.get(), 2.0);
    assert!(ir
        .design
        .sketch_entities
        .iter()
        .all(|entity| entity.construction.is_none()));
    let FeatureOperation::Datum(datum) = &ir
        .design
        .features
        .iter()
        .find(|item| item.id == ir.model.features[1].id)
        .expect("datum")
        .definition
        .operation
    else {
        panic!("datum")
    };
    let DatumElement::Points(points) = &datum.element else {
        panic!("points")
    };
    let cadmpeg_ir::design::construction::InputReference::Picks(selection) =
        points[0].inputs[0].reference.as_ref().expect("reference")
    else {
        panic!("picks")
    };
    let Picks::Named(picks) = &selection.picks else {
        panic!("named")
    };
    assert_eq!(
        picks.as_slice()[0].target,
        PickTarget::SketchEntity(entity_id("point"))
    );
}

#[test]
fn spline_keeps_knots_poles_weights_and_periodicity() {
    let mut ir = fixture();
    let geometry: cadmpeg_ir::sketches::SketchGeometry = json::from_value(cadmpeg_core::json!({
        "kind":"nurbs", "curve": {"degree":1, "knots":[0.0,0.0,1.0,1.0],
        "poles":{"kind":"rational", "points":[
            {"point":{"u":2.0,"v":3.0},"weight":1.0},
            {"point":{"u":4.0,"v":5.0},"weight":0.5}]}, "periodic":false}
    }))
    .expect("spline");
    ir.model
        .sketch_entities
        .push(cadmpeg_ir::sketches::SketchEntity::new(
            entity_id("spline"),
            sketch_id(),
            geometry,
        ));
    run(&mut ir);
    let EntityDefinition::Free(PlanarCurve::Nurbs(curve)) =
        &ir.design.sketch_entities[0].definition
    else {
        panic!("spline")
    };
    let wire = json::to_value(curve).expect("wire");
    assert_eq!(
        wire,
        cadmpeg_core::json!({"degree":1, "knots":[0.0,0.0,1.0,1.0],
        "poles":{"kind":"rational", "points":[
            {"point":{"u":2.0,"v":3.0},"weight":1.0},
            {"point":{"u":4.0,"v":5.0},"weight":0.5}]}, "periodic":false})
    );
}

#[test]
fn unread_placement_states_one_unread_support_input() {
    let mut ir = fixture();
    ir.model.sketches[0].placement = SketchPlacement::Unresolved {};
    run(&mut ir);
    let SketchPlane::Placed(frame) = &ir.design.sketches[0].plane else {
        panic!("placed")
    };
    assert!(frame.resolved.is_none());
    assert!(frame.after.is_empty());
    let [input] = frame.inputs.as_slice() else {
        panic!("one support input");
    };
    assert_eq!(input.role, None);
    assert_eq!(input.value, None);
    assert!(matches!(
        &input.reference,
        Some(InputReference::Picks(Selection {
            at: Anchor::Input,
            picks: Picks::Unread,
            native: None,
        }))
    ));
    let findings = cadmpeg_ir::validate::validate_neutral(&ir, Vec::new())
        .expect("validation")
        .findings
        .into_iter()
        .filter(|finding| finding.check == Check::Design)
        .map(|finding| finding.message)
        .collect::<Vec<_>>();
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn read_endpoint_references_bind_points_to_curve_roles() {
    let mut ir = fixture();
    add_entity(
        &mut ir,
        "start",
        Geometry::Point {
            position: Point2::new(0.0, 0.0),
        },
        false,
    );
    add_entity(
        &mut ir,
        "end",
        Geometry::Point {
            position: Point2::new(4.0, 0.0),
        },
        false,
    );
    add_entity(
        &mut ir,
        "line",
        Geometry::Line {
            start: Point2::new(0.0, 0.0),
            end: Point2::new(4.0, 0.0),
        },
        false,
    );
    ir.model.sketch_entities[2].endpoint_refs = vec![
        "inventor:pmdc:entity#start".into(),
        "inventor:pmdc:entity#end".into(),
    ];
    run(&mut ir);
    for (name, role) in [("start", DefiningRole::Start), ("end", DefiningRole::End)] {
        let point = ir
            .design
            .sketch_entities
            .iter()
            .find(|entity| entity.id == entity_id(name))
            .expect("point");
        assert_eq!(
            point.roles,
            vec![EntityRole {
                of: entity_id("line"),
                role
            }]
        );
    }
    let line = ir
        .design
        .sketch_entities
        .iter()
        .find(|entity| entity.id == entity_id("line"))
        .expect("line");
    assert!(line.roles.is_empty());
}

#[test]
fn shared_endpoint_roles_survive_legacy_arena_sorting() {
    let mut ir = fixture();
    add_entity(
        &mut ir,
        "joint",
        Geometry::Point {
            position: Point2::new(0.0, 0.0),
        },
        false,
    );
    for (name, start, end, refs) in [
        ("z", [-1.0, 0.0], [0.0, 0.0], ["other_z", "joint"]),
        ("a", [0.0, 0.0], [1.0, 0.0], ["joint", "other_a"]),
    ] {
        add_entity(
            &mut ir,
            name,
            Geometry::Line {
                start: Point2::new(start[0], start[1]),
                end: Point2::new(end[0], end[1]),
            },
            false,
        );
        ir.model
            .sketch_entities
            .last_mut()
            .expect("line")
            .endpoint_refs = Vec::from(refs.map(|name| format!("inventor:pmdc:entity#{name}")));
    }
    run(&mut ir);
    let joint = ir
        .design
        .sketch_entities
        .iter()
        .find(|entity| entity.id == entity_id("joint"))
        .expect("joint");
    assert_eq!(
        joint.roles,
        vec![
            EntityRole {
                of: entity_id("a"),
                role: DefiningRole::Start
            },
            EntityRole {
                of: entity_id("z"),
                role: DefiningRole::End
            },
        ]
    );
    ir.model.finalize();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    ir.native
        .namespace_mut(cadmpeg_ir::codec_id!("inventor"))
        .set_arena(
            &ctx,
            "pm_dc_features",
            Vec::<crate::feature::PmDcFeature>::new().as_slice(),
        )
        .expect("native arena");
    assert!(crate::design_v11::tests::validate_from_native(&ctx, &ir)
        .expect("validation")
        .is_empty());
}

#[test]
fn endpoint_roles_keep_reference_names_in_their_own_sketch() {
    let sketches = [
        sketch_id(),
        SketchId::mint("inventor:design:sketch#other").expect("id"),
    ];
    let entities = sketches
        .iter()
        .enumerate()
        .map(|(index, sketch)| {
            cadmpeg_ir::sketches::SketchEntity::new(
                entity_id(&format!("curve-{index}")),
                sketch.clone(),
                Geometry::Line {
                    start: Point2::new(0.0, 0.0),
                    end: Point2::new(1.0, 0.0),
                }
                .try_into()
                .expect("geometry"),
            )
            .with_endpoint_refs(vec!["shared-start".into(), "shared-end".into()])
        })
        .collect::<Vec<_>>();
    let roles = endpoint_roles(&entities);
    for (index, sketch) in sketches.iter().enumerate() {
        assert_eq!(
            roles[&(sketch, "shared-start")],
            vec![EntityRole {
                of: entity_id(&format!("curve-{index}")),
                role: DefiningRole::Start
            }]
        );
        assert_eq!(
            roles[&(sketch, "shared-end")],
            vec![EntityRole {
                of: entity_id(&format!("curve-{index}")),
                role: DefiningRole::End
            }]
        );
    }
}
