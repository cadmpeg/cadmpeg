// SPDX-License-Identifier: Apache-2.0
//! Written pcurves on a carrier the writer normalizes to a positive radius.

#![allow(clippy::unwrap_used)]

use std::f64::consts::{FRAC_PI_2, PI};
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::decode::{pcurve_uv, surface_point_solved};
use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::ids::{PcurveId, VertexId};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::topology::PcurveUse;
use cadmpeg_ir::CadIr;

use crate::test_support::exchange::export;
use crate::test_support::revolved_wedge::{revolved_wedge, Piece};
use crate::StepCodec;

const EPS_PCURVE_VERTEX: f64 = 1.0e-9;
const ANGLES: [f64; 2] = [0.3, 1.5];

/// A line pcurve `origin + t * direction` on the carrier's signed chart.
type Line = ([f64; 2], [f64; 2]);

/// A line pcurve on each of the four edges of the swept face of profile piece
/// `arc`: `swept` on its two circles and `meridian` on its side-plane arcs.
fn with_line_pcurves(mut ir: CadIr, arc: usize, swept: [Line; 2], meridian: [Line; 2]) -> CadIr {
    let uses = [
        (format!("swept-{arc}"), swept[0]),
        (format!("swept-{}", arc + 1), swept[1]),
        (format!("meridian-{arc}-0"), meridian[0]),
        (format!("meridian-{arc}-1"), meridian[1]),
    ];
    for (edge, (origin, direction)) in uses {
        let id = PcurveId::mint(format!("test:model:pcurve#{edge}")).unwrap();
        ir.model.pcurves.push(Pcurve {
            id: id.clone(),
            geometry: PcurveGeometry::Line(
                LinePcurve::try_new(
                    Point2::new(origin[0], origin[1]),
                    Point2::new(direction[0], direction[1]),
                )
                .unwrap(),
            ),
            metadata: PcurveMetadata::default(),
        });
        let coedge = format!("test:model:coedge#swept-{arc}-{edge}");
        ir.model
            .coedges
            .iter_mut()
            .find(|candidate| candidate.id.as_str() == coedge)
            .unwrap()
            .pcurves
            .push(PcurveUse {
                pcurve: id,
                isoparametric: Some(true),
                parameter_range: None,
            });
    }
    ir.finalize();
    ir
}

/// Each written pcurve lifts through the written surface to its edge's vertices
/// at both ends of the edge's range.
fn assert_pcurves_reach_their_vertices(source: &CadIr) {
    let decoded = StepCodec::default()
        .decode(
            &mut Cursor::new(export(source).into_bytes()),
            &DecodeOptions::default(),
        )
        .expect("decode the written wedge")
        .into_parts()
        .0;
    let model = &decoded.model;
    let position = |vertex: &VertexId| {
        let vertex = model.vertices.iter().find(|v| &v.id == vertex).unwrap();
        let point = model.points.iter().find(|p| p.id == vertex.point).unwrap();
        point.position().get()
    };
    let mut checked = 0;
    for coedge in &model.coedges {
        let Some(use_) = coedge.pcurves.first() else {
            continue;
        };
        let pcurve = model.pcurves.iter().find(|p| p.id == use_.pcurve).unwrap();
        let surface = model
            .loops
            .iter()
            .find(|ring| ring.id == coedge.owner_loop)
            .and_then(|ring| model.faces.iter().find(|face| face.id == ring.face))
            .and_then(|face| model.surfaces.iter().find(|s| s.id == face.surface))
            .and_then(|surface| surface.geometry.solved())
            .unwrap();
        let edge = model.edges.iter().find(|e| e.id == coedge.edge).unwrap();
        let [first, last] = edge.param_range().unwrap().get();
        for (parameter, vertex) in [(first, &edge.start), (last, &edge.end)] {
            let uv = pcurve_uv(EvaluationAdmission::Standard, &pcurve.geometry, parameter)
                .unwrap()
                .get();
            let lifted = surface_point_solved(EvaluationAdmission::Standard, surface, uv.u, uv.v)
                .unwrap()
                .get();
            let miss = lifted.distance(position(vertex));
            assert!(miss < EPS_PCURVE_VERTEX, "{} misses by {miss}", edge.id);
        }
        checked += 1;
    }
    assert_eq!(checked, 4, "the arc face's four coedges keep their pcurves");
}

/// On a torus of major radius 6 and minor radius -2 about `z = 4`, the signed
/// chart reaches `(6, 2)` in the meridian plane at latitude `π/2` and `(4, 4)`
/// at latitude 0, and latitude falls along the arc's parameter.
#[test]
fn pcurves_on_a_negative_minor_torus_take_the_written_latitude() {
    let ir = revolved_wedge(
        [1.0, 0.0],
        &[
            Piece::Line { to: [12.0, 0.0] },
            Piece::Line { to: [12.0, 2.0] },
            Piece::Line { to: [6.0, 2.0] },
            Piece::Arc {
                to: [4.0, 4.0],
                center: [6.0, 4.0],
                material_inside: false,
            },
            Piece::Line { to: [4.0, 10.0] },
            Piece::Line { to: [1.0, 10.0] },
            Piece::Line { to: [1.0, 0.0] },
        ],
        ANGLES,
    );
    let with_pcurves = with_line_pcurves(
        ir,
        3,
        [([0.0, FRAC_PI_2], [1.0, 0.0]), ([0.0, 0.0], [1.0, 0.0])],
        [
            ([ANGLES[0], FRAC_PI_2], [0.0, -1.0]),
            ([ANGLES[1], FRAC_PI_2], [0.0, -1.0]),
        ],
    );
    assert_pcurves_reach_their_vertices(&with_pcurves);
}

/// On a sphere of radius -13 about `z = 15`, the signed chart reaches the point
/// at angle `a` off the outward radial direction of the side plane at `w` at
/// `(w + π, -a)`, and `a` falls along the arc's parameter.
#[test]
fn pcurves_on_a_negative_radius_sphere_take_the_written_chart() {
    let end_z = 15.0 - 168.0_f64.sqrt();
    let ir = revolved_wedge(
        [1.0, 0.0],
        &[
            Piece::Line { to: [14.0, 0.0] },
            Piece::Line { to: [14.0, 10.0] },
            Piece::Line { to: [12.0, 10.0] },
            Piece::Arc {
                to: [1.0, end_z],
                center: [0.0, 15.0],
                material_inside: false,
            },
            Piece::Line { to: [1.0, 0.0] },
        ],
        ANGLES,
    );
    let start = -(-5.0_f64).atan2(12.0);
    let end = -(end_z - 15.0).atan2(1.0);
    let with_pcurves = with_line_pcurves(
        ir,
        3,
        [([PI, start], [1.0, 0.0]), ([PI, end], [1.0, 0.0])],
        [
            ([ANGLES[0] + PI, start], [0.0, 1.0]),
            ([ANGLES[1] + PI, start], [0.0, 1.0]),
        ],
    );
    assert_pcurves_reach_their_vertices(&with_pcurves);
}
