// SPDX-License-Identifier: Apache-2.0
use super::super::{PlanarOuter, PlanarTrim, PlaneFrame};
use cadmpeg_core::decode::WorkBudget;
use cadmpeg_ir::math::{Point2, Point3, Vector3};

const EPS_AREA_SUM: f64 = 1e-10;

#[test]
fn sole_surface_owner_respects_its_planar_trim() {
    let mut model = super::model_with_body();
    let face = super::add_square_face(&mut model, "bounded", 0.);
    super::set_shell_faces(&mut model, vec![face]);
    model.tessellations.push(
        cadmpeg_ir::tessellation::Tessellation::new(
            cadmpeg_ir::tessellation::TessellationId::mint("synthetic:test:tessellation#outside")
                .unwrap(),
            cadmpeg_ir::tessellation::TessellationMesh::List {
                vertices: vec![
                    Point3::new(10., 10., 0.),
                    Point3::new(11., 10., 0.),
                    Point3::new(10., 11., 0.),
                ],
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .unwrap(),
    );
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(
        super::super::assign_unique_surface_owners(&ctx, &mut model, &mut Vec::new())
            .unwrap()
            .is_empty()
    );
    assert!(model.tessellations[0].faces.is_empty());
    assert!(model.tessellations[0].body.is_none());
}

#[test]
fn owner_work_cap_leaves_tables_unassigned_and_names_the_bound() {
    let mesh = |name: &str| {
        cadmpeg_ir::tessellation::Tessellation::new(
            cadmpeg_ir::tessellation::TessellationId::mint(name).unwrap(),
            cadmpeg_ir::tessellation::TessellationMesh::List {
                vertices: vec![
                    Point3::new(0.1, 0.1, 0.),
                    Point3::new(0.4, 0.1, 0.),
                    Point3::new(0.1, 0.4, 0.),
                ],
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .unwrap()
    };
    let model = || {
        let mut model = super::model_with_body();
        let face = super::add_square_face(&mut model, "bounded", 0.);
        super::set_shell_faces(&mut model, vec![face]);
        model
            .tessellations
            .push(mesh("synthetic:test:tessellation#first"));
        model
            .tessellations
            .push(mesh("synthetic:test:tessellation#second"));
        model
    };
    let ctx = cadmpeg_test_support::service_decode_context();

    let mut unbounded = model();
    let mut losses = Vec::new();
    let assigned =
        super::super::assign_unique_surface_owners(&ctx, &mut unbounded, &mut losses).unwrap();
    assert_eq!(assigned.len(), 2);
    assert!(losses.is_empty());

    let mut capped = model();
    let mut losses = Vec::new();
    let assigned =
        super::super::assign_unique_surface_owners_within(&ctx, &mut capped, &mut losses, 1)
            .unwrap();
    assert!(assigned.is_empty());
    assert!(capped
        .tessellations
        .iter()
        .all(|mesh| mesh.faces.is_empty() && mesh.body.is_none()));
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::SldprtLossCode::TessellationOwnerWorkCapped.kind()
    );
    assert_eq!(
        losses[0].message,
        "2 DisplayLists tessellation table(s) were left without a surface owner because owner \
         assignment exceeded the local work cap TESSELLATION_OWNER_WORK (1)."
    );
}

#[test]
fn planar_trim_rejects_a_triangle_spanning_a_concave_outer() {
    let boundary = [
        [0., 0.],
        [3., 0.],
        [3., 3.],
        [2., 3.],
        [2., 1.],
        [1., 1.],
        [1., 3.],
        [0., 3.],
    ]
    .map(|p| Point2::new(p[0], p[1]))
    .to_vec();
    let trim = PlanarTrim {
        frame: PlaneFrame::new(
            cadmpeg_ir::features::FinitePoint3::ZERO,
            Vector3::new(0., 0., 1.),
            Vector3::new(1., 0., 0.),
        )
        .unwrap(),
        outer: Some(PlanarOuter::Polygon(boundary)),
        holes: Vec::new(),
        boundary_tolerance: 0.,
    };
    let mesh = cadmpeg_ir::tessellation::Tessellation::new(
        cadmpeg_ir::tessellation::TessellationId::mint("synthetic:test:tessellation#concave")
            .unwrap(),
        cadmpeg_ir::tessellation::TessellationMesh::List {
            vertices: vec![
                Point3::new(0.5, 2.5, 0.),
                Point3::new(2.5, 2.5, 0.),
                Point3::new(1.5, 0.5, 0.),
            ],
            triangles: vec![[0, 1, 2]],
        },
        Vec::new(),
    )
    .unwrap();
    assert!(!trim.contains_mesh(
        &mesh,
        cadmpeg_ir::transform::Transform::identity(),
        super::EPS_DISPLAY_QUANTIZATION,
        &WorkBudget::new(usize::MAX),
    ));
}

#[test]
fn polygon_triangle_check_accepts_edges_on_the_outer_boundary() {
    let boundary = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]].map(|p| Point2::new(p[0], p[1]));
    assert!(super::super::polygon_contains_triangle(
        &boundary,
        [boundary[0], boundary[1], boundary[2]],
        super::EPS_DISPLAY_QUANTIZATION
    ));
}

#[test]
fn single_planar_polygon_yields_a_planar_trim() {
    let mut model = super::model_with_body();
    let face_id = super::add_square_face(&mut model, "budget", 0.);
    let loops = model.loops.iter().map(|entry| (&entry.id, entry)).collect();
    let coedges = model
        .coedges
        .iter()
        .map(|entry| (&entry.id, entry))
        .collect();
    let edges = model.edges.iter().map(|entry| (&entry.id, entry)).collect();
    let vertices = model
        .vertices
        .iter()
        .map(|entry| (&entry.id, entry))
        .collect();
    let points = model
        .points
        .iter()
        .map(|entry| (&entry.id, entry.position().get()))
        .collect();
    let curves = model
        .curves
        .iter()
        .map(|entry| (&entry.id, &entry.geometry))
        .collect();
    let topology = super::super::TrimTopology {
        loops: &loops,
        coedges: &coedges,
        edges: &edges,
        vertices: &vertices,
        points: &points,
        curves: &curves,
        coordinate_scale: super::super::trim_coordinate_scale(&points),
    };
    let face = model.faces.iter().find(|face| face.id == face_id).unwrap();
    let surface = &model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)
        .unwrap()
        .geometry;
    let work = WorkBudget::new(usize::MAX);
    assert!(super::super::planar_trim(face, surface, &topology, &work).is_some());
}

#[test]
fn polygon_triangle_check_refuses_a_self_crossing_outer() {
    let boundary = [[0., 0.], [2., 0.], [0., 2.], [2., 2.]].map(|p| Point2::new(p[0], p[1]));
    assert!(!super::super::polygon_contains_triangle(
        &boundary,
        [boundary[0], boundary[1], boundary[2]],
        super::EPS_DISPLAY_QUANTIZATION
    ));
}

#[test]
fn convex_polygon_ear_search_preserves_area() {
    let polygon = (0..256)
        .map(|i| {
            let angle = f64::from(i) * std::f64::consts::TAU / 256.0;
            Point2::new(angle.cos(), angle.sin())
        })
        .collect::<Vec<_>>();
    let triangles = super::super::triangulate_polygon(
        &polygon,
        super::EPS_DISPLAY_QUANTIZATION,
        &WorkBudget::new(usize::MAX),
    )
    .unwrap();
    assert_eq!(triangles.len(), polygon.len() - 2);
    let area = triangles
        .iter()
        .map(|triangle| super::super::signed_area_twice(triangle[0], triangle[1], triangle[2]))
        .sum::<f64>();
    let expected =
        super::super::simple_polygon_area_twice(&polygon, super::EPS_DISPLAY_QUANTIZATION)
            .unwrap()
            .get();
    assert!((area - expected).abs() <= EPS_AREA_SUM);
}

#[test]
fn checked_convex_boundary_is_reused_for_every_triangle() {
    let polygon = (0..256)
        .map(|i| {
            let angle = f64::from(i) * std::f64::consts::TAU / 256.0;
            Point2::new(angle.cos(), angle.sin())
        })
        .collect::<Vec<_>>();
    let work = WorkBudget::new(usize::MAX);
    let boundary =
        super::super::CheckedPlanarPolygon::new(&polygon, super::EPS_DISPLAY_QUANTIZATION, &work)
            .unwrap();
    for index in 1..polygon.len() - 1 {
        assert!(boundary.contains_triangle([polygon[0], polygon[index], polygon[index + 1]]));
    }
}

#[test]
fn planar_boundary_pairs_detect_outside_and_shared_edge() {
    let polygon = (0..4096)
        .map(|i| {
            let angle = f64::from(i) * std::f64::consts::TAU / 4096.0;
            Point2::new(10.0 + angle.cos(), 10.0 + angle.sin())
        })
        .collect::<Vec<_>>();
    let outside = [
        Point2::new(-2.0, -2.0),
        Point2::new(2.0, -2.0),
        Point2::new(0.0, 2.0),
    ];
    let shared_edge = [polygon[0], polygon[1], Point2::new(10.0, 10.0)];
    assert!(!super::super::polygon_inside_polygon(
        &polygon,
        &outside,
        super::EPS_DISPLAY_QUANTIZATION,
    ));
    assert!(super::super::polygons_overlap(
        &polygon,
        &shared_edge,
        super::EPS_DISPLAY_QUANTIZATION,
    ));
}

#[test]
fn disjoint_planar_boundaries_do_not_overlap() {
    let polygon = |center: f64| {
        (0u32..4096)
            .map(|index| {
                let angle = f64::from(index) * std::f64::consts::TAU / 4096.0;
                Point2::new(center + angle.cos(), angle.sin())
            })
            .collect::<Vec<_>>()
    };
    let first = polygon(0.0);
    let second = polygon(4.0);
    assert!(!super::super::polygons_overlap(
        &first,
        &second,
        super::EPS_DISPLAY_QUANTIZATION
    ));
}

#[test]
fn convex_outer_box_proves_inner_boundary_containment() {
    let polygon = |radius: f64| {
        (0u32..4096)
            .map(|index| {
                let angle = f64::from(index) * std::f64::consts::TAU / 4096.0;
                Point2::new(radius * angle.cos(), radius * angle.sin())
            })
            .collect::<Vec<_>>()
    };
    let inner = polygon(1.0);
    let outer = polygon(4.0);
    assert!(super::super::polygon_inside_polygon(
        &inner,
        &outer,
        super::EPS_DISPLAY_QUANTIZATION
    ));
}
