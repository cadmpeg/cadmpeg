// SPDX-License-Identifier: Apache-2.0
//! Shell decoding contracts.

use cadmpeg_core::decode::View;

use crate::dialect::Grammar;

use super::{append_shell, apply, determinant, strip_triangles, MeshData, ShellError};
use crate::hsf::scene::Scene;
use crate::test_support::hsf::{
    matrix, translation, BodySpec, Item, ModelSpec, NodeSpec, IDENTITY,
};

const LIMITS: (usize, usize) = (1 << 20, 1 << 20);

fn one_body(items: Vec<Item>) -> Vec<u8> {
    ModelSpec {
        document: "doc".into(),
        bodies: vec![BodySpec {
            colour: None,
            subs: vec![items],
        }],
        tree: vec![NodeSpec::Part {
            segment: "assempart0".into(),
            name: "p".into(),
            body: 0,
            matrix: None,
            user_name: false,
        }],
        state: None,
    }
    .stream()
}

fn quad(z: f32) -> Item {
    Item::Shell {
        points: vec![[0.0, 0.0, z], [1.0, 0.0, z], [1.0, 1.0, z], [0.0, 1.0, z]],
        strips: vec![vec![0, 1, 3, 2]],
    }
}

#[test]
fn strips_alternate_their_winding() {
    let mut out = Vec::new();
    strip_triangles(&[0, 1, 2, 3, 4], &mut out);
    assert_eq!(out, [[0, 1, 2], [2, 1, 3], [2, 3, 4]]);
    out.clear();
    strip_triangles(&[7, 8], &mut out);
    assert!(out.is_empty());
}

#[test]
fn the_row_vector_matrix_maps_basis_rows() {
    let m = matrix(
        [[0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]],
        [0.5, 0.0, 0.0],
    );
    assert_eq!(apply(&m, [1.0, 0.0, 0.0]), [0.5, 1.0, 0.0]);
    assert_eq!(apply(&m, [0.0, 1.0, 0.0]), [0.5, 0.0, 1.0]);
    assert_eq!(apply(&m, [0.0, 0.0, 1.0]), [1.5, 0.0, 0.0]);
    assert_eq!(determinant(&m), 1.0);
    assert_eq!(
        determinant(&matrix(
            [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            [0.0; 3]
        )),
        -1.0
    );
}

#[test]
fn a_shell_decodes_to_millimetres() {
    let bytes = one_body(vec![quad(0.002)]);
    let scene = Scene::collect(View::over_retained(&bytes), Grammar::Resaved).unwrap();
    let mut mesh = MeshData::default();
    append_shell(&scene, 4, None, LIMITS, &mut mesh).unwrap();
    assert_eq!(mesh.triangles, [[0, 1, 3], [3, 1, 2]]);
    assert_eq!(mesh.positions.len(), 4);
    assert!((mesh.positions.get(2).unwrap()[2] - 2.0).abs() < 1e-6);
    assert!((mesh.positions.get(2).unwrap()[0] - 1000.0).abs() < 1e-6);
}

#[test]
fn an_instance_applies_its_affine_to_the_source_points() {
    let m = matrix(
        [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        [2.0, 0.0, 0.0],
    );
    let bytes = one_body(vec![
        quad(0.0),
        Item::Instance {
            source: 0,
            matrix: m,
        },
    ]);
    let scene = Scene::collect(View::over_retained(&bytes), Grammar::Resaved).unwrap();
    let mut mesh = MeshData::default();
    // Object ids 0 to 3 are the document and part names and paths; shells follow.
    append_shell(&scene, 5, None, LIMITS, &mut mesh).unwrap();
    assert_eq!(mesh.positions.first().unwrap()[0], 2000.0);
    assert_eq!(mesh.positions.get(1).unwrap()[0], 1000.0);
    // The strips are reused without a winding flip.
    assert_eq!(mesh.triangles, [[0, 1, 3], [3, 1, 2]]);
}

#[test]
fn a_mirroring_world_matrix_flips_the_winding() {
    let bytes = one_body(vec![quad(0.0)]);
    let scene = Scene::collect(View::over_retained(&bytes), Grammar::Resaved).unwrap();
    let mirror = matrix(
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]],
        [0.0; 3],
    );
    let mut mesh = MeshData::default();
    append_shell(&scene, 4, Some(&mirror), LIMITS, &mut mesh).unwrap();
    assert_eq!(mesh.triangles, [[0, 3, 1], [3, 2, 1]]);
    let mut moved = MeshData::default();
    append_shell(
        &scene,
        4,
        Some(&translation([0.0, 0.0, 1.0])),
        LIMITS,
        &mut moved,
    )
    .unwrap();
    assert_eq!(moved.positions.first().unwrap()[2], 1000.0);
    assert_eq!(moved.triangles, [[0, 1, 3], [3, 1, 2]]);
}

#[test]
fn wide_face_lists_use_sixteen_bit_indices() {
    let points: Vec<[f32; 3]> = (0..300_u16).map(|i| [f32::from(i), 0.0, 0.0]).collect();
    let item = Item::Shell {
        points,
        strips: vec![vec![0, 1, 299]],
    };
    let bytes = one_body(vec![item]);
    let scene = Scene::collect(View::over_retained(&bytes), Grammar::Resaved).unwrap();
    let mut mesh = MeshData::default();
    append_shell(&scene, 4, None, LIMITS, &mut mesh).unwrap();
    assert_eq!(mesh.triangles, [[0, 1, 299]]);
}

#[test]
fn bad_shells_report_their_reason() {
    let outside = Item::Shell {
        points: vec![[0.0; 3]; 3],
        strips: vec![vec![0, 1, 3]],
    };
    let overrun = Item::Shell {
        points: vec![[0.0; 3]; 3],
        strips: vec![vec![0, 1, 2]],
    };
    let bytes = one_body(vec![
        outside,
        overrun,
        Item::Instance {
            source: 9,
            matrix: IDENTITY,
        },
    ]);
    let mut scene = Scene::collect(View::over_retained(&bytes), Grammar::Resaved).unwrap();
    let mut mesh = MeshData::default();
    assert_eq!(
        append_shell(&scene, 4, None, LIMITS, &mut mesh),
        Err(ShellError::Index)
    );
    assert_eq!(
        append_shell(&scene, 99, None, LIMITS, &mut mesh),
        Err(ShellError::Missing)
    );
    // The instance names object 0, which is a text object and not in the store.
    assert_eq!(
        append_shell(&scene, 6, None, LIMITS, &mut mesh),
        Err(ShellError::Missing)
    );
    assert_eq!(
        append_shell(&scene, 5, None, (2, 2), &mut mesh),
        Err(ShellError::TooLarge)
    );
    assert!(mesh.positions.is_empty());
    // A face list whose count overruns the section.
    if let Some(crate::hsf::scene::Object::Shell(shell)) = scene.objects.get_mut(&5) {
        shell.facelist = &[8, 5, 0, 1];
    }
    assert_eq!(
        append_shell(&scene, 5, None, LIMITS, &mut mesh),
        Err(ShellError::FaceList)
    );
}

#[test]
fn box_shells_decode_to_outward_triangles() {
    let bytes = one_body(crate::test_support::hsf::box_shells([0.01; 3]));
    let scene = Scene::collect(View::over_retained(&bytes), Grammar::Resaved).unwrap();
    let mut mesh = MeshData::default();
    for id in 4..10 {
        append_shell(&scene, id, None, LIMITS, &mut mesh).unwrap();
    }
    let volume: f64 = mesh
        .triangles
        .iter()
        .map(|t| {
            let p: Vec<[f64; 3]> = t
                .iter()
                .map(|&i| mesh.positions[usize::try_from(i).unwrap()])
                .collect();
            let cross = [
                p[1][1] * p[2][2] - p[1][2] * p[2][1],
                p[1][2] * p[2][0] - p[1][0] * p[2][2],
                p[1][0] * p[2][1] - p[1][1] * p[2][0],
            ];
            (p[0][0] * cross[0] + p[0][1] * cross[1] + p[0][2] * cross[2]) / 6.0
        })
        .sum();
    assert!((volume - 1000.0).abs() < 1e-3, "{volume}");
}
