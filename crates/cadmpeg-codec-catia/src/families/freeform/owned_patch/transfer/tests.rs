// SPDX-License-Identifier: Apache-2.0

use super::*;

mod native_accuracy;
mod shared_recovery;
use crate::families::freeform::owned_patch::{
    line_pcurve, sheet_carrier::Carrier, SheetBoundaryRecovery,
};
use cadmpeg_ir::geometry::{analytic::PlaneSurface, nurbs::NurbsCurve, SolvedSurfaceGeometry};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::report::loss::LossNote;

fn two_face_sheet() -> Vec<Patch> {
    let vertices = [
        [0.0, 0.0],
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [2.0, 0.0],
        [2.0, 1.0],
    ];
    let plane = PlaneSurface::try_new(
        Point3::new(0.0, 0.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    )
    .expect("fixture plane");
    let edge = |edge, a, b, forward| {
        let uv: [[f64; 2]; 2] = [vertices[a], vertices[b]];
        let points = uv.map(|[x, y]| Point3::new(x, y, 0.0));
        Use {
            edge,
            vertices: [(a, points[0]), (b, points[1])],
            curve: NurbsCurve::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                points.to_vec(),
                None,
                false,
            )
            .expect("linear fixture edge"),
            pcurve: line_pcurve([0.0, 1.0], uv).expect("linear fixture pcurve"),
            forward,
            inferred_accuracy: None,
            lift_error_mm: 0.0,
        }
    };
    let patch = |face, forward, sides| Patch {
        face,
        body: (0, 10),
        forward,
        sense_inferred: false,
        surface_fit: None,
        surface: SolvedSurfaceGeometry::Plane(plane),
        priority: 0,
        sides,
        inner_starts: Vec::new(),
        recovery: SheetBoundaryRecovery::default(),
    };
    vec![
        patch(
            20,
            true,
            vec![
                edge(0, 0, 1, true),
                edge(1, 1, 2, true),
                edge(2, 2, 3, true),
                edge(3, 3, 0, true),
            ],
        ),
        patch(
            30,
            true,
            vec![
                edge(4, 1, 4, true),
                edge(5, 4, 5, true),
                edge(6, 5, 2, true),
                edge(1, 1, 2, false),
            ],
        ),
    ]
}

#[test]
fn owned_sheet_emits_one_shared_edge_and_reports_partial_transfer() {
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let loss = crate::test_support::with_service_context(|ctx| {
        append(
            &mut ir,
            &mut annotations,
            two_face_sheet(),
            &mut crate::nurbs::LaneRefusals::new(),
            &mut FamilyEntityAdmission::new(ctx),
        )
    })
    .expect("sheet transfer succeeds")
    .expect("sheet transfer reports partial recovery");

    assert_eq!(loss.code.local_code(), "topology.owned-patch-subset");
    assert!(loss.message.starts_with("Transferred 2 bounded faces"));
    assert_eq!(ir.model.bodies.len(), 1);
    assert_eq!(ir.model.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(ir.model.faces.len(), 2);
    for face in &ir.model.faces {
        assert_outer_winding(&ir, face);
        assert_eq!(face.sense, Sense::Forward);
    }
    assert_eq!(ir.model.edges.len(), 7);
    assert_eq!(ir.model.coedges.len(), 8);
    let shared: Vec<_> = ir
        .model
        .coedges
        .iter()
        .filter(|side| side.radial_next != side.id)
        .collect();
    assert_eq!(shared.len(), 2);
    assert_eq!(shared[0].edge, shared[1].edge);
    assert_ne!(shared[0].owner_loop, shared[1].owner_loop);
    assert_eq!(shared[0].radial_next, shared[1].id);
    assert_eq!(shared[1].radial_next, shared[0].id);
    assert_eq!(shared[0].sense, Sense::Forward);
    assert_eq!(shared[1].sense, Sense::Reversed);
}

#[test]
fn owned_sheet_reports_a_fitted_offset_surface() {
    use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};

    let domain = [[0.0, 1.0]; 2];
    let axis = || NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false);
    let support = NurbsSurface::new(
        axis(),
        axis(),
        NurbsPoleGrid::Polynomial {
            rows: [0.0, 1.0]
                .map(|u| vec![Point3::new(u, 0.0, -1.0), Point3::new(u, 1.0, -1.0)])
                .to_vec(),
        },
        false,
    )
    .expect("planar spline support");
    let offset = Carrier::OffsetTensor(&support, domain, 1.0);
    let mut patches = two_face_sheet();
    patches[0].surface = offset
        .solved(domain, false)
        .expect("fit the offset carrier");
    patches[0].surface_fit = offset.surface_fit();

    let notes = transfer_fit_patches(patches, 2);
    let [fit] = notes.as_slice() else {
        panic!("one fitting loss for the offset face");
    };
    assert_eq!(fit.code.local_code(), "geometry.sheet-surface-fitted");
    assert!(fit.message.contains("1 offset"));
}

#[test]
fn owned_sheet_reports_a_fitted_extrusion() {
    use crate::families::freeform::owned_patch::extrusion::Extrusion;

    let domain = [[0.0, 1.0]; 2];
    let mut chart_bytes = vec![0x03];
    chart_bytes.extend(
        [0.0_f64, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0]
            .into_iter()
            .flat_map(f64::to_le_bytes),
    );
    chart_bytes.extend([0x05, 0x05]);
    let chart = Extrusion::read(&chart_bytes, domain).expect("unit extrusion chart");

    let mut offset_bytes = vec![0x03];
    offset_bytes.extend([0.0_f64, 1.0].into_iter().flat_map(f64::to_le_bytes));
    offset_bytes.push(0x05);
    offset_bytes.extend(
        [1.0_f64, 0.0, 1.0, 0.0, 0.0, 1.0]
            .into_iter()
            .flat_map(f64::to_le_bytes),
    );
    let (_, offset) = chart.offset(&offset_bytes).expect("unit directrix offset");
    let surface = chart
        .surface(&offset, |u| Some(Point3::new(u, 0.0, -1.0)))
        .expect("fit the extrusion directrix");
    let carrier = Carrier::Extrusion(Box::new(surface));
    let mut patches = two_face_sheet();
    patches[0].surface = carrier.solved(domain, false).expect("extrusion carrier");
    patches[0].surface_fit = carrier.surface_fit();

    let notes = transfer_fit_patches(patches, 2);
    let [fit] = notes.as_slice() else {
        panic!("one fitting loss for the extrusion face");
    };
    assert_eq!(fit.code.local_code(), "geometry.sheet-surface-fitted");
    assert!(fit.message.contains("1 extrusion"));
}

#[test]
fn owned_sheet_reports_actual_rolling_ball_rail_movement() {
    use crate::families::a5a8::records::a5_owned_rolling_curves;
    use crate::families::freeform::owned_patch::{fit, Boundary, Member, Side};
    use crate::test_support::test_a5a8::a5_freeform_curve_stream;
    use crate::wire::records::consolidated_records;
    use cadmpeg_ir::features::FinitePoint3;
    use cadmpeg_ir::geometry::ProceduralSurface;
    use cadmpeg_ir::ids::ProceduralSurfaceId;

    let bytes = a5_freeform_curve_stream();
    let records = consolidated_records(&bytes);
    let mut jet = a5_owned_rolling_curves(&bytes, &records)
        .next()
        .expect("rolling-ball fixture");
    jet.sites[1].site = jet.sites[0].site.clone();
    let upper_point =
        |x, y| FinitePoint3::new(Point3::new(x, y, 1.0)).expect("finite upper station point");
    jet.sites[1].site.limit1 = upper_point(1.0, 0.0);
    jet.sites[1].site.limit2 = upper_point(0.0, 1.0);
    jet.sites[1].site.center = upper_point(0.0, 0.0);

    let u = [jet.sites[0].knot.get(), jet.sites[1].knot.get()];
    let radius = 1.000_000_2;
    let chart_end = jet.chart_v_end;
    let carrier = Carrier::rolling_ball_at_radius(jet, radius, chart_end)
        .expect("rolling-ball carrier with adjusted rails");
    let Carrier::OwnedRollingBall { ref definition, .. } = carrier else {
        panic!("owned rolling-ball carrier");
    };
    let mut source = CadIr::empty();
    let construction =
        ProceduralSurfaceId::mint("catia:test:construction#0").expect("fixture construction ID");
    source
        .model
        .procedural_surfaces
        .push(ProceduralSurface::new(
            construction.clone(),
            definition.as_ref().as_ref().clone(),
            None,
        ));
    source.model.surfaces.push(Surface {
        id: SurfaceId::mint("catia:test:surface#0").expect("fixture surface ID"),
        geometry: SurfaceGeometry::Procedural {
            construction,
            cache: None,
        },
        source_object: None,
    });

    let corners = [[u[0], 0.0], [u[1], 0.0], [u[1], radius], [u[0], radius]];
    let boundary = Boundary {
        carrier: 0,
        sides: [0, 1, 2, 3].map(|edge| {
            let vertices = [edge, (edge + 1) % 4];
            Side {
                edge,
                vertices,
                uv: vertices.map(|index| corners[index]),
            }
        }),
        u,
        radius,
    };
    let member = Member {
        body: (0, 10),
        forward: true,
    };
    let mut patch = fit(&source, 0, 20, member, &boundary).expect("fit the rolling-ball face");
    patch.surface_fit = carrier.surface_fit();

    let notes = transfer_fit_patches(vec![patch], 1);
    assert!(notes.iter().any(|note| {
        note.code.local_code() == "geometry.sheet-surface-fitted"
            && note.message.contains("1 rolling-ball")
    }));
    let adjusted = notes
        .iter()
        .find(|note| note.code.local_code() == "geometry.rolling-ball-rails-adjusted")
        .expect("rail-adjustment loss");
    assert!(adjusted.message.contains("1 rolling-ball sheet faces"));
    assert!(adjusted.message.contains(&format!("{} mm", radius - 1.0)));
}

fn transfer_fit_patches(patches: Vec<Patch>, faces: usize) -> Vec<LossNote> {
    let mut ir = CadIr::empty();
    let mut refusal = LaneRefusals::new();
    crate::test_support::with_service_context(|ctx| {
        append(
            &mut ir,
            &mut AnnotationBuilder::new(),
            patches,
            &mut refusal,
            &mut FamilyEntityAdmission::new(ctx),
        )
    })
    .expect("transfer the sheet");
    assert_eq!(ir.model.faces.len(), faces);
    refusal.take_notes()
}

fn assert_outer_winding(ir: &CadIr, face: &Face) {
    let FaceLoops::Classified { outer, .. } = &face.loops else {
        panic!("fixture face must have an outer loop");
    };
    let boundary = ir
        .model
        .loops
        .iter()
        .find(|loop_| &loop_.id == outer)
        .expect("emitted outer loop");
    let surface = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)
        .expect("emitted face carrier");
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) = &surface.geometry else {
        panic!("fixture carrier must be planar");
    };
    let normal = *plane.frame().axis().as_raw();
    let oriented_normal = match face.sense {
        Sense::Forward => normal,
        Sense::Reversed => normal.scale(-1.0),
    };
    let point = |vertex_id: &VertexId| {
        let vertex = ir
            .model
            .vertices
            .iter()
            .find(|vertex| &vertex.id == vertex_id)
            .expect("emitted vertex");
        ir.model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .expect("emitted point")
            .position()
            .get()
    };
    let origin = plane.origin().get();
    let area_twice: f64 = boundary
        .coedges()
        .iter()
        .map(|id| {
            let coedge = ir
                .model
                .coedges
                .iter()
                .find(|coedge| &coedge.id == id)
                .expect("emitted coedge");
            let edge = ir
                .model
                .edges
                .iter()
                .find(|edge| edge.id == coedge.edge)
                .expect("emitted edge");
            let (start, end) = match coedge.sense {
                Sense::Forward => (&edge.start, &edge.end),
                Sense::Reversed => (&edge.end, &edge.start),
            };
            point(start)
                .vector_from(origin)
                .cross(point(end).vector_from(origin))
                .dot(oriented_normal)
        })
        .sum();
    assert!(
        area_twice > 0.0,
        "outer loop on {} must run counterclockwise about its oriented normal",
        face.id
    );
}

#[test]
fn planar_transfer_preserves_pcurves_with_native_accuracy() {
    use crate::families::freeform::owned_patch::InferredEdgeAccuracy;

    let mut patches = two_face_sheet();
    patches[0].sides[0].inferred_accuracy = Some(InferredEdgeAccuracy {
        source: 17,
        bound_mm: 0.02,
    });
    patches[0].sides[0].lift_error_mm = 0.02;
    let mut ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    crate::test_support::with_service_context(|ctx| {
        append(
            &mut ir,
            &mut annotations,
            patches,
            &mut crate::nurbs::LaneRefusals::new(),
            &mut FamilyEntityAdmission::new(ctx),
        )
    })
    .expect("transfer");
    assert_eq!(ir.model.pcurves.len(), 8);
    assert!(ir.model.coedges.iter().all(|side| side.pcurves.len() == 1));
    assert_eq!(
        ir.model.edges[0].tolerance.map(PositiveReal::get),
        Some(0.02 + GAP_MM)
    );
}
