// SPDX-License-Identifier: Apache-2.0

use super::{bound_material, Bound};
use crate::schema::tests::with_layer;

const LOOKS: &str = "def Scope \"Looks\"\n{\n    def Material \"M\"\n    {\n        token outputs:surface.connect = </Looks/M/S.outputs:surface>\n        float inputs:rough = 0.75\n        def Shader \"S\"\n        {\n            uniform token info:id = \"UsdPreviewSurface\"\n            color3f inputs:diffuseColor = (1, 0, 0)\n            float inputs:roughness.connect = </Looks/M.inputs:rough>\n            float inputs:metallic.connect = </Looks/M/T.outputs:r>\n            token outputs:surface\n        }\n        def Shader \"T\"\n        {\n            uniform token info:id = \"UsdUVTexture\"\n            float outputs:r\n        }\n    }\n    def Material \"Mdl\"\n    {\n        token outputs:mdl:surface.connect = </Looks/Mdl/S.outputs:out>\n    }\n}\n";

#[test]
fn a_preview_surface_reads_values_through_interface_inputs() {
    let text = format!("#usda 1.0\n{LOOKS}def Xform \"G\"\n{{\n    rel material:binding = </Looks/M>\n    def Mesh \"Part\"\n    {{\n    }}\n}}\n");
    with_layer(&text, |stage, issues| {
        let part = stage.find("/G/Part").unwrap();
        let Some(Bound::Surface(surface)) = bound_material(stage, part, issues) else {
            panic!("the inherited binding resolves");
        };
        assert_eq!(surface.material, "/Looks/M");
        assert_eq!(surface.input("diffuseColor"), Some(&[1.0, 0.0, 0.0][..]));
        assert_eq!(surface.input("roughness"), Some(&[0.75][..]));
        assert_eq!(surface.connected, ["metallic"]);
    });
}

#[test]
fn another_surface_is_reported_by_path() {
    let text = format!(
        "#usda 1.0\n{LOOKS}def Mesh \"Part\"\n{{\n    rel material:binding = </Looks/Mdl>\n}}\n"
    );
    with_layer(&text, |stage, issues| {
        let part = stage.find("/Part").unwrap();
        assert_eq!(
            bound_material(stage, part, issues),
            Some(Bound::Other("/Looks/Mdl".to_owned()))
        );
    });
}
