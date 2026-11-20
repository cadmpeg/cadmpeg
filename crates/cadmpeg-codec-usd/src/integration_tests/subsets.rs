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
        .filter_map(|binding| match &binding.target.entity {
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

/// A `face` subset of a `BrepArray` binds its material to the faces it
/// selects, and the faces keep the material through a round trip.
#[test]
fn a_face_subset_of_a_brep_array_binds_its_faces() {
    use cadmpeg_ir::appearance::AppearanceTarget;
    let text = std::fs::read_to_string(format!(
        "{}/tests/golden/fixtures/brep_array_box.usda",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture");
    let first = crate::test_support::decode(&text);
    let faces = |result: &cadmpeg_ir::codec::DecodeResult| {
        result
            .ir()
            .model
            .appearance_bindings
            .iter()
            .filter(|binding| matches!(binding.target.entity, AppearanceTarget::Face(_)))
            .count()
    };
    assert_eq!(faces(&first), 6);
    let (written, _) = crate::test_support::encode(first.ir());
    assert!(
        written.contains("uniform token elementType = \"face\""),
        "{written}"
    );
    assert_eq!(faces(&crate::test_support::decode(&written)), 6);
}

/// A mesh from the document binds the material of each face that a triangle
/// group names to that group's triangles, in a `materialBind` subset; the
/// group's face identity stays a loss.
#[test]
fn a_face_named_by_a_triangle_group_binds_its_material_to_the_group() {
    use cadmpeg_ir::tessellation::TessellationTriangleGroup;
    let text = std::fs::read_to_string(format!(
        "{}/tests/golden/fixtures/brep_array_box.usda",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture");
    let mut ir = decode(&text).ir().clone();
    let faces: Vec<_> = ir
        .geometry
        .faces
        .iter()
        .map(|face| face.id.clone())
        .collect();
    // Face 0 gets a material of its own.
    let mut other = ir.model.appearances[0].clone();
    other.id = cadmpeg_ir::ids::AppearanceId::try_from("test:usd:appearance#other".to_owned())
        .expect("an id");
    other.name = Some("Other".into());
    let other_id = other.id.clone();
    ir.model.appearances.push(other);
    for binding in &mut ir.model.appearance_bindings {
        if binding.target.entity == AppearanceTarget::Face(faces[0].clone()) {
            binding.appearance = other_id.clone();
        }
    }
    let mesh = &mut ir.model.tessellations[0];
    let count = u32::try_from(mesh.triangles().len()).expect("a count");
    let half = count / 2;
    let groups = vec![
        TessellationTriangleGroup::new(None, (0..half).collect()).with_face(faces[0].clone()),
        TessellationTriangleGroup::new(None, (half..count).collect()).with_face(faces[1].clone()),
    ];
    *mesh = mesh
        .clone()
        .with_triangle_groups(groups)
        .expect("a partition");
    let (written, report) = encode(&ir);
    // The mesh's subsets: the BrepArray before it has face subsets of the same names.
    let mesh_prim = &written[written.find("def Mesh").expect("the mesh")..];
    let indices = |name: &str| -> Vec<u32> {
        let at = mesh_prim
            .find(&format!("def GeomSubset \"{name}\""))
            .unwrap_or_else(|| panic!("no {name} subset:\n{written}"));
        let rest = &mesh_prim[at..];
        let body = &rest[rest.find("int[] indices = [").unwrap() + 17..];
        body[..body.find(']').unwrap()]
            .split(',')
            .filter_map(|value| value.trim().parse().ok())
            .collect()
    };
    assert_eq!(indices("Other"), (0..half).collect::<Vec<_>>());
    assert_eq!(
        indices("Box_face_1_material"),
        (half..count).collect::<Vec<_>>()
    );
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::UsdLossCode::ExportGroupFaceNotWritten.kind()));
}
