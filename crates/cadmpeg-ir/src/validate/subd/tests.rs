// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::geometry::{Curve, CurveGeometry};
use crate::ids::{CurveId, SubdId};
use crate::math::Point3;
use crate::provenance::SourceObjectAssociation;
use crate::subd::{
    SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdScheme, SubdSurface, SubdVertex,
    SubdVertexTag,
};
use crate::topology::Color;
use crate::validate::validate_neutral;
use crate::CadIr;

#[test]
fn subd_rejects_short_rings_and_negative_sharpness() {
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.subds.push(SubdSurface {
        id: SubdId("synthetic:subd:surface#short".into()),
        scheme: SubdScheme::CatmullClark,
        vertices: vec![
            SubdVertex {
                point: Point3::new(0.0, 0.0, 0.0),
                tag: SubdVertexTag::Smooth,
            },
            SubdVertex {
                point: Point3::new(1.0, 0.0, 0.0),
                tag: SubdVertexTag::Smooth,
            },
        ],
        edges: vec![SubdEdge {
            vertices: [0, 1],
            sharpness: [-0.1, 0.0],
            tag: SubdEdgeTag::Smooth,
            sector_coefficients: [0.0, 0.0],
        }],
        faces: vec![SubdFace {
            edges: vec![
                SubdEdgeUse {
                    edge: 0,
                    reversed: false,
                },
                SubdEdgeUse {
                    edge: 0,
                    reversed: true,
                },
            ],
        }],
        source_object: None,
    });
    let findings = validate_neutral(&ir, Vec::new()).findings;
    assert!(findings
        .iter()
        .any(|finding| finding.message.contains("fewer than three")));
    assert!(findings
        .iter()
        .any(|finding| finding.message.contains("edge 0 is invalid")));
}

#[test]
fn source_association_is_a_free_carrier_root() {
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.curves.push(Curve {
        id: CurveId("synthetic:source:curve#0".into()),
        geometry: CurveGeometry::Unknown { record: None },
        source_object: Some(SourceObjectAssociation {
            format: "rhino".into(),
            object_id: "00000000-0000-0000-0000-000000000000".into(),
            name: Some("curve".into()),
            color: None,
            visible: Some(true),
            layer: Some("layer-0".into()),
            instance_path: Vec::new(),
        }),
    });
    let report = validate_neutral(&ir, Vec::new());
    assert!(report.is_ok(), "{:?}", report.findings);
    let parsed = CadIr::from_json(&ir.to_canonical_json().unwrap()).unwrap();
    assert_eq!(parsed, ir);
}

#[test]
fn source_association_rejects_out_of_range_color() {
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.curves.push(Curve {
        id: CurveId("synthetic:source:curve#color".into()),
        geometry: CurveGeometry::Unknown { record: None },
        source_object: Some(SourceObjectAssociation {
            format: "rhino".into(),
            object_id: "object".into(),
            name: None,
            color: Some(Color {
                r: 1.1,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }),
            visible: None,
            layer: None,
            instance_path: Vec::new(),
        }),
    });
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("outside [0, 1]")));
}
