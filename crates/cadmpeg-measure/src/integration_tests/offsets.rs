// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, LegacyExtensionFlags, ProceduralSurface, ProceduralSurfaceDefinition,
    SolvedCurveGeometry, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::{BodyKind, Sense};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::select::Selection;
use crate::test_support::builders::{assert_measured, cuboid, measure, Builder};

fn make_offset(ir: &mut CadIr, surface: &SurfaceId, distance: f64) {
    let index = ir
        .model
        .surfaces
        .iter()
        .position(|candidate| candidate.id == *surface)
        .unwrap();
    let support = SurfaceId::mint(format!(
        "test:model:offset-support#{}",
        ir.model.surfaces.len()
    ))
    .unwrap();
    let construction = ProceduralSurfaceId::mint(format!(
        "test:model:offset#{}",
        ir.model.procedural_surfaces.len()
    ))
    .unwrap();
    let geometry = ir.model.surfaces[index].geometry.clone();
    ir.model.surfaces.push(Surface {
        id: support.clone(),
        source_object: None,
        geometry,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        construction.clone(),
        ProceduralSurfaceDefinition::Offset(OffsetSurfaceConstruction::legacy(
            support,
            FiniteReal::new(distance).unwrap(),
            None,
            None,
            false,
            LegacyExtensionFlags::Absent {},
            None,
        )),
        None,
    ));
    ir.model.surfaces[index].geometry = SurfaceGeometry::Procedural {
        construction,
        cache: None,
    };
}

#[test]
fn cacheless_nurbs_offsets_measure_curved_area_with_both_distance_signs() {
    let radius = 7.25;
    let height = 4.0;
    for distance in [-2.5, 2.5] {
        let mut builder = Builder::new();
        let column = |x, y, weight| vec![([x, y, 0.0], weight), ([x, y, height], weight)];
        let surface = builder.nurbs(
            (2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),
            (1, vec![0.0, 0.0, 1.0, 1.0]),
            &[
                column(radius, 0.0, 1.0),
                column(radius, radius, std::f64::consts::FRAC_1_SQRT_2),
                column(0.0, radius, 1.0),
            ],
        );
        builder.face(&surface, Sense::Forward, &[], false);
        let mut ir = builder.finish(BodyKind::Sheet);
        make_offset(&mut ir, &surface, distance);
        ir.finalize();
        let report = measure(
            &ir,
            &Selection {
                all_faces: true,
                ..Selection::default()
            },
        );
        let [face] = report.faces().as_slice() else {
            panic!("one face")
        };
        assert_measured(
            face.measures().area(),
            (radius + distance) * height * std::f64::consts::FRAC_PI_2,
            radius * height,
        );
    }
}

#[test]
fn cacheless_offset_cuboid_preserves_flux_and_placement() {
    for distance in [-2.5, 2.5] {
        for scale in [1.0, 2.0] {
            let mut ir = cuboid([0.0; 3], [2.0, 3.0, 4.0]);
            let surfaces = ir
                .model
                .surfaces
                .iter()
                .map(|surface| surface.id.clone())
                .collect::<Vec<_>>();
            for (ordinal, id) in surfaces.into_iter().enumerate() {
                let surface = ir
                    .model
                    .surfaces
                    .iter_mut()
                    .find(|surface| surface.id == id)
                    .unwrap();
                let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) =
                    &surface.geometry
                else {
                    panic!("plane")
                };
                let origin = plane.origin().get();
                let normal = *plane.frame().axis().as_raw();
                let eu = *plane.frame().reference().as_raw();
                let ev = normal.cross(eu);
                let rows = [-8.0, 8.0]
                    .map(|u| {
                        [-8.0, 8.0]
                            .map(|v| {
                                Point3::new(
                                    origin.x - distance * normal.x + u * eu.x + v * ev.x,
                                    origin.y - distance * normal.y + u * eu.y + v * ev.y,
                                    origin.z - distance * normal.z + u * eu.z + v * ev.z,
                                )
                            })
                            .to_vec()
                    })
                    .to_vec();
                if ordinal % 2 == 0 {
                    let profile =
                        CurveId::mint(format!("test:model:sweep-profile#{ordinal}")).unwrap();
                    let construction =
                        ProceduralSurfaceId::mint(format!("test:model:linear-sweep#{ordinal}"))
                            .unwrap();
                    let poles = [-8.0, 8.0]
                        .map(|u| {
                            Point3::new(
                                origin.x - distance * normal.x + u * eu.x,
                                origin.y - distance * normal.y + u * eu.y,
                                origin.z - distance * normal.z + u * eu.z,
                            )
                        })
                        .to_vec();
                    surface.geometry = SurfaceGeometry::Procedural {
                        construction: construction.clone(),
                        cache: None,
                    };
                    ir.model.curves.push(Curve {
                        id: profile.clone(),
                        source_object: None,
                        parameter_range: None,
                        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                            NurbsCurve::from_lanes(
                                1,
                                vec![-8.0, -8.0, 8.0, 8.0],
                                poles,
                                None,
                                false,
                            )
                            .unwrap(),
                        )),
                    });
                    ir.model.procedural_surfaces.push(ProceduralSurface::new(
                        construction,
                        ProceduralSurfaceDefinition::LinearSweep(
                            cadmpeg_ir::geometry::surface_payloads::LinearSweepSurfaceConstruction::try_new(
                                profile,
                                cadmpeg_ir::math::Vector3::new(1000.0 * ev.x, 1000.0 * ev.y, 1000.0 * ev.z),
                            ).unwrap(),
                        ),
                        None,
                    ));
                } else {
                    surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
                        NurbsSurface::from_lanes(
                            NurbsSurfaceAxis::new(1, vec![-8.0, -8.0, 8.0, 8.0], false),
                            NurbsSurfaceAxis::new(1, vec![-8.0, -8.0, 8.0, 8.0], false),
                            NurbsSurfaceLanes::new(rows, None),
                            false,
                        )
                        .unwrap(),
                    ));
                }
                make_offset(&mut ir, &id, distance);
            }
            ir.model.bodies[0].transform = Some(
                Transform::affine([
                    [scale, 0.0, 0.0, 17.0],
                    [0.0, scale, 0.0, -11.0],
                    [0.0, 0.0, scale, 5.0],
                ])
                .unwrap(),
            );
            ir.finalize();
            let report = measure(&ir, &Selection::default());
            let [body] = report.bodies().as_slice() else {
                panic!("one body")
            };
            assert_measured(body.area().as_ref().unwrap(), 52.0 * scale * scale, 52.0);
            assert_measured(
                body.volume().as_ref().unwrap(),
                24.0 * scale * scale * scale,
                24.0,
            );
        }
    }
}

#[test]
fn a_whole_periodic_offset_measures_a_closed_toroidal_shell() {
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
    let knots = vec![
        0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
    ];
    let major = 8.0;
    let minor = 2.0;
    for distance in [-0.25, 0.25] {
        let mut builder = Builder::new();
        let surface = builder.plane([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
        builder.face(&surface, Sense::Forward, &[], false);
        let mut ir = builder.finish(BodyKind::Solid);
        let rows = circle
            .iter()
            .map(|(x, y)| {
                circle
                    .iter()
                    .map(|(r, z)| {
                        Point3::new((major + minor * r) * x, (major + minor * r) * y, minor * z)
                    })
                    .collect()
            })
            .collect();
        let weights = (0..9)
            .map(|u| {
                (0..9)
                    .map(|v| {
                        let weight = |i| {
                            if i % 2 == 0 {
                                1.0
                            } else {
                                std::f64::consts::FRAC_1_SQRT_2
                            }
                        };
                        weight(u) * weight(v)
                    })
                    .collect()
            })
            .collect();
        ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
            NurbsSurface::from_lanes(
                NurbsSurfaceAxis::new(2, knots.clone(), true),
                NurbsSurfaceAxis::new(2, knots.clone(), true),
                NurbsSurfaceLanes::new(rows, Some(weights)),
                false,
            )
            .unwrap(),
        ));
        make_offset(&mut ir, &surface, distance);
        ir.finalize();
        let report = measure(&ir, &Selection::default());
        let [body] = report.bodies().as_slice() else {
            panic!("one body")
        };
        let pi_squared = std::f64::consts::PI * std::f64::consts::PI;
        let radius = minor + distance;
        assert_measured(
            body.area().as_ref().unwrap(),
            4.0 * pi_squared * major * radius,
            major * radius,
        );
        assert_measured(
            body.volume().as_ref().unwrap(),
            2.0 * pi_squared * major * radius * radius,
            major * radius * radius,
        );
        assert!(*body.shells()[0].closed());
        assert_eq!(*body.edges(), 0);
    }
}
