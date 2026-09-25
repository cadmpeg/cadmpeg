// SPDX-License-Identifier: Apache-2.0
use cadmpeg_test_support::EditableDecodeResult;

use super::analytic_surface_normal;
use super::analytic_surface_residual;
use super::assign_persistent_owners;
use super::assign_unique_surface_owners;
use super::chordal_hole_constraint;
use super::circle_overlaps_polygon;
use super::circular_interval;
use super::circular_interval_contains;
use super::circular_outer_and_holes;
use super::is_simple_polygon;
use super::parse_table;
use super::persistent_surface_references;
use super::planar_boundary_samples;
use super::plane_frame;
use super::polygon_contains;
use super::shortest_arc_span;
use super::ByteRange;
use super::CircularHole;
use super::ConicalTrim;
use super::DisplayFace;
use super::Mesh;
use super::PersistentFaceBinding;
use super::PersistentSurfaceReference;
use super::PlanarHole;
use super::PlanarOuter;
use super::PlanarTrim;
use super::PlaneFrame;
use super::CLASS_MARKER;
use super::EPS_DISPLAY_QUANTIZATION;
use super::SCENE_SOURCE_MARKER;
use crate::brep::feature_source::FeatureSourceId;
use crate::brep::PersistentFaceIdentity;
use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::topology::Sense;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::loss::LossTaxonomy;

use crate::SldprtCodec;

use crate::test_support::container::make_block;
use crate::test_support::container::sldprt_with_body;
use crate::test_support::parasolid::triangle_body;
use crate::test_support::tessellation::descriptor;
use crate::test_support::tessellation::display_list_payload;
use crate::test_support::tessellation::extended_display_list_payload;
use crate::test_support::tessellation::sldprt_with_body_and_display_list;
use cadmpeg_ir::geometry::{nurbs::NurbsSurface, Curve, Surface};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
    VertexId,
};
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Shell, Vertex,
};

mod display_tables;

fn decoded_references(payload: &[u8], range: ByteRange) -> Vec<PersistentSurfaceReference> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        payload,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("root");
    persistent_surface_references(&ctx, payload, range).expect("service profile admits references")
}

fn table() -> Vec<u8> {
    let mut out = descriptor(4, 8, 1, &3_u32.to_le_bytes());
    let positions = [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    out.extend(descriptor(12, 100, 3, &positions));
    out.extend(descriptor(12, 100, 3, &[0; 36]));
    out.extend(descriptor(4, 8, 4, &[0; 16]));
    out.extend(descriptor(4, 8, 1, &4_u32.to_le_bytes()));
    out.extend(descriptor(1, 8, 4, &[0; 4]));
    out
}

fn class(payload: &mut Vec<u8>, name: &str, sources: &[u32]) {
    payload.extend_from_slice(CLASS_MARKER);
    payload.extend_from_slice(&(name.len() as u16).to_le_bytes());
    payload.extend_from_slice(name.as_bytes());
    for source in sources {
        payload.extend_from_slice(SCENE_SOURCE_MARKER);
        payload.extend_from_slice(&source.to_le_bytes());
    }
}

#[test]
fn analytic_surface_residuals_measure_normal_distance() {
    let plane = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 2.0),
            Vector3::new(0.0, 0.0, 2.0).unit().unwrap(),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));
    let cylinder = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 2.0).unit().unwrap(),
            Vector3::new(1.0, 0.0, 0.0),
            3.0,
        )
        .unwrap(),
    ));
    let sphere = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
        cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            4.0,
        )
        .unwrap(),
    ));
    let torus = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
        cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            5.0,
            2.0,
        )
        .unwrap(),
    ));
    let cone = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            3.0,
            0.5,
            std::f64::consts::FRAC_PI_4,
        )
        .unwrap(),
    ));

    for (surface, point, displaced) in [
        (
            &plane,
            Point3::new(3.0, 4.0, 2.0),
            Point3::new(3.0, 4.0, 2.5),
        ),
        (
            &cylinder,
            Point3::new(3.0, 0.0, 7.0),
            Point3::new(3.5, 0.0, 7.0),
        ),
        (
            &sphere,
            Point3::new(5.0, 2.0, 3.0),
            Point3::new(5.5, 2.0, 3.0),
        ),
        (
            &torus,
            Point3::new(7.0, 0.0, 0.0),
            Point3::new(7.5, 0.0, 0.0),
        ),
    ] {
        assert_eq!(
            analytic_surface_residual(surface.solved().expect("solved carrier"), point),
            Some(0.0)
        );
        assert!(
            analytic_surface_residual(surface.solved().expect("solved carrier"), displaced)
                .is_some_and(|residual| residual > 0.0)
        );
    }

    let local_radius = 3.0 + 2.0 * std::f64::consts::FRAC_PI_4.tan();
    let cone_point = Point3::new(local_radius, 0.0, 2.0);
    assert!(
        analytic_surface_residual(cone.solved().expect("solved carrier"), cone_point)
            .is_some_and(|residual| residual <= f64::EPSILON * 128.0)
    );
    assert!(analytic_surface_residual(
        cone.solved().expect("solved carrier"),
        Point3::new(local_radius + 0.5, 0.0, 2.0)
    )
    .is_some_and(|residual| residual > 0.0));
}

fn add_face(
    model: &mut cadmpeg_ir::document::Model,
    name: &str,
    geometry: SurfaceGeometry,
    corners: [Point3; 4],
) -> FaceId {
    let face_id =
        FaceId::mint(format!("synthetic:test:face#face-{name}")).expect("identity grammar");
    let loop_id =
        LoopId::mint(format!("synthetic:test:loop#loop-{name}")).expect("identity grammar");
    let surface_id = SurfaceId::mint(format!("synthetic:test:surface#surface-{name}"))
        .expect("identity grammar");
    model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry,
        source_object: None,
    });

    let coedge_ids = (0..4)
        .map(|index| {
            CoedgeId::mint(format!("synthetic:test:coedge#coedge-{name}-{index}"))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    for (index, corner) in corners.iter().copied().enumerate() {
        let point_id = PointId::mint(format!("synthetic:test:point#point-{name}-{index}"))
            .expect("identity grammar");
        let vertex_id = VertexId::mint(format!("synthetic:test:vertex#vertex-{name}-{index}"))
            .expect("identity grammar");
        model.points.push(Point::new(
            point_id.clone(),
            cadmpeg_ir::features::FinitePoint3::new(corner).expect("a finite position is a point"),
            None,
        ));
        model.vertices.push(Vertex {
            id: vertex_id,
            point: point_id,
            tolerance: None,
        });
    }
    for (index, origin) in corners.iter().copied().enumerate() {
        let next = (index + 1) % 4;
        let curve_id = CurveId::mint(format!("synthetic:test:curve#curve-{name}-{index}"))
            .expect("identity grammar");
        let edge_id = EdgeId::mint(format!("synthetic:test:edge#edge-{name}-{index}"))
            .expect("identity grammar");
        let direction = corners[next].vector_from(origin).unit().unwrap();
        model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(origin, direction).unwrap(),
            )),
            source_object: None,
        });
        model.edges.push(Edge {
            id: edge_id.clone(),
            carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(curve_id)),
            start: VertexId::mint(format!("synthetic:test:vertex#vertex-{name}-{index}"))
                .expect("identity grammar"),
            end: VertexId::mint(format!("synthetic:test:vertex#vertex-{name}-{next}"))
                .expect("identity grammar"),
            tolerance: None,
        });
        model.coedges.push(Coedge {
            id: coedge_ids[index].clone(),
            owner_loop: loop_id.clone(),
            edge: edge_id,
            radial_next: coedge_ids[index].clone(),
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
        });
    }
    model.loops.push(Loop {
        id: loop_id.clone(),
        face: face_id.clone(),
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
            cadmpeg_ir::topology::LoopRing::new(coedge_ids, Vec::new()).expect("valid loop ring"),
        ),
    });
    model.faces.push(Face {
        id: face_id.clone(),
        shell: ShellId::mint("synthetic:test:shell#shell").expect("identity grammar"),
        surface: surface_id,
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![loop_id]),
        name: None,
        color: None,
        tolerance: None,
    });
    face_id
}

fn add_square_face(model: &mut cadmpeg_ir::document::Model, name: &str, x: f64) -> FaceId {
    add_face(
        model,
        name,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        [
            Point3::new(x, -1.0, 0.0),
            Point3::new(x + 2.0, -1.0, 0.0),
            Point3::new(x + 2.0, 1.0, 0.0),
            Point3::new(x, 1.0, 0.0),
        ],
    )
}

#[test]
fn face_tolerance_below_display_resolution_is_refused() {
    let mut model = cadmpeg_ir::document::Model::default();
    let face_id = add_square_face(&mut model, "below-display-floor", 0.0);
    let stated = EPS_DISPLAY_QUANTIZATION / 2.0;
    model
        .faces
        .iter_mut()
        .find(|face| face.id == face_id)
        .expect("test face exists")
        .tolerance = Some(
        cadmpeg_ir::scalar::PositiveReal::new(stated)
            .expect("the test tolerance is positive and finite"),
    );

    let error = assign_unique_surface_owners(&mut model)
        .expect_err("a display lane cannot evaluate a finer stated tolerance");
    let text = error.to_string();
    assert!(text.contains(face_id.as_str()), "{text}");
    assert!(text.contains(&stated.to_string()), "{text}");
    assert!(
        text.contains(&EPS_DISPLAY_QUANTIZATION.to_string()),
        "{text}"
    );
}

fn test_nurbs_surface() -> NurbsSurface {
    let heights = [0.0, 0.25, 0.0, 0.25, 0.9, 0.25, 0.0, 0.25, 0.0];
    let control_points = (0..3)
        .map(|u| {
            (0..3)
                .map(|v| Point3::new(u as f64, v as f64, heights[u * 3 + v]))
                .collect()
        })
        .collect();
    NurbsSurface::from_lanes(
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            false,
        ),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            false,
        ),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(control_points, None),
        false,
    )
    .expect("valid test NURBS surface")
}

fn flat_test_nurbs_surface() -> NurbsSurface {
    let mut surface = test_nurbs_surface();
    surface
        .edit_control_points(|point| {
            point.z = 0.0;
            Ok(())
        })
        .unwrap();
    surface
}

fn test_nurbs_corners(surface: &NurbsSurface) -> [Point3; 4] {
    [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)].map(|(u, v)| {
        cadmpeg_ir::eval::nurbs_surface_point(surface, u, v)
            .unwrap()
            .get()
    })
}

fn test_nurbs_point_normal(surface: &NurbsSurface, u: f64, v: f64) -> (Point3, Vector3) {
    let partials = cadmpeg_ir::eval::nurbs_surface_partials(surface, u, v).unwrap();
    (
        partials.point.get(),
        partials.du.cross(partials.dv.get()).unit().unwrap(),
    )
}

fn add_cylindrical_patch_face(
    model: &mut cadmpeg_ir::document::Model,
    name: &str,
    min_z: f64,
    max_z: f64,
) -> FaceId {
    let radius = 5.0;
    let angles = [0.0, std::f64::consts::FRAC_PI_2];
    let point_at = |angle: f64, z: f64| Point3::new(radius * angle.cos(), radius * angle.sin(), z);
    let corners = [
        point_at(angles[0], min_z),
        point_at(angles[1], min_z),
        point_at(angles[1], max_z),
        point_at(angles[0], max_z),
    ];
    let face_id =
        FaceId::mint(format!("synthetic:test:face#face-{name}")).expect("identity grammar");
    let loop_id =
        LoopId::mint(format!("synthetic:test:loop#loop-{name}")).expect("identity grammar");
    let surface_id = SurfaceId::mint(format!("synthetic:test:surface#surface-{name}"))
        .expect("identity grammar");
    model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                radius,
            )
            .unwrap(),
        )),
        source_object: None,
    });

    let vertex_ids = corners
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let point_id = PointId::mint(format!("synthetic:test:point#point-{name}-{index}"))
                .expect("identity grammar");
            let vertex_id = VertexId::mint(format!("synthetic:test:vertex#vertex-{name}-{index}"))
                .expect("identity grammar");
            model.points.push(Point::new(
                point_id.clone(),
                cadmpeg_ir::features::FinitePoint3::new(*point)
                    .expect("a finite position is a point"),
                None,
            ));
            model.vertices.push(Vertex {
                id: vertex_id.clone(),
                point: point_id,
                tolerance: None,
            });
            vertex_id
        })
        .collect::<Vec<_>>();
    let coedge_ids = (0..4)
        .map(|index| {
            CoedgeId::mint(format!("synthetic:test:coedge#coedge-{name}-{index}"))
                .expect("identity grammar")
        })
        .collect::<Vec<_>>();
    let curve_geometries = [
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(0.0, 0.0, min_z),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                radius,
            )
            .unwrap(),
        )),
        CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                corners[1],
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        )),
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(0.0, 0.0, max_z),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                radius,
            )
            .unwrap(),
        )),
        CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                corners[3],
                Vector3::new(0.0, 0.0, -1.0),
            )
            .unwrap(),
        )),
    ];
    for (index, geometry) in curve_geometries.into_iter().enumerate() {
        let next = (index + 1) % 4;
        let curve_id = CurveId::mint(format!("synthetic:test:curve#curve-{name}-{index}"))
            .expect("identity grammar");
        let edge_id = EdgeId::mint(format!("synthetic:test:edge#edge-{name}-{index}"))
            .expect("identity grammar");
        model.curves.push(Curve {
            id: curve_id.clone(),
            geometry,
            source_object: None,
        });
        model.edges.push(Edge {
            id: edge_id.clone(),
            carrier: cadmpeg_ir::topology::EdgeCarrier::unbounded(Some(curve_id)),
            start: vertex_ids[index].clone(),
            end: vertex_ids[next].clone(),
            tolerance: None,
        });
        model.coedges.push(Coedge {
            id: coedge_ids[index].clone(),
            owner_loop: loop_id.clone(),
            edge: edge_id,
            radial_next: coedge_ids[index].clone(),
            sense: Sense::Forward,
            pcurves: Vec::new(),
            use_curve: None,
        });
    }
    model.loops.push(Loop {
        id: loop_id.clone(),
        face: face_id.clone(),
        boundary: cadmpeg_ir::topology::LoopBoundary::Ring(
            cadmpeg_ir::topology::LoopRing::new(coedge_ids, Vec::new()).expect("valid loop ring"),
        ),
    });
    model.faces.push(Face {
        id: face_id.clone(),
        shell: ShellId::mint("synthetic:test:shell#shell").expect("identity grammar"),
        surface: surface_id,
        sense: Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(vec![loop_id]),
        name: None,
        color: None,
        tolerance: None,
    });
    face_id
}

fn model_with_body() -> cadmpeg_ir::document::Model {
    let mut model = cadmpeg_ir::document::Model::default();
    model.bodies = vec![Body {
        id: BodyId::mint("synthetic:test:body#body").expect("identity grammar"),
        kind: BodyKind::Solid,
        regions: vec![RegionId::mint("synthetic:test:region#region").expect("identity grammar")],
        transform: None,
        name: None,
        color: None,
        visible: None,
    }];
    model.regions = vec![Region {
        id: RegionId::mint("synthetic:test:region#region").expect("identity grammar"),
        body: BodyId::mint("synthetic:test:body#body").expect("identity grammar"),
        shells: vec![ShellId::mint("synthetic:test:shell#shell").expect("identity grammar")],
    }];

    model
}

fn set_shell_faces(model: &mut cadmpeg_ir::document::Model, faces: Vec<FaceId>) {
    model.shells = vec![Shell::new(
        model.regions[0].shells[0].clone(),
        model.regions[0].id.clone(),
        faces,
        Vec::new(),
        Vec::new(),
    )
    .unwrap()];
}

fn persistent_mesh(id: &str) -> Tessellation {
    mesh_from(
        id,
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ],
        vec![[0, 1, 2]],
    )
}

fn mesh_from(
    id: impl Into<String>,
    vertices: Vec<Point3>,
    triangles: Vec<[u32; 3]>,
) -> Tessellation {
    Tessellation::new(
        id,
        cadmpeg_ir::tessellation::TessellationMesh::List {
            vertices,
            triangles,
        },
        Vec::new(),
    )
    .expect("valid tessellation")
}

fn persistent_identity(source: u32, local: u32, trailing_fields: &[u32]) -> PersistentFaceIdentity {
    PersistentFaceIdentity {
        feature_source_id: source.try_into().unwrap(),
        local_id: local,
        trailing_fields: trailing_fields.to_vec(),
    }
}

fn framed_surface_reference(text: &str) -> Vec<u8> {
    let units = text.encode_utf16().collect::<Vec<_>>();
    let mut payload = vec![0xff, 0xfe, 0xff, units.len().try_into().unwrap()];
    payload.extend(units.into_iter().flat_map(u16::to_le_bytes));
    payload
}

fn reference_limit_error(
    payload: &[u8],
    policy: &cadmpeg_core::decode::DecodePolicy,
) -> cadmpeg_core::CodecError {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(payload, &arena, policy)
        .expect("root");
    persistent_surface_references(
        &ctx,
        payload,
        ByteRange::new(0, payload.len()).expect("ordered range"),
    )
    .expect_err("reference allocation exceeds the limit")
}

#[test]
fn display_reference_units_refuse_collection_limit_before_allocation() {
    let payload = framed_surface_reference("moContent3IntSurfIdRep_c,300,4,-1,0,");
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = u64::from(payload[3]) - 1;
    assert!(matches!(reference_limit_error(&payload, &policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == "decode display-list reference units"));
}

#[test]
fn display_reference_text_refuses_materialized_limit_before_allocation() {
    let payload = framed_surface_reference("moContent3IntSurfIdRep_c,300,4,-1,0,");
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = u64::from(payload[3]) * 3 - 1;
    assert!(matches!(reference_limit_error(&payload, &policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
                && limit.operation == "decode display-list reference text"));
}

fn reference_collection_refusal(extra: u64, operation: &'static str) {
    let payload = framed_surface_reference("moContent3IntSurfIdRep_c,300,4,-1,0,");
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = u64::from(payload[3]) + extra;
    assert!(matches!(reference_limit_error(&payload, &policy),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                && limit.operation == operation));
}

#[test]
fn display_reference_fields_refuse_collection_limit_before_scanning() {
    reference_collection_refusal(0, "scan display-list reference fields");
}

#[test]
fn display_reference_numeric_fields_refuse_collection_limit_before_allocation() {
    reference_collection_refusal(3, "decode display-list reference fields");
}

#[test]
fn display_references_refuse_collection_limit_before_insertion() {
    reference_collection_refusal(5, "collect display-list references");
}

#[test]
fn overlapping_display_face_tables_narrow_to_an_empty_metadata_range() {
    // Display-face metadata is narrowed to where the following table starts. Two
    // overlapping tables put that end below the metadata start; the range then
    // collapses at its own start instead of inverting and silently reading nothing.
    assert!(ByteRange::new(64, 32).is_none());
    let metadata = ByteRange::new(64, 128).expect("ordered range");
    let overlapped = metadata.truncated(32);
    assert_eq!((overlapped.start(), overlapped.end()), (64, 64));
    let mut payload = vec![0; 192];
    let reference = framed_surface_reference("moPlaneSurfIdRep_c,7,3,");
    payload[64..64 + reference.len()].copy_from_slice(&reference);
    assert!(decoded_references(&payload, overlapped).is_empty());
    let narrowed = metadata.truncated(120);
    assert_eq!((narrowed.start(), narrowed.end()), (64, 120));
    assert_eq!(
        decoded_references(&payload, narrowed).len(),
        decoded_references(&payload, metadata).len()
    );
}

#[test]
fn persistent_surface_reference_decodes_signed_tail() {
    let payload = framed_surface_reference("moContent3IntSurfIdRep_c,300,4,-1,0,");
    let references = decoded_references(
        &payload,
        ByteRange::new(0, payload.len()).expect("ordered range"),
    );
    assert_eq!(
        references,
        vec![PersistentSurfaceReference::Complete(persistent_identity(
            300,
            4,
            &[u32::MAX, 0],
        ))]
    );
}

#[test]
fn opaque_surface_suffix_remains_source_only() {
    let payload = framed_surface_reference("moFromSktEntSurfIdRep_c,7,3,opaque");
    let references = decoded_references(
        &payload,
        ByteRange::new(0, payload.len()).expect("ordered range"),
    );
    assert_eq!(
        references,
        vec![PersistentSurfaceReference::SourceOnly {
            feature_source_id: 7_u32.try_into().unwrap(),
            local_surface_id: 3,
        }]
    );
    let face = DisplayFace {
        mesh: Mesh::default(),
        table: ByteRange::new(0, 1).expect("ordered range"),
        metadata: ByteRange::new(1, 2).expect("ordered range"),
        surface_references: references,
    };
    assert_eq!(
        face.feature_source_id().map(FeatureSourceId::value),
        Some(7)
    );
    assert_eq!(face.persistent_surface_identity(), None);
}

#[test]
fn persistent_surface_identity_requires_agreeing_duplicates() {
    let face = DisplayFace {
        mesh: Mesh::default(),
        table: ByteRange::new(0, 1).expect("ordered range"),
        metadata: ByteRange::new(1, 2).expect("ordered range"),
        surface_references: vec![
            PersistentSurfaceReference::Complete(persistent_identity(7, 3, &[])),
            PersistentSurfaceReference::Complete(persistent_identity(7, 3, &[])),
        ],
    };
    assert_eq!(
        face.feature_source_id().map(FeatureSourceId::value),
        Some(7)
    );
    assert_eq!(
        face.persistent_surface_identity(),
        Some(persistent_identity(7, 3, &[]))
    );

    let mut conflicting = face;
    if let PersistentSurfaceReference::Complete(identity) = &mut conflicting.surface_references[1] {
        identity.local_id = 4;
    }
    assert_eq!(
        conflicting.feature_source_id().map(FeatureSourceId::value),
        Some(7)
    );
    assert_eq!(conflicting.persistent_surface_identity(), None);
}

#[test]
fn persistent_surface_identity_binds_one_face_and_body() {
    let mut model = model_with_body();
    let face = add_square_face(&mut model, "persistent", 0.0);
    set_shell_faces(&mut model, vec![face.clone()]);
    model
        .tessellations
        .push(persistent_mesh("synthetic:test:tessellation#mesh"));

    let face_identities = vec![(face.clone(), persistent_identity(7, 3, &[]))];
    let bindings = vec![PersistentFaceBinding {
        tessellation: "synthetic:test:tessellation#mesh".into(),
        identity: persistent_identity(7, 3, &[]),
    }];

    assert_eq!(
        assign_persistent_owners(&mut model, &face_identities, &bindings),
        vec!["synthetic:test:tessellation#mesh"]
    );
    assert_eq!(model.tessellations[0].faces, vec![face]);
    assert_eq!(
        model.tessellations[0].body,
        Some(BodyId::mint("synthetic:test:body#body").expect("identity grammar"))
    );
}

#[test]
fn persistent_surface_identity_rejects_ambiguous_face_or_mesh_keys() {
    let mut model = model_with_body();
    let first = add_square_face(&mut model, "first-persistent", 0.0);
    let second = add_square_face(&mut model, "second-persistent", 3.0);
    set_shell_faces(&mut model, vec![first.clone(), second.clone()]);
    model
        .tessellations
        .push(persistent_mesh("synthetic:test:tessellation#mesh"));
    let face_identities = vec![
        (first.clone(), persistent_identity(7, 3, &[])),
        (second.clone(), persistent_identity(7, 3, &[])),
    ];
    let binding = PersistentFaceBinding {
        tessellation: "synthetic:test:tessellation#mesh".into(),
        identity: persistent_identity(7, 3, &[]),
    };
    assert!(assign_persistent_owners(&mut model, &face_identities, &[binding]).is_empty());
    assert!(model.tessellations[0].faces.is_empty());

    let mut model = model_with_body();
    let first = add_square_face(&mut model, "first-mesh", 0.0);
    let second = add_square_face(&mut model, "second-mesh", 3.0);
    set_shell_faces(&mut model, vec![first.clone(), second.clone()]);
    model
        .tessellations
        .push(persistent_mesh("synthetic:test:tessellation#mesh"));
    let face_identities = vec![
        (first.clone(), persistent_identity(7, 3, &[])),
        (second.clone(), persistent_identity(8, 4, &[])),
    ];
    let bindings = vec![
        PersistentFaceBinding {
            tessellation: "synthetic:test:tessellation#mesh".into(),
            identity: persistent_identity(7, 3, &[]),
        },
        PersistentFaceBinding {
            tessellation: "synthetic:test:tessellation#mesh".into(),
            identity: persistent_identity(8, 4, &[]),
        },
    ];
    assert!(assign_persistent_owners(&mut model, &face_identities, &bindings).is_empty());
    assert!(model.tessellations[0].faces.is_empty());
}

#[test]
fn persistent_surface_identity_distinguishes_trailing_path_fields() {
    let mut model = model_with_body();
    let first = add_square_face(&mut model, "first-tail", 0.0);
    let second = add_square_face(&mut model, "second-tail", 3.0);
    set_shell_faces(&mut model, vec![first.clone(), second.clone()]);
    model
        .tessellations
        .push(persistent_mesh("synthetic:test:tessellation#mesh"));
    let face_identities = vec![
        (first.clone(), persistent_identity(266, 2, &[0])),
        (second.clone(), persistent_identity(266, 2, &[1])),
    ];
    let binding = PersistentFaceBinding {
        tessellation: "synthetic:test:tessellation#mesh".into(),
        identity: persistent_identity(266, 2, &[1]),
    };

    assert_eq!(
        assign_persistent_owners(&mut model, &face_identities, &[binding]),
        vec!["synthetic:test:tessellation#mesh"]
    );
    assert_eq!(model.tessellations[0].faces, vec![second]);
}

#[test]
fn bounded_planar_trim_selects_between_coincident_supports() {
    let mut model = model_with_body();
    let first = add_square_face(&mut model, "first", -4.0);
    let second = add_square_face(&mut model, "second", 2.0);
    set_shell_faces(&mut model, vec![first.clone(), second.clone()]);
    model.tessellations.push(
        Tessellation::new(
            "synthetic:test:tessellation#mesh",
            cadmpeg_ir::tessellation::TessellationMesh::List {
                vertices: vec![
                    Point3::new(2.25, -0.75, 0.0),
                    Point3::new(3.75, -0.75, 0.0),
                    Point3::new(3.0, 0.75, 0.0),
                ],
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .expect("valid tessellation"),
    );

    assert_eq!(
        assign_unique_surface_owners(&mut model).unwrap(),
        vec!["synthetic:test:tessellation#mesh"]
    );
    assert_eq!(model.tessellations[0].faces, vec![second]);
    assert_eq!(
        model.tessellations[0].body,
        Some(BodyId::mint("synthetic:test:body#body").expect("identity grammar"))
    );

    model
        .faces
        .iter_mut()
        .find(|face| face.id == first)
        .unwrap()
        .loops = cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new());
    model.tessellations[0].body = None;
    model.tessellations[0].faces.clear();
    assert!(assign_unique_surface_owners(&mut model).unwrap().is_empty());
    assert!(model.tessellations[0].faces.is_empty());
}

#[test]
fn bounded_cylindrical_trim_selects_between_coincident_supports() {
    let mut model = model_with_body();
    let lower = add_cylindrical_patch_face(&mut model, "lower", 0.0, 1.0);
    let upper = add_cylindrical_patch_face(&mut model, "upper", 2.0, 3.0);
    set_shell_faces(&mut model, vec![lower.clone(), upper.clone()]);
    model.tessellations.push(
        Tessellation::new(
            "synthetic:test:tessellation#lower-mesh",
            cadmpeg_ir::tessellation::TessellationMesh::List {
                vertices: vec![
                    Point3::new(5.0, 0.0, 0.25),
                    Point3::new(0.0, 5.0, 0.25),
                    Point3::new(5.0, 0.0, 0.75),
                ],
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .expect("valid tessellation"),
    );

    assert_eq!(
        assign_unique_surface_owners(&mut model).unwrap(),
        vec!["synthetic:test:tessellation#lower-mesh"]
    );
    assert_eq!(model.tessellations[0].faces, vec![lower]);
    assert_eq!(
        model.tessellations[0].body,
        Some(BodyId::mint("synthetic:test:body#body").expect("identity grammar"))
    );
}

#[test]
fn chordal_cylindrical_mesh_records_measured_support_deflection() {
    let mut model = model_with_body();
    let face = add_cylindrical_patch_face(&mut model, "chordal", 0.0, 1.0);
    set_shell_faces(&mut model, vec![face.clone()]);
    let deflection = 0.1;
    model.tessellations.push(
        Tessellation::new(
            "synthetic:test:tessellation#chordal-mesh",
            cadmpeg_ir::tessellation::TessellationMesh::from_list_lanes(
                vec![
                    Point3::new(5.0 - deflection, 0.0, 0.25),
                    Point3::new(0.0, 5.0 - deflection, 0.25),
                    Point3::new(5.0 - deflection, 0.0, 0.75),
                ],
                vec![[0, 1, 2]],
                Some(vec![
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 1.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                ]),
            )
            .expect("normals cover the mesh"),
            Vec::new(),
        )
        .expect("valid tessellation"),
    );

    assert_eq!(
        assign_unique_surface_owners(&mut model).unwrap(),
        vec!["synthetic:test:tessellation#chordal-mesh"]
    );
    assert_eq!(model.tessellations[0].faces, vec![face]);
    assert!(model.tessellations[0]
        .chordal_deflection()
        .is_some_and(|value| (value.get() - deflection).abs() <= f64::EPSILON * 128.0));
}

#[test]
fn chordal_cylindrical_mesh_uses_unique_trim_when_normals_disagree() {
    let mut model = model_with_body();
    let face = add_cylindrical_patch_face(&mut model, "inconsistent-normals", 0.0, 1.0);
    set_shell_faces(&mut model, vec![face.clone()]);
    let deflection = 0.1;
    model.tessellations.push(
        Tessellation::new(
            "synthetic:test:tessellation#inconsistent-normals-mesh",
            cadmpeg_ir::tessellation::TessellationMesh::from_list_lanes(
                vec![
                    Point3::new(5.0 - deflection, 0.0, 0.25),
                    Point3::new(0.0, 5.0 - deflection, 0.25),
                    Point3::new(5.0 - deflection, 0.0, 0.75),
                ],
                vec![[0, 1, 2]],
                Some(vec![
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(0.0, 0.0, 1.0),
                ]),
            )
            .expect("normals cover the mesh"),
            Vec::new(),
        )
        .expect("valid tessellation"),
    );

    assert_eq!(
        assign_unique_surface_owners(&mut model).unwrap(),
        vec!["synthetic:test:tessellation#inconsistent-normals-mesh"]
    );
    assert_eq!(model.tessellations[0].faces, vec![face]);
    assert!(model.tessellations[0]
        .chordal_deflection()
        .is_some_and(|value| (value.get() - deflection).abs() <= f64::EPSILON * 128.0));
}

#[test]
fn off_surface_planar_mesh_does_not_become_a_chordal_cache() {
    let mut model = model_with_body();
    let face = add_square_face(&mut model, "off-surface", 0.0);
    set_shell_faces(&mut model, vec![face]);
    model.tessellations.push(
        Tessellation::new(
            "synthetic:test:tessellation#off-surface-mesh",
            cadmpeg_ir::tessellation::TessellationMesh::from_list_lanes(
                vec![
                    Point3::new(0.25, -0.75, 0.1),
                    Point3::new(1.75, -0.75, 0.1),
                    Point3::new(1.0, 0.75, 0.1),
                ],
                vec![[0, 1, 2]],
                Some(vec![
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(0.0, 0.0, 1.0),
                ]),
            )
            .expect("normals cover the mesh"),
            Vec::new(),
        )
        .expect("valid tessellation"),
    );

    assert!(assign_unique_surface_owners(&mut model).unwrap().is_empty());
    assert!(model.tessellations[0].body.is_none());
    assert!(model.tessellations[0].faces.is_empty());
}

#[test]
fn cylindrical_trim_uses_the_short_boundary_arc() {
    let (start, span) = circular_interval(&[0.0, std::f64::consts::FRAC_PI_2]).unwrap();
    assert_eq!(start, 0.0);
    assert_eq!(span, std::f64::consts::FRAC_PI_2);
    assert!(circular_interval_contains(
        start,
        span,
        std::f64::consts::FRAC_PI_4,
        0.0
    ));
    assert!(!circular_interval_contains(
        start,
        span,
        std::f64::consts::PI,
        0.0
    ));
}

#[test]
fn cylindrical_trim_accepts_quantized_points_within_boundary_tolerance() {
    let start = 1.0;
    let span = 0.5;
    let tolerance = f64::EPSILON * 4096.0;

    assert!(circular_interval_contains(
        start,
        span,
        start - tolerance * 0.5,
        tolerance,
    ));
    assert!(circular_interval_contains(
        start,
        span,
        start + span + tolerance * 0.5,
        tolerance,
    ));
    assert!(!circular_interval_contains(
        start,
        span,
        start - tolerance * 2.0,
        tolerance,
    ));
}

#[test]
fn cone_support_binds_display_list_face() {
    let mut model = model_with_body();
    let cone = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            3.0,
            0.5,
            std::f64::consts::FRAC_PI_4,
        )
        .unwrap(),
    ));
    let v = 2.0;
    let local_radius = 3.0 + v * std::f64::consts::FRAC_PI_4.tan();
    let face = add_face(
        &mut model,
        "cone",
        cone,
        [
            Point3::new(local_radius, 0.0, v),
            Point3::new(0.0, local_radius * 0.5, v),
            Point3::new(-local_radius, 0.0, v),
            Point3::new(0.0, -local_radius * 0.5, v),
        ],
    );
    set_shell_faces(&mut model, vec![face.clone()]);
    model.tessellations.push(
        Tessellation::new(
            "synthetic:test:tessellation#cone-mesh",
            cadmpeg_ir::tessellation::TessellationMesh::List {
                vertices: vec![
                    Point3::new(local_radius, 0.0, v),
                    Point3::new(0.0, local_radius * 0.5, v),
                    Point3::new(-local_radius, 0.0, v),
                ],
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .expect("valid tessellation"),
    );

    assert_eq!(
        assign_unique_surface_owners(&mut model).unwrap(),
        vec!["synthetic:test:tessellation#cone-mesh"]
    );
    assert_eq!(model.tessellations[0].faces, vec![face]);
    assert_eq!(
        model.tessellations[0].body,
        Some(BodyId::mint("synthetic:test:body#body").expect("identity grammar"))
    );
}

#[test]
fn cone_chordal_display_list_uses_analytic_normal_for_ownership() {
    let mut model = model_with_body();
    let cone = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
        cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            3.0,
            0.5,
            std::f64::consts::FRAC_PI_4,
        )
        .unwrap(),
    ));
    let axial = 2.0;
    let surface_radius = 3.0 + axial * std::f64::consts::FRAC_PI_4.tan();
    let face = add_face(
        &mut model,
        "cone-cache",
        cone.clone(),
        [
            Point3::new(surface_radius, 0.0, axial),
            Point3::new(0.0, surface_radius * 0.5, axial),
            Point3::new(-surface_radius, 0.0, axial),
            Point3::new(0.0, -surface_radius * 0.5, axial),
        ],
    );
    set_shell_faces(&mut model, vec![face.clone()]);
    let cache_radius = surface_radius - 0.1;
    let vertices = vec![
        Point3::new(cache_radius, 0.0, axial),
        Point3::new(0.0, cache_radius * 0.5, axial),
        Point3::new(-cache_radius, 0.0, axial),
    ];
    let normals = Some(
        vertices
            .iter()
            .map(|point| {
                analytic_surface_normal(cone.solved().expect("solved cone"), *point).unwrap()
            })
            .collect(),
    );
    model.tessellations.push(
        Tessellation::new(
            "synthetic:test:tessellation#cone-cache-mesh",
            cadmpeg_ir::tessellation::TessellationMesh::from_list_lanes(
                vertices,
                vec![[0, 1, 2]],
                normals,
            )
            .expect("normals cover the mesh"),
            Vec::new(),
        )
        .expect("valid tessellation"),
    );

    assert_eq!(
        assign_unique_surface_owners(&mut model).unwrap(),
        vec!["synthetic:test:tessellation#cone-cache-mesh"]
    );
    assert_eq!(model.tessellations[0].faces, vec![face]);
    assert_eq!(
        model.tessellations[0].body,
        Some(BodyId::mint("synthetic:test:body#body").expect("identity grammar"))
    );
    assert!(model.tessellations[0]
        .chordal_deflection()
        .is_some_and(|deflection| deflection.get() > 0.09 && deflection.get() < 0.11));
}

#[test]
fn conical_trim_uses_scaled_angular_coordinate() {
    let trim = ConicalTrim {
        origin: Point3::new(0.0, 0.0, 0.0),
        frame: cadmpeg_ir::units::OrthonormalFrame3::new(
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        radius: 3.0,
        ratio: cadmpeg_ir::scalar::PositiveReal::new(0.5).unwrap(),
        slope: 1.0,
        min_axial: 0.0,
        max_axial: 2.0,
        angular_start: 0.0,
        angular_span: std::f64::consts::FRAC_PI_2,
    };
    let mesh = |point: Point3, id: &str| {
        Tessellation::new(
            id,
            cadmpeg_ir::tessellation::TessellationMesh::List {
                vertices: vec![point],
                triangles: Vec::new(),
            },
            Vec::new(),
        )
        .expect("valid tessellation")
    };
    let point_at = |angle: f64| {
        let local_radius = 4.0;
        Point3::new(
            local_radius * angle.cos(),
            local_radius * trim.ratio.get() * angle.sin(),
            1.0,
        )
    };

    assert!(trim.contains_mesh(
        &mesh(
            point_at(std::f64::consts::FRAC_PI_4),
            "synthetic:test:tessellation#inside"
        ),
        cadmpeg_ir::transform::Transform::identity(),
        0.0,
    ));
    assert!(!trim.contains_mesh(
        &mesh(
            point_at(3.0 * std::f64::consts::FRAC_PI_4),
            "synthetic:test:tessellation#outside"
        ),
        cadmpeg_ir::transform::Transform::identity(),
        0.0,
    ));
}

#[test]
fn unique_nurbs_support_binds_exact_display_list_face() {
    let mut model = model_with_body();
    let surface = test_nurbs_surface();
    let face = add_face(
        &mut model,
        "nurbs-exact",
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface.clone())),
        test_nurbs_corners(&surface),
    );
    set_shell_faces(&mut model, vec![face.clone()]);
    let vertices = [(0.15, 0.2), (0.8, 0.2), (0.5, 0.8)]
        .map(|(u, v)| {
            cadmpeg_ir::eval::nurbs_surface_point(&surface, u, v)
                .unwrap()
                .get()
        })
        .to_vec();
    model.tessellations.push(mesh_from(
        "synthetic:test:tessellation#nurbs-exact-mesh",
        vertices,
        vec![[0, 1, 2]],
    ));

    assert_eq!(
        assign_unique_surface_owners(&mut model).unwrap(),
        vec!["synthetic:test:tessellation#nurbs-exact-mesh"]
    );
    assert_eq!(model.tessellations[0].faces, vec![face]);
    assert_eq!(
        model.tessellations[0].body,
        Some(BodyId::mint("synthetic:test:body#body").expect("identity grammar"))
    );
    assert!(model.tessellations[0].chordal_deflection().is_none());
}

#[test]
fn non_exact_nurbs_support_does_not_use_an_unbounded_cache_fit() {
    let mut model = model_with_body();
    let surface = test_nurbs_surface();
    let face = add_face(
        &mut model,
        "nurbs-cache",
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface.clone())),
        test_nurbs_corners(&surface),
    );
    set_shell_faces(&mut model, vec![face]);
    let samples =
        [(0.15, 0.2), (0.8, 0.2), (0.5, 0.8)].map(|(u, v)| test_nurbs_point_normal(&surface, u, v));
    let deflection = 0.02;
    model.tessellations.push(
        Tessellation::new(
            "synthetic:test:tessellation#nurbs-cache-mesh",
            cadmpeg_ir::tessellation::TessellationMesh::from_list_lanes(
                samples
                    .iter()
                    .map(|(point, normal)| point.translated(*normal, deflection))
                    .collect(),
                vec![[0, 1, 2]],
                Some(samples.iter().map(|(_, normal)| *normal).collect()),
            )
            .expect("normals cover the mesh"),
            Vec::new(),
        )
        .expect("valid tessellation"),
    );

    assert!(assign_unique_surface_owners(&mut model).unwrap().is_empty());
    assert!(model.tessellations[0].faces.is_empty());
    assert!(model.tessellations[0].body.is_none());
    assert!(model.tessellations[0].chordal_deflection().is_none());
}

#[test]
fn coincident_nurbs_supports_do_not_choose_a_display_list_face() {
    let mut model = model_with_body();
    let surface = test_nurbs_surface();
    let corners = test_nurbs_corners(&surface);
    let first = add_face(
        &mut model,
        "nurbs-coincident-first",
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface.clone())),
        corners,
    );
    let second = add_face(
        &mut model,
        "nurbs-coincident-second",
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface.clone())),
        corners,
    );
    {
        for face in [first, second] {
            set_shell_faces(&mut model, vec![face]);
        }
    };
    model.tessellations.push(
        Tessellation::new(
            "synthetic:test:tessellation#nurbs-ambiguous-mesh",
            cadmpeg_ir::tessellation::TessellationMesh::List {
                vertices: [(0.15, 0.2), (0.8, 0.2), (0.5, 0.8)]
                    .map(|(u, v)| {
                        cadmpeg_ir::eval::nurbs_surface_point(&surface, u, v)
                            .unwrap()
                            .get()
                    })
                    .to_vec(),
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .expect("valid tessellation"),
    );

    assert!(assign_unique_surface_owners(&mut model).unwrap().is_empty());
    assert!(model.tessellations[0].faces.is_empty());
    assert!(model.tessellations[0].body.is_none());
}

#[test]
fn coincident_nurbs_and_analytic_supports_do_not_fall_through_to_analytic_fit() {
    let mut model = model_with_body();
    let surface = flat_test_nurbs_surface();
    let corners = test_nurbs_corners(&surface);
    let nurbs_face = add_face(
        &mut model,
        "nurbs-plane-coincident",
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface.clone())),
        corners,
    );
    let plane_face = add_face(
        &mut model,
        "plane-coincident",
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        corners,
    );
    {
        for face in [nurbs_face, plane_face] {
            set_shell_faces(&mut model, vec![face]);
        }
    };
    model.tessellations.push(
        Tessellation::new(
            "synthetic:test:tessellation#nurbs-plane-ambiguous-mesh",
            cadmpeg_ir::tessellation::TessellationMesh::from_list_lanes(
                [(0.15, 0.2), (0.8, 0.2), (0.5, 0.8)]
                    .map(|(u, v)| {
                        cadmpeg_ir::eval::nurbs_surface_point(&surface, u, v)
                            .unwrap()
                            .get()
                    })
                    .to_vec(),
                vec![[0, 1, 2]],
                Some(vec![Vector3::new(0.0, 0.0, 1.0); 3]),
            )
            .expect("normals cover the mesh"),
            Vec::new(),
        )
        .expect("valid tessellation"),
    );

    assert!(assign_unique_surface_owners(&mut model).unwrap().is_empty());
    assert!(model.tessellations[0].faces.is_empty());
    assert!(model.tessellations[0].body.is_none());
}

#[test]
fn circular_hole_excludes_crossing_triangles_but_allows_boundary_chords() {
    let trim = PlanarTrim {
        frame: PlaneFrame::new(
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        outer: Some(PlanarOuter::Polygon(vec![
            Point2::new(-3.0, -3.0),
            Point2::new(3.0, -3.0),
            Point2::new(3.0, 3.0),
            Point2::new(-3.0, 3.0),
        ])),
        holes: vec![PlanarHole::Circle(CircularHole {
            center: Point2::new(0.0, 0.0),
            radius: 1.0,
        })],
        boundary_tolerance: 0.0,
    };
    let mesh = |vertices, triangle| {
        mesh_from("synthetic:test:tessellation#mesh", vertices, vec![triangle])
    };
    let boundary_chord = mesh(
        vec![
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(2.0, 2.0, 0.0),
        ],
        [0, 1, 2],
    );
    let crossing = mesh(
        vec![
            Point3::new(-2.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
        ],
        [0, 1, 2],
    );

    assert!(trim.contains_mesh(
        &boundary_chord,
        cadmpeg_ir::transform::Transform::identity(),
        1.0e-9
    ));
    assert!(!trim.contains_mesh(
        &crossing,
        cadmpeg_ir::transform::Transform::identity(),
        1.0e-9
    ));
}

#[test]
fn polygonal_planar_hole_excludes_inner_face_mesh() {
    let trim = PlanarTrim {
        frame: PlaneFrame::new(
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        outer: Some(PlanarOuter::Polygon(vec![
            Point2::new(-4.0, -4.0),
            Point2::new(4.0, -4.0),
            Point2::new(4.0, 4.0),
            Point2::new(-4.0, 4.0),
        ])),
        holes: vec![PlanarHole::polygon(
            vec![
                Point2::new(-2.0, -2.0),
                Point2::new(2.0, -2.0),
                Point2::new(2.0, 2.0),
                Point2::new(-2.0, 2.0),
            ],
            EPS_DISPLAY_QUANTIZATION,
        )
        .unwrap()],
        boundary_tolerance: 0.0,
    };
    let mesh =
        |vertices, triangles| mesh_from("synthetic:test:tessellation#mesh", vertices, triangles);
    let inner_face = mesh(
        vec![
            Point3::new(-2.0, -2.0, 0.0),
            Point3::new(2.0, -2.0, 0.0),
            Point3::new(2.0, 2.0, 0.0),
            Point3::new(-2.0, 2.0, 0.0),
        ],
        vec![[0, 1, 2], [0, 2, 3]],
    );
    let outer_face = mesh(
        vec![
            Point3::new(-4.0, -4.0, 0.0),
            Point3::new(-3.0, -4.0, 0.0),
            Point3::new(-4.0, -3.0, 0.0),
        ],
        vec![[0, 1, 2]],
    );
    let exterior_boundary_chord = mesh(
        vec![
            Point3::new(-2.0, -2.0, 0.0),
            Point3::new(2.0, -2.0, 0.0),
            Point3::new(0.0, -4.0, 0.0),
        ],
        vec![[0, 1, 2]],
    );
    let interior_boundary_chord = mesh(
        vec![
            Point3::new(-2.0, -2.0, 0.0),
            Point3::new(2.0, -2.0, 0.0),
            Point3::new(4.0, 4.0, 0.0),
        ],
        vec![[0, 1, 2]],
    );

    assert!(!trim.contains_mesh(
        &inner_face,
        cadmpeg_ir::transform::Transform::identity(),
        EPS_DISPLAY_QUANTIZATION
    ));
    assert!(trim.contains_mesh(
        &outer_face,
        cadmpeg_ir::transform::Transform::identity(),
        EPS_DISPLAY_QUANTIZATION
    ));
    assert!(trim.contains_mesh(
        &exterior_boundary_chord,
        cadmpeg_ir::transform::Transform::identity(),
        EPS_DISPLAY_QUANTIZATION
    ));
    assert!(!trim.contains_mesh(
        &interior_boundary_chord,
        cadmpeg_ir::transform::Transform::identity(),
        EPS_DISPLAY_QUANTIZATION
    ));
}

#[test]
fn mixed_planar_holes_reject_overlap() {
    let polygon = vec![
        Point2::new(-2.0, -2.0),
        Point2::new(2.0, -2.0),
        Point2::new(2.0, 2.0),
        Point2::new(-2.0, 2.0),
    ];
    assert!(circle_overlaps_polygon(
        CircularHole {
            center: Point2::new(0.0, 0.0),
            radius: 1.0,
        },
        &polygon,
        EPS_DISPLAY_QUANTIZATION
    ));
    assert!(!circle_overlaps_polygon(
        CircularHole {
            center: Point2::new(4.0, 0.0),
            radius: 1.0,
        },
        &polygon,
        EPS_DISPLAY_QUANTIZATION
    ));
}

#[test]
fn chordal_hole_constraint_uses_the_boundary_sampling_sagitta() {
    let hole = CircularHole {
        center: Point2::new(0.0, 0.0),
        radius: 1.0,
    };
    let boundary = (0..6)
        .map(|index| {
            let angle = f64::from(index) * std::f64::consts::TAU / 6.0;
            Point2::new(angle.cos(), angle.sin())
        })
        .collect::<Vec<_>>();
    let mut chordal = boundary.clone();
    let angle = std::f64::consts::PI / 6.0;
    chordal.push(Point2::new(0.9 * angle.cos(), 0.9 * angle.sin()));
    let (exclusion, boundary_circle) =
        chordal_hole_constraint(hole, &chordal, EPS_DISPLAY_QUANTIZATION).unwrap();
    assert_eq!(boundary_circle.radius, hole.radius);
    assert!(exclusion.radius < hole.radius);
    assert!(exclusion.radius > 0.8);

    let mut deep = boundary;
    deep.push(Point2::new(0.7 * angle.cos(), 0.7 * angle.sin()));
    assert!(chordal_hole_constraint(hole, &deep, EPS_DISPLAY_QUANTIZATION).is_none());

    let interior = vec![Point2::new(0.5, 0.0), Point2::new(0.0, 0.5)];
    assert!(chordal_hole_constraint(hole, &interior, EPS_DISPLAY_QUANTIZATION).is_none());
}

#[test]
fn circular_planar_bounds_choose_one_enclosing_outer() {
    let circles = vec![
        CircularHole {
            center: Point2::new(0.0, 0.0),
            radius: 10.0,
        },
        CircularHole {
            center: Point2::new(6.0, 0.0),
            radius: 2.0,
        },
        CircularHole {
            center: Point2::new(-6.0, 0.0),
            radius: 2.0,
        },
    ];
    let (outer, holes) = circular_outer_and_holes(&circles, EPS_DISPLAY_QUANTIZATION).unwrap();
    assert_eq!(outer.radius, 10.0);
    assert_eq!(holes.len(), 2);

    let ambiguous = vec![
        CircularHole {
            center: Point2::new(0.0, 0.0),
            radius: 10.0,
        },
        CircularHole {
            center: Point2::new(0.0, 0.0),
            radius: 10.0,
        },
    ];
    assert!(circular_outer_and_holes(&ambiguous, EPS_DISPLAY_QUANTIZATION).is_none());
}

#[test]
fn decode_reports_display_list_geometry() {
    let f = sldprt_with_body_and_display_list(&triangle_body());
    let mut cur = Cursor::new(f);

    let result = EditableDecodeResult::from(
        SldprtCodec
            .decode(&mut cur, &DecodeOptions::default())
            .unwrap(),
    );
    let source = result.ir().source.as_ref().expect("source metadata");

    assert_eq!(
        source
            .attributes
            .get("displaylist_vertices")
            .map(String::as_str),
        Some("3")
    );
    assert_eq!(
        source
            .attributes
            .get("displaylist_triangles")
            .map(String::as_str),
        Some("1")
    );
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(result.ir().model.tessellations[0].vertices().len(), 3);
    assert_eq!(result.ir().model.tessellations[0].vertices()[1].x, 1000.0);
    assert_eq!(
        result.ir().model.tessellations[0].triangles(),
        vec![[0, 1, 2]]
    );
    assert_eq!(result.ir().model.tessellations[0].strip_lengths(), vec![3]);
    assert_eq!(result.ir().model.tessellations[0].vertex_normals().len(), 3);
    assert_eq!(result.ir().model.tessellations[0].channels().len(), 6);
    assert_eq!(
        result.ir().model.tessellations[0].faces,
        [result.ir().model.faces[0].id.clone()]
    );
    assert_eq!(
        result.ir().model.tessellations[0].body.as_ref(),
        Some(&result.ir().model.bodies[0].id)
    );
    assert!(!result.report().losses.iter().any(|loss| {
        loss.code.taxonomy() == LossTaxonomy::ReferenceGraphNotClosed
            && loss.message.contains("DisplayLists tessellation")
    }));
    assert!(result
        .ir()
        .native_unknowns("sldprt")
        .unwrap()
        .iter()
        .any(|record| {
            result
                .source_fidelity()
                .annotations
                .provenance
                .get(record.id.as_str())
                .and_then(|note| note.tag.as_deref())
                == Some("displaylist_tessellation")
                && result
                    .source_fidelity()
                    .retained_record(record.id.as_str())
                    .is_some_and(|source| source.data().is_some())
        }));
}

#[test]
fn decode_reports_extended_header_display_list_geometry() {
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(
        0x41,
        "Contents/DisplayLists",
        &extended_display_list_payload(),
    ));
    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(result.ir().model.tessellations[0].triangles(), [[0, 1, 2]]);
}

#[test]
fn decode_rejects_incoherent_display_list_header_counts() {
    let mut payload = display_list_payload();
    let marker = b"uoTempFaceTessData_c";
    let header = payload
        .windows(marker.len())
        .position(|bytes| bytes == marker)
        .expect("face tessellation class")
        + marker.len();
    payload[header..header + 4].copy_from_slice(&2_u32.to_le_bytes());
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x41, "Contents/DisplayLists", &payload));

    let error = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect_err("an incoherent display-face header must refuse the decode");
    let text = error.to_string();
    assert!(
        text.contains("display-face table")
            && text.contains("header states 2 triangle(s) and 1 strip(s)")
            && text.contains("parsed mesh has 1 triangle(s) and 1 strip(s)"),
        "{text}"
    );
}

#[test]
fn decode_rejects_inconsistent_display_list_table() {
    let mut payload = display_list_payload();
    let marker = b"uoTempFaceTessData_c";
    let at = payload
        .windows(marker.len())
        .position(|bytes| bytes == marker)
        .unwrap()
        + marker.len()
        + 8
        + 16;
    payload[at..at + 4].copy_from_slice(&4u32.to_le_bytes());
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x41, "Contents/DisplayLists", &payload));

    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.tessellations.is_empty());
    assert!(!result
        .ir()
        .source
        .as_ref()
        .unwrap()
        .attributes
        .contains_key("displaylist_vertices"));
}

#[test]
fn decode_rejects_nonfinite_display_list_values() {
    let mut payload = display_list_payload();
    let marker = b"uoTempFaceTessData_c";
    let position_data = payload
        .windows(marker.len())
        .position(|bytes| bytes == marker)
        .unwrap()
        + marker.len()
        + 8
        + 16
        + 4
        + 16;
    payload[position_data..position_data + 4].copy_from_slice(&f32::NAN.to_le_bytes());
    let mut source = sldprt_with_body(&triangle_body());
    source.extend(make_block(0x41, "Contents/DisplayLists", &payload));

    let result = SldprtCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .unwrap();
    assert!(result.ir().model.tessellations.is_empty());
}

#[test]
fn planar_boundary_accepts_bounded_ellipse_arcs() {
    const SAMPLE_TOLERANCE: f64 = 1.0e-4;
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));
    let frame = plane_frame(surface.solved().expect("solved carrier")).unwrap();
    let curve = CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
        cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            1.0,
        )
        .unwrap(),
    ));
    let (samples, boundary_tolerance) = planar_boundary_samples(
        &curve,
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
        &surface,
        frame,
        EPS_DISPLAY_QUANTIZATION,
        SAMPLE_TOLERANCE,
    )
    .unwrap();

    assert!(samples.len() > 1);
    assert!(boundary_tolerance <= SAMPLE_TOLERANCE);
    assert_eq!(samples.first(), Some(&Point2::new(2.0, 0.0)));
    assert!(shortest_arc_span(0.0, std::f64::consts::PI).is_none());
}

#[test]
fn planar_boundary_accepts_bounded_circle_arcs() {
    const SAMPLE_TOLERANCE: f64 = 1.0e-4;
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));
    let frame = plane_frame(surface.solved().expect("solved carrier")).unwrap();
    let curve = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .unwrap(),
    ));
    let (samples, boundary_tolerance) = planar_boundary_samples(
        &curve,
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
        &surface,
        frame,
        EPS_DISPLAY_QUANTIZATION,
        SAMPLE_TOLERANCE,
    )
    .unwrap();

    assert!(samples.len() > 1);
    assert!(boundary_tolerance <= SAMPLE_TOLERANCE);
    assert_eq!(samples.first(), Some(&Point2::new(2.0, 0.0)));
}

#[test]
fn circular_arc_trim_disambiguates_coincident_planar_supports() {
    let mut model = model_with_body();
    let target = add_square_face(&mut model, "arc-target", 0.0);
    let competitor = add_face(
        &mut model,
        "arc-competitor",
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        [
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        ],
    );
    for (curve_id, radius) in [
        ("synthetic:test:curve#curve-arc-competitor-0", 2.0),
        ("synthetic:test:curve#curve-arc-competitor-2", 1.0),
    ] {
        model
            .curves
            .iter_mut()
            .find(|curve| curve.id.as_str() == curve_id)
            .unwrap()
            .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                radius,
            )
            .unwrap(),
        ));
    }
    set_shell_faces(&mut model, vec![target.clone(), competitor]);
    model.tessellations.push(
        Tessellation::new(
            "synthetic:test:tessellation#arc-trim-mesh",
            cadmpeg_ir::tessellation::TessellationMesh::List {
                vertices: vec![
                    Point3::new(0.25, -0.75, 0.0),
                    Point3::new(1.75, -0.75, 0.0),
                    Point3::new(1.0, -0.25, 0.0),
                ],
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .expect("valid tessellation"),
    );

    assert_eq!(
        assign_unique_surface_owners(&mut model).unwrap(),
        vec!["synthetic:test:tessellation#arc-trim-mesh"]
    );
    assert_eq!(model.tessellations[0].faces, vec![target]);
    assert_eq!(
        model.tessellations[0].body,
        Some(BodyId::mint("synthetic:test:body#body").expect("identity grammar"))
    );
}

#[test]
fn planar_trim_accepts_concave_simple_loops_and_rejects_crossings() {
    const CONTAINMENT_TOLERANCE: f64 = 1.0e-9;
    let concave = vec![
        Point2::new(0.0, 0.0),
        Point2::new(4.0, 0.0),
        Point2::new(4.0, 4.0),
        Point2::new(2.0, 4.0),
        Point2::new(2.0, 2.0),
        Point2::new(0.0, 2.0),
    ];
    assert!(is_simple_polygon(&concave, CONTAINMENT_TOLERANCE));
    assert!(PlanarHole::polygon(concave.clone(), CONTAINMENT_TOLERANCE).is_some());
    assert!(polygon_contains(
        &concave,
        Point2::new(1.0, 1.0),
        CONTAINMENT_TOLERANCE
    ));
    assert!(polygon_contains(
        &concave,
        Point2::new(3.0, 3.0),
        CONTAINMENT_TOLERANCE
    ));
    assert!(!polygon_contains(
        &concave,
        Point2::new(1.0, 3.0),
        CONTAINMENT_TOLERANCE
    ));

    let crossing = vec![
        Point2::new(0.0, 0.0),
        Point2::new(4.0, 4.0),
        Point2::new(0.0, 4.0),
        Point2::new(4.0, 0.0),
    ];
    assert!(!is_simple_polygon(&crossing, CONTAINMENT_TOLERANCE));
}

#[test]
fn persistent_surface_source_sentinels_are_absent() {
    for source in [0, u32::MAX] {
        let payload = framed_surface_reference(&format!("moPlaneSurfIdRep_c,{source},3,"));
        let references = decoded_references(
            &payload,
            ByteRange::new(0, payload.len()).expect("ordered range"),
        );
        assert!(references.is_empty());
    }
}

/// A display-list table whose normal lane does not cover its vertex lane
/// states a shaded mesh it cannot fill. It is refused by name, not dropped.
#[test]
fn a_short_normal_lane_refuses_the_display_table() {
    let mut payload = descriptor(4, 8, 1, &3_u32.to_le_bytes());
    let positions = [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect::<Vec<_>>();
    payload.extend(descriptor(12, 100, 3, &positions));
    // Two normals against three vertices.
    payload.extend(descriptor(12, 100, 2, &[0; 24]));
    payload.extend(descriptor(4, 8, 4, &[0; 16]));
    payload.extend(descriptor(4, 8, 1, &4_u32.to_le_bytes()));
    payload.extend(descriptor(1, 8, 4, &[0; 4]));

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &payload,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("root");
    let error = parse_table(&ctx, &payload, 0)
        .expect_err("a short normal lane is refused")
        .to_string();
    assert!(error.contains("vertex normal(s)"), "{error}");
}

const EPS_FOLLOWUP_ARC_SAGITTA: f64 = 1e-9;

#[test]
fn numerical_followup_arc_error_retains_the_sagitta_at_the_segment_cap() {
    let (segments, error) = super::planar_arc_segments(1e-5, 1e12, EPS_FOLLOWUP_ARC_SAGITTA);
    assert_eq!(segments, super::MAX_PLANAR_TRIM_ARC_SEGMENTS);
    let expected = 2e12 * (1e-5 / (4.0 * segments as f64)).sin().powi(2);
    assert!(error > EPS_FOLLOWUP_ARC_SAGITTA);
    assert!((error / expected - 1.0).abs() <= 4.0 * f64::EPSILON);
}

mod geometry_predicates;
