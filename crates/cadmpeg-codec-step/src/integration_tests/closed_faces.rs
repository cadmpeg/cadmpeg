// SPDX-License-Identifier: Apache-2.0
//! Exact whole-carrier solid round trips.

use std::f64::consts::PI;
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{SphereSurface, TorusSurface};
use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{BodyId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{
    Body, BodyKind, Face, FaceLoops, Loop, LoopBoundary, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::CadIr;
use cadmpeg_measure::{MeasureInput, MeasureOptions, Measurer, Selection};

use crate::export::write_step;
use crate::{StepCodec, StepSchema, StepWriteOptions};

const EPS_VOLUME: f64 = 1.0e-8;

fn id<T>(
    mint: fn(String) -> Result<T, cadmpeg_ir::ids::IdentityError>,
    kind: &str,
    index: usize,
) -> T {
    mint(format!("test:closed:{kind}#{index}")).expect("identity")
}

fn solid(ir: &mut CadIr, surfaces: Vec<(SolvedSurfaceGeometry, Sense)>) {
    let b = ir.model.bodies.len();
    let body = id(BodyId::mint, "body", b);
    let region = id(RegionId::mint, "region", b);
    let mut shells = Vec::new();
    for (geometry, sense) in surfaces {
        let index = ir.model.faces.len();
        let shell = id(ShellId::mint, "shell", index);
        let surface = id(SurfaceId::mint, "surface", index);
        let face = id(FaceId::mint, "face", index);
        ir.model.surfaces.push(Surface {
            id: surface.clone(),
            geometry: SurfaceGeometry::Solved(geometry),
            source_object: None,
        });
        ir.model.faces.push(Face {
            id: face.clone(),
            shell: shell.clone(),
            surface,
            sense,
            loops: FaceLoops::unspecified(Vec::new()),
            name: None,
            color: None,
            tolerance: None,
        });
        ir.model
            .shells
            .push(Shell::with_face(shell.clone(), region.clone(), face));
        shells.push(shell);
    }
    ir.model.regions.push(Region {
        id: region.clone(),
        body: body.clone(),
        shells,
    });
    ir.model.bodies.push(Body {
        id: body,
        kind: BodyKind::Solid,
        regions: vec![region],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
}

fn sphere(radius: f64) -> SolvedSurfaceGeometry {
    SolvedSurfaceGeometry::Sphere(
        SphereSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            radius,
        )
        .expect("sphere"),
    )
}

fn torus(radius: f64) -> SolvedSurfaceGeometry {
    SolvedSurfaceGeometry::Torus(
        TorusSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            4.0,
            radius,
        )
        .expect("torus"),
    )
}

fn nurbs(spheroid: bool, reversed: bool) -> SolvedSurfaceGeometry {
    let circle = [
        (1.0, 0.0),
        (1.0, 1.0),
        (0.0, 1.0),
        (-1.0, 1.0),
        (-1.0, 0.0),
        (-1.0, -1.0),
        (0.0, -1.0),
        (1.0, -1.0),
        (1.0, 0.0),
    ];
    let weights = [
        1.0,
        std::f64::consts::FRAC_1_SQRT_2,
        1.0,
        std::f64::consts::FRAC_1_SQRT_2,
        1.0,
        std::f64::consts::FRAC_1_SQRT_2,
        1.0,
        std::f64::consts::FRAC_1_SQRT_2,
        1.0,
    ];
    let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 4.0];
    let profile = if spheroid {
        vec![(0.0, -3.0), (2.0, -3.0), (2.0, 0.0), (2.0, 3.0), (0.0, 3.0)]
    } else {
        circle.iter().map(|(r, z)| (4.0 + r, *z)).collect()
    };
    let mut grid = Vec::new();
    let mut grid_weights = Vec::new();
    for (i, (x, y)) in circle.into_iter().enumerate() {
        grid.push(
            profile
                .iter()
                .map(|(r, z)| Point3::new(r * x, r * y, *z))
                .collect(),
        );
        grid_weights.push(
            profile
                .iter()
                .enumerate()
                .map(|(j, _)| weights[i] * weights[j])
                .collect(),
        );
    }
    let u = NurbsSurfaceAxis::new(2, knots.clone(), true);
    let v = if spheroid {
        NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 2.0], false)
    } else {
        NurbsSurfaceAxis::new(2, knots, true)
    };
    SolvedSurfaceGeometry::Nurbs(
        NurbsSurface::new(
            u,
            v,
            NurbsPoleGrid::from_lanes(grid, Some(grid_weights)).expect("grid"),
            reversed,
        )
        .expect("NURBS"),
    )
}

fn poles(ir: &mut CadIr) {
    for (index, z) in [-3.0, 3.0].into_iter().enumerate() {
        let point = id(PointId::mint, "point", index);
        let vertex = id(VertexId::mint, "vertex", index);
        let lp = id(LoopId::mint, "loop", index);
        ir.model.points.push(Point::new(
            point.clone(),
            FinitePoint3::new(Point3::new(0.0, 0.0, z)).expect("point"),
            None,
        ));
        ir.model.vertices.push(Vertex {
            id: vertex.clone(),
            point,
            tolerance: None,
        });
        ir.model.loops.push(Loop {
            id: lp.clone(),
            face: ir.model.faces[0].id.clone(),
            boundary: LoopBoundary::Vertex {
                vertex,
                pcurves: Vec::new(),
            },
        });
        let mut loops: Vec<_> = ir.model.faces[0].loops.iter().cloned().collect();
        loops.push(lp);
        ir.model.faces[0].loops = FaceLoops::unspecified(loops);
    }
}

fn round_trip(ir: &CadIr, volumes: &[f64]) {
    for schema in [StepSchema::Ap214, StepSchema::Ap242Edition3] {
        let mut bytes = Vec::new();
        let report = write_step(ir, &mut bytes, schema, &StepWriteOptions::default())
            .expect("write closed surfaces");
        assert!(
            !report
                .losses
                .iter()
                .any(|loss| loss.code.local_code().starts_with("topology.")),
            "{:?}",
            report.losses
        );
        let text = String::from_utf8(bytes.clone()).expect("text");
        assert_eq!(
            text.matches("= CLOSED_SHELL(").count(),
            ir.model.shells.len()
        );
        let decoded = StepCodec::default()
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode");
        let ir2 = decoded.ir();
        assert_eq!(ir2.model.faces.len(), ir.model.faces.len());
        assert_eq!(ir2.model.bodies.len(), volumes.len());
        assert!(ir2
            .model
            .bodies
            .iter()
            .all(|body| body.kind == BodyKind::Solid));
        let arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0).expect("context");
        let measured = Measurer::new(
            MeasureInput {
                ir: ir2,
                fidelity: None,
                geometry_transferred: true,
            },
            MeasureOptions::default(),
        )
        .measure(&ctx, &Selection::default())
        .expect("measure");
        for (body, expected) in measured.bodies().iter().zip(volumes) {
            let volume = body
                .volume()
                .as_ref()
                .and_then(|v| v.measured())
                .unwrap_or_else(|| panic!("volume: {body:?}"));
            assert!(
                (volume.value().value.get() - expected).abs()
                    <= volume.numeric_error().get() + EPS_VOLUME * expected.abs(),
                "{body:?}"
            );
        }
    }
}

#[test]
fn full_sphere_round_trips_with_polarity() {
    for radius in [2.0, -2.0] {
        let mut ir = CadIr::empty();
        solid(
            &mut ir,
            vec![(
                sphere(radius),
                if radius > 0.0 {
                    Sense::Forward
                } else {
                    Sense::Reversed
                },
            )],
        );
        round_trip(&ir, &[4.0 * PI * 8.0 / 3.0]);
    }
}

#[test]
fn full_torus_round_trips_with_polarity() {
    for radius in [1.0, -1.0] {
        let mut ir = CadIr::empty();
        solid(&mut ir, vec![(torus(radius), Sense::Forward)]);
        round_trip(&ir, &[8.0 * PI * PI]);
    }
}

#[test]
fn spheroid_with_two_poles_round_trips_with_polarity() {
    for reversed in [false, true] {
        let mut ir = CadIr::empty();
        solid(
            &mut ir,
            vec![(
                nurbs(true, reversed),
                if reversed {
                    Sense::Reversed
                } else {
                    Sense::Forward
                },
            )],
        );
        poles(&mut ir);
        round_trip(&ir, &[16.0 * PI]);
    }
}

#[test]
fn doubly_periodic_nurbs_torus_round_trips_with_polarity() {
    for reversed in [false, true] {
        let mut ir = CadIr::empty();
        solid(
            &mut ir,
            vec![(
                nurbs(false, reversed),
                if reversed {
                    Sense::Reversed
                } else {
                    Sense::Forward
                },
            )],
        );
        round_trip(&ir, &[8.0 * PI * PI]);
    }
}

#[test]
fn hollow_sphere_and_inner_ball_round_trip() {
    let mut ir = CadIr::empty();
    solid(
        &mut ir,
        vec![
            (sphere(5.0), Sense::Forward),
            (sphere(4.0), Sense::Reversed),
        ],
    );
    solid(&mut ir, vec![(sphere(2.0), Sense::Forward)]);
    round_trip(
        &ir,
        &[4.0 * PI * (125.0 - 64.0) / 3.0, 4.0 * PI * 8.0 / 3.0],
    );
}

#[test]
fn closed_surface_sense_is_retained() {
    for geometry in [sphere(2.0), torus(1.0), nurbs(false, false)] {
        for sense in [Sense::Forward, Sense::Reversed] {
            let mut ir = CadIr::empty();
            solid(&mut ir, vec![(geometry.clone(), sense)]);
            let mut bytes = Vec::new();
            write_step(
                &ir,
                &mut bytes,
                StepSchema::Ap214,
                &StepWriteOptions::default(),
            )
            .expect("write either face sense");
            let decoded = StepCodec::default()
                .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
                .expect("decode either face sense");
            assert_eq!(decoded.ir().model.faces[0].sense, sense);
        }
    }
}

#[test]
fn self_intersecting_loopless_torus_keeps_its_loss() {
    for minor in [4.0, 5.0] {
        let mut ir = CadIr::empty();
        let surface = TorusSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            4.0,
            minor,
        )
        .expect("torus carrier");
        solid(
            &mut ir,
            vec![(SolvedSurfaceGeometry::Torus(surface), Sense::Forward)],
        );
        let mut bytes = Vec::new();
        let report = write_step(
            &ir,
            &mut bytes,
            StepSchema::Ap214,
            &StepWriteOptions::default(),
        )
        .expect("report the omitted face");
        assert!(report
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::StepLossCode::FaceNoWritableBounds.kind()));
        assert!(!String::from_utf8(bytes)
            .expect("text")
            .contains("= ADVANCED_FACE("));
    }
}
