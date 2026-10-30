// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use super::{parse_layer, unescape, DOCUMENTATION_KEY};
use crate::text::syntax::{
    AttributeField, AttributeValue, DictionaryValue, Layer, ListOp, MetaValue, Number, Numbers,
    Packed, PathList, PropertySpec, RelationshipField, Specifier, Value, Variability,
};

fn parse(text: &str) -> Result<Layer, String> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(text.as_bytes(), &arena, &policy).unwrap();
    let parsed = parse_layer(&ctx, text.as_bytes()).map_err(|error| error.to_string());
    ctx.finish_session().unwrap();
    parsed
}

fn layer(text: &str) -> Layer {
    parse(text).unwrap_or_else(|error| panic!("{error}\n{text}"))
}

fn prim_body(body: &str) -> String {
    format!("#usda 1.0\ndef \"A\"\n{{\n{body}\n}}\n")
}

fn attribute(body: &str) -> crate::text::syntax::AttributeSpec {
    let layer = layer(&prim_body(body));
    match layer.prims[0].body.properties.last().cloned() {
        Some(PropertySpec::Attribute(attribute)) => attribute,
        other => panic!("not an attribute: {other:?}"),
    }
}

fn default_value(body: &str) -> Value {
    match attribute(body).field {
        AttributeField::Default(AttributeValue::Value(value)) => value,
        other => panic!("no default value: {other:?}"),
    }
}

fn number(value: &Value) -> Number {
    match value {
        Value::Number(number) => *number,
        other => panic!("not a number: {other:?}"),
    }
}

#[test]
fn comments_of_each_form_are_padding() {
    let layer = layer("#usda 1.0\n# line\n// line\n/* block\n across */ def \"A\" {\n}\n");
    assert_eq!(layer.prims.len(), 1);
    assert_eq!(layer.prims[0].name, "A");
}

#[test]
fn a_block_comment_does_not_replace_the_token_space() {
    assert!(parse("#usda 1.0\ndef /*c*/\"A\" {\n}\n").is_ok());
    assert!(parse("#usda 1.0\ndef/*c*/ \"A\" {\n}\n").is_ok());
    assert!(parse("#usda 1.0\ndef/*c*/\"A\" {\n}\n").is_err());
}

#[test]
fn statements_end_at_a_line_end_or_semicolon() {
    let layer = layer(&prim_body("float a = 1; float b = 2\nfloat c = 3;"));
    let names: Vec<_> = layer.prims[0]
        .body
        .properties
        .iter()
        .map(PropertySpec::name)
        .collect();
    assert_eq!(names, ["a", "b", "c"]);
    assert!(parse(&prim_body("float a = 1 float b = 2")).is_err());
    assert!(parse("#usda 1.0\ndef \"A\" { float a = 1 }\n").is_err());
}

#[test]
fn a_child_prim_ends_at_a_line_end() {
    assert!(parse("#usda 1.0\ndef \"A\" { }\n").is_ok());
    assert!(parse("#usda 1.0\ndef \"A\" { def \"B\" { } }\n").is_err());
    let layer = layer("#usda 1.0\ndef \"A\"\n{\n    def \"B\" { }\n}\n");
    assert_eq!(layer.prims[0].body.children[0].name, "B");
}

#[test]
fn numbers_follow_the_number_grammar() {
    let cases: [(&str, f64, Option<i128>); 8] = [
        ("1e3", 1000.0, None),
        (".5", 0.5, None),
        ("5.", 5.0, None),
        ("-1.5E-2", -0.015, None),
        ("-0", -0.0, Some(0)),
        ("42", 42.0, Some(42)),
        ("inf", f64::INFINITY, None),
        ("-inf", f64::NEG_INFINITY, None),
    ];
    for (text, float, integer) in cases {
        let parsed = number(&default_value(&format!("double a = {text}")));
        assert_eq!(parsed.float.to_bits(), float.to_bits(), "{text}");
        assert_eq!(parsed.integer, integer, "{text}");
    }
    assert!(number(&default_value("double a = nan")).float.is_nan());
    for text in ["+1", "0x10", "1f", "1e", ".e1"] {
        assert!(
            parse(&prim_body(&format!("double a = {text}"))).is_err(),
            "{text}"
        );
    }
}

#[test]
fn integer_text_keeps_its_exact_value() {
    let parsed = number(&default_value("uint64 a = 18446744073709551615"));
    assert_eq!(parsed.integer, Some(18_446_744_073_709_551_615));
}

#[test]
fn string_escapes_follow_the_escape_table() {
    assert_eq!(
        unescape(r"a\x41\101\n\t\\\'\u0041").unwrap(),
        "aAA\n\t\\'u0041"
    );
    assert_eq!(unescape(r"\x").unwrap(), "\0");
    assert_eq!(unescape(r"\8").unwrap(), "8");
    assert_eq!(unescape(r"\303\277").unwrap(), "\u{ff}");
    assert_eq!(unescape(r"\777"), None);
}

#[test]
fn quoted_strings_of_each_delimiter_parse() {
    assert_eq!(
        default_value(r#"string a = "x\"y""#),
        Value::String("x\"y".into())
    );
    assert_eq!(default_value("string a = 'x'"), Value::String("x".into()));
    assert_eq!(
        default_value("string a = \"\"\"a\nb\"c\"\"\""),
        Value::String("a\nb\"c".into())
    );
    assert_eq!(
        default_value("string a = '''a\nb'c'''"),
        Value::String("a\nb'c".into())
    );
    assert!(parse(&prim_body("string a = \"a\nb\"")).is_err());
}

#[test]
fn asset_paths_of_each_delimiter_parse() {
    assert_eq!(
        default_value("asset a = @./x.usd@"),
        Value::Asset("./x.usd".into())
    );
    assert_eq!(
        default_value("asset a = @@@a@b@@@"),
        Value::Asset("a@b".into())
    );
    assert_eq!(
        default_value(r"asset a = @@@a\@@@b@@@"),
        Value::Asset("a@@@b".into())
    );
    assert_eq!(
        default_value("asset a = @@@a@@@@@"),
        Value::Asset("a@@".into())
    );
}

#[test]
fn tuples_lists_and_empty_lists_parse() {
    // A list of numbers or of tuples of numbers is held flat.
    assert_eq!(
        default_value("float3[] a = [(1, 2, 3), (4, 5, 6,)]"),
        Value::Packed(Packed {
            numbers: Numbers::Literals {
                integers: vec![1, 2, 3, 4, 5, 6],
                floats: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            },
            width: 3,
        })
    );
    assert_eq!(default_value("float[] a = []"), Value::List(Vec::new()));
    assert_eq!(
        default_value("float[] a = [1,\n  2,\n]"),
        Value::Packed(Packed {
            numbers: Numbers::Literals {
                integers: vec![1, 2],
                floats: vec![1.0, 2.0],
            },
            width: 1,
        })
    );
    assert!(parse(&prim_body("float[] a = [1\n, 2]")).is_err());
    assert!(parse(&prim_body("float3 a = ()")).is_err());
}

#[test]
fn a_packed_list_keeps_reals_matrices_and_negative_zero() {
    assert_eq!(
        default_value("float[] a = [1, 2.5, -0]"),
        Value::Packed(Packed {
            numbers: Numbers::Reals(vec![1.0, 2.5, -0.0]),
            width: 1,
        })
    );
    let Value::Packed(packed) = default_value("float[] a = [-0, 1]") else {
        panic!("a packed list");
    };
    let Numbers::Literals { floats, .. } = packed.numbers else {
        panic!("integer literals");
    };
    assert!(floats[0].is_sign_negative());
    assert_eq!(
        default_value("matrix2d[] a = [((1, 0), (0, 1))]"),
        Value::Packed(Packed {
            numbers: Numbers::Literals {
                integers: vec![1, 0, 0, 1],
                floats: vec![1.0, 0.0, 0.0, 1.0],
            },
            width: 4,
        })
    );
    // A list with an item that is not a number is not packed.
    assert_eq!(
        default_value("float[] a = [1, \"b\"]"),
        Value::List(vec![
            Value::Number(Number {
                float: 1.0,
                integer: Some(1)
            }),
            Value::String("b".into()),
        ])
    );
}

#[test]
fn attribute_declarations_carry_their_keywords() {
    let custom = attribute("custom uniform token[] a:b = [\"x\"]");
    assert!(custom.custom);
    assert_eq!(custom.variability, Variability::Uniform);
    assert_eq!(custom.type_name, "token");
    assert!(custom.array);
    assert_eq!(custom.name, "a:b");
    let keyword_name = attribute("float def = 1");
    assert_eq!(keyword_name.name, "def");
    assert!(matches!(
        attribute("float a").field,
        AttributeField::Declared
    ));
    assert!(matches!(
        attribute("float a = None").field,
        AttributeField::Default(AttributeValue::None)
    ));
    assert!(matches!(
        attribute("float a = AnimationBlock").field,
        AttributeField::Default(AttributeValue::AnimationBlock)
    ));
    assert!(parse(&prim_body("custom varying float a = 1")).is_err());
}

#[test]
fn time_samples_require_commas() {
    let AttributeField::TimeSamples(samples) =
        attribute("float a.timeSamples = {\n 0: 1,\n 1.5: None,\n}").field
    else {
        panic!("no time samples");
    };
    assert_eq!(samples.len(), 2);
    assert_eq!(samples[1].time.float, 1.5);
    assert_eq!(samples[1].value, AttributeValue::None);
    assert!(parse(&prim_body("float a.timeSamples = {\n 0: 1\n 1.5: 2\n}")).is_err());
}

#[test]
fn connections_and_relationships_parse() {
    let connect = attribute("prepend float a.connect = [</B.c>, </D.e>]");
    assert_eq!(connect.list_op, Some(ListOp::Prepend));
    assert_eq!(
        connect.field,
        AttributeField::Connect(PathList::Paths(vec!["/B.c".into(), "/D.e".into()]))
    );
    let layer = layer(&prim_body(
        "rel r = [</A>, </B.x>]\nprepend rel material:binding = </M> \nrel n = None\nrel d.default = </X>",
    ));
    let relationships: Vec<_> = layer.prims[0]
        .body
        .properties
        .iter()
        .map(|property| match property {
            PropertySpec::Relationship(relationship) => {
                (relationship.list_op, relationship.field.clone())
            }
            PropertySpec::Attribute(_) => panic!("not a relationship"),
        })
        .collect();
    assert_eq!(
        relationships,
        [
            (
                None,
                RelationshipField::Targets(PathList::Paths(vec!["/A".into(), "/B.x".into()]))
            ),
            (
                Some(ListOp::Prepend),
                RelationshipField::Targets(PathList::Paths(vec!["/M".into()]))
            ),
            (None, RelationshipField::Targets(PathList::None)),
            (None, RelationshipField::DefaultTarget("/X".into())),
        ]
    );
}

#[test]
fn layer_metadata_items_parse() {
    let layer = layer(
        "#usda 1.0\n(\n    \"doc text\"\n    defaultPrim = \"World\"; upAxis = \"Z\"\n    metersPerUnit = 0.001\n    subLayers = [\n        @a.usda@ (offset = 2; scale = 0.5),\n        @b.usda@\n    ]\n    customLayerData = {\n        dictionary inner = {\n            int x = 1\n        }\n        string \"a key\" = \"v\"\n    }\n)\n",
    );
    let keys: Vec<_> = layer
        .metadata
        .iter()
        .map(|item| item.key.as_str())
        .collect();
    assert_eq!(
        keys,
        [
            DOCUMENTATION_KEY,
            "defaultPrim",
            "upAxis",
            "metersPerUnit",
            "subLayers",
            "customLayerData"
        ]
    );
    let MetaValue::SubLayers(layers) = &layer.metadata[4].value else {
        panic!("no sublayers");
    };
    assert_eq!(layers[0].asset, "a.usda");
    assert_eq!(layers[0].offset.map(|number| number.float), Some(2.0));
    assert_eq!(layers[0].scale.map(|number| number.float), Some(0.5));
    assert_eq!(layers[1].offset, None);
    let MetaValue::Dictionary(dictionary) = &layer.metadata[5].value else {
        panic!("no dictionary");
    };
    assert!(matches!(
        dictionary
            .entries
            .iter()
            .find(|entry| entry.key == "inner")
            .map(|entry| &entry.value),
        Some(DictionaryValue::Dictionary(_))
    ));
    assert!(matches!(
        dictionary.entries.iter().find(|entry| entry.key == "a key").map(|entry| &entry.value),
        Some(DictionaryValue::Typed { type_name, .. }) if type_name == "string"
    ));
}

#[test]
fn prim_metadata_arcs_parse() {
    let layer = layer(
        "#usda 1.0\ndef Xform \"A\" (\n    kind = \"component\"\n    prepend references = [ @a.usd@</X> (offset = 1; scale = 2), </Y> ]\n    payload = @b.usd@\n    inherits = </C>\n    prepend apiSchemas = [\"MaterialBindingAPI\"]\n    variants = {\n        string shape = \"round\"\n    }\n    prepend variantSets = \"shape\"\n    instanceable = true\n)\n{\n}\n",
    );
    let prim = &layer.prims[0];
    assert_eq!(prim.specifier, Specifier::Def);
    assert_eq!(prim.type_name.as_deref(), Some("Xform"));
    let MetaValue::References(references) = &prim.metadata[1].value else {
        panic!("no references");
    };
    assert_eq!(prim.metadata[1].list_op, Some(ListOp::Prepend));
    assert_eq!(references[0].asset.as_deref(), Some("a.usd"));
    assert_eq!(references[0].prim_path.as_deref(), Some("/X"));
    assert_eq!(references[0].scale.map(|number| number.float), Some(2.0));
    assert_eq!(references[1].asset, None);
    assert_eq!(references[1].prim_path.as_deref(), Some("/Y"));
    assert!(
        matches!(&prim.metadata[2].value, MetaValue::Payloads(payloads) if payloads[0].asset.as_deref() == Some("b.usd"))
    );
    assert_eq!(prim.metadata[3].value, MetaValue::Paths(vec!["/C".into()]));
    assert_eq!(prim.metadata[4].key, "apiSchemas");
    assert_eq!(
        prim.metadata[6].value,
        MetaValue::Names(vec!["shape".into()])
    );
    assert!(parse("#usda 1.0\ndef \"A\" (\n prepend apiSchemas = \"X\"\n)\n{\n}\n").is_err());
}

#[test]
fn variant_sets_hold_variant_bodies() {
    let layer = layer(
        "#usda 1.0\ndef \"A\" (\n    variants = { string v = \"x\" }\n)\n{\n    variantSet \"v\" = {\n        \"x\" {\n            def \"B\" {\n            }\n        }\n        \"y\" (doc = \"d\") {\n            float a = 1\n        }\n    }\n}\n",
    );
    let set = &layer.prims[0].body.variant_sets[0];
    assert_eq!(set.name, "v");
    assert_eq!(set.variants.len(), 2);
    assert_eq!(set.variants[0].body.children[0].name, "B");
    assert_eq!(set.variants[1].metadata[0].key, "doc");
    assert_eq!(set.variants[1].body.properties.len(), 1);
}

#[test]
fn order_statements_and_prim_type_names_parse() {
    let layer = layer(
        "#usda 1.0\nreorder rootPrims = [\"B\", \"A\"]\ndef Foo.Bar \"A\"\n{\n    reorder nameChildren = [\"C\"]\n    reorder properties = [\"a\"]\n    reorder rel r = </X>\n}\ndef Ns::T \"B\" {\n}\n",
    );
    assert_eq!(layer.root_order, Some(vec!["B".into(), "A".into()]));
    assert_eq!(layer.prims[0].type_name.as_deref(), Some("Foo.Bar"));
    assert_eq!(layer.prims[1].type_name.as_deref(), Some("Ns::T"));
    assert_eq!(layer.prims[0].body.child_orders, vec![vec!["C".to_owned()]]);
    assert_eq!(
        layer.prims[0].body.property_orders,
        vec![vec!["a".to_owned()]]
    );
    assert_eq!(layer.prims[0].body.properties.len(), 1);
}

#[test]
fn splines_and_array_edits_keep_their_text() {
    assert!(matches!(
        attribute("double a.spline = {\n 1: 0,\n}").field,
        AttributeField::Spline(text) if text.starts_with('{') && text.ends_with('}')
    ));
    assert!(matches!(
        attribute("int[] a = edit [ append 4; erase [0] ]").field,
        AttributeField::Default(AttributeValue::ArrayEdit(text)) if text.starts_with("edit [")
    ));
}

#[test]
fn attribute_metadata_admits_its_items() {
    let attribute = attribute(
        "color3f[] primvars:displayColor = [(1, 0, 0)] (\n    interpolation = \"constant\"\n    elementSize = 1\n    displayUnit = mm\n    \"doc\"\n)",
    );
    let keys: Vec<_> = attribute
        .metadata
        .iter()
        .map(|item| item.key.as_str())
        .collect();
    assert_eq!(
        keys,
        [
            "interpolation",
            "elementSize",
            "displayUnit",
            DOCUMENTATION_KEY
        ]
    );
}

#[test]
fn a_bare_identifier_value_parses_as_an_identifier() {
    assert_eq!(
        default_value("bool a = true"),
        Value::Identifier("true".into())
    );
}

#[test]
fn invalid_utf8_and_unclosed_constructs_are_malformed() {
    assert!(parse("#usda 1.0\ndef \"A\" {\n").is_err());
    assert!(parse("#usda 1.0\n/* open").is_err());
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let bytes = b"#usda 1.0\ndef \"\xff\" {\n}\n";
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    assert!(parse_layer(&ctx, bytes).is_err());
    ctx.finish_session().unwrap();
}
