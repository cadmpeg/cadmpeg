// SPDX-License-Identifier: Apache-2.0
//! Composition across the layers that a root layer names (spec §8.4).

use crate::loss::UsdLossCode;
use crate::test_support::{decode, decode_with, Files};

const PART: &str = "#usda 1.0\n(\n    defaultPrim = \"Bolt\"\n    metersPerUnit = 0.001\n)\ndef Xform \"Bolt\"\n{\n    def Mesh \"Body\"\n    {\n        int[] faceVertexCounts = [3]\n        int[] faceVertexIndices = [0, 1, 2]\n        point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0)]\n    }\n}\n";

fn has_loss(result: &cadmpeg_ir::codec::DecodeResult, code: UsdLossCode) -> bool {
    result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == code.kind())
}

#[test]
fn a_reference_to_an_opened_layer_composes_its_default_prim() {
    let root = "#usda 1.0\n(\n    metersPerUnit = 0.001\n)\ndef Xform \"Assembly\"\n{\n    def Xform \"First\" (\n        references = @./bolt.usda@\n    )\n    {\n    }\n}\n";
    let result = decode_with(root, &Files(vec![("bolt.usda", PART.as_bytes())]));
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert!(!has_loss(&result, UsdLossCode::CompositionExternalArc));
    let unresolved = decode(root);
    assert!(unresolved.ir().model.tessellations.is_empty());
    assert!(has_loss(&unresolved, UsdLossCode::CompositionExternalArc));
    assert!(has_loss(
        &unresolved,
        UsdLossCode::CompositionLayerUnavailable
    ));
}

#[test]
fn a_sublayer_adds_weaker_opinions_to_the_root_layer_stack() {
    let root = "#usda 1.0\n(\n    metersPerUnit = 0.001\n    subLayers = [@./base.usda@]\n)\nover \"Bolt\"\n{\n    over \"Body\" (\n        active = false\n    )\n    {\n    }\n}\ndef Xform \"Extra\"\n{\n}\n";
    let result = decode_with(root, &Files(vec![("base.usda", PART.as_bytes())]));
    assert!(result.ir().model.tessellations.is_empty());
    let names: Vec<_> = result
        .ir()
        .model
        .occurrences
        .iter()
        .filter_map(|occurrence| occurrence.name.clone())
        .collect();
    assert!(names.iter().any(|name| name == "Bolt"), "{names:?}");
    assert!(names.iter().any(|name| name == "Extra"), "{names:?}");
    assert!(!has_loss(
        &result,
        UsdLossCode::CompositionSublayersNotComposed
    ));
}

#[test]
fn instances_of_one_external_layer_share_a_definition() {
    let root = "#usda 1.0\n(\n    metersPerUnit = 0.001\n)\ndef Xform \"Assembly\"\n{\n    def Xform \"First\" (\n        instanceable = true\n        references = @./bolt.usda@\n    )\n    {\n    }\n    def Xform \"Second\" (\n        instanceable = true\n        references = @./bolt.usda@\n    )\n    {\n    }\n}\n";
    let result = decode_with(root, &Files(vec![("bolt.usda", PART.as_bytes())]));
    assert_eq!(result.ir().model.tessellations.len(), 1);
}

const CUBE_REFERENCE: &str = "#usda 1.0\n(\n    metersPerUnit = 0.01\n)\ndef Xform \"Assembly\"\n{\n    def Xform \"Part\" (\n        references = @./cube.usdc@\n    )\n    {\n    }\n}\n";

#[test]
fn a_reference_to_a_usdc_layer_composes_its_default_prim() {
    let crate_layer: &[u8] = include_bytes!("../../tests/golden/fixtures/mesh_cube_0_8_0.usdc");
    let result = decode_with(CUBE_REFERENCE, &Files(vec![("cube.usdc", crate_layer)]));
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert!(!has_loss(&result, UsdLossCode::CompositionLayerUnavailable));
}

#[test]
fn a_reference_to_a_package_composes_its_root_layer() {
    let package = crate::usdz::package(&[("bolt.usda", PART.as_bytes())]).expect("package");
    let package: &'static [u8] = Box::leak(package.into_boxed_slice());
    let root = CUBE_REFERENCE.replace("./cube.usdc", "./bolt.usdz");
    let result = decode_with(&root, &Files(vec![("bolt.usdz", package)]));
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert!(!has_loss(&result, UsdLossCode::CompositionLayerUnavailable));
}

/// A source that serves layers by URL, resolving a relative path against
/// the URL of the layer that names it, and records each request.
#[derive(Debug, Default)]
struct Server {
    requests: std::cell::RefCell<Vec<(Option<String>, String)>>,
}

impl Server {
    fn resolve(anchor: Option<&str>, path: &str) -> String {
        match (path.strip_prefix("./"), anchor) {
            (Some(relative), Some(anchor)) => {
                let directory = anchor.rsplit_once('/').map_or(anchor, |(head, _)| head);
                format!("{directory}/{relative}")
            }
            _ => path.to_owned(),
        }
    }
}

impl cadmpeg_core::decode::Companions for Server {
    fn open(
        &self,
        anchor: Option<&str>,
        path: &str,
        _max_bytes: u64,
    ) -> Result<cadmpeg_core::decode::Companion, String> {
        self.requests
            .borrow_mut()
            .push((anchor.map(str::to_owned), path.to_owned()));
        let id = Self::resolve(anchor, path);
        let bytes = match id.as_str() {
            "omniverse://nucleus/lib/assembly.usda" => "#usda 1.0\n(\n    defaultPrim = \"Assembly\"\n    metersPerUnit = 0.001\n)\ndef Xform \"Assembly\"\n{\n    def Xform \"Bolt\" (\n        references = @./parts/bolt.usda@\n    )\n    {\n    }\n}\n",
            "omniverse://nucleus/lib/parts/bolt.usda" => PART,
            _ => return Err(format!("{id}: not served")),
        };
        Ok(cadmpeg_core::decode::Companion {
            id,
            bytes: bytes.as_bytes().to_vec(),
        })
    }
}

#[test]
fn url_asset_paths_and_anchors_reach_the_companion_source_unchanged() {
    let root = "#usda 1.0\n(\n    metersPerUnit = 0.001\n)\ndef Xform \"Scene\" (\n    references = @omniverse://nucleus/lib/assembly.usda@\n)\n{\n}\n";
    let server = Server::default();
    let result = decode_with(root, &server);
    assert_eq!(result.ir().model.tessellations.len(), 1);
    assert_eq!(
        *server.requests.borrow(),
        [
            (None, "omniverse://nucleus/lib/assembly.usda".to_owned()),
            (
                Some("omniverse://nucleus/lib/assembly.usda".to_owned()),
                "./parts/bolt.usda".to_owned()
            ),
        ]
    );
}
