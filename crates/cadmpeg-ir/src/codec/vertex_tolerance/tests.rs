// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use super::widen;
use crate::annotations::{AnnotationBuilder, Annotations};
use crate::codec::loss::DecodeLossCode;
use crate::codec::{DecodeBody, DecodeResult, Decoded, FormatId};
use crate::document::Topology;
use crate::examples::unit_cube;
use crate::features::geometry::FinitePoint3;
use crate::geometry::pcurve::{
    Pcurve, PcurveGeometry, PcurveMetadata, PcurveNurbs, PcurveNurbsPoles,
};
use crate::math::{Point3, Vector3};
use crate::provenance::Exactness;
use crate::source_fidelity::SourceFidelity;
use crate::topology::PcurveUse;
use crate::validate::validate_neutral;
use crate::CadIr;

/// The unit cube (side 10, each edge a line ranged from its start corner)
/// with the point of `corner` moved to `position`.
fn cube_with_corner_at(corner: usize, position: Point3) -> CadIr {
    let mut ir = unit_cube().unwrap();
    let suffix = format!("point#{corner}");
    ir.geometry
        .points
        .iter_mut()
        .find(|point| point.id.as_str().ends_with(&suffix))
        .unwrap()
        .set_position(FinitePoint3::new(position).unwrap());
    ir
}

fn vertex_tolerance(geometry: &Topology, corner: usize) -> Option<f64> {
    let suffix = format!("vertex#{corner}");
    geometry
        .vertices
        .iter()
        .find(|vertex| vertex.id.as_str().ends_with(&suffix))
        .unwrap()
        .tolerance
        .map(crate::scalar::PositiveLength::get)
}

#[test]
fn a_vertex_off_the_ends_of_its_edges_states_its_largest_distance_marked_derived() {
    // Corner 0 off each of its three edges' start by (0.02, 0.02, 0.02): a
    // distance of 0.02·√3 from each end, 0.02·√2 from each line.
    let ir = cube_with_corner_at(0, Point3::new(-0.02, -0.02, -0.02));
    let decoded = Decoded {
        ir,
        body: DecodeBody::new(crate::report::decode::DecodeTransfer::full(true)),
        source_fidelity: SourceFidelity::new(crate::hash::digest::Sha256Digest::digest(
            b"test source",
        )),
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = DecodeResult::from_decoded(
        &ctx,
        decoded,
        FormatId::new("test"),
        &crate::report::strictness::Strictness::default(),
    )
    .unwrap();
    ctx.finish_session().unwrap();

    let expected = 0.02 * 3.0_f64.sqrt();
    let stated = vertex_tolerance(&result.ir().geometry, 0).unwrap();
    assert!(
        (stated - expected).abs() <= 16.0 * f64::EPSILON * expected,
        "{stated}"
    );
    for corner in 1..8 {
        assert_eq!(vertex_tolerance(&result.ir().geometry, corner), None);
    }
    let vertex = result
        .ir()
        .geometry
        .vertices
        .iter()
        .find(|vertex| vertex.id.as_str().ends_with("vertex#0"))
        .unwrap();
    let note = &result.source_fidelity().annotations.exactness()[vertex.id.as_str()];
    assert_eq!(note.entity(), Exactness::ByteExact);
    assert_eq!(
        note.fields()
            .iter()
            .map(|(field, exactness)| (field.as_str(), *exactness))
            .collect::<Vec<_>>(),
        [("tolerance", Exactness::Derived)]
    );
    let widened = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == DecodeLossCode::VertexToleranceWidened.note("").code)
        .collect::<Vec<_>>();
    let [loss] = widened.as_slice() else {
        panic!("one loss for the widened vertex: {widened:?}");
    };
    assert_eq!(loss.severity, crate::report::Severity::Info);
    assert!(
        loss.message.starts_with("widened 1 vertex tolerance(s)")
            && loss.message.ends_with(&format!(": {}", vertex.id.as_str())),
        "{}",
        loss.message
    );
}

#[test]
fn a_vertex_on_its_edges_ends_keeps_its_tolerance() {
    let mut ir = unit_cube().unwrap();
    let mut annotations = Annotations::default();
    assert_eq!(
        widen(&mut ir, &mut annotations, &DecodePolicy::default()),
        None
    );
    assert_eq!(ir, unit_cube().unwrap());
    assert_eq!(annotations, Annotations::default());
}

#[test]
fn a_vertex_on_a_carrier_far_from_its_end_keeps_its_tolerance() {
    // Corner 1 moved to the middle of edge 0's line: 5 from that line's end
    // and on the line itself, so the edge's range is off, not the vertex.
    let mut ir = cube_with_corner_at(1, Point3::new(5.0, 0.0, 0.0));
    let before = ir.clone();
    let mut annotations = Annotations::default();
    assert_eq!(
        widen(&mut ir, &mut annotations, &DecodePolicy::default()),
        None
    );
    assert_eq!(ir, before);
}

#[test]
fn a_vertex_whose_point_the_decoder_computed_keeps_its_tolerance() {
    let mut ir = cube_with_corner_at(0, Point3::new(-0.02, -0.02, -0.02));
    let point = ir
        .geometry
        .points
        .iter()
        .find(|point| point.id.as_str().ends_with("point#0"))
        .unwrap()
        .id
        .clone();
    let mut builder = AnnotationBuilder::new();
    builder.exactness(&point, Exactness::Derived);
    let mut annotations = builder.build();
    let before = ir.clone();
    assert_eq!(
        widen(&mut ir, &mut annotations, &DecodePolicy::default()),
        None
    );
    assert_eq!(ir, before);
}

/// The unit cube with corner 0 at `position`, each edge's curve marked
/// computed, and on the face in the plane through the origin normal to
/// `axis` a line
/// pcurve per coedge from the stored corner positions of its edge.
fn cube_with_face_pcurves(position: Point3, axis: Vector3) -> (CadIr, Annotations) {
    let stored = unit_cube().unwrap();
    let mut ir = cube_with_corner_at(0, position);
    let corner = |vertex: &crate::ids::VertexId| {
        let vertex = stored
            .geometry
            .vertices
            .iter()
            .find(|candidate| &candidate.id == vertex)
            .unwrap();
        stored
            .geometry
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .unwrap()
            .position()
            .get()
    };
    // The face whose loop runs through corners on the plane through the
    // origin normal to `axis`.
    let on_plane = |coedge: &crate::topology::Coedge| {
        let edge = stored
            .geometry
            .edges
            .iter()
            .find(|edge| edge.id == coedge.edge)
            .unwrap();
        edge.vertices().all(|vertex| {
            let at = corner(vertex);
            at.x * axis.x + at.y * axis.y + at.z * axis.z == 0.0
        })
    };
    let face = stored
        .geometry
        .faces
        .iter()
        .find(|face| {
            stored
                .geometry
                .loops
                .iter()
                .filter(|lp| lp.face == face.id)
                .all(|lp| {
                    stored
                        .geometry
                        .coedges
                        .iter()
                        .filter(|coedge| coedge.owner_loop == lp.id)
                        .all(on_plane)
                })
        })
        .unwrap();
    let plane = stored
        .geometry
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)
        .unwrap()
        .geometry
        .solved()
        .unwrap()
        .clone();
    let loops: Vec<_> = stored
        .geometry
        .loops
        .iter()
        .filter(|lp| lp.face == face.id)
        .map(|lp| lp.id.clone())
        .collect();
    for coedge in &mut ir.geometry.coedges {
        if !loops.contains(&coedge.owner_loop) {
            continue;
        }
        let edge = stored
            .geometry
            .edges
            .iter()
            .find(|edge| edge.id == coedge.edge)
            .unwrap();
        let ends = edge.ends.as_ref().unwrap();
        let uv = |vertex| {
            crate::eval::analytic_surface_parameters_solved(&plane, corner(vertex))
                .unwrap()
                .get()
        };
        let pcurve: crate::ids::PcurveId =
            format!("test:model:pcurve#{}", ir.geometry.pcurves.len())
                .try_into()
                .unwrap();
        ir.geometry.pcurves.push(Pcurve {
            id: pcurve.clone(),
            geometry: PcurveGeometry::Nurbs {
                nurbs: PcurveNurbs::new(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    PcurveNurbsPoles::Polynomial {
                        points: vec![uv(&ends.start), uv(&ends.end)],
                    },
                    false,
                )
                .unwrap(),
            },
            metadata: PcurveMetadata::default(),
            native_ref: None,
        });
        coedge.pcurves.push(PcurveUse {
            pcurve,
            isoparametric: None,
            parameter_range: None,
        });
    }
    let mut builder = AnnotationBuilder::new();
    for curve in &ir.geometry.curves {
        builder.exactness(&curve.id, Exactness::Derived);
    }
    (ir, builder.build())
}

#[test]
fn a_vertex_off_its_face_surface_states_its_distance_from_the_pcurve_images() {
    // Corner 0 moved 0.03 off the bottom face: the bottom face's pcurve images
    // end at the stored corner, 0.03 from the vertex, and the vertex lies
    // 0.03 from that face's plane. The edge curves are computed, so only the
    // pcurve images are compared.
    let (mut ir, mut annotations) =
        cube_with_face_pcurves(Point3::new(0.0, 0.0, -0.03), Vector3::new(0.0, 0.0, 1.0));
    let widened = widen(&mut ir, &mut annotations, &DecodePolicy::default()).unwrap();
    assert!(
        widened.message.starts_with("widened 1 vertex tolerance(s)"),
        "{}",
        widened.message
    );
    let stated = vertex_tolerance(&ir.geometry, 0).unwrap();
    assert!(
        (stated - 0.03).abs() <= 16.0 * f64::EPSILON * 0.03,
        "{stated}"
    );
}

#[test]
fn a_vertex_on_its_face_surface_keeps_its_tolerance_against_the_pcurve_images() {
    // Corner 0 moved 0.03 along the side face x = 0: the images miss it by
    // 0.03, and the vertex lies on that face's plane, so the image is off.
    let (mut ir, mut annotations) =
        cube_with_face_pcurves(Point3::new(0.0, 0.0, -0.03), Vector3::new(1.0, 0.0, 0.0));
    let before = ir.clone();
    assert_eq!(
        widen(&mut ir, &mut annotations, &DecodePolicy::default()),
        None
    );
    assert_eq!(ir, before);
}

#[test]
fn a_session_refusal_widens_nothing_and_is_charged() {
    let mut ir = cube_with_corner_at(0, Point3::new(-0.02, -0.02, -0.02));
    let before = ir.clone();
    let mut annotations = Annotations::default();
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 0;
    let loss = widen(&mut ir, &mut annotations, &policy).unwrap();
    assert_eq!(
        loss.code,
        DecodeLossCode::VertexToleranceUnmeasured.note("").code
    );
    assert_eq!(loss.severity, crate::report::Severity::Warning);
    assert_eq!(ir, before);
    assert_eq!(annotations, Annotations::default());
}

/// The vertex-gap findings validation reports on `holder`.
fn gaps_on(ir: &CadIr, holder: &str) -> usize {
    validate_neutral(ir, Vec::new())
        .unwrap()
        .findings
        .iter()
        .filter(|finding| {
            finding.message.contains("miss the edge's vertex positions")
                && (finding.entity.as_deref() == Some(holder)
                    || finding.related.first().map(String::as_str) == Some(holder))
        })
        .count()
}

/// Leaves `ir` to the pass and asserts that it widens and charges nothing.
fn assert_unwidened(mut ir: CadIr) {
    let before = ir.clone();
    let mut annotations = Annotations::default();
    assert_eq!(
        widen(&mut ir, &mut annotations, &DecodePolicy::default()),
        None
    );
    assert_eq!(ir, before);
    assert_eq!(annotations, Annotations::default());
}

/// The cube's corner at the origin, moved off the ends of its three edges
/// by 0.05 along -z.
fn moved_origin_corner(ir: &CadIr) -> crate::topology::Point {
    let mut corner = ir
        .geometry
        .points
        .iter()
        .find(|point| point.position().get() == Point3::new(0.0, 0.0, 0.0))
        .unwrap()
        .clone();
    corner.set_position(FinitePoint3::new(Point3::new(0.0, 0.0, -0.05)).unwrap());
    corner
}

#[test]
fn an_evaluation_vertex_off_an_edge_its_delta_does_not_restate_is_reported_not_widened() {
    // The evaluation is stored against the current state, and its delta
    // restates only the corner's point: the miss is against the current
    // state's edges. The pass keeps the stored tolerance, and validation
    // reports the gap on the evaluation.
    let mut ir = crate::validate::design::fixture::document();
    let corner = moved_origin_corner(&ir);
    let evaluation = &mut ir.design.evaluations[0];
    let holder = evaluation.id.as_str().to_owned();
    evaluation.content.as_mut().unwrap().delta.records.points = vec![corner];
    assert!(gaps_on(&ir, &holder) > 0);
    assert_unwidened(ir);
}

#[test]
fn a_process_state_vertex_off_its_edges_ends_is_reported_not_widened() {
    let mut ir = crate::validate::process::fixture::document();
    let corner = moved_origin_corner(&ir);
    let state = ir
        .process
        .states
        .iter_mut()
        .find(|state| state.id.as_str().ends_with("state#stock"))
        .unwrap();
    let holder = state.id.as_str().to_owned();
    state.content.as_mut().unwrap().delta.records.points = vec![corner];
    assert!(gaps_on(&ir, &holder) > 0);
    assert_unwidened(ir);
}

#[test]
fn a_history_body_vertex_off_its_edges_ends_is_reported_not_widened() {
    // The nest is the cube with corner 0 off each of its three edges' start.
    let geometry = cube_with_corner_at(0, Point3::new(-0.02, -0.02, -0.02)).geometry;
    let mut nest = crate::history_body::BodyTopology::new(geometry.bodies[0].clone());
    nest.regions = geometry.regions;
    nest.shells = geometry.shells;
    nest.faces = geometry.faces;
    nest.loops = geometry.loops;
    nest.coedges = geometry.coedges;
    nest.edges = geometry.edges;
    nest.vertices = geometry.vertices;
    nest.surfaces = geometry.surfaces;
    nest.curves = geometry.curves;
    nest.pcurves = geometry.pcurves;
    nest.points = geometry.points;
    let body = crate::history_body::HistoryBody {
        id: crate::ids::HistoryBodyId::mint("test:history-body:nest#0").unwrap(),
        produced_by: None,
        consumed_by: None,
        state_of: None,
        state: crate::history_body::HistoryStateRef {
            ordinal: 0,
            identifier: 0,
        },
        topology: Box::new(nest),
    };
    let holder = body.id.as_str().to_owned();
    let mut ir = unit_cube().unwrap();
    ir.model.history_bodies.push(body);
    assert!(gaps_on(&ir, &holder) > 0);
    assert_unwidened(ir);
}
