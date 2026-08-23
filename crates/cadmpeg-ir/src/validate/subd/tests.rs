// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::collections::BTreeMap;
use std::fmt::Debug;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::annotations::{ExactnessNote, StreamProvenance};
use crate::codec::{CadirEncoder, Encoder};
use crate::document::Model;
use crate::examples::{directed_subd_sum, unit_cube};
use crate::features::ExtrudeDirection;
use crate::geometry::{
    Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, SplineSurfaceParameters, SurfaceGeometry,
};
use crate::ids::{
    CoedgeId, CurveId, EdgeId, ProceduralCurveId, ProceduralSurfaceId, SubdId, UnknownId,
};
use crate::math::{Point3, Vector3};
use crate::native::NativeRecord;
use crate::products::{ProductDefinition, ProductDefinitionKind};
use crate::provenance::{Exactness, SourceObjectAssociation};
use crate::report::{Check, LossKind, LossNote, LossTaxonomy, Severity};
use crate::subd::{
    SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdGripDirection, SubdGripWedge, SubdPlaneFrame,
    SubdScheme, SubdSecondaryGrip, SubdSurface, SubdSymmetry, SubdSymmetryKind, SubdVertex,
    SubdVertexGripLayout, SubdVertexTag,
};
use crate::tessellation::{TessellationChannel, TessellationChannelDomain};
use crate::topology::Color;
use crate::unknown::{NativeUnknownRecord, UnknownRecord};
use crate::validate::validate_neutral;
use crate::{diff, CadIr, SourceProvenance};

use super::*;

#[test]
fn subd_rejects_short_rings_and_negative_sharpness() {
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.subds.push(SubdSurface {
        id: SubdId("synthetic:subd:surface#short".into()),
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
        ],
        edges: vec![SubdEdge {
            vertices: [0, 1],
            sharpness: [-0.1, 0.0],
            tag: SubdEdgeTag::Smooth,
            knot_interval: None,
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
fn subd_rejects_invalid_secondary_grip_sector_arity() {
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.subds.push(SubdSurface {
        id: SubdId("synthetic:subd:surface#grips".into()),
        scheme: SubdScheme::CatmullClark,
        symmetries: Vec::new(),
        vertices: vec![
            SubdVertex {
                point: Point3::new(0.0, 0.0, 0.0),
                tag: SubdVertexTag::Smooth,
                secondary_grips: Some(SubdVertexGripLayout {
                    direction: SubdGripDirection::North,
                    wedges: vec![SubdGripWedge {
                        edge: Some(0),
                        sector_face: None,
                        phantom: false,
                        spokes: vec![Some(SubdSecondaryGrip {
                            source_index: 0,
                            point: Point3::new(0.25, 0.0, 0.0),
                            weight: 1.0,
                        })],
                        sectors: Vec::new(),
                    }],
                }),
            },
            SubdVertex {
                point: Point3::new(1.0, 0.0, 0.0),
                tag: SubdVertexTag::Smooth,
                secondary_grips: None,
            },
        ],
        edges: vec![SubdEdge {
            vertices: [0, 1],
            sharpness: [0.0, 0.0],
            tag: SubdEdgeTag::Smooth,
            knot_interval: None,
            sector_coefficients: [0.0, 0.0],
        }],
        faces: Vec::new(),
        source_object: None,
    });
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("invalid sector arity")));
}

#[test]
fn subd_rejects_secondary_grip_edge_not_incident_to_owner() {
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.subds.push(SubdSurface {
        id: SubdId("synthetic:subd:surface#grip-incidence".into()),
        scheme: SubdScheme::CatmullClark,
        symmetries: Vec::new(),
        vertices: vec![
            SubdVertex {
                point: Point3::new(0.0, 0.0, 0.0),
                tag: SubdVertexTag::Smooth,
                secondary_grips: Some(SubdVertexGripLayout {
                    direction: SubdGripDirection::North,
                    wedges: vec![SubdGripWedge {
                        edge: Some(0),
                        sector_face: None,
                        phantom: false,
                        spokes: Vec::new(),
                        sectors: Vec::new(),
                    }],
                }),
            },
            SubdVertex {
                point: Point3::new(1.0, 0.0, 0.0),
                tag: SubdVertexTag::Smooth,
                secondary_grips: None,
            },
            SubdVertex {
                point: Point3::new(2.0, 0.0, 0.0),
                tag: SubdVertexTag::Smooth,
                secondary_grips: None,
            },
        ],
        edges: vec![SubdEdge {
            vertices: [1, 2],
            sharpness: [0.0, 0.0],
            tag: SubdEdgeTag::Smooth,
            knot_interval: Some(1.0),
            sector_coefficients: [0.0, 0.0],
        }],
        faces: Vec::new(),
        source_object: None,
    });

    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message.contains("is not incident to its owner")));
}

#[test]
fn subd_rejects_secondary_grip_sector_face_not_incident_to_owner() {
    let edge = |vertices| SubdEdge {
        vertices,
        sharpness: [0.0, 0.0],
        tag: SubdEdgeTag::Smooth,
        knot_interval: Some(1.0),
        sector_coefficients: [0.0, 0.0],
    };
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.subds.push(SubdSurface {
        id: SubdId("synthetic:subd:surface#grip-sector-incidence".into()),
        scheme: SubdScheme::CatmullClark,
        symmetries: Vec::new(),
        vertices: (0..4)
            .map(|index| SubdVertex {
                point: Point3::new(f64::from(index), 0.0, 0.0),
                tag: SubdVertexTag::Smooth,
                secondary_grips: (index == 0).then_some(SubdVertexGripLayout {
                    direction: SubdGripDirection::North,
                    wedges: vec![SubdGripWedge {
                        edge: Some(0),
                        sector_face: Some(0),
                        phantom: false,
                        spokes: Vec::new(),
                        sectors: Vec::new(),
                    }],
                }),
            })
            .collect(),
        edges: vec![edge([0, 1]), edge([1, 2]), edge([2, 3]), edge([3, 1])],
        faces: vec![SubdFace {
            edges: vec![
                SubdEdgeUse {
                    edge: 1,
                    reversed: false,
                },
                SubdEdgeUse {
                    edge: 2,
                    reversed: false,
                },
                SubdEdgeUse {
                    edge: 3,
                    reversed: false,
                },
            ],
        }],
        source_object: None,
    });

    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding
            .message
            .contains("sector face is not incident to its owner")));
}

#[test]
fn subd_rejects_invalid_symmetry_carriers() {
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.subds.push(SubdSurface {
        id: SubdId("synthetic:subd:surface#symmetry".into()),
        scheme: SubdScheme::CatmullClark,
        symmetries: vec![SubdSymmetry {
            kind: SubdSymmetryKind::Radial {
                segments: 0,
                sweep: f64::NAN,
            },
            plane: SubdPlaneFrame {
                origin: Point3::new(0.0, 0.0, 0.0),
                first_axis: Vector3::new(1.0, 0.0, 0.0),
                second_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            face_pairs: vec![[0, 1]],
            edge_pairs: vec![[0, 1]],
            vertex_pairs: vec![[0, 3]],
            radial_maps: Vec::new(),
        }],
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
                sharpness: [0.0, 0.0],
                tag: SubdEdgeTag::Smooth,
                knot_interval: None,
                sector_coefficients: [0.0, 0.0],
            },
            SubdEdge {
                vertices: [1, 2],
                sharpness: [0.0, 0.0],
                tag: SubdEdgeTag::Smooth,
                knot_interval: None,
                sector_coefficients: [0.0, 0.0],
            },
            SubdEdge {
                vertices: [2, 0],
                sharpness: [0.0, 0.0],
                tag: SubdEdgeTag::Smooth,
                knot_interval: None,
                sector_coefficients: [0.0, 0.0],
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
    let findings = validate_neutral(&ir, Vec::new()).findings;
    assert!(findings
        .iter()
        .any(|finding| finding.message.contains("plane frame is invalid")));
    assert!(findings
        .iter()
        .any(|finding| finding.message.contains("radial controls are invalid")));
    assert!(findings
        .iter()
        .any(|finding| finding.message.contains("vertex pair 0 is invalid")));
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
