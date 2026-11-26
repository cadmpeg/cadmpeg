// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::serde_erase::json;
use serde_json::json;

use super::{
    dimension, native_entity_kind, native_relation, planar_curve, stored_frame, DimensionValue,
    Loci,
};
use crate::design::dimension::Dimension;
use crate::design::ops::sketch::{DistanceAxis, Operand, Planar, PlanarCurve, SketchRelation};
use crate::features::parameters::ParameterValue as StoredValue;
use crate::scalar::{Angle, Length};
use crate::sketches::{
    SketchConstraintDefinitionInput as Kind, SketchEntityId, SketchGeometry, SketchLocus,
    SketchPlacement,
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
    } = native_relation("f3d", &distance, Vec::new())
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
    assert_eq!(native_entity_kind("f3d", &text).as_str(), "f3d:SketchText");
}
