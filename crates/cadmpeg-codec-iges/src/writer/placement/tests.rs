// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::geometry::sampled::PolygonalSurface;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::{BodyId, FaceId, RegionId, ShellId, SurfaceId};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Body, BodyKind, Face, FaceLoops, Region, Sense, Shell};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use super::placed_bodies;

/// The largest stretch of the shear `(x, y, z) ↦ (x + y, y, z)`, the golden
/// ratio.
const SHEAR_STRETCH: f64 = 1.618_033_988_749_895;
/// Bound on the error of a placed vertex or deflection.
const PLACED: f64 = 1.0e-12;
const DEFLECTION: f64 = 0.1;

/// A sheet body under `transform` whose one face lies on a polygonal
/// surface of two triangles.
fn polygonal_sheet(transform: Option<Transform>) -> CadIr {
    let mut ir = CadIr::empty();
    let geometry = &mut ir.geometry;
    let id = |kind: &str| format!("test:model:{kind}#sheet");
    geometry.surfaces.push(Surface {
        id: SurfaceId::try_from(id("surface")).expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Polygonal(
            PolygonalSurface::new(
                vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(4.0, 0.0, 1.0),
                    Point3::new(4.0, 3.0, 2.0),
                    Point3::new(0.0, 3.0, 1.0),
                ],
                vec![[0, 1, 2], [0, 2, 3]],
                DEFLECTION,
            )
            .expect("a polygonal surface"),
        )),
        source_object: None,
        native_ref: None,
    });
    geometry.faces.push(Face {
        id: FaceId::try_from(id("face")).expect("identity grammar"),
        shell: ShellId::try_from(id("shell")).expect("identity grammar"),
        surface: SurfaceId::try_from(id("surface")).expect("identity grammar"),
        sense: Sense::Forward,
        loops: FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
        native_ref: None,
    });
    geometry.shells.push(Shell::new(
        ShellId::try_from(id("shell")).expect("identity grammar"),
        RegionId::try_from(id("region")).expect("identity grammar"),
    ));
    geometry.regions.push(Region::new(
        RegionId::try_from(id("region")).expect("identity grammar"),
        BodyId::try_from(id("body")).expect("identity grammar"),
        vec![ShellId::try_from(id("shell")).expect("identity grammar")],
    ));
    geometry.bodies.push(Body {
        id: BodyId::try_from(id("body")).expect("identity grammar"),
        kind: BodyKind::Sheet,
        transform,
        name: None,
        color: None,
        visible: None,
        native_ref: None,
        unstated: None,
    });
    ir.finalize();
    ir
}

fn polygonal(ir: &CadIr) -> &PolygonalSurface {
    match &ir.geometry.surfaces[0].geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Polygonal(polygonal)) => polygonal,
        other => panic!("the support is placed as {other:?}"),
    }
}

#[test]
fn a_polygonal_surface_maps_its_vertices_and_scales_its_deflection_by_the_largest_stretch() {
    let rows = [
        [1.0, 1.0, 0.0, 2.0],
        [0.0, 1.0, 0.0, -1.0],
        [0.0, 0.0, 1.0, 0.5],
    ];
    let source = polygonal_sheet(Transform::affine(rows));
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let (placed, _) = placed_bodies(&ctx, &source).expect("the body is placed");
    let placed = polygonal(placed.as_ref());
    let expected = polygonal(&source)
        .vertices()
        .iter()
        .map(|vertex| {
            let vertex = vertex.get();
            rows.map(|row| {
                row[0].mul_add(vertex.x, row[1].mul_add(vertex.y, row[2] * vertex.z)) + row[3]
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(placed.vertices().len(), expected.len());
    for (vertex, expected) in placed.vertices().iter().zip(&expected) {
        let vertex = vertex.get();
        assert!(
            [vertex.x, vertex.y, vertex.z]
                .iter()
                .zip(expected)
                .all(|(actual, expected)| (actual - expected).abs() <= PLACED),
            "{vertex:?} differs from {expected:?}"
        );
    }
    assert_eq!(placed.triangles(), polygonal(&source).triangles());
    assert!(
        (placed.chordal_deflection().get() - DEFLECTION * SHEAR_STRETCH).abs() <= PLACED,
        "the deflection {} is not scaled by the largest stretch",
        placed.chordal_deflection().get()
    );
}
