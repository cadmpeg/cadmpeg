// SPDX-License-Identifier: Apache-2.0

use super::read;
use crate::schema::tests::with_layer;

fn patch(body: &str) -> Option<super::PatchData> {
    let text = format!("#usda 1.0\ndef NurbsPatch \"P\"\n{{\n{body}\n}}\n");
    with_layer(&text, |stage, issues| read(&stage.prims[0], issues))
}

const GRID: &str = "int uVertexCount = 2\nint vVertexCount = 3\nint uOrder = 2\nint vOrder = 2\ndouble[] uKnots = [0, 0, 1, 1]\ndouble[] vKnots = [0, 0, 1, 2, 2]\npoint3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0), (1, 1, 0), (0, 2, 0), (1, 2, 0)]";

#[test]
fn the_u_index_varies_fastest_in_points() {
    let patch = patch(GRID).unwrap();
    assert_eq!(patch.rows.len(), 2);
    assert_eq!(
        patch.rows[1],
        [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 2.0, 0.0]]
    );
    assert_eq!(patch.u_degree, 1);
    assert!(!patch.range_narrowed);
    assert!(patch.trims.is_empty());
}

#[test]
fn counts_that_disagree_reject_the_patch() {
    assert!(patch(&GRID.replace("int vVertexCount = 3", "int vVertexCount = 4")).is_none());
    assert!(patch(&GRID.replace(
        "double[] uKnots = [0, 0, 1, 1]",
        "double[] uKnots = [0, 1, 1]"
    ))
    .is_none());
}

#[test]
fn a_narrow_range_is_flagged() {
    let patch = patch(&format!("{GRID}\ndouble2 vRange = (0, 1)")).unwrap();
    assert!(patch.range_narrowed);
}

#[test]
fn trim_points_are_homogeneous() {
    let patch = patch(&format!("{GRID}\nint[] trimCurve:counts = [1]\nint[] trimCurve:orders = [2]\nint[] trimCurve:vertexCounts = [2]\ndouble[] trimCurve:knots = [0, 0, 1, 1]\ndouble3[] trimCurve:points = [(0.2, 0.4, 2), (1, 1, 1)]")).unwrap();
    assert!(!patch.trims_malformed);
    let curve = &patch.trims[0][0];
    assert_eq!(curve.points, [[0.1, 0.2], [1.0, 1.0]]);
    assert_eq!(curve.weights, [2.0, 1.0]);
}
