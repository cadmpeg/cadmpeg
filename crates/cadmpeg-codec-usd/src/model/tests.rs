// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::products::{ProductDefinitionKind, Storage};
use cadmpeg_ir::topology::BodyKind;

use crate::loss::UsdLossCode;
use crate::test_support::decode;

const EPS_LENGTH: f64 = 1e-9;

fn has_loss(result: &cadmpeg_ir::codec::DecodeResult, code: UsdLossCode) -> bool {
    result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == code.kind())
}

#[test]
fn lengths_scale_from_scene_units_to_millimetres() {
    let result = decode(
        "#usda 1.0\n(\n    metersPerUnit = 0.01\n)\ndef Xform \"A\"\n{\n    double3 xformOp:translate = (1, 2, 3)\n    uniform token[] xformOpOrder = [\"xformOp:translate\"]\n    def Mesh \"M\"\n    {\n        int[] faceVertexCounts = [3]\n        int[] faceVertexIndices = [0, 1, 2]\n        point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0)]\n        uniform token subdivisionScheme = \"none\"\n    }\n}\n",
    );
    let ir = result.ir();
    let root = ir
        .model
        .occurrences
        .iter()
        .find(|occurrence| occurrence.name.as_deref() == Some("A"))
        .unwrap();
    assert_eq!(root.transform.unwrap().affine_rows()[0][3], 10.0);
    assert_eq!(root.transform.unwrap().affine_rows()[2][3], 30.0);
    let vertices = ir.model.tessellations[0].vertices();
    assert!((vertices[1].get().x - 10.0).abs() <= EPS_LENGTH);
    assert_eq!(ir.up_axis, Some(cadmpeg_ir::up_axis::UpAxis::Y));
}

#[test]
fn prims_become_definitions_and_occurrences() {
    let result = decode(
        "#usda 1.0\ndef Xform \"Asm\" (\n    kind = \"assembly\"\n)\n{\n    def Mesh \"Part\"\n    {\n        int[] faceVertexCounts = [3]\n        int[] faceVertexIndices = [0, 1, 2]\n        point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0)]\n    }\n    def Camera \"Cam\"\n    {\n    }\n}\n",
    );
    let ir = result.ir();
    assert_eq!(ir.model.occurrences.len(), 2);
    let kinds: Vec<_> = ir
        .model
        .product_definitions
        .iter()
        .filter_map(|definition| definition.kind)
        .collect();
    assert!(kinds.contains(&ProductDefinitionKind::Part));
    assert!(kinds.contains(&ProductDefinitionKind::Object));
    assert!(ir.geometry.bodies.is_empty());
    let part = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.kind == Some(ProductDefinitionKind::Object))
        .unwrap();
    assert_eq!(
        ir.model.tessellations[0].of,
        cadmpeg_ir::tessellation::Meshed::definition(part.id.clone())
    );
    assert!(has_loss(&result, UsdLossCode::PrimTypeNotTransferred));
    assert!(has_loss(&result, UsdLossCode::MeshSubdivisionNotApplied));
}

#[test]
fn instances_of_one_prototype_share_definitions() {
    let result = decode(
        "#usda 1.0\nclass \"Proto\"\n{\n    def Mesh \"Part\"\n    {\n        int[] faceVertexCounts = [3]\n        int[] faceVertexIndices = [0, 1, 2]\n        point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0)]\n        uniform token subdivisionScheme = \"none\"\n    }\n}\ndef \"I1\" (\n    instanceable = true\n    references = </Proto>\n)\n{\n}\ndef \"I2\" (\n    instanceable = true\n    references = </Proto>\n)\n{\n}\n",
    );
    // §8.1: both instances place one definition, which states its `Part`
    // occurrence once.
    let ir = result.ir();
    assert_eq!(ir.model.occurrences.len(), 3);
    assert_eq!(ir.model.tessellations.len(), 1);
    let prototype = |name: &str| {
        ir.model
            .occurrences
            .iter()
            .find(|occurrence| occurrence.name.as_deref() == Some(name))
            .map(|occurrence| occurrence.prototype.clone())
            .unwrap()
    };
    assert_eq!(prototype("I1"), prototype("I2"));
    let shared = ir
        .model
        .product_definitions
        .iter()
        .find(|definition| definition.id == prototype("I1"))
        .unwrap();
    let [part] = shared.occurrences.as_slice() else {
        panic!("one Part occurrence: {:?}", shared.occurrences);
    };
    assert!(ir
        .model
        .occurrences
        .iter()
        .any(|occurrence| &occurrence.id == part && occurrence.name.as_deref() == Some("Part")));
}

#[test]
fn an_external_arc_is_an_external_prototype() {
    let result =
        decode("#usda 1.0\ndef Xform \"A\" (\n    references = @parts/bolt.usd@</Bolt>\n)\n{\n}\n");
    let external: Vec<_> = result
        .ir()
        .model
        .product_definitions
        .iter()
        .filter_map(|definition| match &definition.storage {
            Storage::External(document) => Some(document.object.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(external, [cadmpeg_core::text::NonBlankString::new("/Bolt")]);
    assert!(has_loss(&result, UsdLossCode::CompositionExternalArc));
}

#[test]
fn curves_become_wire_bodies() {
    let result = decode(
        "#usda 1.0\n(\n    metersPerUnit = 0.001\n)\ndef BasisCurves \"C\"\n{\n    uniform token type = \"linear\"\n    int[] curveVertexCounts = [2, 3]\n    point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 0, 1), (1, 0, 1), (0, 0, 1)]\n}\n",
    );
    let ir = result.ir();
    assert_eq!(ir.geometry.bodies[0].kind, BodyKind::Wire);
    assert_eq!(ir.geometry.edges.len(), 2);
    assert_eq!(ir.geometry.curves.len(), 2);
    let closed = &ir.geometry.edges[1];
    assert_eq!(
        closed.ends.as_ref().unwrap().start,
        closed.ends.as_ref().unwrap().end
    );
    assert_eq!(ir.geometry.vertices.len(), 3);
}

#[test]
fn a_patch_becomes_a_sheet_body_face() {
    let result = decode(
        "#usda 1.0\ndef NurbsPatch \"P\"\n{\n    int uVertexCount = 2\n    int vVertexCount = 2\n    int uOrder = 2\n    int vOrder = 2\n    double[] uKnots = [0, 0, 1, 1]\n    double[] vKnots = [0, 0, 1, 1]\n    uniform token orientation = \"leftHanded\"\n    point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0), (1, 1, 0)]\n    int[] trimCurve:counts = [1]\n    int[] trimCurve:orders = [2]\n    int[] trimCurve:vertexCounts = [2]\n    double[] trimCurve:knots = [0, 0, 1, 1]\n    double3[] trimCurve:points = [(0, 0, 1), (1, 1, 1)]\n}\n",
    );
    let ir = result.ir();
    assert_eq!(ir.geometry.bodies[0].kind, BodyKind::Sheet);
    assert_eq!(
        ir.geometry.faces[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert_eq!(ir.geometry.faces[0].loops.stated().count(), 1);
    assert_eq!(ir.geometry.coedges.len(), 4);
    assert!(has_loss(&result, UsdLossCode::NurbsTrimNotTransferred));
}

#[test]
fn a_preview_material_becomes_an_appearance() {
    let result = decode(
        "#usda 1.0\ndef Material \"M\"\n{\n    token outputs:surface.connect = </M/S.outputs:surface>\n    def Shader \"S\"\n    {\n        uniform token info:id = \"UsdPreviewSurface\"\n        color3f inputs:diffuseColor = (0.5, 0.25, 1)\n        float inputs:opacity = 0.5\n        token outputs:surface\n    }\n}\ndef Mesh \"Part\"\n{\n    int[] faceVertexCounts = [3]\n    int[] faceVertexIndices = [0, 1, 2]\n    point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0)]\n    rel material:binding = </M>\n}\n",
    );
    let ir = result.ir();
    let color = ir.model.appearances[0].base_color.unwrap();
    assert_eq!(
        [color.r(), color.g(), color.b(), color.a()],
        [0.5, 0.25, 1.0, 0.5]
    );
    assert_eq!(ir.model.appearance_bindings.len(), 1);
}

#[test]
fn an_unparsable_unit_is_reported() {
    let result = decode("#usda 1.0\n(\n    metersPerUnit = -1\n)\ndef \"A\" {\n}\n");
    assert!(has_loss(&result, UsdLossCode::AttributeValueMalformed));
}

#[test]
fn an_unparsable_mass_unit_or_up_axis_is_reported() {
    for metadata in [
        "kilogramsPerUnit = \"heavy\"",
        "kilogramsPerUnit = 0",
        "upAxis = \"W\"",
        "upAxis = 1",
    ] {
        let result = decode(&format!(
            "#usda 1.0\n(\n    {metadata}\n)\ndef \"A\" {{\n}}\n"
        ));
        assert!(
            has_loss(&result, UsdLossCode::AttributeValueMalformed),
            "{metadata}"
        );
    }
    let clean = decode(
        "#usda 1.0\n(\n    kilogramsPerUnit = 0.001\n    upAxis = \"Z\"\n)\ndef \"A\" {\n}\n",
    );
    assert!(!has_loss(&clean, UsdLossCode::AttributeValueMalformed));
}

#[test]
fn a_partial_brep_face_is_a_loss_that_names_it() {
    let reason = "/B: face#0: no loop on a plane, which no face covers whole";
    let tallies = super::Tallies {
        unstated_boundaries: vec![reason.to_owned()],
        ..super::Tallies::default()
    };
    let metrics = super::Metrics {
        scale: 1.0,
        meters_per_unit: 0.001,
        kilograms_per_unit: 1.0,
        up_axis: cadmpeg_ir::up_axis::UpAxis::Z,
        default_prim: None,
        malformed_units: false,
        malformed_mass_units: false,
        malformed_up_axis: false,
    };
    let losses = super::tally_losses(&tallies, &crate::schema::Issues::default(), &metrics);
    let unstated: Vec<_> = losses
        .iter()
        .filter(|loss| loss.code == UsdLossCode::TopologyFaceBoundaryUnstated.kind())
        .collect();
    let [loss] = unstated.as_slice() else {
        panic!("one loss: {losses:#?}");
    };
    assert!(loss.message.ends_with(reason), "{}", loss.message);
}
