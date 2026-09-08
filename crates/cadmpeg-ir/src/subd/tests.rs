// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::ids::SubdId;
use crate::math::{Point3, Vector3};
use crate::subd::{
    SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdGripWedge, SubdPlaneFrame,
    SubdRadialMapSelector, SubdRadialSymmetryMap, SubdScheme, SubdSurface, SubdSymmetry,
    SubdSymmetryKind, SubdVertex, SubdVertexTag,
};
use crate::validate::validate_neutral;
use crate::CadIr;

#[test]
fn subd_round_trip_and_directed_ring_validation() {
    let mut ir = CadIr::empty();
    ir.model.subds.push(SubdSurface {
        id: SubdId::mint("synthetic:subd:surface#0").expect("valid identity"),
        scheme: SubdScheme::CatmullClark,
        source_object: None,
        cage: crate::subd::SubdCage::new(
            vec![
                SubdVertex {
                    point: Point3::new(0.0, 0.0, 0.0),
                    tag: SubdVertexTag::Smooth,
                    secondary_grips: None,
                },
                SubdVertex {
                    point: Point3::new(1.0, 0.0, 0.0),
                    tag: SubdVertexTag::Smooth,
                    secondary_grips: None,
                },
                SubdVertex {
                    point: Point3::new(0.0, 1.0, 0.0),
                    tag: SubdVertexTag::Smooth,
                    secondary_grips: None,
                },
            ],
            vec![
                SubdEdge::new([0, 1], [0.0, 0.25], SubdEdgeTag::Smooth, None, [1.0, 1.0]).unwrap(),
                SubdEdge::new([1, 2], [0.25, 0.0], SubdEdgeTag::SmoothX, None, [1.0, 1.0]).unwrap(),
                SubdEdge::new([2, 0], [0.0, 0.0], SubdEdgeTag::Smooth, None, [1.0, 1.0]).unwrap(),
            ],
            vec![SubdFace::new(vec![
                SubdEdgeUse {
                    edge: 0,
                    reversed: false,
                },
                SubdEdgeUse {
                    edge: 1,
                    reversed: false,
                },
                SubdEdgeUse {
                    edge: 2,
                    reversed: false,
                },
            ])
            .unwrap()],
            Vec::new(),
        )
        .unwrap(),
    });
    assert!(validate_neutral(&ir, Vec::new()).is_ok());
    let parsed = CadIr::from_json(&ir.to_canonical_json().unwrap()).unwrap();
    assert_eq!(parsed, ir);
    let wire = serde_json::to_value(&ir.model.subds[0]).unwrap();
    assert!(wire.get("cage").is_none());
    assert_eq!(wire["vertices"].as_array().unwrap().len(), 3);
    assert_eq!(wire["edges"].as_array().unwrap().len(), 3);
    assert_eq!(wire["faces"][0]["edges"].as_array().unwrap().len(), 3);
    assert_eq!(
        serde_json::to_value(SubdEdgeTag::SmoothX).unwrap(),
        serde_json::json!("smooth_x")
    );
    let cage = &ir.model.subds[0].cage;
    let mut edges = cage.faces()[0].edges().to_vec();
    edges[1].reversed = true;
    assert!(crate::subd::SubdCage::new(
        cage.vertices().to_vec(),
        cage.edges().to_vec(),
        vec![SubdFace::new(edges).unwrap()],
        Vec::new()
    )
    .is_err());
}

#[test]
fn grip_wedge_keeps_the_flat_wire_shape() {
    let phantom_wire = serde_json::json!({
        "edge": null,
        "sector_face": null,
        "phantom": true,
        "spokes": [],
        "sectors": [],
    });
    assert_eq!(
        serde_json::to_value(SubdGripWedge::Phantom).unwrap(),
        phantom_wire
    );
    assert_eq!(
        serde_json::from_value::<SubdGripWedge>(phantom_wire).unwrap(),
        SubdGripWedge::Phantom
    );

    let slot = SubdGripWedge::Slot {
        edge: Some(3),
        sector_face: None,
        spokes: Vec::new(),
        sectors: Vec::new(),
    };
    assert_eq!(
        serde_json::to_value(&slot).unwrap(),
        serde_json::json!({
            "edge": 3,
            "sector_face": null,
            "phantom": false,
            "spokes": [],
            "sectors": [],
        })
    );
}

#[test]
fn grip_wedge_rejects_phantom_payload() {
    let error = serde_json::from_value::<SubdGripWedge>(serde_json::json!({
        "edge": 3,
        "sector_face": null,
        "phantom": true,
        "spokes": [],
        "sectors": [],
    }))
    .unwrap_err();
    assert!(error.to_string().contains("phantom SubD grip wedge"));
}

#[test]
fn radial_symmetry_keeps_maps_at_the_flat_wire_boundary() {
    let symmetry = SubdSymmetry {
        kind: SubdSymmetryKind::Radial {
            segments: 4,
            sweep: 1.0,
            radial_maps: vec![SubdRadialSymmetryMap {
                selector: SubdRadialMapSelector::Ef,
                pairs: vec![[1, 2]],
            }],
        },
        plane: SubdPlaneFrame {
            origin: Point3::new(0.0, 0.0, 0.0),
            first_axis: Vector3::new(1.0, 0.0, 0.0),
            second_axis: Vector3::new(0.0, 1.0, 0.0),
        },
        face_pairs: Vec::new(),
        edge_pairs: Vec::new(),
        vertex_pairs: Vec::new(),
    };
    let wire = serde_json::to_value(&symmetry).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({
            "kind": { "kind": "radial", "segments": 4, "sweep": 1.0 },
            "plane": {
                "origin": { "x": 0.0, "y": 0.0, "z": 0.0 },
                "first_axis": { "x": 1.0, "y": 0.0, "z": 0.0 },
                "second_axis": { "x": 0.0, "y": 1.0, "z": 0.0 },
            },
            "radial_maps": [{ "selector": "ef", "pairs": [[1, 2]] }],
        })
    );
    assert_eq!(
        serde_json::from_value::<SubdSymmetry>(wire).unwrap(),
        symmetry
    );
}

#[test]
fn correspondence_symmetry_rejects_radial_maps() {
    let error = serde_json::from_value::<SubdSymmetry>(serde_json::json!({
        "kind": { "kind": "correspondence" },
        "plane": {
            "origin": { "x": 0.0, "y": 0.0, "z": 0.0 },
            "first_axis": { "x": 1.0, "y": 0.0, "z": 0.0 },
            "second_axis": { "x": 0.0, "y": 1.0, "z": 0.0 },
        },
        "radial_maps": [{ "selector": "ef", "pairs": [[1, 2]] }],
    }))
    .unwrap_err();
    assert!(error.to_string().contains("cannot carry radial_maps"));
}

fn triangle_cage() -> crate::subd::SubdCage {
    crate::subd::SubdCage::new(
        [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ]
        .into_iter()
        .map(|point| SubdVertex {
            point,
            tag: SubdVertexTag::Smooth,
            secondary_grips: None,
        })
        .collect(),
        [[0, 1], [1, 2], [2, 0]]
            .into_iter()
            .map(|vertices| {
                SubdEdge::new(vertices, [0.0, 0.0], SubdEdgeTag::Smooth, None, [0.0, 0.0]).unwrap()
            })
            .collect(),
        vec![SubdFace::new(
            (0..3)
                .map(|edge| SubdEdgeUse {
                    edge,
                    reversed: false,
                })
                .collect(),
        )
        .unwrap()],
        Vec::new(),
    )
    .unwrap()
}

fn rejects_cage_wire(wire: serde_json::Value) {
    let parts: super::SubdCageWire = serde_json::from_value(wire.clone()).unwrap();
    assert!(
        super::SubdCage::new(parts.vertices, parts.edges, parts.faces, parts.symmetries).is_err()
    );
    assert!(serde_json::from_value::<super::SubdCage>(wire).is_err());
}

#[test]
fn face_admission_requires_three_edge_uses() {
    for count in 0..3 {
        let edges = vec![
            SubdEdgeUse {
                edge: 0,
                reversed: false
            };
            count
        ];
        assert!(SubdFace::new(edges.clone()).is_err());
        assert!(serde_json::from_value::<SubdFace>(serde_json::json!({ "edges": edges })).is_err());
    }
    assert!(SubdFace::new(vec![
        SubdEdgeUse {
            edge: 0,
            reversed: false
        };
        3
    ])
    .is_ok());
}

#[test]
fn cage_admission_requires_valid_edge_indices_and_closed_directed_rings() {
    for endpoints in [[0, 3], [3, 0]] {
        let mut wire = serde_json::to_value(triangle_cage()).unwrap();
        wire["edges"][0]["vertices"] = serde_json::json!(endpoints);
        rejects_cage_wire(wire);
    }
    let mut wire = serde_json::to_value(triangle_cage()).unwrap();
    wire["faces"][0]["edges"][0]["edge"] = 3.into();
    rejects_cage_wire(wire);
    let mut wire = serde_json::to_value(triangle_cage()).unwrap();
    wire["faces"][0]["edges"][0]["reversed"] = true.into();
    rejects_cage_wire(wire);
}

#[test]
fn cage_admission_requires_non_empty_grip_layouts_and_valid_sector_arity() {
    for wedges in [
        serde_json::json!([]),
        serde_json::json!([{
            "edge": 0, "sector_face": null, "phantom": false, "spokes": [null], "sectors": []
        }]),
    ] {
        let mut wire = serde_json::to_value(triangle_cage()).unwrap();
        wire["vertices"][0]["secondary_grips"] =
            serde_json::json!({ "direction": "north", "wedges": wedges });
        rejects_cage_wire(wire);
    }
}

#[test]
fn cage_admission_requires_incident_grip_topology() {
    for (owner, edge, face) in [
        (0, Some(3), None),
        (0, Some(1), None),
        (0, None, Some(1)),
        (3, None, Some(0)),
    ] {
        let mut wire = serde_json::to_value(triangle_cage()).unwrap();
        if owner == 3 {
            let extra = wire["vertices"][0].clone();
            wire["vertices"].as_array_mut().unwrap().push(extra);
        }
        wire["vertices"][owner]["secondary_grips"] = serde_json::json!({
            "direction": "north", "wedges": [{ "edge": edge, "sector_face": face, "phantom": false, "spokes": [], "sectors": [] }]
        });
        rejects_cage_wire(wire);
    }
}

#[test]
fn grip_indices_are_unique_across_the_whole_cage() {
    let mut wire = serde_json::to_value(triangle_cage()).unwrap();
    for owner in 0..2 {
        wire["vertices"][owner]["secondary_grips"] = serde_json::json!({
            "direction": "north", "wedges": [{ "edge": 0, "sector_face": null, "phantom": false,
            "spokes": [{ "source_index": 42, "point": { "x": 0.0, "y": 0.0, "z": 0.0 }, "weight": 1.0 }], "sectors": [null] }]
        });
    }
    rejects_cage_wire(wire.clone());
    wire["vertices"][1]["secondary_grips"]["wedges"][0]["spokes"][0]["source_index"] = 43.into();
    assert!(serde_json::from_value::<super::SubdCage>(wire).is_ok());
}

#[test]
fn cage_mutation_rejects_invalid_layout_without_changing_the_cage() {
    let mut cage = triangle_cage();
    let original = cage.clone();
    assert!(cage
        .edit_vertices(|vertices| {
            vertices[0].tag = SubdVertexTag::Corner;
            vertices[1].secondary_grips = Some(super::SubdVertexGripLayout {
                direction: super::SubdGripDirection::North,
                wedges: Vec::new(),
            });
            Ok(())
        })
        .is_err());
    assert_eq!(cage, original);
    cage.edit_vertices(|vertices| {
        vertices[0].tag = SubdVertexTag::Corner;
        Ok(())
    })
    .unwrap();
    assert_eq!(cage.vertices()[0].tag, SubdVertexTag::Corner);
}

#[test]
fn cage_symmetry_references_stay_in_range() {
    let mut wire = serde_json::to_value(triangle_cage()).unwrap();
    wire["symmetries"] = serde_json::json!([{
        "kind": { "kind": "correspondence" },
        "plane": { "origin": { "x": 0.0, "y": 0.0, "z": 0.0 }, "first_axis": { "x": 1.0, "y": 0.0, "z": 0.0 }, "second_axis": { "x": 0.0, "y": 1.0, "z": 0.0 } },
        "vertex_pairs": [[0, 3]]
    }]);
    rejects_cage_wire(wire.clone());
    wire["symmetries"][0]["vertex_pairs"] = serde_json::json!([[1, 1]]);
    assert!(serde_json::from_value::<super::SubdCage>(wire).is_ok());
}

#[test]
fn edge_admission_requires_distinct_endpoints() {
    assert!(SubdEdge::new([0, 0], [0.0, 0.0], SubdEdgeTag::Smooth, None, [0.0, 0.0]).is_err());
    let mut wire = serde_json::to_value(&triangle_cage().edges()[0]).unwrap();
    wire["vertices"] = serde_json::json!([0, 0]);
    assert!(serde_json::from_value::<SubdEdge>(wire).is_err());
    assert!(SubdEdge::new(
        [u32::MAX, 0],
        [0.0, 0.0],
        SubdEdgeTag::Smooth,
        None,
        [0.0, 0.0]
    )
    .is_ok());
}

#[test]
fn edge_numeric_admission_rejects_invalid_controls() {
    let base = serde_json::to_value(&triangle_cage().edges()[0]).unwrap();
    for invalid in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for index in 0..2 {
            let mut sharpness = [0.0, 0.0];
            sharpness[index] = invalid;
            assert!(
                SubdEdge::new([0, 1], sharpness, SubdEdgeTag::Smooth, None, [0.0, 0.0]).is_err()
            );
            let mut wire = base.clone();
            wire["sharpness"] = serde_json::json!(sharpness);
            assert!(serde_json::from_value::<SubdEdge>(wire).is_err());
        }
    }
    for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(SubdEdge::new(
            [0, 1],
            [0.0, 0.0],
            SubdEdgeTag::Smooth,
            Some(invalid),
            [0.0, 0.0]
        )
        .is_err());
    }
    for invalid in [0.0, -1.0] {
        let mut wire = base.clone();
        wire["knot_interval"] = invalid.into();
        assert!(serde_json::from_value::<SubdEdge>(wire).is_err());
    }
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for index in 0..2 {
            let mut coefficients = [0.0, 0.0];
            coefficients[index] = invalid;
            assert!(
                SubdEdge::new([0, 1], [0.0, 0.0], SubdEdgeTag::Smooth, None, coefficients).is_err()
            );
            let mut wire = base.clone();
            wire["sector_coefficients"] = serde_json::json!(coefficients);
            assert!(serde_json::from_value::<SubdEdge>(wire).is_err());
        }
    }
}

#[test]
fn edge_admission_preserves_signed_coefficients_and_optional_intervals() {
    for interval in [None, Some(f64::MIN_POSITIVE), Some(f64::MAX)] {
        let edge = SubdEdge::new(
            [1, 0],
            [0.0, f64::MAX],
            SubdEdgeTag::SmoothX,
            interval,
            [-2.0, 3.0],
        )
        .unwrap();
        assert_eq!(edge.vertices(), [1, 0]);
        assert_eq!(edge.sharpness(), [0.0, f64::MAX]);
        assert_eq!(edge.knot_interval(), interval);
        assert_eq!(edge.sector_coefficients(), [-2.0, 3.0]);
        let wire = serde_json::to_value(&edge).unwrap();
        assert_eq!(serde_json::from_value::<SubdEdge>(wire).unwrap(), edge);
    }
}

#[test]
fn secondary_grip_admission_requires_finite_points_and_positive_weights() {
    let point = Point3::new(-1.0, 2.0, 3.0);
    let base = serde_json::json!({"source_index": 0, "point": point, "weight": 1.0});
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for point in [
            Point3::new(invalid, 0.0, 0.0),
            Point3::new(0.0, invalid, 0.0),
            Point3::new(0.0, 0.0, invalid),
        ] {
            assert!(super::SubdSecondaryGrip::new(0, point, 1.0).is_err());
            let mut wire = base.clone();
            wire["point"] = serde_json::json!(point);
            assert!(serde_json::from_value::<super::SubdSecondaryGrip>(wire).is_err());
        }
    }
    for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(super::SubdSecondaryGrip::new(0, point, invalid).is_err());
        let mut wire = base.clone();
        wire["weight"] = serde_json::json!(invalid);
        assert!(serde_json::from_value::<super::SubdSecondaryGrip>(wire).is_err());
    }
    for weight in [f64::MIN_POSITIVE, 1.0, f64::MAX] {
        let grip = super::SubdSecondaryGrip::new(u32::MAX, point, weight).unwrap();
        assert_eq!(grip.point(), point);
        assert_eq!(grip.weight(), weight);
        let wire = serde_json::to_value(&grip).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({"source_index": u32::MAX, "point": point, "weight": weight})
        );
        assert_eq!(
            serde_json::from_value::<super::SubdSecondaryGrip>(wire).unwrap(),
            grip
        );
    }
}
