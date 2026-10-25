// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::brep::{records::payloads_untracked, TextEdgeRepresentation, TextOrientation};

fn payload() -> ShapePayloadRecord {
    let source = super::super::fixtures::archive(false, &super::super::fixtures::member(false));
    let decoded = super::super::decode(&source);
    payloads_untracked(decoded.ir().native.namespace("fcstd").unwrap())
        .unwrap()
        .remove(0)
}

#[test]
fn source_slot_normalization_keeps_flags_variants_and_child_orientation_exact() {
    let payload = payload();
    let slots = Slots::new(&payload);
    let original = slots.tables.tshapes.resolve(3).unwrap().clone();
    let expected = project(&slots, &slots, &original, false).unwrap();
    let mut changed = original.clone();
    changed.flags[0] = !changed.flags[0];
    assert_ne!(Some(expected), project(&slots, &slots, &changed, false));
    let mut changed = original.clone();
    changed.geometry = TextTShapeGeometry::Shell;
    assert_ne!(
        project(&slots, &slots, &original, false),
        project(&slots, &slots, &changed, false)
    );
    let mut original = original;
    original.children.push(TextShapeUse {
        shape: 1,
        location: 0.into(),
        orientation: TextOrientation::Forward,
    });
    let mut changed = original.clone();
    changed.children[0].orientation = TextOrientation::Reversed;
    assert_ne!(
        project(&slots, &slots, &original, false),
        project(&slots, &slots, &changed, false)
    );
}

#[test]
fn missing_curve_prunes_only_its_representation_and_preserves_other_source_slots() {
    let original = payload();
    let mut wire = serde_json::to_value(&original).unwrap();
    wire["text"]["curves"].as_array_mut().unwrap().remove(0);
    wire["text"]["source_indices"]["2"] = serde_json::json!([2, 3]);
    let surviving = serde_json::from_value::<ShapePayloadRecord>(wire).unwrap();
    let original = Slots::new(&original);
    let surviving = Slots::new(&surviving);
    let shape = TextTShape {
        geometry: TextTShapeGeometry::Edge {
            tolerance: FiniteReal::ZERO,
            same_parameter: true,
            same_range: false,
            degenerated: false,
            representations: [1, 2]
                .into_iter()
                .map(|curve| TextEdgeRepresentation::Curve3d {
                    curve,
                    location: 0,
                    parameter_range: [FiniteReal::ZERO; 2],
                })
                .collect(),
        },
        flags: [false; 7],
        children: Vec::new(),
    };
    let projected = project(&original, &surviving, &shape, true).unwrap();
    let Geometry::Ordinary(TextTShapeGeometry::Edge {
        representations,
        same_parameter,
        same_range,
        ..
    }) = &projected.geometry
    else {
        panic!("edge family must survive its optional representation-list pruning")
    };
    assert!(*same_parameter);
    assert!(!*same_range);
    assert_eq!(representations.len(), 1);
    assert!(matches!(
        representations[0],
        TextEdgeRepresentation::Curve3d { curve: 2, .. }
    ));
    // The current dense index 1 names original source slot 2, not the lost slot 1.
    let mut current = shape;
    if let TextTShapeGeometry::Edge {
        representations, ..
    } = &mut current.geometry
    {
        representations.remove(0);
        if let TextEdgeRepresentation::Curve3d { curve, .. } = &mut representations[0] {
            *curve = 1;
        }
    }
    let actual = project(&surviving, &surviving, &current, false).unwrap();
    assert_eq!(projected, actual);
}

#[test]
fn a_missing_required_face_surface_drops_only_the_affected_shape() {
    let original = payload();
    let mut wire = serde_json::to_value(&original).unwrap();
    wire["text"]["surfaces"].as_array_mut().unwrap().remove(0);
    wire["text"]["source_indices"]["5"] = serde_json::json!([2, 3]);
    let surviving = serde_json::from_value::<ShapePayloadRecord>(wire).unwrap();
    let original = Slots::new(&original);
    let surviving = Slots::new(&surviving);
    let face = |surface| {
        let shapes = serde_json::from_value::<crate::brep::TextTShapes>(serde_json::json!([{
            "index":1,"kind":"face","flags":[false,true,false,true,false,true,false],"children":[],
            "geometry":{"kind":"face","natural_restriction":true,"tolerance":0.25,
                "surface":surface,"location":0,"triangulation":null}
        }]))
        .unwrap();
        shapes.resolve(1).unwrap().clone()
    };
    assert!(project(&original, &surviving, &face(1), true).is_none());
    let expected = project(&original, &surviving, &face(2), true).unwrap();
    let actual = project(&surviving, &surviving, &face(1), false).unwrap();
    assert_eq!(expected, actual);
}
