// SPDX-License-Identifier: Apache-2.0
//! Texture coordinates and image textures through decode, write, and
//! decode again, as layers and as packages.

use cadmpeg_ir::assets::AssetContent;
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::tessellation::TextureCoordinateAddressing;

use crate::loss::UsdLossCode;
use crate::test_support::{decode, decode_bytes, decode_with, encode, encode_bytes, Files};

/// Relative error of one binary32 rounding.
const EPS_BINARY32: f64 = 1e-7;

/// A textured quad: two triangles' worth of face-varying, indexed `st`
/// coordinates, and a material whose preview surface reads `wood.png`
/// through a transform and the green channel of `orm.png` as roughness.
const QUAD: &str = r#"#usda 1.0
def Mesh "Quad" (
    prepend apiSchemas = ["MaterialBindingAPI"]
)
{
    int[] faceVertexCounts = [4]
    int[] faceVertexIndices = [0, 1, 2, 3]
    point3f[] points = [(0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0)]
    texCoord2f[] primvars:st = [(0, 0), (1, 0), (1, 1), (0, 1)] (
        interpolation = "faceVarying"
    )
    int[] primvars:st:indices = [0, 1, 2, 3]
    uniform token subdivisionScheme = "none"
    rel material:binding = </Looks/Wood>
}
def Scope "Looks"
{
    def Material "Wood"
    {
        token outputs:surface.connect = </Looks/Wood/Surface.outputs:surface>
        def Shader "Surface"
        {
            uniform token info:id = "UsdPreviewSurface"
            color3f inputs:diffuseColor.connect = </Looks/Wood/Diffuse.outputs:rgb>
            float inputs:roughness.connect = </Looks/Wood/Packed.outputs:g>
            token outputs:surface
        }
        def Shader "Diffuse"
        {
            uniform token info:id = "UsdUVTexture"
            asset inputs:file = @./wood.png@
            float2 inputs:st.connect = </Looks/Wood/Transform.outputs:result>
            float3 outputs:rgb
        }
        def Shader "Packed"
        {
            uniform token info:id = "UsdUVTexture"
            asset inputs:file = @./orm.png@
            float2 inputs:st.connect = </Looks/Wood/Reader.outputs:result>
            float outputs:g
        }
        def Shader "Transform"
        {
            uniform token info:id = "UsdTransform2d"
            float2 inputs:in.connect = </Looks/Wood/Reader.outputs:result>
            float2 inputs:scale = (2, 2)
            float2 outputs:result
        }
        def Shader "Reader"
        {
            uniform token info:id = "UsdPrimvarReader_float2"
            string inputs:varname = "st"
            float2 outputs:result
        }
    }
}
"#;

fn files() -> Files {
    Files(vec![("wood.png", b"wood"), ("orm.png", b"orm")])
}

/// The slots, files, scales, and coordinate sets of the textures of the
/// first appearance.
fn textures(
    result: &cadmpeg_ir::codec::DecodeResult,
) -> Vec<(String, String, f64, Option<String>)> {
    let model = &result.ir().model;
    model.appearances[0]
        .textures
        .iter()
        .map(|texture| {
            let asset = model
                .assets
                .iter()
                .find(|asset| asset.id.as_str() == texture.asset_guid)
                .expect("the texture's asset");
            let file = match &asset.content {
                AssetContent::External { uri } => uri.to_string(),
                AssetContent::Embedded { .. } => "embedded".to_owned(),
            };
            (
                texture.slot.clone(),
                file,
                texture.mapping.u_scale.get(),
                texture
                    .mapping
                    .coordinates
                    .as_ref()
                    .map(|name| name.as_str().to_owned()),
            )
        })
        .collect()
}

#[test]
fn coordinates_and_textures_are_read_and_written_back() {
    let first = decode_with(QUAD, &files());
    let model = &first.ir().model;
    let [set] = model.tessellations[0].texture_coordinates() else {
        panic!("one coordinate set");
    };
    assert_eq!(set.name.as_str(), "st");
    assert_eq!(set.addressing, TextureCoordinateAddressing::Corner);
    assert_eq!(set.values.len(), 6);
    let expected = vec![
        (
            "diffuseColor".to_owned(),
            "wood.png".to_owned(),
            2.0,
            Some("st".to_owned()),
        ),
        (
            "roughness.g".to_owned(),
            "orm.png".to_owned(),
            1.0,
            Some("st".to_owned()),
        ),
    ];
    assert_eq!(textures(&first), expected);
    assert!(!first.report().losses.iter().any(|loss| loss.code
        == UsdLossCode::MaterialNotTransferred.kind()
        || loss.code == UsdLossCode::MeshPrimvarNotTransferred.kind()));

    let (written, _) = encode(first.ir());
    assert!(written.contains("info:id = \"UsdUVTexture\""), "{written}");
    assert!(
        written.contains("info:id = \"ND_image_color3\"")
            || written.contains("ND_tiledimage_color3"),
        "{written}"
    );
    let second = decode_with(&written, &files());
    assert_eq!(textures(&second), expected, "{written}");
    let again = second.ir().model.tessellations[0].texture_coordinates();
    for (left, right) in set.values.iter().zip(&again[0].values) {
        let (left, right) = (left.get(), right.get());
        assert!((left.u - right.u).abs() <= EPS_BINARY32, "{written}");
        assert!((left.v - right.v).abs() <= EPS_BINARY32, "{written}");
    }
}

#[test]
fn a_package_embeds_the_images_of_its_textures() {
    let source = crate::usdz::package(&[
        ("root.usda", QUAD.as_bytes()),
        ("wood.png", b"wood"),
        ("orm.png", b"orm"),
    ])
    .expect("package");
    let first = decode_bytes(&source);
    let embedded = |result: &cadmpeg_ir::codec::DecodeResult| -> Vec<Vec<u8>> {
        result
            .ir()
            .model
            .assets
            .iter()
            .filter_map(|asset| match &asset.content {
                AssetContent::Embedded { data } => Some(data.as_bytes().to_vec()),
                AssetContent::External { .. } => None,
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    assert_eq!(embedded(&first), [b"orm".to_vec(), b"wood".to_vec()]);
    let package = encode_bytes(first.ir(), TargetRequest::Explicit("usdz"));
    let second = decode_bytes(&package);
    assert_eq!(embedded(&second), [b"orm".to_vec(), b"wood".to_vec()]);
    let slots: Vec<String> = textures(&second).into_iter().map(|row| row.0).collect();
    assert_eq!(slots, ["diffuseColor", "roughness.g"]);

    let (_, report) = encode(first.ir());
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == UsdLossCode::ExportTextureExternal.kind()));
}

#[test]
fn a_texture_that_names_no_file_keeps_its_path() {
    let result = decode(QUAD);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == UsdLossCode::TextureUnlocated.kind()));
    let files: Vec<String> = textures(&result).into_iter().map(|row| row.1).collect();
    assert_eq!(files, ["./wood.png", "./orm.png"]);
}
