// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use super::inputs;
use crate::test_support::decode;

fn appearance(text: &str) -> cadmpeg_ir::appearance::Appearance {
    decode(text).ir().model.appearances[0].clone()
}

const PREVIEW: &str = "#usda 1.0\ndef Material \"M\"\n{\n    token outputs:surface.connect = </M/S.outputs:surface>\n    def Shader \"S\"\n    {\n        uniform token info:id = \"UsdPreviewSurface\"\n        color3f inputs:diffuseColor = (0.5, 0.25, 1)\n        float inputs:metallic = 1\n        float inputs:roughness = 0.25\n        int inputs:useSpecularWorkflow = 0\n        token outputs:surface\n    }\n}\ndef Mesh \"Part\"\n{\n    int[] faceVertexCounts = [3]\n    int[] faceVertexIndices = [0, 1, 2]\n    point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0)]\n    rel material:binding = </M>\n}\n";

#[test]
fn shared_parameters_translate_between_families() {
    let source = appearance(PREVIEW);
    let mut written = BTreeSet::new();
    let open_pbr = inputs(
        &source,
        ("UsdPreviewSurface", Some(0)),
        (super::OPEN_PBR, Some(1)),
        &mut written,
    );
    let find = |name: &str| {
        open_pbr
            .iter()
            .find(|input| input.1 == name)
            .map(|input| input.2.clone())
    };
    assert_eq!(find("base_metalness").as_deref(), Some("1"));
    assert_eq!(find("specular_roughness").as_deref(), Some("0.25"));
    assert_eq!(find("base_color").as_deref(), Some("(0.5, 0.25, 1)"));
}

#[test]
fn own_inputs_keep_their_types() {
    let source = appearance(PREVIEW);
    let mut written = BTreeSet::new();
    let preview = inputs(
        &source,
        ("UsdPreviewSurface", Some(0)),
        ("UsdPreviewSurface", Some(0)),
        &mut written,
    );
    let workflow = preview
        .iter()
        .find(|input| input.1 == "useSpecularWorkflow")
        .unwrap();
    assert_eq!((workflow.0.as_str(), workflow.2.as_str()), ("int", "0"));
    assert!(written.contains("useSpecularWorkflow"));
}

#[test]
fn an_unknown_family_shares_nothing() {
    let mut source = appearance(PREVIEW);
    source.schema = Some("GenericSchema".to_owned());
    let mut written = BTreeSet::new();
    let open_pbr = inputs(
        &source,
        ("GenericSchema", None),
        (super::OPEN_PBR, Some(1)),
        &mut written,
    );
    assert!(
        open_pbr.iter().all(|input| input.1 == "base_color"),
        "{open_pbr:?}"
    );
}

const PRESET: &str = "#usda 1.0\ndef Material \"M\"\n{\n    token outputs:mdl:surface.connect = </M/S.outputs:out>\n    def Shader \"S\"\n    {\n        uniform token info:implementationSource = \"sourceAsset\"\n        uniform asset info:mdl:sourceAsset = @Library/Aluminium_Brushed_A.mdl@\n        uniform token info:mdl:sourceAsset:subIdentifier = \"Aluminium_Brushed_A\"\n        color3f inputs:diffuse_tint = (0.5, 0.5, 0.5)\n        float inputs:reflection_roughness_constant = 0.25\n        float inputs:metallic_constant = 1\n        token outputs:out\n    }\n}\ndef Mesh \"Part\"\n{\n    int[] faceVertexCounts = [3]\n    int[] faceVertexIndices = [0, 1, 2]\n    point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0)]\n    rel material:binding = </M>\n}\n";

#[test]
fn an_mdl_preset_keeps_its_module_and_shares_the_omni_pbr_parameters() {
    let source = appearance(PRESET);
    assert_eq!(source.schema.as_deref(), Some("Aluminium_Brushed_A"));
    assert_eq!(
        source.library_id.as_deref(),
        Some("Library/Aluminium_Brushed_A.mdl")
    );
    let base = source.base_color.expect("a base color");
    assert!((f64::from(base.r()) - 0.1).abs() < 1e-6, "{base:?}");
    let (text, _) = crate::test_support::encode(decode(PRESET).ir());
    assert!(
        text.contains("uniform asset info:mdl:sourceAsset = @Library/Aluminium_Brushed_A.mdl@"),
        "{text}"
    );
    assert!(
        text.contains("uniform token info:mdl:sourceAsset:subIdentifier = \"Aluminium_Brushed_A\""),
        "{text}"
    );
    assert!(text.contains("float inputs:roughness = 0.25"), "{text}");
    assert!(
        text.contains("float inputs:metallic_constant = 1"),
        "{text}"
    );
}
