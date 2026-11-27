// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::math::Point3;

use super::{key, longer_edge};

#[test]
fn an_equilateral_triangle_splits_the_edge_with_the_lower_vertex_ids() {
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.5, 3.0_f64.sqrt() / 2.0, 0.0),
    ];
    let first = (0, 1);
    let second = (1, 2);
    assert_eq!(
        longer_edge(&points, first, second),
        key(first.0, first.1).cmp(&key(second.0, second.1))
    );
}

#[test]
fn last_place_length_noise_does_not_change_the_split_edge() {
    let one = 1.0_f64;
    let above = f64::from_bits(one.to_bits() + 1);
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(one, 0.0, 0.0),
        Point3::new(above, 0.0, 0.0),
    ];
    assert_eq!(
        longer_edge(&points, (0, 1), (0, 2)),
        key(0, 1).cmp(&key(0, 2))
    );
}

#[test]
fn a_clearly_longer_edge_wins() {
    let points = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(3.0, 0.0, 0.0),
    ];
    assert_eq!(
        longer_edge(&points, (0, 1), (0, 2)),
        std::cmp::Ordering::Less
    );
}

/// Two triangles of a flat square in the parameter plane, with the surface
/// normal `+z` at every corner: the mesh folds when a triangle's corners
/// lie on the surface in the opposite turn.
fn square(points: Vec<Point3>) -> super::Mesh {
    use cadmpeg_ir::math::Vector3;
    super::Mesh {
        uv: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        normals: vec![Some(Vector3::new(0.0, 0.0, 1.0)); 4],
        scales: vec![[1.0, 1.0]; 4],
        points,
        triangles: vec![Some([0, 1, 2]), Some([0, 2, 3])],
        fixed: std::collections::BTreeSet::new(),
        anisotropy: [1.0, 1.0],
    }
}

#[test]
fn a_triangle_against_the_surface_normal_folds_the_mesh() {
    let flat = square(vec![
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
    ]);
    assert!(!flat.folded());
    // The fourth corner lands across the diagonal, turning the second
    // triangle over.
    let folded = square(vec![
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
    ]);
    assert!(folded.folded());
}

/// The turned-over second triangle holds half the square's model area.
#[test]
fn the_folded_share_is_the_area_of_the_turned_triangles() {
    let flat = square(vec![
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
    ]);
    assert_eq!(flat.folded_share(), 0.0);
    let folded = square(vec![
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(1.0, 1.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
    ]);
    assert!((folded.folded_share() - 0.5).abs() < 1e-12);
}

/// Height of a triangle's corners above its surface: twice the chord limit.
const LIFT: f64 = 0.02;
/// Chord limit of the lifted-triangle test.
const CHORD: f64 = 0.01;

/// A triangle whose sides are within the chord limit but whose corners lie
/// off the surface misses the limit by a gap no split closes, so it stays.
#[test]
fn a_triangle_within_the_chord_limit_stays_whole() {
    use cadmpeg_core::decode::WorkBudget;
    use cadmpeg_ir::index::ModelIndex;

    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let index = ModelIndex::new_model_only(&ir);
    let work = WorkBudget::new(1 << 20);
    let carrier = crate::surface::Carrier::new(
        &index,
        &work,
        cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
        &ir.geometry.faces[0].surface,
    )
    .unwrap();
    let uv = vec![[0.5, 0.5], [0.504, 0.5], [0.5, 0.504]];
    let mut points = Vec::new();
    let mut normals = Vec::new();
    for at in &uv {
        let on = carrier.point(*at).unwrap().unwrap();
        let normal = carrier.normal(*at).unwrap().unwrap();
        let lift = LIFT / normal.norm();
        points.push(Point3::new(
            on.x + lift * normal.x,
            on.y + lift * normal.y,
            on.z + lift * normal.z,
        ));
        normals.push(Some(normal));
    }
    let mut mesh = super::Mesh {
        uv,
        points,
        normals,
        scales: vec![[1.0, 1.0]; 3],
        triangles: vec![Some([0, 1, 2])],
        fixed: [(0, 1), (1, 2), (0, 2)].into_iter().collect(),
        anisotropy: [1.0, 1.0],
    };
    let limits = super::Limits {
        chord: CHORD,
        angle: 0.25,
        max_vertices: 1000,
    };
    mesh.refine(&carrier, &limits).unwrap();
    assert_eq!(mesh.uv.len(), 3);
}
