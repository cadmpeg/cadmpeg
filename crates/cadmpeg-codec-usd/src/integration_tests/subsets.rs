// SPDX-License-Identifier: Apache-2.0
//! Geometry subsets: partial partitions and the materials they bind.

use cadmpeg_ir::appearance::AppearanceTarget;

use crate::test_support::{decode, encode};

/// A quad of two triangles whose first face has its own material, and
/// whose mesh binds another.
const QUAD: &str = r#"#usda 1.0
def Mesh "M" (
    prepend apiSchemas = ["MaterialBindingAPI"]
)
{
    int[] faceVertexCounts = [3, 3]
    int[] faceVertexIndices = [0, 1, 2, 0, 2, 3]
    point3f[] points = [(0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0)]
    uniform token subdivisionScheme = "none"
    rel material:binding = </Looks/Grey>
    def GeomSubset "Front" (
        prepend apiSchemas = ["MaterialBindingAPI"]
    )
    {
        uniform token elementType = "face"
        uniform token familyName = "materialBind"
        int[] indices = [0]
        rel material:binding = </Looks/Red>
    }
}
def Scope "Looks"
{
    def Material "Grey"
    {
        token outputs:surface.connect = </Looks/Grey/S.outputs:surface>
        def Shader "S"
        {
            uniform token info:id = "UsdPreviewSurface"
            color3f inputs:diffuseColor = (0.5, 0.5, 0.5)
            token outputs:surface
        }
    }
    def Material "Red"
    {
        token outputs:surface.connect = </Looks/Red/S.outputs:surface>
        def Shader "S"
        {
            uniform token info:id = "UsdPreviewSurface"
            color3f inputs:diffuseColor = (1, 0, 0)
            token outputs:surface
        }
    }
}
"#;

/// The appearance name of each triangle group binding, by group.
fn group_bindings(result: &cadmpeg_ir::codec::DecodeResult) -> Vec<(String, String)> {
    let model = &result.ir().model;
    model
        .appearance_bindings
        .iter()
        .filter_map(|binding| match &binding.target {
            AppearanceTarget::TriangleGroup { group, .. } => {
                let appearance = model
                    .appearances
                    .iter()
                    .find(|appearance| appearance.id == binding.appearance)?;
                Some((
                    group.rsplit('/').next()?.to_owned(),
                    appearance.name.clone()?,
                ))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_partial_subset_keeps_its_mesh_and_binds_its_material() {
    let first = decode(QUAD);
    let model = &first.ir().model;
    let [mesh] = model.tessellations.as_slice() else {
        panic!("one mesh");
    };
    let groups: Vec<(Option<&str>, &[u32])> = mesh
        .triangle_groups()
        .iter()
        .map(|group| (group.source_id.as_deref(), group.triangles.as_slice()))
        .collect();
    assert_eq!(groups, [(Some("/M/Front"), &[0][..]), (None, &[1][..])]);
    assert_eq!(
        group_bindings(&first),
        [("Front".to_owned(), "Red".to_owned())]
    );
    assert!(!first
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("material")));

    let (written, report) = encode(first.ir());
    assert!(
        written.contains("uniform token familyName = \"materialBind\""),
        "{written}"
    );
    assert!(
        written.contains("subsetFamily:materialBind:familyType"),
        "{written}"
    );
    assert!(!report
        .losses
        .iter()
        .any(|loss| loss.message.contains("appearance bindings")));
    let second = decode(&written);
    assert_eq!(group_bindings(&second), group_bindings(&first), "{written}");
}
