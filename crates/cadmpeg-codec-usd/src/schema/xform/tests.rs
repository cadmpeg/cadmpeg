// SPDX-License-Identifier: Apache-2.0

use super::{compose, invert, local_transform, Affine, IDENTITY};
use crate::schema::tests::with_prim;

const EPS_MATRIX: f64 = 1e-12;

fn assert_close(actual: &Affine, expected: &Affine) {
    for (row, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        for (column, (actual, expected)) in actual.iter().zip(expected).enumerate() {
            assert!(
                (actual - expected).abs() <= EPS_MATRIX,
                "[{row}][{column}]: {actual} != {expected}"
            );
        }
    }
}

fn apply(matrix: &Affine, [x, y, z]: [f64; 3]) -> [f64; 3] {
    matrix.map(|row| row[0] * x + row[1] * y + row[2] * z + row[3])
}

#[test]
fn the_first_op_is_outermost() {
    with_prim(
        "double3 xformOp:translate = (10, 0, 0)\nfloat3 xformOp:rotateXYZ = (90, 90, 0)\nuniform token[] xformOpOrder = [\"xformOp:translate\", \"xformOp:rotateXYZ\"]",
        |prim, issues| {
            let local = local_transform(prim, issues).unwrap();
            let x = apply(&local.matrix, [1.0, 0.0, 0.0]);
            let y = apply(&local.matrix, [0.0, 1.0, 0.0]);
            for (actual, expected) in x.iter().zip([10.0, 0.0, -1.0]).chain(y.iter().zip([11.0, 0.0, 0.0])) {
                assert!((actual - expected).abs() <= EPS_MATRIX);
            }
        },
    );
}

#[test]
fn a_three_axis_rotation_applies_its_first_letter_first() {
    with_prim(
        "float3 xformOp:rotateZYX = (90, 90, 0)\nuniform token[] xformOpOrder = [\"xformOp:rotateZYX\"]",
        |prim, issues| {
            let point = apply(&local_transform(prim, issues).unwrap().matrix, [1.0, 0.0, 0.0]);
            for (actual, expected) in point.iter().zip([0.0, 1.0, 0.0]) {
                assert!((actual - expected).abs() <= EPS_MATRIX);
            }
        },
    );
}

#[test]
fn a_quaternion_is_real_first() {
    with_prim(
        "quatd xformOp:orient = (0.7071067811865476, 0.7071067811865476, 0, 0)\nuniform token[] xformOpOrder = [\"xformOp:orient\"]",
        |prim, issues| {
            let point = apply(&local_transform(prim, issues).unwrap().matrix, [0.0, 1.0, 0.0]);
            for (actual, expected) in point.iter().zip([0.0, 0.0, 1.0]) {
                assert!((actual - expected).abs() <= EPS_MATRIX);
            }
        },
    );
}

#[test]
fn a_matrix_op_is_transposed_and_divided_by_w() {
    with_prim(
        "matrix4d xformOp:transform = ((1, 0, 0, 0), (0, 1, 0, 0), (0, 0, 1, 0), (3, 0, 0, 2))\nuniform token[] xformOpOrder = [\"xformOp:transform\"]",
        |prim, issues| {
            let point = apply(&local_transform(prim, issues).unwrap().matrix, [1.0, 0.0, 0.0]);
            assert_eq!(point, [2.0, 0.0, 0.0]);
        },
    );
    with_prim(
        "matrix4d xformOp:transform = ((1, 0, 0, 1), (0, 1, 0, 0), (0, 0, 1, 0), (0, 0, 0, 1))\nuniform token[] xformOpOrder = [\"xformOp:transform\"]",
        |prim, issues| assert!(local_transform(prim, issues).is_err()),
    );
}

#[test]
fn an_inverted_pivot_cancels_its_translation() {
    with_prim(
        "double3 xformOp:translate:pivot = (5, 0, 0)\nfloat xformOp:rotateZ = 90\nuniform token[] xformOpOrder = [\"xformOp:translate:pivot\", \"xformOp:rotateZ\", \"!invert!xformOp:translate:pivot\"]",
        |prim, issues| {
            let point = apply(&local_transform(prim, issues).unwrap().matrix, [5.0, 0.0, 0.0]);
            for (actual, expected) in point.iter().zip([5.0, 0.0, 0.0]) {
                assert!((actual - expected).abs() <= EPS_MATRIX);
            }
        },
    );
}

#[test]
fn a_valueless_op_is_the_identity_and_a_reset_is_first() {
    with_prim(
        "double3 xformOp:translate\nfloat3 xformOp:scale = (2, 3, 4)\nuniform token[] xformOpOrder = [\"!resetXformStack!\", \"xformOp:translate\", \"xformOp:scale\"]",
        |prim, issues| {
            let local = local_transform(prim, issues).unwrap();
            assert!(local.resets_stack);
            assert_eq!(apply(&local.matrix, [1.0, 1.0, 1.0]), [2.0, 3.0, 4.0]);
        },
    );
    with_prim(
        "uniform token[] xformOpOrder = [\"xformOp:scaleX\", \"!resetXformStack!\"]",
        |prim, issues| assert!(local_transform(prim, issues).is_err()),
    );
}

#[test]
fn inversion_undoes_composition() {
    let matrix: Affine = [
        [0.0, -2.0, 0.0, 1.0],
        [2.0, 0.0, 0.0, 2.0],
        [0.0, 0.0, 2.0, 3.0],
    ];
    let inverse = invert(&matrix).unwrap();
    assert_close(&compose(&matrix, &inverse), &IDENTITY);
    assert!(invert(&[[0.0; 4]; 3]).is_none());
}
