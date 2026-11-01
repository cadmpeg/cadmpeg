// SPDX-License-Identifier: Apache-2.0

use super::{absolute, elements, write};
use crate::test_support::{decode, decode_bytes, with_context};
use crate::text::grammar::parse_layer;

#[test]
fn paths_split_into_prim_variant_and_property_elements() {
    let split = elements("/World{shape=round}Ball.points").expect("absolute");
    assert_eq!(
        split,
        [
            ("World".to_owned(), false),
            ("{shape=round}".to_owned(), false),
            ("Ball".to_owned(), false),
            ("points".to_owned(), true),
        ]
    );
    assert_eq!(absolute("/A/B", "../C"), "/A/C");
    assert_eq!(absolute("/A/B", "Child.attr"), "/A/B/Child.attr");
    assert_eq!(absolute("/A/B", ".attr"), "/A/B.attr");
}

/// Every text fixture states the same model as its USDC form.
#[test]
fn every_text_fixture_reads_the_same_as_its_crate_form() {
    let harness = cadmpeg_test_support::golden::Harness::new(
        env!("CARGO_MANIFEST_DIR"),
        "usda",
        "UPDATE_GOLDEN=1 cargo test-fast golden",
    );
    for (name, bytes) in harness.fixture_inputs() {
        let text = std::str::from_utf8(&bytes).expect("text");
        let crate_bytes = with_context(text.as_bytes(), |ctx| {
            let layer = parse_layer(ctx, text.as_bytes()).expect("parse");
            write(&layer).expect("write")
        });
        let from_text = decode(text);
        let from_crate = decode_bytes(&crate_bytes);
        assert_eq!(from_text.ir().model, from_crate.ir().model, "{name}");
    }
}

/// Statements the IR writer does not use: variants, payloads, sublayers,
/// time samples, dictionaries, and list operations of each kind.
#[test]
fn composition_and_animation_statements_survive_the_crate_form() {
    let text = r#"#usda 1.0
(
    defaultPrim = "World"
    metersPerUnit = 0.01
    customLayerData = {
        string author = "test"
        dictionary nested = {
            int[] counts = [1, 2]
        }
    }
)
def Xform "World" (
    kind = "assembly"
    variantSets = ["shape"]
    variants = {
        string shape = "round"
    }
    prepend apiSchemas = ["MaterialBindingAPI"]
    delete apiSchemas = ["Retired"]
)
{
    double3 xformOp:translate.timeSamples = {
        0: (0, 0, 0),
        10: (1, 2, 3),
    }
    uniform token[] xformOpOrder = ["xformOp:translate"]
    variantSet "shape" = {
        "round" {
            def Sphere "Ball"
            {
                double radius = 2
            }
        }
        "square" {
            def Cube "Box"
            {
            }
        }
    }
    def Xform "Part" (
        prepend payload = @./part.usda@</Part>
        append references = </World/Template> (offset = 2; scale = 0.5)
    )
    {
        custom string note = "hello"
        float3[] empty = []
        quatf orientation = (0.5, 0.5, -0.5, 0.5)
        matrix4d xformOp:transform = ((1, 0, 0, 0), (0, 1, 0, 0), (0, 0, 1, 0), (5, 6, 7, 1))
        rel binding = </World/Template>
        token input.connect = </World/Template.outputs:out>
    }
    class Xform "Template"
    {
        token outputs:out
    }
}
"#;
    let crate_bytes = with_context(text.as_bytes(), |ctx| {
        let layer = parse_layer(ctx, text.as_bytes()).expect("parse");
        write(&layer).expect("write")
    });
    let read = crate::test_support::with_root(&crate_bytes, |ctx, root| {
        crate::usdc::read(ctx, root).expect("read")
    });
    let (layer, unread) = read;
    assert!(unread.is_empty(), "{unread:?}");
    let world = &layer.prims[0];
    assert_eq!(world.name, "World");
    assert_eq!(world.body.variant_sets[0].variants.len(), 2);
    assert_eq!(world.body.children.len(), 2);
    let keys: Vec<&str> = world
        .metadata
        .iter()
        .map(|item| item.key.as_str())
        .collect();
    for key in ["kind", "variantSets", "variants", "apiSchemas"] {
        assert!(keys.contains(&key), "{keys:?}");
    }
    let from_text = decode(text);
    let from_crate = decode_bytes(&crate_bytes);
    assert_eq!(from_text.ir().model, from_crate.ir().model);
}
