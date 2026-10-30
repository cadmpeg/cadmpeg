// SPDX-License-Identifier: Apache-2.0

use super::{display_color, read, Normals};
use crate::schema::tests::with_layer;

fn mesh_layer(body: &str) -> String {
    format!("#usda 1.0\ndef Mesh \"M\"\n{{\n    point3f[] points = [(0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0), (2, 0, 0)]\n{body}\n}}\n")
}

#[test]
fn faces_fan_from_their_first_vertex() {
    with_layer(
        &mesh_layer("int[] faceVertexCounts = [4, 3]\nint[] faceVertexIndices = [0, 1, 2, 3, 1, 4, 2]\nuniform token subdivisionScheme = \"none\""),
        |stage, issues| {
            let mesh = read(stage, &stage.prims[0], issues).unwrap();
            assert_eq!(mesh.triangles, [[0, 1, 2], [0, 2, 3], [1, 4, 2]]);
            assert_eq!(mesh.subdivision, None);
            assert_eq!(mesh.rejected_faces, 0);
        },
    );
}

#[test]
fn a_left_handed_mesh_reverses_each_triangle() {
    with_layer(
        &mesh_layer("int[] faceVertexCounts = [4]\nint[] faceVertexIndices = [0, 1, 2, 3]\nuniform token orientation = \"leftHanded\""),
        |stage, issues| {
            let mesh = read(stage, &stage.prims[0], issues).unwrap();
            assert_eq!(mesh.triangles, [[0, 2, 1], [0, 3, 2]]);
            assert_eq!(mesh.subdivision.as_deref(), Some("catmullClark"));
        },
    );
}

#[test]
fn holes_degenerate_faces_and_bad_indices_drop_their_faces() {
    with_layer(
        &mesh_layer("int[] faceVertexCounts = [3, 2, 3, 3]\nint[] faceVertexIndices = [0, 1, 2, 0, 1, 0, 1, 9, 1, 4, 2]\nint[] holeIndices = [3]"),
        |stage, issues| {
            let mesh = read(stage, &stage.prims[0], issues).unwrap();
            assert_eq!(mesh.triangles, [[0, 1, 2]]);
            assert_eq!(mesh.rejected_faces, 2);
        },
    );
}

#[test]
fn normals_map_by_their_interpolation() {
    let counts = "int[] faceVertexCounts = [4]\nint[] faceVertexIndices = [0, 1, 2, 3]";
    with_layer(
        &mesh_layer(&format!("{counts}\nnormal3f[] normals = [(0, 0, 1), (0, 0, 1), (0, 0, 1), (0, 0, 1), (0, 0, 1)]")),
        |stage, issues| {
            let mesh = read(stage, &stage.prims[0], issues).unwrap();
            assert!(matches!(mesh.normals, Normals::Vertex(ref normals) if normals.len() == 5));
        },
    );
    with_layer(
        &mesh_layer(&format!("{counts}\nnormal3f[] normals = [(1, 0, 0), (0, 1, 0), (0, 0, 1), (1, 1, 0)] (\n    interpolation = \"faceVarying\"\n)")),
        |stage, issues| {
            let mesh = read(stage, &stage.prims[0], issues).unwrap();
            assert_eq!(
                mesh.normals,
                Normals::Corner(vec![
                    [1.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    [0.0, 0.0, 1.0],
                    [1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0],
                    [1.0, 1.0, 0.0]
                ])
            );
        },
    );
    with_layer(
        &mesh_layer(&format!("{counts}\nnormal3f[] primvars:normals = [(0, 0, 1)] (\n    interpolation = \"uniform\"\n)\nnormal3f[] normals = [(9, 9, 9)]")),
        |stage, issues| {
            let mesh = read(stage, &stage.prims[0], issues).unwrap();
            assert_eq!(mesh.normals, Normals::Corner(vec![[0.0, 0.0, 1.0]; 6]));
        },
    );
    with_layer(
        &mesh_layer(&format!("{counts}\nnormal3f[] normals = [(0, 0, 1)]")),
        |stage, issues| {
            let mesh = read(stage, &stage.prims[0], issues).unwrap();
            assert_eq!(mesh.normals, Normals::None);
            assert!(mesh.dropped_normals);
        },
    );
}

#[test]
fn indexed_normals_expand_through_their_indices() {
    with_layer(
        &mesh_layer("int[] faceVertexCounts = [3]\nint[] faceVertexIndices = [0, 1, 2]\nnormal3f[] primvars:normals = [(0, 0, 1), (1, 0, 0)] (\n    interpolation = \"faceVarying\"\n)\nint[] primvars:normals:indices = [1, 0, 0]"),
        |stage, issues| {
            let mesh = read(stage, &stage.prims[0], issues).unwrap();
            assert_eq!(
                mesh.normals,
                Normals::Corner(vec![[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]])
            );
        },
    );
}

#[test]
fn face_subsets_become_triangle_sets() {
    let subsets = "def GeomSubset \"Left\"\n{\n    uniform token elementType = \"face\"\n    uniform token familyName = \"materialBind\"\n    int[] indices = [1]\n}\ndef GeomSubset \"Again\"\n{\n    int[] indices = [1]\n}\ndef GeomSubset \"Points\"\n{\n    uniform token elementType = \"point\"\n    int[] indices = [0]\n}";
    with_layer(
        &mesh_layer(&format!("int[] faceVertexCounts = [3, 4]\nint[] faceVertexIndices = [1, 4, 2, 0, 1, 2, 3]\n{subsets}")),
        |stage, issues| {
            let mesh = read(stage, &stage.prims[0], issues).unwrap();
            assert_eq!(mesh.subsets.len(), 1);
            assert_eq!(mesh.subsets[0].path, "/M/Left");
            assert_eq!(mesh.subsets[0].family.as_deref(), Some("materialBind"));
            assert_eq!(mesh.subsets[0].triangles, [1, 2]);
            assert_eq!(mesh.dropped_subsets, ["/M/Again", "/M/Points"]);
        },
    );
}

#[test]
fn a_constant_display_color_and_opacity_are_read() {
    with_layer(
        &mesh_layer("color3f[] primvars:displayColor = [(1, 0.5, 0)]\nfloat[] primvars:displayOpacity = [0.25] (\n    interpolation = \"constant\"\n)"),
        |stage, issues| {
            let (color, dropped) = display_color(&stage.prims[0], issues);
            assert_eq!(color, Some([1.0, 0.5, 0.0, 0.25]));
            assert!(!dropped);
        },
    );
    with_layer(
        &mesh_layer("color3f[] primvars:displayColor = [(1, 0, 0), (0, 1, 0)] (\n    interpolation = \"vertex\"\n)"),
        |stage, issues| {
            let (color, dropped) = display_color(&stage.prims[0], issues);
            assert_eq!(color, None);
            assert!(dropped);
        },
    );
}
