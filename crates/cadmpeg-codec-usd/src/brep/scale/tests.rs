// SPDX-License-Identifier: Apache-2.0

use crate::brep::{
    AnalyticCurve, AnalyticSurface, Brep, Catalog, CurveKind, EdgeRecord, FaceRecord, Form, Frame,
    LoopRecord, NurbsCurveRecord, OrientedEdgeRecord, SurfaceKind,
};

/// One face on a cylinder of radius 2 about Z, bounded by a line edge with
/// a NURBS pcurve.
fn cylinder() -> Brep {
    let frame = Frame {
        position: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
    };
    Brep {
        vertices: vec![[2.0, 0.0, 0.0], [2.0, 0.0, 4.0]],
        edges: vec![EdgeRecord {
            vertices: [0, 1],
            curve: Some((Catalog::Analytic, 0)),
            range: [0.0, 4.0],
            same_sense: true,
            tolerance: Some(0.5),
        }],
        oriented_edges: vec![OrientedEdgeRecord {
            edge: 0,
            orientation: true,
            pcurve: Some((Catalog::Nurbs, 0)),
        }],
        loops: vec![LoopRecord {
            oriented_edges: vec![0],
            vertex: None,
        }],
        faces: vec![FaceRecord {
            loops: vec![0],
            surface: (Catalog::Analytic, 0),
            same_sense: true,
            id: 0,
        }],
        analytic_surfaces: vec![AnalyticSurface {
            kind: SurfaceKind::Cylinder,
            frame: frame.clone(),
            params: vec![2.0],
            profile: None,
        }],
        analytic_curves: vec![AnalyticCurve {
            kind: CurveKind::Line,
            frame: Frame {
                position: [2.0, 0.0, 0.0],
                ..frame
            },
            params: Vec::new(),
        }],
        nurbs_pcurves: vec![NurbsCurveRecord {
            order: 2,
            points: vec![[1.5, 0.0], [1.5, 4.0]],
            weights: vec![1.0, 1.0],
            knots: vec![0.0, 0.0, 1.0, 1.0],
            form: Form::Open,
        }],
        tolerance: 1e-6,
        angle_scale: 1.0,
        valid: true,
        ..Brep::default()
    }
}

#[test]
fn lengths_divide_and_angles_keep_their_values() {
    let original = cylinder();
    let scaled = original.divided(2.0);
    assert_eq!(
        scaled.analytic_surfaces[0].params[0],
        original.analytic_surfaces[0].params[0] * 0.5
    );
    for (before, after) in original.vertices.iter().zip(&scaled.vertices) {
        assert_eq!(after.map(|value| value * 2.0), *before);
    }
    // On a cylinder, u is an angle and v a length.
    let pcurve = scaled
        .faces
        .iter()
        .flat_map(|face| &face.loops)
        .flat_map(|index| &scaled.loops[*index].oriented_edges)
        .find_map(|index| scaled.oriented_edges[*index].pcurve)
        .expect("a pcurve");
    assert_eq!(pcurve.0, Catalog::Nurbs);
    for (before, after) in original.nurbs_pcurves[pcurve.1]
        .points
        .iter()
        .zip(&scaled.nurbs_pcurves[pcurve.1].points)
    {
        assert_eq!(after[0], before[0]);
        assert_eq!(after[1], before[1] * 0.5);
    }
    assert_eq!(scaled.tolerance, original.tolerance * 0.5);
}

#[test]
fn dividing_by_a_power_of_two_and_its_inverse_restores_the_records() {
    let original = cylinder();
    assert_eq!(original.divided(0.25).divided(4.0), original);
}
