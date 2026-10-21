// SPDX-License-Identifier: Apache-2.0
//! Route-classification controls for the hand-written reader census.

use std::collections::BTreeSet;

use syn::spanned::Spanned;

use super::classify::{classify_hand_reader, classify_hand_reader_with_bindings, classify_route};
use super::{
    collect_macro_routes, collect_source_items, insert_wire, nodes_from_stream, source_span_text,
    tokenize_block_text, HandImplSource, HandReaderClass, SourceIndex,
};

#[test]
fn reader_route_contract_rejects_unclosed_object_routes() {
    let denied = BTreeSet::from(["ClosedWire".to_owned()]);
    let no_local_wire = BTreeSet::new();
    assert_eq!(
        classify_hand_reader(
            "fixture.rs",
            "Scalar",
            "String::deserialize(deserializer)?",
            &denied,
            &no_local_wire,
        ),
        Ok(HandReaderClass::Keyless)
    );
    assert_eq!(
        classify_hand_reader(
            "fixture.rs",
            "Closed",
            "ClosedWire::deserialize(deserializer)?",
            &denied,
            &no_local_wire,
        ),
        Ok(HandReaderClass::Wire)
    );
    assert_eq!(
        classify_hand_reader(
            "fixture.rs",
            "LocalClosed",
            "#[serde(deny_unknown_fields)] struct Wire {} Wire::deserialize(deserializer)?",
            &denied,
            &no_local_wire,
        ),
        Ok(HandReaderClass::Wire)
    );
    assert!(classify_hand_reader(
        "fixture.rs",
        "LocalOpen",
        "struct Wire {} Wire::deserialize(deserializer)?",
        &denied,
        &no_local_wire,
    )
    .is_err());
    assert!(classify_hand_reader(
        "fixture.rs",
        "CommentAndLiteral",
        "// ClosedWire::deserialize\nlet marker = \"deny_unknown_fields\";",
        &denied,
        &no_local_wire,
    )
    .is_err());
    assert!(classify_hand_reader(
        "fixture.rs",
        "LiteralRoute",
        "let marker = \"ClosedWire::deserialize\";",
        &denied,
        &no_local_wire,
    )
    .is_err());
    assert!(classify_hand_reader(
        "fixture.rs",
        "Open",
        "OpenWire::deserialize(deserializer)?",
        &denied,
        &no_local_wire,
    )
    .is_err());
    assert_eq!(
        classify_hand_reader(
            "fixture.rs",
            "OpenMap",
            "cadmpeg_core::distinct_keys::json_object(deserializer)?",
            &denied,
            &no_local_wire,
        ),
        Ok(HandReaderClass::FreeForm)
    );
    assert_eq!(
            classify_hand_reader(
                "fixture.rs",
                "Version",
                "let version = serde_json::Value::deserialize(deserializer)?; check_ir_version(Some(&version))?;",
                &denied,
                &no_local_wire,
            ),
            Ok(HandReaderClass::ValidatedValue)
        );
}

#[test]
fn lexical_markers_without_owned_calls_do_not_prove_a_route() {
    let denied = BTreeSet::from(["Other".to_owned()]);
    let no_local_wire = BTreeSet::new();
    assert!(classify_hand_reader(
        "fixture.rs",
        "UnrelatedBox",
        "let _box = Box::<Thing>; OpenWire::deserialize(deserializer)?",
        &denied,
        &no_local_wire,
    )
    .is_err());
    assert!(classify_hand_reader(
        "fixture.rs",
        "UnattachedDeny",
        "let deny_unknown_fields = true; OpenWire::deserialize(deserializer)?",
        &denied,
        &no_local_wire,
    )
    .is_err());
    assert!(classify_hand_reader(
        "fixture.rs",
        "BareVersionCheck",
        "serde_json::Value::deserialize(deserializer)?; let _ = check_ir_version;",
        &denied,
        &no_local_wire,
    )
    .is_err());
    assert!(classify_hand_reader(
        "fixture.rs",
        "UnpropagatedVersionCheck",
        "serde_json::Value::deserialize(deserializer)?; check_ir_version(None);",
        &denied,
        &no_local_wire,
    )
    .is_err());
    assert!(classify_hand_reader(
        "fixture.rs",
        "DisconnectedGeneric",
        "let _ = Other::<Thing>; OpenWire::deserialize(deserializer)?",
        &denied,
        &no_local_wire,
    )
    .is_err());
}

#[test]
fn receiver_shapes_and_input_bindings_are_load_bearing() {
    let denied = BTreeSet::new();
    let no_local_wire = BTreeSet::new();
    for (name, body) in [
        ("BoxObject", "Box::<Open>::deserialize(deserializer)?;"),
        (
            "QSelfObject",
            "<Open as Deserialize>::deserialize(deserializer)?;",
        ),
        (
            "QualifiedScalar",
            "hostile::String::deserialize(deserializer)?;",
        ),
    ] {
        assert!(
            classify_hand_reader("fixture.rs", name, body, &denied, &no_local_wire,).is_err(),
            "{name} must not inherit a keyless basename contract",
        );
    }

    assert_eq!(
        classify_hand_reader(
            "fixture.rs",
            "QSelfScalar",
            "<String as Deserialize>::deserialize(deserializer)?;",
            &denied,
            &no_local_wire,
        ),
        Ok(HandReaderClass::Keyless)
    );
    assert_eq!(
        classify_hand_reader(
            "fixture.rs",
            "QSelfArray",
            "<[f64; 2]>::deserialize(deserializer)?;",
            &denied,
            &no_local_wire,
        ),
        Ok(HandReaderClass::Keyless)
    );
    assert_eq!(
        classify_hand_reader(
            "fixture.rs",
            "BoxArray",
            "Box::<[u8; 3]>::deserialize(deserializer)?;",
            &denied,
            &no_local_wire,
        ),
        Ok(HandReaderClass::Keyless)
    );

    let input = BTreeSet::from(["input".to_owned()]);
    assert!(
        classify_hand_reader_with_bindings(
            "fixture.rs",
            "ShadowedInput",
            "let deserializer = make_input(); String::deserialize(deserializer)?;",
            &input,
            &denied,
            &no_local_wire,
        )
        .is_err(),
        "a local variable named deserializer is not the method input",
    );
    assert!(
        classify_hand_reader(
            "fixture.rs",
            "NestedInputMention",
            "String::deserialize(make_input(deserializer))?;",
            &denied,
            &no_local_wire,
        )
        .is_err(),
        "a nested expression is not a direct deserializer binding",
    );
}

#[test]
fn version_check_must_consume_a_deserialized_value() {
    let denied = BTreeSet::new();
    let no_local_wire = BTreeSet::new();
    assert_eq!(
            classify_hand_reader(
                "fixture.rs",
                "Version",
                "let version = serde_json::Value::deserialize(deserializer)?; check_ir_version(Some(&version))?;",
                &denied,
                &no_local_wire,
            ),
            Ok(HandReaderClass::ValidatedValue)
        );
    assert!(
            classify_hand_reader(
                "fixture.rs",
                "UnrelatedVersion",
                "let _ = serde_json::Value::deserialize(deserializer)?; check_ir_version(Some(&serde_json::Value::from(6)))?;",
                &denied,
                &no_local_wire,
            )
            .is_err(),
            "a check over unrelated data does not validate the consumed value",
        );
    for body in [
            "let value = serde_json::Value::deserialize(deserializer)?; let moved = value; check_ir_version(Some(&value))?;",
            "let value = serde_json::Value::deserialize(deserializer)?; drop(value); check_ir_version(Some(&value))?;",
            "let value = serde_json::Value::deserialize(deserializer)?; inspect(value); check_ir_version(Some(&value))?;",
            "let first = serde_json::Value::deserialize(deserializer)?; let second = serde_json::Value::deserialize(deserializer)?; check_ir_version(Some(&first))?;",
            "fn check_ir_version<E: serde::de::Error>(_: Option<&serde_json::Value>) -> Result<(), E> { Ok(()) } let value = serde_json::Value::deserialize(deserializer)?; check_ir_version(Some(&value))?;",
            "let check_ir_version = replacement; let value = serde_json::Value::deserialize(deserializer)?; check_ir_version(Some(&value))?;",
        ] {
            assert!(
                classify_hand_reader("fixture.rs", "Reader", body, &denied, &no_local_wire)
                    .is_err(),
                "a moved, duplicated, or shadowed validation value does not prove a version gate: {body}",
            );
        }
}

#[test]
fn reused_input_names_do_not_prove_which_value_was_read() {
    let denied = BTreeSet::new();
    let no_local_wire = BTreeSet::new();
    for body in [
            "let saved = deserializer; let deserializer = replacement(); String::deserialize(deserializer)?; read_open(saved)?;",
            "let saved = deserializer; let (deserializer,) = (replacement(),); String::deserialize(deserializer)?; read_open(saved)?;",
            "let saved = deserializer; let read = |deserializer| String::deserialize(deserializer); read(replacement())?; read_open(saved)?;",
        ] {
            assert!(
                classify_hand_reader("fixture.rs", "Reader", body, &denied, &no_local_wire)
                    .is_err(),
                "a reused parameter name does not identify the consumed input: {body}",
            );
        }
}

#[test]
fn deserialization_errors_and_execution_paths_are_load_bearing() {
    let denied = BTreeSet::new();
    let no_local_wire = BTreeSet::new();
    for (name, body) in [
        (
            "AssignedIf",
            "let _ = if false { Some(u8::deserialize(deserializer)?) } else { None };",
        ),
        (
            "AssignedMatch",
            "let _ = match false { true => Some(u8::deserialize(deserializer)?), false => None };",
        ),
        (
            "EarlyReturn",
            "return Ok(Self); let _ = u8::deserialize(deserializer)?;",
        ),
        ("Discarded", "let _ = u8::deserialize(deserializer);"),
        (
            "DefaultOnError",
            "let _ = u8::deserialize(deserializer).unwrap_or_default();",
        ),
        (
            "ErrorToOption",
            "let _ = u8::deserialize(deserializer).ok();",
        ),
        (
            "ErrorToSuccess",
            "u8::deserialize(deserializer).or_else(|_| Ok(0)).map(|_| Self)",
        ),
        (
            "ShortCircuit",
            "let _ = true || bool::deserialize(deserializer)?;",
        ),
        (
            "UnknownInputCall",
            "u8::deserialize(deserializer)?; inspect(deserializer)?;",
        ),
        (
            "OpaqueInputMacro",
            "u8::deserialize(deserializer)?; inspect!(deserializer);",
        ),
    ] {
        assert!(
            classify_hand_reader("fixture.rs", name, body, &denied, &no_local_wire).is_err(),
            "{name} must not certify a route whose execution or error is unproved",
        );
    }
    assert!(
        classify_hand_reader(
            "fixture.rs",
            "DiscardedMacro",
            "discard!(u8::deserialize(deserializer)?);",
            &denied,
            &no_local_wire,
        )
        .is_err(),
        "a deserializer call passed to an opaque macro cannot certify a route",
    );

    assert_eq!(
        classify_hand_reader(
            "fixture.rs",
            "Propagated",
            "u8::deserialize(deserializer)?;",
            &denied,
            &no_local_wire,
        ),
        Ok(HandReaderClass::Keyless)
    );
    assert!(
        classify_hand_reader(
            "fixture.rs",
            "UnknownInputMethod",
            "u8::deserialize(deserializer)?; deserializer.deserialize_seq(visitor)?;",
            &denied,
            &no_local_wire,
        )
        .is_err(),
        "an unknown method receiving the deserializer cannot be ignored",
    );
    assert_eq!(
        classify_hand_reader(
            "fixture.rs",
            "Returned",
            "return u8::deserialize(deserializer)?;",
            &denied,
            &no_local_wire,
        ),
        Ok(HandReaderClass::Keyless)
    );
    assert_eq!(
        classify_hand_reader(
            "fixture.rs",
            "ForIterable",
            "for _ in Vec::<u8>::deserialize(deserializer)? {}",
            &denied,
            &no_local_wire,
        ),
        Ok(HandReaderClass::Keyless),
        "an unconditional for-loop iterable propagates its direct input route"
    );
}

fn classify_fixture_reader(source: &str) -> Result<HandReaderClass, String> {
    let parsed = syn::parse_file(source).map_err(|error| error.to_string())?;
    let mut index = SourceIndex::default();
    collect_source_items(&parsed.items, "fixture.rs", source, &[], &mut index);
    let reader = index
        .sources
        .iter()
        .find(|route| route.name == "Reader")
        .ok_or_else(|| "fixture has no Reader implementation".to_owned())?;
    classify_route(reader, &index)
}

fn open_reader_fixture(prefix: &str, route: &str) -> String {
    format!(
        r"
                {prefix}
                struct Open;
                impl<'de> serde::Deserialize<'de> for Open {{
                    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {{
                        let _ = serde_json::Value::deserialize(d)?;
                        Ok(Self)
                    }}
                }}
                struct Reader;
                impl<'de> serde::Deserialize<'de> for Reader {{
                    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {{
                        {route}
                    }}
                }}
            "
    )
}

#[test]
fn lexical_imports_resolve_standard_and_hostile_names() {
    let positive = [
        "use std::string::String as Scalar;",
        "use std as standard;",
        "use std as standard; use standard::string::String as Scalar;",
    ];
    let routes = [
        "Scalar::deserialize(d)?;",
        "standard::string::String::deserialize(d)?;",
        "Scalar::deserialize(d)?;",
    ];
    for (prefix, route) in positive.into_iter().zip(routes) {
        let source = if route.starts_with("Scalar") && prefix == "use std as standard;" {
            open_reader_fixture(prefix, "standard::string::String::deserialize(d)?;")
        } else {
            open_reader_fixture(prefix, route)
        };
        assert_eq!(
            classify_fixture_reader(&source),
            Ok(HandReaderClass::Keyless),
            "standard import route must retain its keyless contract: {prefix} {route}"
        );
    }
    let absolute = open_reader_fixture("mod std {}", "::std::string::String::deserialize(d)?;");
    assert_eq!(
        classify_fixture_reader(&absolute),
        Ok(HandReaderClass::Keyless),
        "an absolute standard path is not shadowed by a local module"
    );
    let absolute_alias = open_reader_fixture(
        "use std::string::String as Hostile;",
        "::Hostile::deserialize(d)?;",
    );
    assert!(
        classify_fixture_reader(&absolute_alias).is_err(),
        "an absolute path cannot inherit a lexical alias from the current module"
    );

    let hostile = [
        ("use hostile::String as Scalar;", "Scalar::deserialize(d)?;"),
        ("use hostile::*;", "String::deserialize(d)?;"),
        ("use hostile::{*};", "String::deserialize(d)?;"),
        ("use hostile as std;", "std::String::deserialize(d)?;"),
        (
            "use hostile as std; use std::String as Scalar;",
            "Scalar::deserialize(d)?;",
        ),
    ];
    for (prefix, route) in hostile {
        let source = open_reader_fixture(
            &format!("mod hostile {{ pub use super::Open as String; }} {prefix}"),
            route,
        );
        assert!(
            classify_fixture_reader(&source).is_err(),
            "a hostile alias must not inherit the standard keyless basename: {prefix} {route}"
        );
    }

    let nested = open_reader_fixture(
        "mod hostile { pub mod string { pub use crate::Open as String; } } use hostile as std;",
        "std::string::String::deserialize(d)?;",
    );
    assert!(
        classify_fixture_reader(&nested).is_err(),
        "module aliases must resolve through their nested re-export"
    );
    let chained = open_reader_fixture(
            "mod hostile { pub mod string { pub use crate::Open as String; } } use hostile as std; use std::string::String as Scalar;",
            "Scalar::deserialize(d)?;",
        );
    assert!(
        classify_fixture_reader(&chained).is_err(),
        "alias chains must retain the hostile target identity"
    );
}

#[test]
fn helper_routes_resolve_the_declared_function_owner() {
    for (prefix, route, class) in [
        (
            "use cadmpeg_core::bytes::deserialize as read_bytes;",
            "read_bytes(d)?;",
            HandReaderClass::Keyless,
        ),
        (
            "use cadmpeg_core::distinct_keys::json_object as read_object;",
            "read_object(d)?;",
            HandReaderClass::FreeForm,
        ),
    ] {
        assert_eq!(
            classify_fixture_reader(&open_reader_fixture(prefix, route)),
            Ok(class),
            "qualified helper imports retain their owned route: {prefix}"
        );
    }

    let hostile = open_reader_fixture(
            "mod hostile { pub fn deserialize<T>(_: T) -> Result<(), ()> { Ok(()) } } use hostile::deserialize as read;",
            "read(d)?;",
        );
    assert!(
        classify_fixture_reader(&hostile).is_err(),
        "a local function with a helper basename cannot inherit a codec route"
    );
}

#[test]
fn version_gate_must_directly_check_the_consumed_value() {
    let denied = BTreeSet::new();
    let no_local_wire = BTreeSet::new();
    for body in [
            "let value = serde_json::Value::deserialize(deserializer)?; let _old = value; let value = replacement(); check_ir_version(Some(&value))?;",
            "let mut value = serde_json::Value::deserialize(deserializer)?; value = replacement(); check_ir_version(Some(&value))?;",
            "let value = serde_json::Value::deserialize(deserializer)?; check_ir_version((Some(&value), Some(&replacement())).1)?;",
            "let value = serde_json::Value::deserialize(deserializer)?; check_ir_version(Some(&{ drop(value); replacement() }))?;",
            "let value = serde_json::Value::deserialize(deserializer)?; let unchecked = || check_ir_version(Some(&value));",
            "let value = serde_json::Value::deserialize(deserializer)?; if false { check_ir_version(Some(&value))?; }",
            "let value = serde_json::Value::deserialize(deserializer)?; mutate(&mut value); check_ir_version(Some(&value))?;",
        ] {
            assert!(
                classify_hand_reader("fixture.rs", "Reader", body, &denied, &no_local_wire)
                    .is_err(),
                "a later mention does not prove validation of the consumed value: {body}",
            );
        }
}

#[test]
fn manual_target_must_have_a_closed_consumed_route() {
    let source = r"
            struct Open;
            impl<'de> serde::Deserialize<'de> for Open {
                fn deserialize<D: serde::Deserializer<'de>>(deserializer: D)
                    -> Result<Self, D::Error>
                {
                    #[serde(deny_unknown_fields)]
                    struct UnrelatedClosed;
                    let _ = serde_json::Value::deserialize(deserializer)?;
                    Ok(Self)
                }
            }
            struct Reader;
            impl<'de> serde::Deserialize<'de> for Reader {
                fn deserialize<D: serde::Deserializer<'de>>(deserializer: D)
                    -> Result<Self, D::Error>
                {
                    Open::deserialize(deserializer)?;
                    Ok(Self)
                }
            }
        ";
    let parsed = syn::parse_file(source).expect("parse manual target fixture");
    let mut index = SourceIndex::default();
    collect_source_items(&parsed.items, "fixture.rs", source, &[], &mut index);
    let reader = index
        .sources
        .iter()
        .find(|route| route.name == "Reader")
        .expect("collect outer manual reader");
    assert!(
        classify_route(reader, &index).is_err(),
        "an unrelated local denied declaration cannot close an open target",
    );
}

#[test]
fn scoped_wire_names_do_not_cross_module_boundaries() {
    let route = HandImplSource {
        path: "fixture.rs".to_owned(),
        scope: vec!["module_b".to_owned()],
        name: "Reader".to_owned(),
        body: "Wire::deserialize(deserializer)?".to_owned(),
        method_nodes: tokenize_block_text(
            "fixture.rs",
            "Reader",
            "{Wire::deserialize(deserializer)?}",
        ),
        method_block: None,
        deserializer_bindings: BTreeSet::from(["deserializer".to_owned()]),
        local_denied: BTreeSet::new(),
    };
    let mut index = SourceIndex::default();
    insert_wire(
        &mut index.denied,
        "fixture.rs",
        &["module_a".to_owned()],
        "Wire".to_owned(),
    );
    assert!(classify_route(&route, &index).is_err());
}

#[test]
fn exact_spans_and_macro_routes_are_isolated() {
    let source = "impl<'de> serde::Deserialize<'de> for First { fn deserialize<D>(d: D) -> Result<Self, D::Error> { todo!() } } impl<'de> serde::Deserialize<'de> for Second { fn deserialize<D>(d: D) -> Result<Self, D::Error> { todo!() } }";
    let file: syn::File = syn::parse_str(source).expect("parse adjacent impls");
    let syn::Item::Impl(first) = &file.items[0] else {
        panic!("first item is not an impl")
    };
    let first_text = source_span_text(source, first.span());
    assert!(first_text.contains("First"));
    assert!(!first_text.contains("Second"));

    let macro_file: syn::File = syn::parse_str(
            r"macro_rules! readers {
                ($name:ident) => {
                    impl Serialize for $name { fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error> { todo!() } }
                    impl<'de> serde::Deserialize<'de> for $name {
                        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> {
                            String::deserialize(deserializer)?
                        }
                    }
                };
            }",
        )
        .expect("parse macro route fixture");
    let syn::Item::Macro(item) = &macro_file.items[0] else {
        panic!("fixture item is not a macro");
    };
    let nodes = nodes_from_stream(&item.mac.tokens);
    let mut routes = Vec::new();
    collect_macro_routes(&nodes, "fixture.rs", &[], &mut routes);
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].name, "$name");
    assert!(!routes[0].body.contains("serialize<S>"));
    assert_eq!(
        classify_route(&routes[0], &SourceIndex::default()),
        Ok(HandReaderClass::Keyless)
    );

    let conditional_macro: syn::File = syn::parse_str(
        r"macro_rules! conditional_reader {
                ($name:ident) => {
                    impl<'de> serde::Deserialize<'de> for $name {
                        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> {
                            if false {
                                String::deserialize(deserializer)?;
                            }
                            Ok(Self)
                        }
                    }
                };
            }",
    )
    .expect("parse conditional macro route fixture");
    let syn::Item::Macro(item) = &conditional_macro.items[0] else {
        panic!("conditional fixture item is not a macro");
    };
    let nodes = nodes_from_stream(&item.mac.tokens);
    let mut routes = Vec::new();
    collect_macro_routes(&nodes, "fixture.rs", &[], &mut routes);
    assert_eq!(routes.len(), 1);
    assert!(
        classify_route(&routes[0], &SourceIndex::default()).is_err(),
        "a route in a macro conditional cannot certify the generated reader"
    );

    let opaque_macro: syn::File = syn::parse_str(
        r"macro_rules! opaque_reader {
                ($name:ident) => {
                    impl<'de> serde::Deserialize<'de> for $name {
                        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> {
                            inspect!(deserializer);
                            String::deserialize(deserializer)?
                        }
                    }
                };
            }",
    )
    .expect("parse opaque macro route fixture");
    let syn::Item::Macro(item) = &opaque_macro.items[0] else {
        panic!("opaque fixture item is not a macro");
    };
    let nodes = nodes_from_stream(&item.mac.tokens);
    let mut routes = Vec::new();
    collect_macro_routes(&nodes, "fixture.rs", &[], &mut routes);
    assert_eq!(routes.len(), 1);
    assert!(
        classify_route(&routes[0], &SourceIndex::default()).is_err(),
        "an unknown macro receiving the deserializer cannot be ignored"
    );
}
