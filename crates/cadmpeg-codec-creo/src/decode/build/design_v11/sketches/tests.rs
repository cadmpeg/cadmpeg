// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json;

use super::*;

const SKETCH: &str = "creo:model:sketch#917";

fn entity(key: &str, geometry: &serde_json::Value, endpoints: &[u32]) -> LegacyEntity {
    json::from_value(json!({
        "id": format!("creo:model:sketch_entity#917:{key}"),
        "sketch": SKETCH,
        "construction": false,
        "endpoint_refs": endpoints
            .iter()
            .map(|point| format!("creo:featdefs:sketch#917:point#{point}"))
            .collect::<Vec<_>>(),
        "geometry": geometry,
    }))
    .expect("v10 sketch entity")
}

fn id(key: &str) -> SketchEntityId {
    SketchEntityId::mint(format!("creo:model:sketch_entity#917:{key}")).expect("entity id")
}

fn input(definition: serde_json::Value) -> Input {
    json::from_value(definition).expect("v10 constraint")
}

fn parameter(key: &str) -> ParameterId {
    ParameterId::mint(format!("creo:featdefs:parameter#917:{key}")).expect("parameter id")
}

/// A line from (0, 0) to (10, 0), a quarter arc about (10, 5) from (10, 0)
/// to (15, 5), and the point record that both share.
fn section() -> Vec<LegacyEntity> {
    vec![
        entity(
            "line",
            &json!({"kind": "line", "start": {"u": 0.0, "v": 0.0}, "end": {"u": 10.0, "v": 0.0}}),
            &[1, 2],
        ),
        entity(
            "arc",
            &json!({
                "kind": "arc",
                "center": {"u": 10.0, "v": 5.0},
                "radius": 5.0,
                "start_angle": -std::f64::consts::FRAC_PI_2,
                "end_angle": 0.0,
            }),
            &[2, 3],
        ),
        entity(
            "joint",
            &json!({"kind": "point", "position": {"u": 10.0, "v": 0.0}}),
            &[2],
        ),
    ]
}

fn sketch() -> LegacySketch {
    json::from_value(json!({
        "id": SKETCH,
        "native_ref": "creo:featdefs:sketch#917",
        "placement": {"kind": "unresolved"},
        "profiles": [],
    }))
    .expect("v10 sketch")
}

fn section_points() -> Points {
    let section = section();
    Points::new(&sketch(), section.iter()).expect("points")
}

fn kinds() -> BTreeMap<ParameterId, ValueKind> {
    BTreeMap::from([
        (parameter("length"), ValueKind::Quantity(Quantity::LENGTH)),
        (parameter("angle"), ValueKind::Quantity(Quantity::ANGLE)),
    ])
}

#[test]
fn a_shared_point_record_is_one_point_entity_with_a_role_per_curve() {
    let mut points = section_points();
    let joint = id("joint");
    assert_eq!(
        points.point(&SketchLocus::End(id("line"))),
        Some(joint.clone())
    );
    assert_eq!(
        points.point(&SketchLocus::Start(id("arc"))),
        Some(joint.clone())
    );
    assert_eq!(
        points.roles[&joint],
        [
            EntityRole {
                of: id("line"),
                role: DefiningRole::End
            },
            EntityRole {
                of: id("arc"),
                role: DefiningRole::Start
            },
        ]
    );
    let start = points
        .point(&SketchLocus::Start(id("line")))
        .expect("minted start");
    assert_eq!(start.as_str(), "creo:model:sketch_point#917/1");
    assert_eq!(
        points.position(&start),
        FinitePoint2::new(Point2 { u: 0.0, v: 0.0 })
    );
    let end = points.point(&SketchLocus::End(id("arc"))).expect("arc end");
    let at = points.position(&end).expect("arc end position");
    assert!((at.get().u - 15.0).abs() < 1e-12 && (at.get().v - 5.0).abs() < 1e-12);
}

#[test]
fn a_centre_is_minted_once_with_its_role() {
    let mut points = section_points();
    let center = points
        .point(&SketchLocus::Center(id("arc")))
        .expect("centre");
    assert_eq!(
        points.point(&SketchLocus::Center(id("arc"))),
        Some(center.clone())
    );
    assert_eq!(
        points.roles[&center],
        [EntityRole {
            of: id("arc"),
            role: DefiningRole::Center
        }]
    );
    assert_eq!(points.point(&SketchLocus::Center(id("line"))), None);
}

#[test]
fn geometric_relations_keep_their_meaning() {
    assert_eq!(
        relation_of(json!({"kind": "horizontal", "entity": id("line")})),
        SketchRelation::Horizontal {
            a: Operand::Entity(id("line")),
            b: None
        }
    );
    assert_eq!(
        relation_of(json!({
            "kind": "tangent_loci",
            "first": {"kind": "end", "value": id("line")},
            "second": {"kind": "start", "value": id("arc")},
        })),
        SketchRelation::Tangent {
            a: Operand::Entity(id("line")),
            b: Operand::Entity(id("arc")),
            contact: Some(Contact::Ends([id("joint"), id("joint")])),
        }
    );
    let mut points = section_points();
    assert_eq!(
        relation(&input(json!({"kind": "disabled"})), &mut points, &kinds()).expect("relation"),
        None,
        "a disabled slot states no relation"
    );
}

fn relation_of(definition: serde_json::Value) -> SketchRelation<Planar> {
    let mut points = section_points();
    relation(&input(definition), &mut points, &kinds())
        .expect("relation")
        .expect("a stated relation")
}

#[test]
fn dimensions_bind_parameters_of_their_quantity() {
    assert_eq!(
        relation_of(json!({
            "kind": "distance_loci",
            "first": {"kind": "start", "value": id("line")},
            "second": {"kind": "end", "value": id("arc")},
            "parameter": parameter("length"),
        })),
        SketchRelation::Distance {
            a: Operand::Entity(
                SketchEntityId::mint("creo:model:sketch_point#917/1").expect("start")
            ),
            b: Operand::Entity(SketchEntityId::mint("creo:model:sketch_point#917/3").expect("end")),
            along: Some(DistanceAxis::Aligned),
            doubled: None,
            value: Dimension::Parameter(parameter("length")),
        }
    );
    assert_eq!(
        relation_of(json!({
            "kind": "diameter",
            "entity": id("arc"),
            "parameter": parameter("length"),
        })),
        SketchRelation::Radial {
            of: id("arc"),
            measure: RadialMeasure::Diameter,
            value: Dimension::Parameter(parameter("length")),
        }
    );
    let SketchRelation::Native { kind, values, .. } = relation_of(json!({
        "kind": "radius",
        "entity": id("arc"),
        "parameter": parameter("angle"),
    })) else {
        panic!("a radius bound to an angle is no radial dimension");
    };
    assert_eq!(kind.as_str(), "creo:radius");
    assert_eq!(
        values,
        [ScalarDimension::Angle(Dimension::Parameter(parameter(
            "angle"
        )))]
    );
}

#[test]
fn a_horizontal_distance_runs_from_the_lesser_coordinate() {
    let SketchRelation::Distance { a, b, along, .. } = relation_of(json!({
        "kind": "horizontal_distance",
        "first": {"kind": "end", "value": id("arc")},
        "second": {"kind": "start", "value": id("line")},
        "parameter": parameter("length"),
    })) else {
        panic!("a horizontal distance");
    };
    assert_eq!(along, Some(DistanceAxis::Horizontal));
    assert_eq!(
        a,
        Operand::Entity(SketchEntityId::mint("creo:model:sketch_point#917/1").expect("start"))
    );
    assert_eq!(
        b,
        Operand::Entity(SketchEntityId::mint("creo:model:sketch_point#917/3").expect("end"))
    );
}

#[test]
fn a_relation_with_no_v11_relation_of_its_meaning_stays_native() {
    let SketchRelation::Native {
        kind,
        operands,
        values,
    } = relation_of(json!({
        "kind": "point_coordinate_values",
        "point": {"kind": "start", "value": id("line")},
        "values": [1.0, 2.0],
    }))
    else {
        panic!("a native relation");
    };
    assert_eq!(kind.as_str(), "creo:point_coordinate_values");
    assert_eq!(
        operands,
        [Operand::Entity(
            SketchEntityId::mint("creo:model:sketch_point#917/1").expect("start")
        )]
    );
    assert_eq!(values.len(), 2);
}

#[test]
fn a_stored_frame_is_the_resolved_plane_and_an_unread_one_states_none() {
    let owner = FeatureId::mint("creo:model:sketch_feature#917").expect("owner");
    let placed: LegacySketch = json::from_value(json!({
        "id": SKETCH,
        "placement": {
            "kind": "resolved",
            "frame": {
                "origin": {"x": 1.0, "y": 2.0, "z": 3.0},
                "normal": {"x": 0.0, "y": 0.0, "z": 2.0},
                "u_axis": {"x": 3.0, "y": 0.0, "z": 0.0},
            },
        },
        "profiles": [],
    }))
    .expect("v10 sketch");
    let SketchPlane::Placed(plane) = project_sketch(&placed, &owner).plane else {
        panic!("a placed plane");
    };
    assert!(plane.inputs.is_empty());
    let frame = plane.resolved.expect("stored frame");
    assert_eq!(frame.x_axis, UnitVector3::X_AXIS);
    assert_eq!(frame.y_axis, UnitVector3::Y_AXIS);
    assert_eq!(frame.z_axis, UnitVector3::Z_AXIS);
    let unplaced: LegacySketch = json::from_value(json!({
        "id": SKETCH,
        "placement": {"kind": "unresolved"},
        "profiles": [],
    }))
    .expect("v10 sketch");
    let SketchPlane::Placed(plane) = project_sketch(&unplaced, &owner).plane else {
        panic!("a placed plane");
    };
    assert_eq!(plane.resolved, None);
    assert!(plane.after.is_empty());
    let [input] = plane.inputs.as_slice() else {
        panic!("one support input");
    };
    assert_eq!(input.role, None);
    assert_eq!(input.value, None);
    assert!(matches!(
        &input.reference,
        Some(InputReference::Picks(Selection {
            at: Anchor::Input,
            picks: Picks::Unread,
            ..
        }))
    ));
}

#[test]
fn the_ends_and_centre_of_an_unread_curve_are_points_with_no_position() {
    let unread = [entity(
        "native_arc",
        &json!({"kind": "native", "native_kind": "arc"}),
        &[4, 5],
    )];
    let mut points = Points::new(&sketch(), unread.iter()).expect("points");
    let start = points
        .point(&SketchLocus::Start(id("native_arc")))
        .expect("start");
    let center = points
        .point(&SketchLocus::Center(id("native_arc")))
        .expect("centre");
    for point in [&start, &center] {
        assert_eq!(points.position(point), None);
        assert_eq!(
            points.minted[point].definition,
            EntityDefinition::Defined {
                form: Box::new(EntityForm::Native(
                    NonBlankString::new("creo:point").expect("kind")
                )),
                solved: None,
            }
        );
    }
    assert_eq!(
        points.roles[&center],
        [EntityRole {
            of: id("native_arc"),
            role: DefiningRole::Center
        }]
    );
}

#[test]
fn a_saved_entity_names_the_record_of_its_sketch() {
    assert_eq!(
        record(
            &sketch(),
            Some(&"creo:featdefs:sketch#917:saved_entity#2".to_owned())
        ),
        Some("creo:featdefs:sketch#917".to_owned())
    );
    assert_eq!(
        record(&sketch(), Some(&"creo:featdefs:sketch#9170".to_owned())),
        Some("creo:featdefs:sketch#9170".to_owned())
    );
}

#[test]
fn a_kind_the_decoder_names_in_the_creo_namespace_keeps_its_name() {
    assert_eq!(creo_kind("creo:segtab:verhor"), "creo:segtab:verhor");
    assert_eq!(creo_kind("radius"), "creo:radius");
}
