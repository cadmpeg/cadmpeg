// SPDX-License-Identifier: Apache-2.0

use super::{compose, resolve_relative, Resolved, Stage};
use crate::layers::Layers;
use crate::test_support::with_context;
use crate::text::grammar::parse_layer;
use crate::text::syntax::{Layer, Value};

fn layer(text: &str) -> Layer {
    with_context(text.as_bytes(), |ctx| {
        parse_layer(ctx, text.as_bytes()).unwrap()
    })
}

fn stage_paths(text: &str) -> Vec<String> {
    let layers = Layers::single(layer(text));
    with_context(text.as_bytes(), |ctx| {
        let stage = compose(ctx, &layers).unwrap();
        stage.prims.iter().map(|prim| prim.path.clone()).collect()
    })
}

fn with_stage<T>(text: &str, run: impl FnOnce(&Stage<'_>) -> T) -> T {
    let layers = Layers::single(layer(text));
    with_context(text.as_bytes(), |ctx| run(&compose(ctx, &layers).unwrap()))
}

#[test]
fn relative_paths_resolve_against_their_prim() {
    assert_eq!(resolve_relative("/A/B", "../C"), "/A/C");
    assert_eq!(resolve_relative("/A/B", "C"), "/A/B/C");
    assert_eq!(resolve_relative("/A/B", ".x"), "/A/B.x");
    assert_eq!(resolve_relative("/A", "../B.x"), "/B.x");
    assert_eq!(resolve_relative("/A", "/Z"), "/Z");
}

#[test]
fn the_traversal_holds_defined_active_prims() {
    let paths = stage_paths(
        "#usda 1.0\nclass \"C\" {\n}\nover \"O\" {\n}\ndef \"A\"\n{\n    def \"B\" {\n    }\n    def \"Off\" (\n        active = false\n    )\n    {\n        def \"Child\" {\n        }\n    }\n}\n",
    );
    assert_eq!(paths, ["/A", "/A/B"]);
}

#[test]
fn the_selected_variant_adds_its_opinions() {
    let text = "#usda 1.0\ndef \"A\" (\n    variants = {\n        string shape = \"round\"\n    }\n)\n{\n    float size = 1\n    variantSet \"shape\" = {\n        \"round\" {\n            float size = 2\n            float radius = 3\n            def \"Disc\" {\n            }\n        }\n        \"square\" {\n            def \"Plate\" {\n            }\n        }\n    }\n}\n";
    with_stage(text, |stage| {
        let paths: Vec<_> = stage.prims.iter().map(|prim| prim.path.as_str()).collect();
        assert_eq!(paths, ["/A", "/A/Disc"]);
        let prim = &stage.prims[0];
        let value = |name| match prim.resolve(name) {
            Some(Resolved::Default(Value::Number(number), _)) => number.float,
            other => panic!("{name}: {other:?}"),
        };
        assert_eq!(value("size"), 1.0);
        assert_eq!(value("radius"), 3.0);
        assert_eq!(stage.tally.unselected_variants, 1);
    });
}

#[test]
fn a_selection_in_a_referenced_prim_selects_a_local_variant() {
    let text = "#usda 1.0\nclass \"Base\" (\n    variants = {\n        string v = \"b\"\n    }\n)\n{\n}\ndef \"A\" (\n    references = </Base>\n)\n{\n    variantSet \"v\" = {\n        \"a\" {\n            def \"FromA\" {\n            }\n        }\n        \"b\" {\n            def \"FromB\" {\n            }\n        }\n    }\n}\n";
    assert_eq!(stage_paths(text), ["/A", "/A/FromB"]);
}

#[test]
fn internal_references_compose_and_remap_paths() {
    let text = "#usda 1.0\nclass \"Proto\"\n{\n    rel material:binding = </Proto/Looks/M>\n    def \"Part\" {\n    }\n    def \"Looks\" {\n        def \"M\" {\n        }\n    }\n}\ndef \"A\" (\n    references = </Proto>\n)\n{\n    def \"Local\" {\n    }\n}\n";
    with_stage(text, |stage| {
        let paths: Vec<_> = stage.prims.iter().map(|prim| prim.path.as_str()).collect();
        assert_eq!(
            paths,
            ["/A", "/A/Local", "/A/Part", "/A/Looks", "/A/Looks/M"]
        );
        assert_eq!(
            stage.prims[0].relationship_targets("material:binding"),
            ["/A/Looks/M"]
        );
    });
}

#[test]
fn a_reference_drops_targets_outside_its_root_and_an_inherit_keeps_them() {
    let text = "#usda 1.0\nclass \"Proto\"\n{\n    rel inside = </Proto/Looks>\n    rel outside = </Shared>\n    def \"Looks\" {\n    }\n}\nclass \"Base\"\n{\n    rel outside = </Shared>\n}\ndef \"Shared\" {\n}\ndef \"A\" (\n    references = </Proto>\n)\n{\n}\ndef \"B\" (\n    inherits = </Base>\n)\n{\n}\n";
    with_stage(text, |stage| {
        let a = stage.find("/A").expect("A");
        assert_eq!(a.relationship_targets("inside"), ["/A/Looks"]);
        assert!(a.relationship_targets("outside").is_empty());
        let b = stage.find("/B").expect("B");
        assert_eq!(b.relationship_targets("outside"), ["/Shared"]);
    });
}

#[test]
fn local_opinions_are_stronger_than_arcs() {
    let text = "#usda 1.0\nclass \"Base\"\n{\n    float a = 1\n    float b = 1\n}\ndef \"A\" (\n    inherits = </Base>\n)\n{\n    float a = 2\n}\n";
    with_stage(text, |stage| {
        let prim = &stage.prims[0];
        let value = |name| match prim.resolve(name) {
            Some(Resolved::Default(Value::Number(number), _)) => number.float,
            other => panic!("{name}: {other:?}"),
        };
        assert_eq!(value("a"), 2.0);
        assert_eq!(value("b"), 1.0);
    });
}

#[test]
fn an_arc_cycle_and_a_missing_target_are_tallied() {
    let text = "#usda 1.0\ndef \"A\" (\n    references = </B>\n)\n{\n}\ndef \"B\" (\n    references = [</A>, </Missing>]\n)\n{\n}\n";
    with_stage(text, |stage| {
        assert!(!stage.tally.cycles.is_empty());
        assert!(stage.tally.missing_targets.contains("/Missing"));
    });
}

#[test]
fn instances_share_a_prototype_key_and_ignore_local_children() {
    let text = "#usda 1.0\nclass \"Proto\"\n{\n    def \"Part\" {\n    }\n}\ndef \"I1\" (\n    instanceable = true\n    references = </Proto>\n)\n{\n    def \"Override\" {\n    }\n}\ndef \"I2\" (\n    instanceable = true\n    references = </Proto>\n)\n{\n}\n";
    with_stage(text, |stage| {
        let paths: Vec<_> = stage.prims.iter().map(|prim| prim.path.as_str()).collect();
        assert_eq!(paths, ["/I1", "/I1/Part", "/I2", "/I2/Part"]);
        assert_eq!(stage.prims[0].instance_key, stage.prims[2].instance_key);
        assert!(stage.prims[0].instance_key.is_some());
        assert_eq!(stage.prims[1].in_instance, Some((0, "Part".to_owned())));
        assert!(stage.tally.instance_overrides.contains("/I1"));
    });
}

#[test]
fn arcs_to_other_layers_stay_external() {
    let text = "#usda 1.0\ndef \"A\" (\n    prepend references = [@a.usd@</X>, @b.usd@]\n    payload = @c.usdc@\n)\n{\n}\n";
    with_stage(text, |stage| {
        let external: Vec<_> = stage.prims[0]
            .external
            .iter()
            .map(|arc| (arc.asset.as_str(), arc.prim_path.as_deref(), arc.payload))
            .collect();
        assert_eq!(
            external,
            [
                ("a.usd", Some("/X"), false),
                ("b.usd", None, false),
                ("c.usdc", None, true)
            ]
        );
    });
}

#[test]
fn a_time_sampled_attribute_resolves_to_its_default_or_first_sample() {
    let text = "#usda 1.0\ndef \"A\"\n{\n    float a = 5\n    float a.timeSamples = {\n        2: 3,\n        1: 4,\n    }\n    float b.timeSamples = {\n        2: 3,\n        1: 4,\n    }\n    float c = None\n}\n";
    with_stage(text, |stage| {
        let prim = &stage.prims[0];
        assert!(
            matches!(prim.resolve("a"), Some(Resolved::Default(Value::Number(number), _)) if number.float == 5.0)
        );
        assert!(
            matches!(prim.resolve("b"), Some(Resolved::FirstSample(Value::Number(number), _)) if number.float == 4.0)
        );
        assert!(matches!(prim.resolve("c"), Some(Resolved::Blocked)));
        assert!(prim.is_animated("a"));
        assert!(prim.resolve("d").is_none());
    });
}

#[test]
fn reorder_statements_order_children() {
    let paths = stage_paths(
        "#usda 1.0\nreorder rootPrims = [\"B\"]\ndef \"A\"\n{\n    reorder nameChildren = [\"Z\", \"Y\"]\n    def \"Y\" {\n    }\n    def \"Z\" {\n    }\n}\ndef \"B\" {\n}\n",
    );
    assert_eq!(paths, ["/B", "/A", "/A/Z", "/A/Y"]);
}
