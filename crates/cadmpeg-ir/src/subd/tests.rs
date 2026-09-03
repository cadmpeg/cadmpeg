// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::ids::SubdId;
use crate::math::Point3;
use crate::subd::{
    SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdScheme, SubdSurface, SubdVertex,
    SubdVertexTag,
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
