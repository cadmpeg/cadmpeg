// SPDX-License-Identifier: Apache-2.0
//! Measured body geometry across a Rhino write round trip, compared without
//! entity identities.

use std::io::Cursor;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::legacy_cache::FitTolerance;
use cadmpeg_ir::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::geometry::pcurve::{LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::topology::{PcurveUse, Sense};
use cadmpeg_ir::CadIr;
use cadmpeg_measure::test_support::round_trip::measures_agree;

use crate::{RhinoArchiveVersion, RhinoCodec};

/// Edge length of [`cadmpeg_ir::examples::unit_cube`] in millimeters.
const CUBE_SIDE: f64 = 10.0;

/// Rise of the center control point of the bulged top face above the cube.
const BULGE_RISE: f64 = 3.0;

/// Allowance of the measured volume against its expression beyond its
/// numeric error, per unit of the expected volume.
const VOLUME_FLOOR: f64 = 1.0e-9;

/// Model-space deviation that each top-face pcurve states.
const TOP_PCURVE_FIT: f64 = 1.0e-4;

/// The unit cube with no face names, which the Rhino writer refuses.
fn unnamed_cube() -> CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    for face in &mut ir.geometry.faces {
        face.name = None;
    }
    ir
}

/// `ir` written as each Rhino archive version and decoded again.
fn round_trips(ir: &CadIr) -> Vec<(RhinoArchiveVersion, CadIr)> {
    [
        RhinoArchiveVersion::V5,
        RhinoArchiveVersion::V6,
        RhinoArchiveVersion::V7,
        RhinoArchiveVersion::V8,
    ]
    .into_iter()
    .map(|version| {
        let mut bytes = Vec::new();
        RhinoCodec
            .plan(
                EncodeInput::new(ir, None),
                TargetRequest::Explicit(version.descriptor().id.as_str()),
            )
            .and_then(|plan| plan.write_to(&mut bytes))
            .expect("write the archive");
        let decoded = RhinoCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode the written archive");
        (version, decoded.ir().clone())
    })
    .collect()
}

/// The unit cube whose top face lies on a biquadratic NURBS surface. The
/// surface keeps the four straight top edges, with `u = x` and `v = y` on
/// them, and its center control point rises by [`BULGE_RISE`].
fn bulged_cube() -> CadIr {
    let mut ir = unnamed_cube();
    let (s, half) = (CUBE_SIDE, CUBE_SIDE / 2.0);
    let poles = [0.0, half, s]
        .into_iter()
        .map(|x| {
            [0.0, half, s]
                .into_iter()
                .map(|y| {
                    let rise = if x == half && y == half {
                        BULGE_RISE
                    } else {
                        0.0
                    };
                    Point3::new(x, y, s + rise)
                })
                .collect()
        })
        .collect();
    let axis = || NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, s, s, s], false);
    let surface =
        NurbsSurface::from_lanes(axis(), axis(), NurbsSurfaceLanes::new(poles, None), false)
            .expect("valid bulged surface");
    let top = ir
        .geometry
        .faces
        .iter()
        .find(|face| face.surface.as_str().ends_with("#top"))
        .expect("the cube has a top face")
        .clone();
    ir.geometry
        .surfaces
        .iter_mut()
        .find(|candidate| candidate.id == top.surface)
        .expect("the top face surface")
        .geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface));
    let top_loops: Vec<_> = ir
        .geometry
        .loops
        .iter()
        .filter(|loop_| loop_.face == top.id)
        .map(|loop_| loop_.id.clone())
        .collect();
    for index in 0..ir.geometry.coedges.len() {
        let coedge = &ir.geometry.coedges[index];
        if !top_loops.contains(&coedge.owner_loop) {
            continue;
        }
        let edge = ir
            .geometry
            .edges
            .iter()
            .find(|edge| edge.id == coedge.edge)
            .expect("cube edge");
        let domain = edge.param_range().expect("cube edge domain");
        let (start, end) = if coedge.sense == Sense::Forward {
            (
                &edge.ends.as_ref().unwrap().start,
                &edge.ends.as_ref().unwrap().end,
            )
        } else {
            (
                &edge.ends.as_ref().unwrap().end,
                &edge.ends.as_ref().unwrap().start,
            )
        };
        let position = |id: &cadmpeg_ir::ids::VertexId| {
            let vertex = ir
                .geometry
                .vertices
                .iter()
                .find(|vertex| vertex.id == *id)?;
            ir.geometry
                .points
                .iter()
                .find(|point| point.id == vertex.point)
                .map(|point| point.position().get())
        };
        let start = position(start).expect("cube vertex");
        let end = position(end).expect("cube vertex");
        let scale = domain[1] - domain[0];
        let direction = Point2::new((end.x - start.x) / scale, (end.y - start.y) / scale);
        let origin = Point2::new(
            start.x - direction.u * domain[0],
            start.y - direction.v * domain[0],
        );
        let id: cadmpeg_ir::ids::PcurveId = format!("cadir:model:pcurve#top.{index}")
            .try_into()
            .expect("valid identity");
        ir.geometry.pcurves.push(Pcurve {
            id: id.clone(),
            geometry: PcurveGeometry::Line(
                LinePcurve::try_new(origin, direction).expect("top pcurve line"),
            ),
            metadata: PcurveMetadata::general(
                None,
                Some(domain),
                Some(FitTolerance::try_new(TOP_PCURVE_FIT).expect("finite fit tolerance")),
            ),
            native_ref: None,
        });
        ir.geometry.coedges[index].pcurves = vec![PcurveUse {
            pcurve: id,
            isoparametric: None,
            parameter_range: None,
        }];
    }
    ir.finalize();
    ir
}

#[test]
fn planar_cube_round_trip_keeps_measured_geometry() {
    let ir = unnamed_cube();
    for (_, written) in round_trips(&ir) {
        assert_eq!(written.geometry.bodies.len(), ir.geometry.bodies.len());
        measures_agree("cube", &ir, &written).expect("the round trip keeps its measured geometry");
    }
}

/// The bulge adds `rise × s² / 9` to the cube volume `s³`: the center
/// basis function of a quadratic Bezier patch integrates to `1/9` of the
/// patch area.
#[test]
fn bulged_cube_round_trip_keeps_measured_geometry() {
    let ir = bulged_cube();
    let expected = CUBE_SIDE.powi(3) + BULGE_RISE * CUBE_SIDE.powi(2) / 9.0;
    for (_, written) in round_trips(&ir) {
        assert_eq!(written.geometry.bodies.len(), ir.geometry.bodies.len());
        let (_, before, _) = measures_agree("bulged cube", &ir, &written)
            .expect("the round trip keeps its measured geometry");
        let volume = before.bodies()[0]
            .volume()
            .as_ref()
            .and_then(|quantity| quantity.measured())
            .expect("measured volume");
        let allowed = volume.numeric_error().get() + VOLUME_FLOOR * expected;
        assert!((volume.value().value.get() - expected).abs() <= allowed);
    }
}
