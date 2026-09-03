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
        id: SubdId("synthetic:subd:surface#0".into()),
        scheme: SubdScheme::CatmullClark,
        symmetries: Vec::new(),
        vertices: vec![
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
        edges: vec![
            SubdEdge {
                vertices: [0, 1],
                sharpness: [0.0, 0.25],
                tag: SubdEdgeTag::Smooth,
                knot_interval: None,
                sector_coefficients: [1.0, 1.0],
            },
            SubdEdge {
                vertices: [1, 2],
                sharpness: [0.25, 0.0],
                tag: SubdEdgeTag::SmoothX,
                knot_interval: None,
                sector_coefficients: [1.0, 1.0],
            },
            SubdEdge {
                vertices: [2, 0],
                sharpness: [0.0, 0.0],
                tag: SubdEdgeTag::Smooth,
                knot_interval: None,
                sector_coefficients: [1.0, 1.0],
            },
        ],
        faces: vec![SubdFace {
            edges: vec![
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
            ],
        }],
        source_object: None,
    });
    assert!(validate_neutral(&ir, Vec::new()).is_ok());
    let parsed = CadIr::from_json(&ir.to_canonical_json().unwrap()).unwrap();
    assert_eq!(parsed, ir);
    assert_eq!(
        serde_json::to_value(SubdEdgeTag::SmoothX).unwrap(),
        serde_json::json!("smooth_x")
    );
    ir.model.subds[0].faces[0].edges[1].reversed = true;
    assert!(!validate_neutral(&ir, Vec::new()).is_ok());
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
