// SPDX-License-Identifier: Apache-2.0
//! Synthetic meshes through [`add_mesh_body`].

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::topology::BodyKind;
use cadmpeg_ir::validate::validate_neutral;

use super::{
    add_mesh_body, repair, weld, MeshBrepError, MeshBrepOptions, MeshBrepSummary, MeshInput,
};

type Positions = Vec<[f64; 3]>;
type Triangles = Vec<[u32; 3]>;

fn index(value: usize) -> u32 {
    u32::try_from(value).unwrap()
}

fn convert(
    positions: &[[f64; 3]],
    triangles: &[[u32; 3]],
) -> Result<(CadIr, MeshBrepSummary), MeshBrepError> {
    let mut ir = CadIr::empty();
    let input = MeshInput {
        scope: "body/0",
        name: Some("test body"),
        color: None,
        positions,
        triangles,
    };
    let summary = add_mesh_body(&mut ir, &input, MeshBrepOptions::default())?;
    ir.finalize();
    Ok((ir, summary))
}

fn assert_valid(ir: &CadIr) {
    let report = validate_neutral(ir, Vec::new()).unwrap();
    let errors: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.severity >= cadmpeg_ir::report::Severity::Error)
        .map(|f| f.message.clone())
        .collect();
    assert!(errors.is_empty(), "validation errors: {errors:?}");
}

fn quads_to_triangles(quads: &[[u32; 4]]) -> Triangles {
    quads
        .iter()
        .flat_map(|&[a, b, c, d]| [[a, b, c], [a, c, d]])
        .collect()
}

fn cube_corners(origin: [f64; 3], size: f64) -> Positions {
    let [x, y, z] = origin;
    let s = size;
    vec![
        [x, y, z],
        [x + s, y, z],
        [x + s, y + s, z],
        [x, y + s, z],
        [x, y, z + s],
        [x + s, y, z + s],
        [x + s, y + s, z + s],
        [x, y + s, z + s],
    ]
}

/// Outward-facing quads over the eight corners; the top is the second.
const CUBE_QUADS: [[u32; 4]; 6] = [
    [0, 3, 2, 1],
    [4, 5, 6, 7],
    [0, 1, 5, 4],
    [1, 2, 6, 5],
    [2, 3, 7, 6],
    [3, 0, 4, 7],
];

fn cube() -> (Positions, Triangles) {
    (
        cube_corners([0.0; 3], 10.0),
        quads_to_triangles(&CUBE_QUADS),
    )
}

fn body_kind(ir: &CadIr) -> BodyKind {
    ir.model.bodies.first().unwrap().kind
}

#[test]
fn unit_cube_has_six_faces() {
    let (positions, triangles) = cube();
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert_eq!((summary.faces, summary.edges, summary.vertices), (6, 12, 8));
    assert!(summary.closed);
    assert_eq!((summary.open_edges, summary.non_manifold_edges), (0, 0));
    assert!((summary.signed_volume_mm3 - 1000.0).abs() < 1e-6);
    assert_eq!(body_kind(&ir), BodyKind::Solid);
    assert_eq!(ir.model.coedges.len(), 24);
    assert_valid(&ir);
}

#[test]
fn inside_out_cube_is_reversed() {
    let (positions, mut triangles) = cube();
    for triangle in &mut triangles {
        triangle.swap(1, 2);
    }
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert!(summary.reversed);
    assert!(summary.closed);
    assert!((summary.signed_volume_mm3 - 1000.0).abs() < 1e-6);
    assert_eq!(body_kind(&ir), BodyKind::Solid);
    assert_valid(&ir);
}

/// A tiny deterministic pseudo-random sequence in `[-1, 1]`.
fn noise(state: &mut u64) -> f64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    let high = u32::try_from(*state >> 33).unwrap();
    f64::from(high) / f64::from(1_u32 << 30) - 1.0
}

/// Every face of a cube as its own `n` by `n` grid with private vertices.
fn grid_cube(n: u32, jitter: f64) -> (Positions, Triangles) {
    grid_cube_with([n; 6], jitter)
}

/// Every face of a cube as its own grid with private vertices; face `k` has
/// `counts[k]` cells per side, so neighbouring faces disagree on their shared
/// edge.
fn grid_cube_with(counts: [u32; 6], jitter: f64) -> (Positions, Triangles) {
    let size = 10.0;
    // origin, u axis, v axis with u x v pointing outward.
    let faces: [([f64; 3], [f64; 3], [f64; 3]); 6] = [
        ([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
        ([0.0, 0.0, size], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ([size, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, size, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        ([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
    ];
    let mut state = 12345_u64;
    let (mut positions, mut triangles) = (Vec::new(), Vec::new());
    for ((origin, u, v), n) in faces.into_iter().zip(counts) {
        let base = index(positions.len());
        for j in 0..=n {
            for i in 0..=n {
                let (s, t) = (
                    f64::from(i) / f64::from(n) * size,
                    f64::from(j) / f64::from(n) * size,
                );
                let mut p = [0.0; 3];
                for k in 0..3 {
                    p[k] = origin[k] + u[k] * s + v[k] * t + jitter * noise(&mut state);
                }
                positions.push(p);
            }
        }
        let at = |i: u32, j: u32| base + j * (n + 1) + i;
        for j in 0..n {
            for i in 0..n {
                triangles.push([at(i, j), at(i + 1, j), at(i + 1, j + 1)]);
                triangles.push([at(i, j), at(i + 1, j + 1), at(i, j + 1)]);
            }
        }
    }
    (positions, triangles)
}

#[test]
fn tessellated_jittered_cube_merges_back_to_six_faces() {
    let (positions, triangles) = grid_cube(3, 1.0e-7);
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert_eq!((summary.faces, summary.edges, summary.vertices), (6, 12, 8));
    assert!(summary.closed, "{summary:?}");
    assert_eq!(summary.welded_points, 40);
    assert_valid(&ir);
}

#[test]
fn tessellated_cube_keeps_collinear_vertices_when_tolerance_is_exceeded() {
    // Pull one mid-edge vertex 0.1 mm off its cube edge: it is no longer
    // collinear, so it must survive as a vertex (the front face goes
    // non-planar there and splits).
    let (mut positions, triangles) = grid_cube(2, 0.0);
    for p in &mut positions {
        if (p[0] - 5.0).abs() < 1e-9 && p[1].abs() < 1e-9 && p[2].abs() < 1e-9 {
            p[1] -= 0.1;
        }
    }
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert!(summary.vertices > 8 && summary.faces > 6, "{summary:?}");
    assert_valid(&ir);
}

fn plate_with_hole() -> (Positions, Triangles) {
    let outer = [[0.0, 0.0], [20.0, 0.0], [20.0, 20.0], [0.0, 20.0]];
    let inner = [[7.0, 7.0], [13.0, 7.0], [13.0, 13.0], [7.0, 13.0]];
    let mut positions = Vec::new();
    for z in [0.0, 2.0] {
        for [x, y] in outer.into_iter().chain(inner) {
            positions.push([x, y, z]);
        }
    }
    // Layer offsets: bottom 0, top 8; outer corners 0..4, inner corners 4..8.
    let at = |layer: u32, ring: u32, k: u32| layer * 8 + ring * 4 + k % 4;
    let mut triangles = Vec::new();
    for k in 0..4 {
        let (o0, o1, i1, i0) = (at(1, 0, k), at(1, 0, k + 1), at(1, 1, k + 1), at(1, 1, k));
        triangles.push([o0, o1, i1]);
        triangles.push([o0, i1, i0]);
        let (o0, o1, i1, i0) = (at(0, 0, k), at(0, 0, k + 1), at(0, 1, k + 1), at(0, 1, k));
        triangles.push([o0, i1, o1]);
        triangles.push([o0, i0, i1]);
        // Outer wall, outward.
        let wall = [at(0, 0, k), at(0, 0, k + 1), at(1, 0, k + 1), at(1, 0, k)];
        triangles.extend(quads_to_triangles(&[wall]));
        // Hole wall, facing the hole axis.
        let hole = [at(0, 1, k + 1), at(0, 1, k), at(1, 1, k), at(1, 1, k + 1)];
        triangles.extend(quads_to_triangles(&[hole]));
    }
    (positions, triangles)
}

#[test]
fn plate_with_through_hole_has_inner_loops() {
    let (positions, triangles) = plate_with_hole();
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert_eq!(
        (summary.faces, summary.edges, summary.vertices),
        (10, 24, 16)
    );
    assert!(summary.closed);
    let with_hole = ir.model.faces.iter().filter(|f| f.loops.len() == 2).count();
    assert_eq!(with_hole, 2);
    assert!(ir
        .model
        .faces
        .iter()
        .filter(|f| f.loops.len() == 2)
        .all(|f| matches!(f.loops, cadmpeg_ir::topology::FaceLoops::Classified { .. })));
    assert_valid(&ir);
}

fn cylinder(segments: u32, radius: f64, height: f64) -> (Positions, Triangles) {
    let mut positions = Vec::new();
    for z in [0.0, height] {
        for k in 0..segments {
            let angle = std::f64::consts::TAU * f64::from(k) / f64::from(segments);
            positions.push([radius * angle.cos(), radius * angle.sin(), z]);
        }
    }
    let (bottom_centre, top_centre) = (2 * segments, 2 * segments + 1);
    positions.push([0.0, 0.0, 0.0]);
    positions.push([0.0, 0.0, height]);
    let mut triangles = Vec::new();
    for k in 0..segments {
        let next = (k + 1) % segments;
        let (b0, b1, t0, t1) = (k, next, segments + k, segments + next);
        triangles.extend(quads_to_triangles(&[[b0, b1, t1, t0]]));
        triangles.push([bottom_centre, b1, b0]);
        triangles.push([top_centre, t0, t1]);
    }
    (positions, triangles)
}

#[test]
fn faceted_cylinder_has_side_faces_and_two_caps() {
    let (positions, triangles) = cylinder(32, 10.0, 20.0);
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert_eq!(
        (summary.faces, summary.edges, summary.vertices),
        (34, 96, 64)
    );
    assert!(summary.closed);
    assert_valid(&ir);
}

#[test]
fn fine_cylinder_does_not_drift_into_one_face() {
    let (positions, triangles) = cylinder(360, 10.0, 20.0);
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert_eq!(summary.faces, 362);
    assert_valid(&ir);
}

#[test]
fn two_disjoint_cubes_are_two_regions_of_one_body() {
    let (mut positions, mut triangles) = cube();
    let offset = index(positions.len());
    positions.extend(cube_corners([100.0, 0.0, 0.0], 5.0));
    triangles.extend(quads_to_triangles(
        &CUBE_QUADS.map(|q| q.map(|i| i + offset)),
    ));
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert_eq!(
        (summary.faces, summary.edges, summary.vertices),
        (12, 24, 16)
    );
    assert_eq!(summary.shells, 2);
    assert_eq!((ir.model.bodies.len(), ir.model.regions.len()), (1, 2));
    assert_eq!(body_kind(&ir), BodyKind::Solid);
    assert_valid(&ir);
}

#[test]
fn cube_without_top_is_a_sheet_with_four_open_edges() {
    let (positions, _) = cube();
    let quads: Vec<[u32; 4]> = CUBE_QUADS
        .iter()
        .copied()
        .filter(|q| *q != CUBE_QUADS[1])
        .collect();
    let (ir, summary) = convert(&positions, &quads_to_triangles(&quads)).unwrap();
    assert_eq!(summary.open_edges, 4);
    assert!(!summary.closed);
    assert_eq!((summary.faces, summary.edges, summary.vertices), (5, 12, 8));
    assert_eq!(body_kind(&ir), BodyKind::Sheet);
    assert_valid(&ir);
}

#[test]
fn hostile_inputs_are_refused_without_panic() {
    let (positions, triangles) = cube();
    let mut bad_index = triangles.clone();
    bad_index[3][1] = 99;
    assert_eq!(
        convert(&positions, &bad_index).unwrap_err(),
        MeshBrepError::IndexOutOfRange {
            triangle: 3,
            index: 99
        }
    );
    let mut nan = positions.clone();
    nan[2][1] = f64::NAN;
    assert_eq!(
        convert(&nan, &triangles).unwrap_err(),
        MeshBrepError::NonFinitePosition { index: 2 }
    );
    assert_eq!(convert(&positions, &[]).unwrap_err(), MeshBrepError::Empty);
    assert_eq!(convert(&[], &[]).unwrap_err(), MeshBrepError::Empty);
    assert_eq!(
        convert(&positions, &[[0, 0, 1], [1, 2, 2]]).unwrap_err(),
        MeshBrepError::AllDegenerate
    );
    let collinear = [[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
    assert_eq!(
        convert(&collinear, &[[0, 1, 2]]).unwrap_err(),
        MeshBrepError::AllDegenerate
    );
    assert_eq!(
        convert(
            &[[1.0e300, 0.0, 0.0], [0.0; 3], [0.0, 1.0, 0.0]],
            &[[0, 1, 2]]
        )
        .unwrap_err(),
        MeshBrepError::CoordinateTooLarge
    );
    let mut ir = CadIr::empty();
    for tolerance in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let input = MeshInput {
            scope: "x",
            name: None,
            color: None,
            positions: &positions,
            triangles: &triangles,
        };
        let options = MeshBrepOptions {
            weld_tolerance_mm: tolerance,
        };
        assert_eq!(
            add_mesh_body(&mut ir, &input, options).unwrap_err(),
            MeshBrepError::InvalidTolerance
        );
    }
    assert!(ir.model.bodies.is_empty());
}

#[test]
fn three_triangles_on_one_edge_become_a_sheet() {
    let positions = vec![
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [0.0, 10.0, 0.0],
        [0.0, -10.0, 0.0],
        [0.0, 0.0, 10.0],
    ];
    let triangles = vec![[0, 1, 2], [1, 0, 3], [0, 1, 4]];
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert_eq!(summary.non_manifold_edges, 1);
    assert!(!summary.closed);
    assert_eq!(body_kind(&ir), BodyKind::Sheet);
    assert_valid(&ir);
}

#[test]
fn same_direction_pair_is_split_and_reported() {
    let positions = vec![
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [0.0, 10.0, 0.0],
        [0.0, -10.0, 0.0],
    ];
    let triangles = vec![[0, 1, 2], [0, 1, 3]];
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert_eq!(summary.non_manifold_edges, 1);
    assert_eq!(body_kind(&ir), BodyKind::Sheet);
    assert_valid(&ir);
}

#[test]
fn conversion_is_deterministic() {
    let (positions, triangles) = grid_cube(3, 1.0e-7);
    let (first, first_summary) = convert(&positions, &triangles).unwrap();
    let (second, second_summary) = convert(&positions, &triangles).unwrap();
    assert_eq!(first_summary, second_summary);
    assert_eq!(format!("{:?}", first.model), format!("{:?}", second.model));
}

#[test]
fn several_bodies_share_one_model() {
    let (positions, triangles) = cube();
    let mut ir = CadIr::empty();
    for scope in ["a", "b"] {
        let input = MeshInput {
            scope,
            name: None,
            color: None,
            positions: &positions,
            triangles: &triangles,
        };
        add_mesh_body(&mut ir, &input, MeshBrepOptions::default()).unwrap();
    }
    ir.finalize();
    assert_eq!(ir.model.bodies.len(), 2);
    assert_valid(&ir);
}

#[test]
fn weld_joins_points_across_a_cell_boundary() {
    // Cell edges lie at multiples of the tolerance.
    let positions = [
        [0.000_999_9, 0.0, 0.0],
        [0.001_000_1, 0.0, 0.0],
        [5.0, 0.0, 0.0],
    ];
    let welded = weld::weld(&positions, &[[0, 1, 2]], 0.001).unwrap();
    assert_eq!(welded.positions.len(), 2);
    assert_eq!(welded.map, vec![0, 0, 1]);
}

/// UV sphere: `slices` by `stacks`, about `2 * slices * stacks` triangles.
fn sphere(slices: u32, stacks: u32, radius: f64) -> (Positions, Triangles) {
    let mut positions = vec![[0.0, 0.0, radius]];
    for i in 1..stacks {
        let polar = std::f64::consts::PI * f64::from(i) / f64::from(stacks);
        for j in 0..slices {
            let azimuth = std::f64::consts::TAU * f64::from(j) / f64::from(slices);
            positions.push([
                radius * polar.sin() * azimuth.cos(),
                radius * polar.sin() * azimuth.sin(),
                radius * polar.cos(),
            ]);
        }
    }
    positions.push([0.0, 0.0, -radius]);
    let south = index(positions.len() - 1);
    let ring = |i: u32, j: u32| 1 + (i - 1) * slices + j % slices;
    let mut triangles = Vec::new();
    for j in 0..slices {
        triangles.push([0, ring(1, j), ring(1, j + 1)]);
        triangles.push([south, ring(stacks - 1, j + 1), ring(stacks - 1, j)]);
    }
    for i in 1..stacks - 1 {
        for j in 0..slices {
            let (a, b, c, d) = (
                ring(i, j),
                ring(i + 1, j),
                ring(i + 1, j + 1),
                ring(i, j + 1),
            );
            triangles.push([a, b, c]);
            triangles.push([a, c, d]);
        }
    }
    (positions, triangles)
}

#[test]
fn small_sphere_is_valid() {
    let (positions, triangles) = sphere(16, 8, 10.0);
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert!(summary.closed);
    assert_valid(&ir);
}

#[test]
#[ignore = "timing probe: cargo test --release -- --ignored --nocapture"]
fn large_sphere_timing() {
    let (positions, triangles) = sphere(224, 224, 50.0);
    let started = std::time::Instant::now();
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    println!(
        "{} triangles -> {} faces, {} edges in {:?}",
        triangles.len(),
        summary.faces,
        summary.edges,
        started.elapsed()
    );
    let started = std::time::Instant::now();
    assert_valid(&ir);
    println!("validation {:?}", started.elapsed());
}

/// Axis-aligned box from the origin with the given side lengths.
fn box_mesh(sides: [f64; 3]) -> (Positions, Triangles) {
    let positions = cube_corners([0.0; 3], 1.0)
        .into_iter()
        .map(|p| [p[0] * sides[0], p[1] * sides[1], p[2] * sides[2]])
        .collect();
    (positions, quads_to_triangles(&CUBE_QUADS))
}

#[test]
fn t_junction_between_two_quads_is_split() {
    // Quad A spans x 0..1; its right edge is one segment. The two quads on
    // x 1..2 meet it at (1, 0.5).
    let positions = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        [2.0, 0.0, 0.0],
        [2.0, 0.5, 0.0],
        [1.0, 0.5, 0.0],
        [1.0, 0.5, 0.0],
        [2.0, 0.5, 0.0],
        [2.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];
    let triangles = quads_to_triangles(&[[0, 1, 2, 3], [4, 5, 6, 7], [8, 9, 10, 11]]);
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert_eq!(summary.t_junction_splits, 1);
    // The perimeter of the 2 x 1 rectangle has seven segments.
    assert_eq!(summary.open_edges, 7, "{summary:?}");
    assert!(!summary.closed);
    assert_eq!(body_kind(&ir), BodyKind::Sheet);
    assert_valid(&ir);
}

#[test]
fn cube_of_independent_faces_with_t_junctions_is_one_solid() {
    for jitter in [0.0, 1.0e-4] {
        let (positions, triangles) = grid_cube_with([1, 2, 3, 4, 5, 7], jitter);
        let (ir, summary) = convert(&positions, &triangles).unwrap();
        assert!(summary.t_junction_splits > 0, "{summary:?}");
        assert!(summary.closed, "{summary:?}");
        assert!(!summary.reversed);
        assert_eq!(
            (summary.faces, summary.edges, summary.vertices),
            (6, 12, 8),
            "jitter {jitter}: {summary:?}"
        );
        assert!((summary.signed_volume_mm3 - 1000.0).abs() < 1.0);
        assert_eq!(body_kind(&ir), BodyKind::Solid);
        assert_valid(&ir);
    }
}

#[test]
fn inside_out_cube_with_t_junctions_is_reversed() {
    let (positions, mut triangles) = grid_cube_with([2, 3, 1, 4, 2, 3], 0.0);
    for triangle in &mut triangles {
        triangle.swap(1, 2);
    }
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert!(summary.closed && summary.reversed, "{summary:?}");
    assert!(summary.signed_volume_mm3 > 999.0);
    assert_eq!(body_kind(&ir), BodyKind::Solid);
    assert_valid(&ir);
}

#[test]
fn open_cube_with_t_junctions_stays_a_sheet() {
    let (positions, triangles) = grid_cube_with([1, 2, 3, 4, 5, 7], 0.0);
    // Face 1 is the top: drop its triangles.
    let top = 2 * 2 * 2;
    let kept: Triangles = triangles
        .iter()
        .enumerate()
        .filter(|(i, _)| !(2..2 + top).contains(i))
        .map(|(_, t)| *t)
        .collect();
    let (ir, summary) = convert(&positions, &kept).unwrap();
    assert!(!summary.closed && summary.open_edges > 0, "{summary:?}");
    assert_eq!(body_kind(&ir), BodyKind::Sheet);
    assert_valid(&ir);
}

#[test]
fn ten_micrometre_plate_survives_the_weld() {
    let (positions, triangles) = box_mesh([10.0, 10.0, 0.01]);
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert_eq!(summary.dropped_degenerate_triangles, 0);
    assert_eq!((summary.faces, summary.vertices), (6, 8));
    assert!(summary.closed);
    assert!((summary.signed_volume_mm3 - 1.0).abs() < 1.0e-9);
    assert_eq!(body_kind(&ir), BodyKind::Solid);
    assert_valid(&ir);
}

#[test]
fn micrometre_plate_is_rebuilt_with_the_fine_weld() {
    // A plate as thin as the weld tolerance welds flat at the first try and is
    // not watertight; the second build at the fine tolerance keeps it a solid.
    let (positions, triangles) = box_mesh([10.0, 10.0, 0.001]);
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert!(summary.fine_weld);
    assert!(summary.closed);
    assert_eq!(summary.dropped_degenerate_triangles, 0);
    assert_eq!((summary.faces, summary.vertices), (6, 8));
    assert!((summary.signed_volume_mm3 - 0.1).abs() < 1.0e-9);
    assert_eq!(body_kind(&ir), BodyKind::Solid);
    assert_valid(&ir);
}

#[test]
fn watertight_body_is_built_once_at_the_requested_weld() {
    let (positions, triangles) = cube();
    let (_, summary) = convert(&positions, &triangles).unwrap();
    assert!(summary.closed);
    assert!(!summary.fine_weld);
}

#[test]
fn zero_thickness_plate_stays_a_sheet_after_the_fine_weld() {
    // Two coincident rectangles facing opposite ways: there is no volume for
    // any weld tolerance to keep.
    let positions = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
    ];
    let triangles = [[0, 1, 2], [0, 2, 3], [0, 2, 1], [0, 3, 2]];
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert!(!summary.closed);
    assert!(!summary.fine_weld);
    assert_eq!(body_kind(&ir), BodyKind::Sheet);
    assert_valid(&ir);
}

#[test]
fn repair_leaves_distant_vertices_alone() {
    // Vertex 3 lies 0.5 mm off the open edge 0-1.
    let positions = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [0.0, 10.0, 0.0],
        [5.0, -0.5, 0.0],
        [10.0, -0.5, 0.0],
    ];
    let triangles = [[0, 1, 2], [3, 4, 1]];
    let repaired = repair::split_t_junctions(&positions, &triangles, 0.01).unwrap();
    assert_eq!(repaired.splits, 0);
    assert_eq!(repaired.tris, triangles);
}

#[test]
fn repair_stops_at_the_probe_budget() {
    // Long edges next to many short ones: sampling the long edges exceeds the
    // budget before the last one, which holds a T-junction vertex.
    let mut positions: Vec<[f64; 3]> = Vec::new();
    let mut triangles: Vec<[usize; 3]> = Vec::new();
    let long = 2000_u32;
    for k in 0..long {
        let y = f64::from(k);
        let base = positions.len();
        positions.extend([[0.0, y, 0.0], [1.0e5, y, 0.0], [0.0, y, 1.0]]);
        triangles.push([base, base + 1, base + 2]);
    }
    let on_last = positions.len();
    positions.extend([
        [5.0e4, f64::from(long - 1), 0.0],
        [5.0e4, 5.0e3, 0.0],
        [5.0e4 + 1.0, 5.0e3, 0.0],
    ]);
    triangles.push([on_last, on_last + 1, on_last + 2]);
    for k in 0..4000 {
        let x = 2.0e6 + f64::from(k) * 0.1;
        let base = positions.len();
        positions.extend([[x, 0.0, 0.0], [x + 0.05, 0.0, 0.0], [x, 0.05, 0.0]]);
        triangles.push([base, base + 1, base + 2]);
    }
    let repaired = repair::split_t_junctions(&positions, &triangles, 0.01).unwrap();
    assert_eq!(repaired.splits, 0);
    assert_eq!(repaired.tris, triangles);
}

#[test]
fn crack_between_independent_faces_closes() {
    // The top face sits 4 micrometres above the side faces: wider than the
    // weld, narrower than the crack tolerance.
    let (mut positions, triangles) = grid_cube(1, 0.0);
    for p in &mut positions[4..8] {
        p[2] += 0.004;
    }
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert!(summary.crack_merged_points > 0, "{summary:?}");
    assert!(summary.closed, "{summary:?}");
    assert_eq!((summary.faces, summary.vertices), (6, 8));
    assert_eq!(body_kind(&ir), BodyKind::Solid);
    assert_valid(&ir);
}

#[test]
fn crack_wider_than_the_tolerance_stays_open() {
    let (mut positions, triangles) = grid_cube(1, 0.0);
    for p in &mut positions[4..8] {
        p[2] += 0.05;
    }
    let (_, summary) = convert(&positions, &triangles).unwrap();
    assert_eq!(summary.crack_merged_points, 0);
    assert!(!summary.closed);
}

#[test]
fn two_cubes_touching_along_an_edge_pair_up() {
    let mut positions = cube_corners([0.0; 3], 10.0);
    let mut triangles = quads_to_triangles(&CUBE_QUADS);
    let offset = index(positions.len());
    positions.extend(cube_corners([10.0, 10.0, 0.0], 10.0));
    triangles.extend(
        quads_to_triangles(&CUBE_QUADS)
            .into_iter()
            .map(|t| t.map(|i| i + offset)),
    );
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert!(summary.closed, "{summary:?}");
    assert!((summary.signed_volume_mm3 - 2000.0).abs() < 1e-6);
    assert_eq!(body_kind(&ir), BodyKind::Solid);
    assert_valid(&ir);
}

#[test]
fn unbalanced_non_manifold_edge_stays_unpaired() {
    // Three triangles on one edge: directions 2 + 1.
    let positions = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];
    let (_, summary) = convert(&positions, &[[0, 1, 2], [1, 0, 3], [0, 1, 4]]).unwrap();
    assert_eq!(summary.non_manifold_edges, 1);
    assert!(!summary.closed);
}

/// Append a cube; `inward` winds its triangles to face the inside.
fn add_cube(
    positions: &mut Positions,
    triangles: &mut Triangles,
    origin: [f64; 3],
    size: f64,
    inward: bool,
) {
    let offset = index(positions.len());
    positions.extend(cube_corners(origin, size));
    let mut added = quads_to_triangles(&CUBE_QUADS.map(|q| q.map(|i| i + offset)));
    if inward {
        for triangle in &mut added {
            triangle.swap(1, 2);
        }
    }
    triangles.extend(added);
}

fn shells_of_regions(ir: &CadIr) -> Vec<usize> {
    ir.model.regions.iter().map(|r| r.shells.len()).collect()
}

#[test]
fn cube_with_inner_cavity_is_one_region_with_a_void_shell() {
    let (mut positions, mut triangles) = (Vec::new(), Vec::new());
    add_cube(&mut positions, &mut triangles, [0.0; 3], 10.0, false);
    add_cube(&mut positions, &mut triangles, [3.0; 3], 3.0, true);
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert!(summary.closed && !summary.reversed, "{summary:?}");
    assert_eq!(shells_of_regions(&ir), [2]);
    assert_eq!((summary.faces, summary.shells), (12, 2));
    assert!((summary.signed_volume_mm3 - 973.0).abs() < 1e-6);
    assert_eq!(body_kind(&ir), BodyKind::Solid);
    assert_valid(&ir);
}

#[test]
fn inside_out_cavity_is_reversed_into_a_void() {
    let (mut positions, mut triangles) = (Vec::new(), Vec::new());
    add_cube(&mut positions, &mut triangles, [0.0; 3], 10.0, false);
    add_cube(&mut positions, &mut triangles, [3.0; 3], 3.0, false);
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert_eq!(summary.reversed_components, 1);
    assert_eq!(shells_of_regions(&ir), [2]);
    assert!((summary.signed_volume_mm3 - 973.0).abs() < 1e-6);
    assert_valid(&ir);
}

#[test]
fn separate_cubes_are_outer_even_when_one_is_inside_out() {
    let (mut positions, mut triangles) = (Vec::new(), Vec::new());
    add_cube(&mut positions, &mut triangles, [0.0; 3], 10.0, false);
    add_cube(&mut positions, &mut triangles, [50.0, 0.0, 0.0], 5.0, true);
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert_eq!(summary.reversed_components, 1);
    assert_eq!(shells_of_regions(&ir), [1, 1]);
    assert!((summary.signed_volume_mm3 - 1125.0).abs() < 1e-6);
    assert_valid(&ir);
}

#[test]
fn cavity_touching_the_outer_shell_at_a_vertex_stays_a_void() {
    let (mut positions, mut triangles) = (Vec::new(), Vec::new());
    add_cube(&mut positions, &mut triangles, [0.0; 3], 10.0, false);
    // A tetrahedron of volume 9 whose apex is the cube corner (0, 0, 0).
    let base = index(positions.len());
    positions.extend([
        [0.0, 0.0, 0.0],
        [4.0, 1.0, 1.0],
        [1.0, 4.0, 1.0],
        [1.0, 1.0, 4.0],
    ]);
    triangles.extend([[0, 1, 2], [0, 3, 1], [0, 2, 3], [1, 3, 2]].map(|t| t.map(|i| i + base)));
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert!(summary.closed, "{summary:?}");
    assert_eq!(shells_of_regions(&ir), [2]);
    assert!((summary.signed_volume_mm3 - 991.0).abs() < 1e-6);
    assert_valid(&ir);
}

#[test]
fn solid_inside_a_cavity_is_a_second_region() {
    let (mut positions, mut triangles) = (Vec::new(), Vec::new());
    add_cube(&mut positions, &mut triangles, [0.0; 3], 30.0, false);
    add_cube(&mut positions, &mut triangles, [5.0; 3], 20.0, true);
    add_cube(&mut positions, &mut triangles, [10.0; 3], 10.0, false);
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert!(!summary.reversed);
    assert_eq!(shells_of_regions(&ir), [2, 1]);
    assert!((summary.signed_volume_mm3 - (27_000.0 - 8_000.0 + 1_000.0)).abs() < 1e-6);
    assert_valid(&ir);
}

#[test]
fn closed_component_without_volume_is_dropped() {
    let (mut positions, mut triangles) = cube();
    let base = index(positions.len());
    positions.extend([[50.0, 0.0, 0.0], [51.0, 0.0, 0.0], [50.0, 1.0, 0.0]]);
    triangles.extend([[base, base + 1, base + 2], [base, base + 2, base + 1]]);
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert_eq!(summary.dropped_component_triangles, 2);
    assert_eq!((summary.faces, ir.model.regions.len()), (6, 1));
    assert!(summary.closed);
    assert_valid(&ir);
}

#[test]
fn region_without_a_unique_outer_loop_splits_into_triangles() {
    let (positions, triangles) = box_mesh([10.0, 10.0, 10.0]);
    let welded = weld::weld(&positions, &triangles, 1.0e-3).unwrap();
    let mesh = super::mesh::Mesh::build(welded, &triangles).unwrap();
    let adjacency = super::adjacency::build(&mesh.tris).unwrap();
    let mut regions = super::regions::build(&mesh, &adjacency.twin).unwrap();
    let mut loops = super::loops::trace(&mesh, &adjacency.twin, &regions).unwrap();
    assert!(loops.ambiguous_regions().is_empty());
    let (region, entry) = loops
        .regions
        .iter_mut()
        .enumerate()
        .find(|(_, r)| !r.loops.is_empty())
        .unwrap();
    entry.outer = None;
    assert_eq!(loops.ambiguous_regions(), [region]);
    let before = regions.list.len();
    regions
        .split_into_triangles(&mesh, &loops.ambiguous_regions())
        .unwrap();
    assert!(regions.list.len() > before);
    let loops = super::loops::trace(&mesh, &adjacency.twin, &regions).unwrap();
    assert!(loops.ambiguous_regions().is_empty());
    assert!(loops
        .regions
        .iter()
        .all(|r| r.loops.is_empty() || r.outer.is_some()));
}

#[test]
fn cavity_thinner_than_the_plane_tolerance_is_dropped() {
    let (mut positions, mut triangles) = (Vec::new(), Vec::new());
    add_cube(&mut positions, &mut triangles, [0.0; 3], 10.0, false);
    // A cube of 10 micrometres: its volume is below twice the plane tolerance
    // times its area, so merging into planes can change its sign.
    add_cube(&mut positions, &mut triangles, [3.0; 3], 0.01, true);
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert_eq!(summary.dropped_component_triangles, 12);
    assert_eq!(shells_of_regions(&ir), [1]);
    assert_valid(&ir);
}

/// A cube whose top front edge carries a sliver triangle that tilts away from
/// both neighbouring planes, so it becomes a face of its own. Its two far
/// edges end at the same vertices once the collinear merge drops the apex.
fn cube_with_sliver_on_top_edge() -> (Positions, Triangles) {
    let (mut positions, _) = cube();
    positions.push([5.0, 0.0004, 10.0003]);
    let apex = index(positions.len() - 1);
    let mut triangles = quads_to_triangles(&[
        CUBE_QUADS[0],
        CUBE_QUADS[2],
        CUBE_QUADS[3],
        CUBE_QUADS[4],
        CUBE_QUADS[5],
    ]);
    triangles.extend([[7, 4, apex], [7, apex, 5], [7, 5, 6], [4, 5, apex]]);
    (positions, triangles)
}

#[test]
fn sliver_face_reduced_to_two_vertices_is_removed_and_neighbours_join() {
    let (positions, triangles) = cube_with_sliver_on_top_edge();
    let (ir, summary) = convert(&positions, &triangles).unwrap();
    assert!(summary.closed);
    assert_eq!(summary.collapsed_faces, 1);
    assert_eq!((summary.faces, summary.edges, summary.vertices), (6, 12, 8));
    assert!(ir
        .model
        .loops
        .iter()
        .all(|l| matches!(&l.boundary, cadmpeg_ir::topology::LoopBoundary::Ring(r) if r.coedges().len() >= 3)));
    // Every edge has exactly two coedges that name each other.
    for coedge in &ir.model.coedges {
        let other = ir
            .model
            .coedges
            .iter()
            .find(|c| c.id == coedge.radial_next)
            .unwrap();
        assert_ne!(other.id, coedge.id);
        assert_eq!(other.edge, coedge.edge);
        assert_eq!(other.radial_next, coedge.id);
    }
    assert_eq!(body_kind(&ir), BodyKind::Solid);
    assert_valid(&ir);
}

#[test]
fn collapsing_a_digon_pairs_the_coedges_across_it() {
    use super::runs::{Run, RunSet};
    let run = |start, end, partner| Run {
        start,
        end,
        partner,
        live: true,
    };
    // Region 0 is the digon (runs 0, 1); runs 2 and 3 lie across its edges;
    // run 4 is open and has no partner.
    let mut set = RunSet {
        runs: vec![
            run(0, 1, 2),
            run(1, 0, 3),
            run(1, 0, 0),
            run(0, 1, 1),
            run(5, 6, 4),
            run(6, 1, 5),
        ],
        rings: vec![vec![vec![0, 1]], vec![vec![2, 4, 5]], vec![vec![3]]],
    };
    assert_eq!(set.collapse_digons().unwrap(), 1);
    assert!(set.rings[0].is_empty());
    assert!(!set.runs[0].live && !set.runs[1].live);
    assert_eq!((set.runs[2].partner, set.runs[3].partner), (3, 2));
    assert_eq!(set.runs[4].partner, 4);
    // An open edge across one side leaves the other side open.
    let mut set = RunSet {
        runs: vec![run(0, 1, 0), run(1, 0, 2), run(0, 1, 1)],
        rings: vec![vec![vec![0, 1]], vec![vec![2]]],
    };
    assert_eq!(set.collapse_digons().unwrap(), 1);
    assert_eq!(set.runs[2].partner, 2);
}

#[test]
fn closed_shell_of_two_faces_has_no_volume_and_is_dropped() {
    use super::runs::{Run, RunSet};
    let run = |start, end, partner| Run {
        start,
        end,
        partner,
        live: true,
    };
    // Regions 0 and 1 are the two faces of one closed shell (component 0);
    // regions 2 to 5 belong to a tetrahedron-like shell (component 1).
    let mut runs = vec![run(0, 1, 1), run(1, 0, 0)];
    for k in 0..4 {
        runs.push(run(2 + k, 3 + k, if k % 2 == 0 { 3 + k } else { 1 + k }));
    }
    let mut set = RunSet {
        runs,
        rings: vec![
            vec![vec![0]],
            vec![vec![1]],
            vec![vec![2]],
            vec![vec![3]],
            vec![vec![4]],
            vec![vec![5]],
        ],
    };
    assert_eq!(set.drop_flat_shells(&[0, 0, 1, 1, 1, 1]).unwrap(), 1);
    assert!(set.rings[0].is_empty() && set.rings[1].is_empty());
    assert!(set.rings[2..].iter().all(|r| !r.is_empty()));
}

/// Component analysis of `triangles` over `positions`, without the repair
/// steps that would merge a tiny open piece away.
fn nesting_of(
    positions: &[[f64; 3]],
    triangles: &[[u32; 3]],
) -> (super::mesh::Mesh, super::nesting::Nesting) {
    let welded = weld::weld(positions, triangles, 0.001).unwrap();
    let mesh = super::mesh::Mesh::build(welded, triangles).unwrap();
    let adjacency = super::adjacency::build(&mesh.tris).unwrap();
    let nesting = super::nesting::components(&mesh, &adjacency.twin).unwrap();
    (mesh, nesting)
}

#[test]
fn open_sliver_beside_a_solid_is_dropped_and_counted() {
    let (mut positions, mut triangles) = cube();
    let base = index(positions.len());
    positions.extend([[30.0, 0.0, 0.0], [30.01, 0.0, 0.0], [30.0, 0.01, 0.0]]);
    triangles.push([base, base + 1, base + 2]);
    let (mesh, nesting) = nesting_of(&positions, &triangles);
    assert_eq!(nesting.components.iter().filter(|c| c.sliver()).count(), 1);
    let rewrite = nesting.rewrite(&mesh).unwrap().unwrap();
    assert_eq!(rewrite.dropped_sliver_components, 1);
    assert_eq!(rewrite.dropped_triangles, 1);
    assert_eq!(rewrite.tris.len(), 12);
}

#[test]
fn a_mesh_of_only_slivers_keeps_them() {
    let positions = vec![[0.0, 0.0, 0.0], [0.01, 0.0, 0.0], [0.0, 0.01, 0.0]];
    let (mesh, nesting) = nesting_of(&positions, &[[0, 1, 2]]);
    assert!(nesting
        .components
        .iter()
        .all(super::nesting::Component::sliver));
    assert!(nesting.rewrite(&mesh).unwrap().is_none());
}

#[test]
fn open_part_larger_than_a_sliver_is_kept() {
    let (mut positions, mut triangles) = cube();
    let base = index(positions.len());
    positions.extend([[30.0, 0.0, 0.0], [31.0, 0.0, 0.0], [30.0, 1.0, 0.0]]);
    triangles.push([base, base + 1, base + 2]);
    let (mesh, nesting) = nesting_of(&positions, &triangles);
    assert!(nesting.components.iter().all(|c| !c.sliver()));
    assert!(nesting.rewrite(&mesh).unwrap().is_none());
}
