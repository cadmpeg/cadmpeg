// SPDX-License-Identifier: Apache-2.0
#![allow(unused_imports)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions, Encoder};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Color, Point};
use cadmpeg_ir::units::Units;
use sha2::{Digest, Sha256};

use super::*;
use crate::{RhinoArchiveVersion, RhinoCodec, RhinoEncoder};

#[test]
fn shared_rational_nurbs_edge_round_trips_c3_and_reversed_c2() {
    let mut ir = adjacent_quad_sheet();
    let edge = &mut ir.model.edges[1];
    edge.param_range = Some([2.0, 5.0]);
    ir.model.curves[1].geometry =
        cadmpeg_ir::geometry::CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
            degree: 2,
            knots: vec![2.0, 2.0, 2.0, 5.0, 5.0, 5.0],
            control_points: vec![
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.25, 0.5, 0.0),
                Point3::new(1.0, 1.0, 0.0),
            ],
            weights: Some(vec![1.0, 0.75, 1.0]),
            periodic: false,
        });
    let expected = ir.model.curves[1].geometry.clone();
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        RhinoEncoder::new(version)
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &ir,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("required invariant");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("required invariant");
        let shared = decoded
            .ir()
            .model
            .edges
            .iter()
            .find(|edge| edge.param_range == Some([2.0, 5.0]))
            .expect("NURBS edge domain");
        let curve = decoded
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| shared.curve.as_ref() == Some(&curve.id))
            .expect("NURBS C3");
        assert_eq!(curve.geometry, expected, "{version:?}");
        let uses = decoded
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.edge == shared.id)
            .collect::<Vec<_>>();
        assert_eq!(uses.len(), 2, "{version:?}");
        assert_ne!(uses[0].sense, uses[1].sense, "{version:?}");
        for use_ in uses {
            let pcurve = decoded
                .ir()
                .model
                .pcurves
                .iter()
                .find(|pcurve| use_.pcurves.first().map(|use_| &use_.pcurve) == Some(&pcurve.id))
                .expect("projected NURBS C2");
            assert!(matches!(
                pcurve.geometry,
                cadmpeg_ir::geometry::PcurveGeometry::Nurbs { .. }
            ));
        }
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
    }
}

#[test]
fn explicit_nurbs_pcurves_round_trip_owned_geometry_and_tolerance() {
    let mut ir = adjacent_quad_sheet();
    ir.model.edges[1].param_range = Some([2.0, 5.0]);
    ir.model.curves[1].geometry =
        cadmpeg_ir::geometry::CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
            degree: 2,
            knots: vec![2.0, 2.0, 2.0, 5.0, 5.0, 5.0],
            control_points: vec![
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.25, 0.5, 0.0),
                Point3::new(1.0, 1.0, 0.0),
            ],
            weights: Some(vec![1.0, 0.75, 1.0]),
            periodic: false,
        });
    for (coedge, reversed) in [(1_usize, false), (7, true)] {
        let id: cadmpeg_ir::ids::PcurveId = format!("cadir:model:pcurve#explicit.{coedge}").into();
        let mut control_points = vec![
            cadmpeg_ir::math::Point2::new(1.0, 0.0),
            cadmpeg_ir::math::Point2::new(1.25, 0.5),
            cadmpeg_ir::math::Point2::new(1.0, 1.0),
        ];
        if reversed {
            control_points.reverse();
        }
        ir.model.pcurves.push(cadmpeg_ir::geometry::Pcurve {
            id: id.clone(),
            geometry: cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
                degree: 2,
                knots: vec![2.0, 2.0, 2.0, 5.0, 5.0, 5.0],
                control_points,
                weights: Some(vec![1.0, 0.75, 1.0]),
                periodic: false,
            },
            wrapper_reversed: Some(false),
            native_tail_flags: None,
            parameter_range: Some([2.0, 5.0]),
            fit_tolerance: Some(0.001),
        });
        ir.model.coedges[coedge].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
            pcurve: id,
            isoparametric: None,
            parameter_range: None,
        }];
    }
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        RhinoEncoder::new(version)
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &ir,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("required invariant");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("required invariant");
        let explicit = decoded
            .ir()
            .model
            .pcurves
            .iter()
            .filter(|pcurve| pcurve.fit_tolerance == Some(0.001))
            .collect::<Vec<_>>();
        assert_eq!(explicit.len(), 2, "{version:?}");
        assert!(explicit.iter().all(|pcurve| {
            pcurve.wrapper_reversed == Some(false)
                && pcurve.parameter_range == Some([2.0, 5.0])
                && matches!(
                    pcurve.geometry,
                    cadmpeg_ir::geometry::PcurveGeometry::Nurbs { .. }
                )
        }));
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
    }
}

#[test]
fn inconsistent_explicit_pcurve_is_rejected_before_output() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    let id: cadmpeg_ir::ids::PcurveId = "cadir:model:pcurve#mismatch".into();
    ir.model.pcurves.push(cadmpeg_ir::geometry::Pcurve {
        id: id.clone(),
        geometry: cadmpeg_ir::geometry::PcurveGeometry::Line {
            origin: cadmpeg_ir::math::Point2::new(0.0, 1.0),
            direction: cadmpeg_ir::math::Point2::new(1.0, 0.0),
        },
        wrapper_reversed: None,
        native_tail_flags: None,
        parameter_range: ir.model.edges[0].param_range,
        fit_tolerance: None,
    });
    ir.model.coedges[0].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
        pcurve: id,
        isoparametric: None,
        parameter_range: None,
    }];
    let mut output = vec![0xaa];
    let error = RhinoEncoder::new(RhinoArchiveVersion::V8)
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &ir,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut output))
        .expect_err("expected error");
    assert!(error.to_string().contains("does not exactly match"));
    assert_eq!(output, [0xaa]);
}

#[test]
fn multiple_pcurve_uses_are_rejected_before_output() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    let first: cadmpeg_ir::ids::PcurveId = "cadir:model:pcurve#first".into();
    let second: cadmpeg_ir::ids::PcurveId = "cadir:model:pcurve#second".into();
    for (id, origin) in [
        (first.clone(), cadmpeg_ir::math::Point2::new(0.0, 0.0)),
        (second.clone(), cadmpeg_ir::math::Point2::new(0.0, 1.0)),
    ] {
        ir.model.pcurves.push(cadmpeg_ir::geometry::Pcurve {
            id,
            geometry: cadmpeg_ir::geometry::PcurveGeometry::Line {
                origin,
                direction: cadmpeg_ir::math::Point2::new(1.0, 0.0),
            },
            wrapper_reversed: None,
            native_tail_flags: None,
            parameter_range: Some([0.0, 2.0]),
            fit_tolerance: None,
        });
    }
    ir.model.coedges[0].pcurves = vec![
        cadmpeg_ir::topology::PcurveUse {
            pcurve: first,
            isoparametric: None,
            parameter_range: None,
        },
        cadmpeg_ir::topology::PcurveUse {
            pcurve: second,
            isoparametric: None,
            parameter_range: None,
        },
    ];

    let mut output = vec![0xaa];
    let error = RhinoEncoder::new(RhinoArchiveVersion::V8)
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &ir,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut output))
        .expect_err("expected error");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
    assert_eq!(output, [0xaa]);
}

#[test]
fn explicit_line_pcurve_round_trips_as_native_c2() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
    ]);
    let id: cadmpeg_ir::ids::PcurveId = "cadir:model:pcurve#line".into();
    ir.model.pcurves.push(cadmpeg_ir::geometry::Pcurve {
        id: id.clone(),
        geometry: cadmpeg_ir::geometry::PcurveGeometry::Line {
            origin: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            direction: cadmpeg_ir::math::Point2::new(1.0, 0.0),
        },
        wrapper_reversed: None,
        native_tail_flags: None,
        parameter_range: Some([0.0, 2.0]),
        fit_tolerance: Some(0.002),
    });
    ir.model.coedges[0].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
        pcurve: id,
        isoparametric: None,
        parameter_range: None,
    }];
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        RhinoEncoder::new(version)
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &ir,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("required invariant");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("required invariant");
        let pcurve = decoded
            .ir()
            .model
            .pcurves
            .iter()
            .find(|pcurve| pcurve.fit_tolerance == Some(0.002))
            .expect("explicit line C2");
        assert_eq!(pcurve.parameter_range, Some([0.0, 2.0]));
        assert!(matches!(
            pcurve.geometry,
            cadmpeg_ir::geometry::PcurveGeometry::Nurbs { degree: 1, .. }
        ));
    }
}

#[test]
fn rational_nurbs_surface_patch_round_trips_exact_boundaries() {
    let ir = rectangular_nurbs_patch();
    let expected_surface = ir.model.surfaces[0].geometry.clone();
    let expected_curves = ir
        .model
        .curves
        .iter()
        .map(|curve| curve.geometry.clone())
        .collect::<Vec<_>>();
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        RhinoEncoder::new(version)
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &ir,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("required invariant");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("required invariant");
        assert_eq!(decoded.ir().model.bodies.len(), 1, "{version:?}");
        assert_eq!(
            decoded.ir().model.bodies[0].kind,
            cadmpeg_ir::topology::BodyKind::Sheet,
            "{version:?}"
        );
        assert_eq!(decoded.ir().model.surfaces[0].geometry, expected_surface);
        assert_eq!(
            decoded
                .ir()
                .model
                .curves
                .iter()
                .map(|curve| curve.geometry.clone())
                .collect::<Vec<_>>(),
            expected_curves,
            "{version:?}"
        );
        assert_eq!(decoded.ir().model.pcurves.len(), 4, "{version:?}");
        assert!(decoded
            .ir()
            .model
            .pcurves
            .iter()
            .all(|pcurve| pcurve.fit_tolerance == Some(0.001)));
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
    }
}

#[test]
fn mixed_plane_and_nurbs_faces_round_trip_shared_edge() {
    let ir = mixed_plane_nurbs_sheet();
    let expected_surface = ir.model.surfaces[0].geometry.clone();
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        RhinoEncoder::new(version)
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &ir,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("required invariant");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("required invariant");
        assert_eq!(decoded.ir().model.bodies.len(), 1, "{version:?}");
        assert_eq!(
            decoded.ir().model.bodies[0].kind,
            cadmpeg_ir::topology::BodyKind::Sheet,
            "{version:?}"
        );
        assert_eq!(decoded.ir().model.faces.len(), 2, "{version:?}");
        assert_eq!(decoded.ir().model.surfaces[0].geometry, expected_surface);
        assert_eq!(decoded.ir().model.edges[1].param_range, Some([30.0, 32.0]));
        let shared_uses = decoded
            .ir()
            .model
            .coedges
            .iter()
            .enumerate()
            .filter(|(_, coedge)| coedge.edge == decoded.ir().model.edges[1].id)
            .collect::<Vec<_>>();
        assert_eq!(shared_uses.len(), 2, "{version:?}");
        assert_ne!(shared_uses[0].1.sense, shared_uses[1].1.sense);
        assert_eq!(
            shared_uses[0].1.radial_next, shared_uses[1].1.id,
            "{version:?}"
        );
        assert_eq!(
            shared_uses[1].1.radial_next, shared_uses[0].1.id,
            "{version:?}"
        );
        assert_eq!(
            decoded
                .ir()
                .model
                .pcurves
                .iter()
                .filter(|pcurve| pcurve.fit_tolerance == Some(0.001))
                .count(),
            4,
            "{version:?}"
        );
        let planar_shared_pcurve = decoded
            .ir()
            .model
            .pcurves
            .iter()
            .find(|pcurve| {
                pcurve.parameter_range == Some([30.0, 32.0]) && pcurve.fit_tolerance != Some(0.001)
            })
            .expect("generated planar shared-edge pcurve");
        assert!(matches!(
            planar_shared_pcurve.geometry,
            cadmpeg_ir::geometry::PcurveGeometry::Nurbs { .. }
        ));
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
    }
}

#[test]
fn generally_trimmed_nurbs_face_round_trips_outer_loop_and_hole() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.25, 0.25, 0.0),
        Point3::new(3.5, 0.75, 0.0),
        Point3::new(2.75, 3.5, 0.0),
        Point3::new(0.5, 2.75, 0.0),
    ]);
    add_polygon_hole(
        &mut ir,
        &[
            Point3::new(1.25, 1.25, 0.0),
            Point3::new(1.5, 2.25, 0.0),
            Point3::new(2.25, 1.5, 0.0),
        ],
    );
    make_planar_nurbs_trimmed_face(&mut ir);
    let domain = ir.model.edges[0].param_range.expect("fixture domain");
    let poles = [
        Point3::new(0.25, 0.25, 0.0),
        Point3::new(2.0, 0.25, 0.0),
        Point3::new(3.5, 0.75, 0.0),
    ];
    ir.model.curves[0].geometry =
        cadmpeg_ir::geometry::CurveGeometry::Nurbs(cadmpeg_ir::geometry::NurbsCurve {
            degree: 2,
            knots: vec![
                domain[0], domain[0], domain[0], domain[1], domain[1], domain[1],
            ],
            control_points: poles.to_vec(),
            weights: Some(vec![1.0, 0.8, 1.0]),
            periodic: false,
        });
    ir.model.pcurves[0].geometry = cadmpeg_ir::geometry::PcurveGeometry::Nurbs {
        degree: 2,
        knots: vec![
            domain[0], domain[0], domain[0], domain[1], domain[1], domain[1],
        ],
        control_points: poles
            .iter()
            .map(|point| cadmpeg_ir::math::Point2::new(point.x, point.y))
            .collect(),
        weights: Some(vec![1.0, 0.8, 1.0]),
        periodic: false,
    };
    let expected_surface = ir.model.surfaces[0].geometry.clone();
    let expected_curve = ir.model.curves[0].geometry.clone();
    for version in [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ] {
        let mut bytes = Vec::new();
        RhinoEncoder::new(version)
            .plan(cadmpeg_ir::codec::EncodeInput {
                ir: &ir,
                fidelity: None,
            })
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("required invariant");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("required invariant");
        assert_eq!(decoded.ir().model.surfaces[0].geometry, expected_surface);
        assert_eq!(decoded.ir().model.curves[0].geometry, expected_curve);
        assert_eq!(decoded.ir().model.loops.len(), 2, "{version:?}");
        assert_eq!(decoded.ir().model.pcurves.len(), 7, "{version:?}");
        assert!(decoded
            .ir()
            .model
            .pcurves
            .iter()
            .all(|pcurve| pcurve.fit_tolerance == Some(0.0001)));
        assert!(cadmpeg_ir::validate_neutral(decoded.ir(), Vec::new()).is_ok());
    }
}

#[test]
fn nurbs_trim_that_misses_its_edge_is_rejected_atomically() {
    let mut ir = polygon_sheet(&[
        Point3::new(0.5, 0.5, 0.0),
        Point3::new(3.5, 0.5, 0.0),
        Point3::new(2.0, 3.0, 0.0),
    ]);
    make_planar_nurbs_trimmed_face(&mut ir);
    let cadmpeg_ir::geometry::PcurveGeometry::Line { direction, .. } =
        &mut ir.model.pcurves[0].geometry
    else {
        unreachable!()
    };
    direction.v += 0.25;
    let mut output = vec![0xaa];
    let error = RhinoEncoder::new(RhinoArchiveVersion::V8)
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &ir,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut output))
        .expect_err("expected error");
    assert!(error.to_string().contains("misses directed edge curve"));
    assert_eq!(output, [0xaa]);
}

#[test]
fn nurbs_surface_patch_without_boundary_pcurves_is_rejected_atomically() {
    let mut ir = rectangular_nurbs_patch();
    ir.model.pcurves.clear();
    for coedge in &mut ir.model.coedges {
        coedge.pcurves.clear();
    }
    let mut output = vec![0xaa];
    let error = RhinoEncoder::new(RhinoArchiveVersion::V8)
        .plan(cadmpeg_ir::codec::EncodeInput {
            ir: &ir,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut output))
        .expect_err("expected error");
    assert!(error.to_string().contains("explicit pcurve"));
    assert_eq!(output, [0xaa]);
}
