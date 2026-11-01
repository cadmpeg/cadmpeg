// SPDX-License-Identifier: Apache-2.0

use super::merged_api_schemas;
use crate::loss::UsdLossCode;
use crate::test_support::{decode, encode};

const MESH: &str = "int[] faceVertexCounts = [3]\n        int[] faceVertexIndices = [0, 1, 2]\n        point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0)]\n        uniform token subdivisionScheme = \"none\"";

/// Relative error of one binary32 rounding.
const EPS_BINARY32: f64 = 1e-7;

fn has_loss(report: &cadmpeg_ir::report::export::ExportReport, code: UsdLossCode) -> bool {
    report.losses.iter().any(|loss| loss.code == code.kind())
}

#[test]
fn the_layer_states_metres_and_one_root() {
    let source = decode(&format!(
        "#usda 1.0\n(\n    upAxis = \"Y\"\n)\ndef Xform \"Robot\"\n{{\n    def Mesh \"Arm\"\n    {{\n        {MESH}\n    }}\n}}\n"
    ));
    let (text, _) = encode(source.ir());
    assert!(text.starts_with(
        "#usda 1.0\n(\n    defaultPrim = \"Robot\"\n    metersPerUnit = 1\n    upAxis = \"Y\"\n)\n"
    ));
    assert!(text.contains("def Xform \"Robot\""));
    assert!(text.contains("    def Mesh \"Arm\""));
}

#[test]
fn each_root_occurrence_is_a_root_prim() {
    let source = decode(&format!(
        "#usda 1.0\ndef Mesh \"A\"\n{{\n        {MESH}\n}}\ndef Mesh \"B\"\n{{\n        {MESH}\n}}\n"
    ));
    let (text, _) = encode(source.ir());
    assert!(text.contains("defaultPrim = \"A\""), "{text}");
    assert!(text.contains("\ndef Mesh \"A\"\n"), "{text}");
    assert!(text.contains("\ndef Mesh \"B\"\n"), "{text}");
    let round = decode(&text);
    assert_eq!(round.ir().model.tessellations.len(), 2);
    assert_eq!(round.ir().model.occurrences.len(), 2);
}

#[test]
fn every_arc_of_a_prim_is_one_reference_list() {
    let source = decode("#usda 1.0\ndef Xform \"A\" (\n    references = @a.usd@\n    payload = @b.usd@</B>\n)\n{\n}\n");
    let (text, _) = encode(source.ir());
    assert!(
        text.contains("prepend references = [@a.usd@, @b.usd@</B>]"),
        "{text}"
    );
    let round = decode(&text);
    assert_eq!(
        round.ir().model.occurrences.len(),
        source.ir().model.occurrences.len()
    );
}

#[test]
fn a_shared_definition_is_one_prototype_with_references() {
    let source = decode(&format!(
        "#usda 1.0\nclass \"Proto\"\n{{\n    def Mesh \"Part\"\n    {{\n        {MESH}\n    }}\n}}\ndef Xform \"Asm\"\n{{\n    def \"I1\" (\n        instanceable = true\n        references = </Proto>\n    )\n    {{\n    }}\n    def \"I2\" (\n        instanceable = true\n        references = </Proto>\n    )\n    {{\n    }}\n}}\n"
    ));
    let (text, _) = encode(source.ir());
    assert_eq!(text.matches("def Mesh").count(), 1, "{text}");
    assert_eq!(
        text.matches("prepend references = </Prototypes/").count(),
        2,
        "{text}"
    );
    assert!(text.contains("class \"Prototypes\""));
    let round = decode(&text);
    assert_eq!(
        round.ir().model.occurrences.len(),
        source.ir().model.occurrences.len()
    );
    let vertices = |result: &cadmpeg_ir::codec::DecodeResult| -> usize {
        result
            .ir()
            .model
            .tessellations
            .iter()
            .map(cadmpeg_ir::tessellation::Tessellation::vertex_count)
            .sum()
    };
    assert_eq!(vertices(&round), vertices(&source));
}

#[test]
fn transforms_are_written_as_row_vector_matrices() {
    let source = decode(&format!(
        "#usda 1.0\n(\n    metersPerUnit = 0.001\n)\ndef Xform \"A\"\n{{\n    double3 xformOp:translate = (1, 2, 3)\n    float xformOp:rotateZ = 90\n    uniform token[] xformOpOrder = [\"xformOp:translate\", \"xformOp:rotateZ\"]\n    def Mesh \"M\"\n    {{\n        {MESH}\n    }}\n}}\n"
    ));
    let (text, _) = encode(source.ir());
    let round = decode(&text);
    let original = &source.ir().model.occurrences[0]
        .transform
        .as_ref()
        .expect("root placement")
        .affine_rows();
    let written = &round.ir().model.occurrences[0]
        .transform
        .as_ref()
        .expect("root placement")
        .affine_rows();
    assert_eq!(original, written);
    assert!(text.contains("(0.001, 0.002, 0.003, 1))"), "{text}");
}

#[test]
fn materials_bind_with_the_binding_schema() {
    let source = decode(&format!(
        "#usda 1.0\ndef Material \"M\"\n{{\n    token outputs:surface.connect = </M/S.outputs:surface>\n    def Shader \"S\"\n    {{\n        uniform token info:id = \"UsdPreviewSurface\"\n        color3f inputs:diffuseColor = (0.5, 0.25, 1)\n        float inputs:roughness = 0.5\n        token outputs:surface\n    }}\n}}\ndef Mesh \"Part\"\n{{\n        {MESH}\n    rel material:binding = </M>\n}}\n"
    ));
    let (text, report) = encode(source.ir());
    assert!(text.contains("prepend apiSchemas = [\"MaterialBindingAPI\"]"));
    assert!(text.contains("rel material:binding = </Looks/M>"), "{text}");
    assert!(text.contains("float inputs:roughness = 0.5"));
    assert!(!has_loss(&report, UsdLossCode::ExportAppearanceReduced));
    let round = decode(&text);
    assert_eq!(round.ir().model.appearances.len(), 1);
    assert_eq!(round.ir().model.appearance_bindings.len(), 1);
}

#[test]
fn patch_points_are_written_with_the_u_index_fastest() {
    let source = decode(
        "#usda 1.0\ndef NurbsPatch \"P\"\n{\n    int uVertexCount = 2\n    int vVertexCount = 3\n    int uOrder = 2\n    int vOrder = 2\n    double[] uKnots = [0, 0, 1, 1]\n    double[] vKnots = [0, 0, 1, 2, 2]\n    uniform token orientation = \"leftHanded\"\n    point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0), (1, 1, 0), (0, 2, 0), (1, 2, 0)]\n}\n",
    );
    let (text, _) = encode(source.ir());
    assert!(text.contains("point3f[] points = [(0, 0, 0), (0.01, 0, 0), (0, 0.01, 0), (0.01, 0.01, 0), (0, 0.02, 0), (0.01, 0.02, 0)]"), "{text}");
    assert!(
        text.contains("uniform token orientation = \"leftHanded\""),
        "{text}"
    );
    let round = decode(&text);
    let poles = |result: &cadmpeg_ir::codec::DecodeResult| -> Vec<f64> {
        let Some(cadmpeg_ir::geometry::SolvedSurfaceGeometry::Nurbs(nurbs)) =
            result.ir().model.surfaces[0].geometry.solved()
        else {
            panic!("a NURBS surface");
        };
        nurbs
            .control_grid()
            .iter()
            .flatten()
            .flat_map(|pole| {
                let point = pole.get();
                [point.x, point.y, point.z]
            })
            .collect()
    };
    for (left, right) in poles(&round).iter().zip(poles(&source)) {
        assert!(
            (left - right).abs() <= EPS_BINARY32 * right.abs().max(1.0),
            "{left} != {right}"
        );
    }
    let senses = |result: &cadmpeg_ir::codec::DecodeResult| -> Vec<_> {
        result
            .ir()
            .model
            .faces
            .iter()
            .map(|face| face.sense)
            .collect()
    };
    assert_eq!(senses(&round), senses(&source));
}

#[test]
fn rounded_coordinates_are_reported() {
    let source = decode(&format!(
        "#usda 1.0\n(\n    metersPerUnit = 0.0001\n)\ndef Mesh \"M\"\n{{\n        {MESH}\n}}\n"
    ));
    let (_, report) = encode(source.ir());
    assert!(has_loss(&report, UsdLossCode::ExportVertexPrecision));
}

#[test]
fn a_solid_with_no_tessellation_is_written_as_a_brep_array_and_a_mesh() {
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let (text, report) = encode(&ir);
    assert!(text.contains("def BrepArray \"Brep\""), "{text}");
    assert!(text.contains("def Mesh"), "{text}");
    assert!(
        text.contains("customData = { string brepArray = \"Brep\" }"),
        "{text}"
    );
    assert!(!has_loss(&report, UsdLossCode::ExportTopologyTessellated));
    assert!(!has_loss(&report, UsdLossCode::ExportBrepNotWritten));
    let round = decode(&text);
    let model = &round.ir().model;
    assert_eq!(model.faces.len(), 6);
    assert_eq!(model.bodies.len(), 1);
    let [mesh] = model.tessellations.as_slice() else {
        panic!("one mesh\n{text}");
    };
    assert_eq!(mesh.triangles().len(), 12);
    assert_eq!(mesh.body.as_ref(), Some(&model.bodies[0].id));
}

#[test]
fn an_external_prototype_is_a_reference() {
    let source =
        decode("#usda 1.0\ndef Xform \"A\" (\n    references = @parts/bolt.usd@</Bolt>\n)\n{\n}\n");
    let (text, _) = encode(source.ir());
    assert!(
        text.contains("prepend references = @parts/bolt.usd@</Bolt>"),
        "{text}"
    );
}

#[test]
fn applied_schemas_are_one_list_in_first_mention_order() {
    let merged = merged_api_schemas(&[
        "prepend apiSchemas = [\"PhysicsRigidBodyAPI\"]".to_owned(),
        "displayName = \"A\"".to_owned(),
        "prepend apiSchemas = [\"MaterialBindingAPI\", \"PhysicsRigidBodyAPI\"]".to_owned(),
    ]);
    assert_eq!(
        merged,
        [
            "prepend apiSchemas = [\"PhysicsRigidBodyAPI\", \"MaterialBindingAPI\"]",
            "displayName = \"A\""
        ]
    );
}

#[test]
fn parts_and_their_ancestors_carry_model_kinds() {
    let source = decode(&format!(
        "#usda 1.0\ndef Xform \"A\" (\n    kind = \"assembly\"\n)\n{{\n    def Xform \"G\"\n    {{\n        def Xform \"C\" (\n            kind = \"component\"\n        )\n        {{\n            def Mesh \"M\"\n            {{\n        {MESH}\n            }}\n        }}\n    }}\n}}\n"
    ));
    let (text, _) = encode(source.ir());
    let kind_of = |name: &str| -> Option<String> {
        let head = text
            .split(&format!("def Xform \"{name}\""))
            .nth(1)?
            .split('{')
            .next()?;
        head.split("kind = \"")
            .nth(1)?
            .split('"')
            .next()
            .map(str::to_owned)
    };
    assert_eq!(kind_of("A").as_deref(), Some("assembly"), "{text}");
    assert_eq!(kind_of("G").as_deref(), Some("group"), "{text}");
    assert_eq!(kind_of("C").as_deref(), Some("component"), "{text}");
}

#[test]
fn an_extent_keeps_the_zero_bound_it_holds() {
    let first = super::grow(None, [0.0, -0.0, 1.0]);
    let (low, high) = super::grow(Some(first), [-0.0, 0.0, 1.0]);
    assert_eq!(
        low.map(f32::to_bits),
        [0.0_f32, -0.0, 1.0].map(f32::to_bits)
    );
    assert_eq!(
        high.map(f32::to_bits),
        [0.0_f32, -0.0, 1.0].map(f32::to_bits)
    );
}
