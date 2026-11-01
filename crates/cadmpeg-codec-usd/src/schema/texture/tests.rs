// SPDX-License-Identifier: Apache-2.0

use super::{connected, omni_pbr};
use crate::schema::tests::with_layer;

/// A material whose preview surface takes its diffuse color from a
/// `UsdUVTexture` that reads coordinates `uv` through a transform, and its
/// roughness from the green channel of a second texture.
const PREVIEW: &str = r#"#usda 1.0
def Material "M"
{
    def Shader "Surface"
    {
        uniform token info:id = "UsdPreviewSurface"
        color3f inputs:diffuseColor.connect = </M/Diffuse.outputs:rgb>
        float inputs:roughness.connect = </M/Packed.outputs:g>
        token outputs:surface
    }
    def Shader "Diffuse"
    {
        uniform token info:id = "UsdUVTexture"
        asset inputs:file = @./wood.png@
        float2 inputs:st.connect = </M/Transform.outputs:result>
        token inputs:wrapS = "clamp"
        float3 outputs:rgb
    }
    def Shader "Packed"
    {
        uniform token info:id = "UsdUVTexture"
        asset inputs:file = @./orm.png@
        float2 inputs:st.connect = </M/Reader.outputs:result>
        float outputs:g
    }
    def Shader "Transform"
    {
        uniform token info:id = "UsdTransform2d"
        float2 inputs:in.connect = </M/Reader.outputs:result>
        float2 inputs:scale = (2, 3)
        float inputs:rotation = 90
        float2 inputs:translation = (0.5, 0.25)
        float2 outputs:result
    }
    def Shader "Reader"
    {
        uniform token info:id = "UsdPrimvarReader_float2"
        string inputs:varname = "uv"
        float2 outputs:result
    }
}
"#;

#[test]
fn a_preview_texture_reads_its_file_coordinates_transform_and_wrap() {
    with_layer(PREVIEW, |stage, issues| {
        let surface = stage.find("/M/Surface").expect("surface");
        let diffuse = connected(stage, surface, "inputs:diffuseColor", issues).expect("diffuse");
        assert_eq!(diffuse.node, "UsdUVTexture");
        assert_eq!(diffuse.file, "./wood.png");
        assert_eq!(diffuse.coordinates.as_deref(), Some("uv"));
        assert_eq!(diffuse.scale, [2.0, 3.0]);
        assert_eq!(diffuse.translation, [0.5, 0.25]);
        assert_eq!(diffuse.rotation, 90.0);
        assert_eq!(diffuse.repeat, [false, true]);
        assert_eq!(diffuse.channel, None);
        let roughness = connected(stage, surface, "inputs:roughness", issues).expect("roughness");
        assert_eq!(roughness.file, "./orm.png");
        assert_eq!(roughness.channel, Some('g'));
        assert_eq!(roughness.scale, [1.0, 1.0]);
    });
}

#[test]
fn a_materialx_image_reads_its_geometric_property_and_extracted_channel() {
    let text = r#"#usda 1.0
def Material "M"
{
    def Shader "Surface"
    {
        uniform token info:id = "ND_open_pbr_surface_surfaceshader"
        float inputs:specular_roughness.connect = </M/Extract.outputs:out>
        token outputs:out
    }
    def Shader "Extract"
    {
        uniform token info:id = "ND_extract_color4"
        color4f inputs:in.connect = </M/Image.outputs:out>
        int inputs:index = 2
        float outputs:out
    }
    def Shader "Image"
    {
        uniform token info:id = "ND_tiledimage_color4"
        asset inputs:file = @orm.png@
        float2 inputs:texcoord.connect = </M/Coordinates.outputs:out>
        float2 inputs:uvtiling = (4, 4)
        color4f outputs:out
    }
    def Shader "Coordinates"
    {
        uniform token info:id = "ND_geompropvalue_vector2"
        string inputs:geomprop = "st1"
        float2 outputs:out
    }
}
"#;
    with_layer(text, |stage, issues| {
        let surface = stage.find("/M/Surface").expect("surface");
        let texture =
            connected(stage, surface, "inputs:specular_roughness", issues).expect("texture");
        assert_eq!(texture.node, "ND_tiledimage_color4");
        assert_eq!(texture.channel, Some('b'));
        assert_eq!(texture.coordinates.as_deref(), Some("st1"));
        assert_eq!(texture.scale, [4.0, 4.0]);
    });
}

#[test]
fn an_omni_pbr_texture_takes_the_material_transform() {
    let text = r#"#usda 1.0
def Shader "S"
{
    uniform token info:implementationSource = "sourceAsset"
    uniform asset info:mdl:sourceAsset = @OmniPBR.mdl@
    asset inputs:diffuse_texture = @./albedo.png@
    float2 inputs:texture_scale = (2, 2)
    float inputs:texture_rotate = 45
}
"#;
    with_layer(text, |stage, issues| {
        let shader = stage.find("/S").expect("shader");
        let texture =
            omni_pbr(stage, shader, "inputs:diffuse_texture", "OmniPBR", issues).expect("texture");
        assert_eq!(texture.file, "./albedo.png");
        assert_eq!(texture.scale, [2.0, 2.0]);
        assert_eq!(texture.rotation, 45.0);
        assert!(omni_pbr(stage, shader, "inputs:normalmap_texture", "OmniPBR", issues).is_none());
    });
}
