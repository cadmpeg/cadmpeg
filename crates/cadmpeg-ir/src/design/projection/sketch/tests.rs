// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::serde_erase::json;
use serde_json::json;

use super::{
    dimension, dimension_parameter, is_dimension, native_entity_kind, native_options,
    native_relation, planar_curve, stored_frame, DimensionValue, Loci, LocusPoints, StoredPoints,
};
use crate::design::construction::ScalarDimension;
use crate::design::dimension::Dimension;
use crate::design::ops::sketch::{
    DefiningRole, DistanceAxis, EntityDefinition, EntityRole, Operand, Planar, PlanarCurve,
    SketchRelation,
};
use crate::design::units::Quantity;
use crate::features::parameters::ParameterValue as StoredValue;
use crate::features::ParameterId;
use crate::scalar::{Angle, Length};
use crate::sketches::{
    SketchConstraintDefinitionInput as Kind, SketchEntity, SketchEntityId, SketchGeometry,
    SketchId, SketchLocus, SketchPlacement,
};

fn geometry(value: serde_json::Value) -> SketchGeometry {
    json::from_value(value).expect("valid geometry")
}

fn placement(value: serde_json::Value) -> SketchPlacement {
    json::from_value(value).expect("valid placement")
}

#[test]
fn a_reference_line_states_its_unit_direction() {
    let curve = planar_curve(&geometry(json!({"kind": "reference_line",
        "origin": {"u": 1.0, "v": 2.0}, "direction": {"u": 3.0, "v": 4.0}})));
    let Some(PlanarCurve::InfiniteLine { through, direction }) = curve else {
        panic!("an infinite line: {curve:?}");
    };
    let through = through.get();
    assert_eq!([through.u, through.v], [1.0, 2.0]);
    let direction = direction.get();
    assert!((direction[0] - 0.6).abs() < 1e-12 && (direction[1] - 0.8).abs() < 1e-12);
}

#[test]
fn an_ellipse_states_its_major_direction_from_its_angle() {
    let curve = planar_curve(&geometry(json!({"kind": "ellipse",
        "center": {"u": 0.0, "v": 0.0}, "major_angle": std::f64::consts::FRAC_PI_2,
        "major_radius": 2.0, "minor_radius": 1.0})));
    let Some(PlanarCurve::Ellipse { major, radii, .. }) = curve else {
        panic!("an ellipse: {curve:?}");
    };
    let major = major.get();
    assert!(major[0].abs() < 1e-12 && (major[1] - 1.0).abs() < 1e-12);
    assert_eq!(radii.map(crate::scalar::PositiveLength::get), [2.0, 1.0]);
}

#[test]
fn a_hyperbola_states_its_major_direction_and_radii() {
    let curve = planar_curve(&geometry(json!({"kind": "hyperbola",
        "center": {"u": 1.0, "v": 0.0}, "major_angle": std::f64::consts::FRAC_PI_2,
        "major_radius": 3.0, "minor_radius": 2.0, "bounds": [-1.0, 1.0]})));
    let Some(PlanarCurve::Hyperbola {
        major,
        radii,
        range,
        ..
    }) = curve
    else {
        panic!("a hyperbola: {curve:?}");
    };
    let major = major.get();
    assert!(major[0].abs() < 1e-12 && (major[1] - 1.0).abs() < 1e-12);
    assert_eq!(radii.map(crate::scalar::PositiveLength::get), [3.0, 2.0]);
    assert!(range.is_some());
}

#[test]
fn a_parabola_states_its_axis_and_focal_length() {
    let curve = planar_curve(&geometry(json!({"kind": "parabola",
        "vertex": {"u": 0.0, "v": 0.0}, "axis_angle": std::f64::consts::PI,
        "focal_length": 0.5, "bounds": [-2.0, 2.0]})));
    let Some(PlanarCurve::Parabola { axis, focal, .. }) = curve else {
        panic!("a parabola: {curve:?}");
    };
    let axis = axis.get();
    assert!((axis[0] + 1.0).abs() < 1e-12 && axis[1].abs() < 1e-12);
    assert_eq!(focal.get(), 0.5);
}

#[test]
fn a_native_geometry_has_no_planar_curve() {
    assert_eq!(
        planar_curve(&geometry(
            json!({"kind": "native", "native_kind": "test:text"})
        )),
        None
    );
}

#[test]
fn a_stored_frame_takes_its_axes_from_the_normal_and_u_axis() {
    let value = stored_frame(placement(json!({"kind": "resolved", "frame": {
        "origin": {"x": 1.0, "y": 2.0, "z": 3.0},
        "normal": {"x": 0.0, "y": 0.0, "z": 2.0},
        "u_axis": {"x": 3.0, "y": 0.0, "z": 0.0}}})))
    .expect("a frame");
    let y = value.y_axis.as_raw();
    assert!(y.x.abs() < 1e-12 && (y.y - 1.0).abs() < 1e-12 && y.z.abs() < 1e-12);
    assert_eq!(stored_frame(placement(json!({"kind": "unresolved"}))), None);
}

/// Two points: `a` at u = 20 and `b` at u = 5.
struct Points;

impl Loci for Points {
    fn operand(&self, locus: &SketchLocus) -> Option<Operand<Planar>> {
        match locus {
            SketchLocus::Entity(id) => Some(Operand::Entity(id.clone())),
            _ => None,
        }
    }

    fn position(&self, locus: &SketchLocus) -> Option<[f64; 2]> {
        match locus {
            SketchLocus::Entity(id) if id.as_str() == "test:model:sketch-entity#a" => {
                Some([20.0, 0.0])
            }
            SketchLocus::Entity(id) if id.as_str() == "test:model:sketch-entity#b" => {
                Some([5.0, 0.0])
            }
            _ => None,
        }
    }
}

fn kind(value: serde_json::Value) -> Kind {
    json::from_value(value).expect("valid relation")
}

#[test]
fn a_horizontal_distance_orders_its_points_by_their_u_coordinate() {
    let kind = kind(json!({"kind": "horizontal_distance",
        "first": {"kind": "entity", "value": "test:model:sketch-entity#a"},
        "second": {"kind": "entity", "value": "test:model:sketch-entity#b"},
        "parameter": "test:model:parameter#width"}));
    let value = DimensionValue::Stored(StoredValue::Length(Length::new(15.0).unwrap()));
    let point = |id: &str| Operand::Entity(SketchEntityId::mint(id).unwrap());
    assert_eq!(
        dimension(&kind, &value, &Points),
        Some(SketchRelation::Distance {
            a: point("test:model:sketch-entity#b"),
            b: point("test:model:sketch-entity#a"),
            along: Some(DistanceAxis::Horizontal),
            doubled: None,
            value: Dimension::Literal(Length::new(15.0).unwrap()),
        })
    );
}

#[test]
fn a_stored_value_of_another_quantity_states_no_dimension() {
    let kind = kind(
        json!({"kind": "radius", "entity": "test:model:sketch-entity#a", "parameter": "test:model:parameter#r"}),
    );
    let angle = DimensionValue::Stored(StoredValue::Angle(Angle::new(1.0).unwrap()));
    assert_eq!(dimension(&kind, &angle, &Points), None);
}

#[test]
fn a_native_relation_and_entity_are_named_in_the_codec_namespace() {
    let distance = kind(
        json!({"kind": "distance", "entities": ["test:model:sketch-entity#a", "test:model:sketch-entity#a"],
        "parameter": "test:model:parameter#d"}),
    );
    let SketchRelation::Native {
        kind,
        operands,
        values,
    } = native_relation(
        &cadmpeg_core::nonblank_literal!("f3d"),
        &distance,
        Vec::new(),
        &Points,
    )
    else {
        panic!("a native relation");
    };
    assert_eq!(kind.as_str(), "f3d:distance");
    assert_eq!(
        operands,
        vec![Operand::Entity(
            SketchEntityId::mint("test:model:sketch-entity#a").unwrap()
        )]
    );
    assert!(values.is_empty());
    let text = geometry(json!({"kind": "native", "native_kind": "f3d:SketchText"}));
    assert_eq!(
        native_entity_kind(&cadmpeg_core::nonblank_literal!("f3d"), &text).as_str(),
        "f3d:SketchText"
    );
}

#[test]
fn a_native_relation_that_names_a_parameter_is_a_dimension_of_its_quantity() {
    let native = kind(json!({"kind": "native", "native_kind": "test:Span",
        "native_properties": {}, "entities": ["test:model:sketch-entity#a"],
        "parameter": "test:model:parameter#span", "operands": []}));
    assert!(is_dimension(&native));
    let (parameter, quantity) = dimension_parameter(&native, |class, _| {
        (class == "test:Span").then_some(Quantity::LENGTH)
    })
    .expect("a dimension");
    assert_eq!(parameter.as_str(), "test:model:parameter#span");
    assert_eq!(quantity, Quantity::LENGTH);
    let value = DimensionValue::Parameter(ParameterId::mint("test:model:parameter#span").unwrap());
    let SketchRelation::Native { kind, values, .. } = native_relation(
        &cadmpeg_core::nonblank_literal!("test"),
        &native,
        value.scalar(quantity).into_iter().collect(),
        &Points,
    ) else {
        panic!("a native relation");
    };
    assert_eq!(kind.as_str(), "test:Span");
    assert_eq!(
        values,
        vec![ScalarDimension::Length(Dimension::Parameter(
            ParameterId::mint("test:model:parameter#span").unwrap()
        ))]
    );
    let unnamed = kind_without_parameter();
    assert!(!is_dimension(&unnamed));
}

#[test]
fn a_native_relation_states_its_state_flags_and_properties_as_native_options() {
    let native = kind(json!({"kind": "native", "native_kind": "test:Span",
        "native_state": 4, "native_flags": u64::MAX,
        "native_properties": {"operand.0.class-tag": "line", "blank": " "},
        "entities": [], "operands": []}));
    let options = native_options(&cadmpeg_core::nonblank_literal!("test"), &native)
        .iter()
        .map(|(name, value)| (name.as_str().to_owned(), value.clone()))
        .collect::<Vec<_>>();
    let text = |value: &str| {
        crate::design::options::NativeOption::Text(Dimension::Literal(
            cadmpeg_core::text::NonBlankString::new(value.to_owned()).unwrap(),
        ))
    };
    assert_eq!(
        options,
        [
            ("test:native_flags".to_owned(), text(&u64::MAX.to_string())),
            (
                "test:native_state".to_owned(),
                crate::design::options::NativeOption::Integer(Dimension::Literal(4)),
            ),
            ("test:operand.0.class-tag".to_owned(), text("line")),
        ]
    );
    assert!(native_options(
        &cadmpeg_core::nonblank_literal!("test"),
        &kind(json!({"kind": "horizontal", "entity": "test:model:sketch-entity#l"}))
    )
    .is_empty());
}

fn kind_without_parameter() -> Kind {
    kind(json!({"kind": "native", "native_kind": "test:Span",
        "native_properties": {}, "entities": [], "operands": []}))
}

#[test]
fn a_relation_on_a_line_end_names_a_point_minted_there_with_its_role() {
    let sketch = SketchId::mint("test:model:sketch#s").unwrap();
    let line = SketchEntity::new(
        SketchEntityId::mint("test:model:sketch-entity#l").unwrap(),
        sketch.clone(),
        geometry(json!({"kind": "line",
            "start": {"u": 1.0, "v": 2.0}, "end": {"u": 4.0, "v": 6.0}})),
    );
    let point = SketchEntity::new(
        SketchEntityId::mint("test:model:sketch-entity#p").unwrap(),
        sketch.clone(),
        geometry(json!({"kind": "point", "position": {"u": 4.0, "v": 6.0}})),
    );
    let on = kind(json!({"kind": "coincident_loci", "loci": [
        {"kind": "end", "value": "test:model:sketch-entity#l"},
        {"kind": "entity", "value": "test:model:sketch-entity#p"}]}));
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let stored = StoredPoints::new(&ctx, &[&line, &point], |_, _, _| None, |a, b| a == b)
        .expect("the session admits the evaluation");
    let points = LocusPoints::new(&ctx, [&on], &[&line, &point], &stored)
        .expect("the session admits the evaluation");
    let end = SketchLocus::End(line.id().clone());
    let minted = SketchEntityId::mint("test:model:sketch-entity#l:end").unwrap();
    assert_eq!(points.operand(&end), Some(Operand::Entity(minted.clone())));
    assert_eq!(points.position(&end), Some([4.0, 6.0]));
    assert!(!points.unplaced(&on));
    let entities = points.entities(&sketch);
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].id, minted);
    assert_eq!(
        entities[0].roles,
        vec![EntityRole {
            of: line.id().clone(),
            role: DefiningRole::End,
        }]
    );
    assert!(matches!(
        entities[0].definition,
        EntityDefinition::Free(PlanarCurve::Point(_))
    ));
}

#[test]
fn a_relation_on_the_end_of_an_unbounded_curve_is_unplaced() {
    let sketch = SketchId::mint("test:model:sketch#s").unwrap();
    let circle = SketchEntity::new(
        SketchEntityId::mint("test:model:sketch-entity#c").unwrap(),
        sketch.clone(),
        geometry(json!({"kind": "circle", "center": {"u": 0.0, "v": 0.0}, "radius": 2.0})),
    );
    let on = kind(json!({"kind": "point_on_object",
        "point": {"kind": "start", "value": "test:model:sketch-entity#c"},
        "entity": "test:model:sketch-entity#c"}));
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let none = StoredPoints::default();
    let points = LocusPoints::new(&ctx, [&on], &[&circle], &none)
        .expect("the session admits the evaluation");
    assert!(points.unplaced(&on));
    assert!(points.entities(&sketch).is_empty());
    let center = kind(json!({"kind": "coincident_loci", "loci": [
        {"kind": "center", "value": "test:model:sketch-entity#c"},
        {"kind": "entity", "value": "test:model:sketch-entity#c"}]}));
    let centers = LocusPoints::new(&ctx, [&center], &[&circle], &none)
        .expect("the session admits the evaluation");
    assert_eq!(
        centers.position(&SketchLocus::Center(circle.id().clone())),
        Some([0.0, 0.0])
    );
}

fn entity(id: &str, value: serde_json::Value) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("test:model:sketch-entity#{id}")).unwrap(),
        SketchId::mint("test:model:sketch#s").unwrap(),
        geometry(value),
    )
}

#[test]
fn a_relation_on_a_stored_end_or_center_names_the_stored_point_with_its_roles() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let line = entity(
        "l",
        json!({"kind": "line", "start": {"u": 0.0, "v": 0.0}, "end": {"u": 2.0, "v": 0.0}}),
    );
    let arc = entity(
        "a",
        json!({"kind": "arc", "center": {"u": 0.0, "v": 0.0}, "radius": 2.0,
            "start_angle": 0.0, "end_angle": 1.5}),
    );
    let origin = entity(
        "o",
        json!({"kind": "point", "position": {"u": 0.0, "v": 0.0}}),
    );
    let shared = entity(
        "e",
        json!({"kind": "point", "position": {"u": 2.0, "v": 0.0}}),
    );
    let entities = [&line, &arc, &origin, &shared];
    let stored = StoredPoints::new(&ctx, &entities, |_, _, _| Some(0), |a, b| a == b)
        .expect("the session admits the evaluation");
    let relations = [
        kind(json!({"kind": "coincident_loci", "loci": [
            {"kind": "start", "value": "test:model:sketch-entity#l"},
            {"kind": "center", "value": "test:model:sketch-entity#a"}]})),
        kind(json!({"kind": "coincident_loci", "loci": [
            {"kind": "end", "value": "test:model:sketch-entity#l"},
            {"kind": "start", "value": "test:model:sketch-entity#a"}]})),
    ];
    let points = LocusPoints::new(&ctx, &relations, &entities, &stored)
        .expect("the session admits the evaluation");
    let operand = |locus| points.operand(&locus);
    assert_eq!(
        operand(SketchLocus::Start(line.id().clone())),
        Some(Operand::Entity(origin.id().clone()))
    );
    assert_eq!(
        operand(SketchLocus::Center(arc.id().clone())),
        Some(Operand::Entity(origin.id().clone()))
    );
    assert_eq!(
        operand(SketchLocus::Start(arc.id().clone())),
        Some(Operand::Entity(shared.id().clone()))
    );
    assert!(points
        .entities(&SketchId::mint("test:model:sketch#s").unwrap())
        .is_empty());
    let role = |of: &SketchEntity, role| EntityRole {
        of: of.id().clone(),
        role,
    };
    assert_eq!(
        stored.roles(origin.id()),
        vec![
            role(&arc, DefiningRole::Center),
            role(&line, DefiningRole::Start)
        ]
    );
    assert_eq!(
        stored.roles(shared.id()),
        vec![
            role(&arc, DefiningRole::Start),
            role(&line, DefiningRole::End)
        ]
    );
}

#[test]
fn two_stored_points_at_one_end_leave_the_end_ambiguous_and_to_a_minted_point() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let line = entity(
        "l",
        json!({"kind": "line", "start": {"u": 0.0, "v": 0.0}, "end": {"u": 2.0, "v": 0.0}}),
    );
    let first = entity(
        "p",
        json!({"kind": "point", "position": {"u": 2.0, "v": 0.0}}),
    );
    let second = entity(
        "q",
        json!({"kind": "point", "position": {"u": 2.0, "v": 0.0}}),
    );
    let entities = [&line, &first, &second];
    let stored = StoredPoints::new(&ctx, &entities, |_, _, _| Some(0), |a, b| a == b)
        .expect("the session admits the evaluation");
    assert!(stored.at(&SketchLocus::End(line.id().clone())).is_none());
    assert!(stored.roles(first.id()).is_empty());
    assert_eq!(
        stored.ambiguous().collect::<Vec<_>>(),
        [(
            line.id(),
            "end",
            &[first.id().clone(), second.id().clone()][..]
        )]
    );
    let on = kind(json!({"kind": "point_on_object",
        "point": {"kind": "end", "value": "test:model:sketch-entity#l"},
        "entity": "test:model:sketch-entity#l"}));
    let points = LocusPoints::new(&ctx, [&on], &entities, &stored)
        .expect("the session admits the evaluation");
    let minted = SketchEntityId::mint("test:model:sketch-entity#l:end").unwrap();
    assert_eq!(
        points.operand(&SketchLocus::End(line.id().clone())),
        Some(Operand::Entity(minted))
    );
}

#[test]
fn a_point_of_a_first_standing_holds_the_end_before_a_point_of_a_later_one() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let line = entity(
        "l",
        json!({"kind": "line", "start": {"u": 0.0, "v": 0.0}, "end": {"u": 2.0, "v": 0.0}}),
    );
    let named = entity(
        "p",
        json!({"kind": "point", "position": {"u": 2.0, "v": 0.0}}),
    );
    let loose = entity(
        "q",
        json!({"kind": "point", "position": {"u": 2.0, "v": 0.0}}),
    );
    let elsewhere = entity(
        "r",
        json!({"kind": "point", "position": {"u": 0.0, "v": 0.0}}),
    );
    let entities = [&line, &named, &loose, &elsewhere];
    let stored = StoredPoints::new(
        &ctx,
        &entities,
        |_, _, point| match point.id().as_str() {
            "test:model:sketch-entity#p" => Some(0),
            "test:model:sketch-entity#q" => Some(1),
            _ => None,
        },
        |a, b| a == b,
    )
    .expect("the session admits the evaluation");
    assert_eq!(
        stored.at(&SketchLocus::End(line.id().clone())),
        Some(named.id())
    );
    assert!(stored.at(&SketchLocus::Start(line.id().clone())).is_none());
    assert!(stored.roles(loose.id()).is_empty());
    assert_eq!(stored.ambiguous().count(), 0);
}

#[test]
fn a_hyperbola_and_a_parabola_place_their_ends_and_a_hyperbola_its_center() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let hyperbola = entity(
        "h",
        json!({"kind": "hyperbola", "center": {"u": 1.0, "v": 2.0},
            "major_angle": std::f64::consts::FRAC_PI_2, "major_radius": 2.0,
            "minor_radius": 3.0, "bounds": [0.0, 1.0]}),
    );
    let parabola = entity(
        "b",
        json!({"kind": "parabola", "vertex": {"u": 0.0, "v": 0.0}, "axis_angle": 0.0,
            "focal_length": 0.5, "bounds": [-1.0, 2.0]}),
    );
    let on = kind(json!({"kind": "coincident_loci", "loci": [
        {"kind": "start", "value": "test:model:sketch-entity#h"},
        {"kind": "end", "value": "test:model:sketch-entity#h"},
        {"kind": "center", "value": "test:model:sketch-entity#h"},
        {"kind": "start", "value": "test:model:sketch-entity#b"},
        {"kind": "end", "value": "test:model:sketch-entity#b"}]}));
    let points = LocusPoints::new(
        &ctx,
        [&on],
        &[&hyperbola, &parabola],
        &StoredPoints::default(),
    )
    .expect("the session admits the evaluation");
    assert!(!points.unplaced(&on));
    let close = |locus, [u, v]: [f64; 2]| {
        let [pu, pv] = points.position(&locus).expect("a position");
        assert!(
            (pu - u).abs() < 1e-12 && (pv - v).abs() < 1e-12,
            "{pu} {pv}"
        );
    };
    let h = || hyperbola.id().clone();
    let b = || parabola.id().clone();
    close(SketchLocus::Center(h()), [1.0, 2.0]);
    close(SketchLocus::Start(h()), [1.0, 4.0]);
    close(
        SketchLocus::End(h()),
        [1.0 - 3.0 * 1.0_f64.sinh(), 2.0 + 2.0 * 1.0_f64.cosh()],
    );
    close(SketchLocus::Start(b()), [0.5, -1.0]);
    close(SketchLocus::End(b()), [2.0, 2.0]);
}

#[test]
fn a_relation_on_a_nurbs_end_names_a_point_at_the_end_of_its_knot_domain() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let curve = crate::geometry::pcurve::PcurveNurbs::from_lanes(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            crate::math::Point2::new(0.0, 0.0),
            crate::math::Point2::new(1.0, 2.0),
            crate::math::Point2::new(3.0, 1.0),
        ],
        Some(vec![1.0, 2.0, 1.0]),
        false,
    )
    .unwrap();
    let nurbs = SketchEntity::new(
        SketchEntityId::mint("test:model:sketch-entity#n").unwrap(),
        SketchId::mint("test:model:sketch#s").unwrap(),
        SketchGeometry::nurbs(curve),
    );
    let on = kind(json!({"kind": "coincident_loci", "loci": [
        {"kind": "start", "value": "test:model:sketch-entity#n"},
        {"kind": "end", "value": "test:model:sketch-entity#n"}]}));
    let points = LocusPoints::new(&ctx, [&on], &[&nurbs], &StoredPoints::default())
        .expect("the session admits the evaluation");
    assert!(!points.unplaced(&on));
    let close = |locus, [u, v]: [f64; 2]| {
        let [pu, pv] = points.position(&locus).expect("a position");
        assert!(
            (pu - u).abs() < 1e-12 && (pv - v).abs() < 1e-12,
            "{pu} {pv}"
        );
    };
    close(SketchLocus::Start(nurbs.id().clone()), [0.0, 0.0]);
    close(SketchLocus::End(nurbs.id().clone()), [3.0, 1.0]);
    assert_eq!(
        points
            .entities(&SketchId::mint("test:model:sketch#s").unwrap())
            .len(),
        2
    );
}
