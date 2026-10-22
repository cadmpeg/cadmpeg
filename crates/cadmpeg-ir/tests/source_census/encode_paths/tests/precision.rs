// SPDX-License-Identifier: Apache-2.0
use super::super::source::fixture;

#[test]
fn ordinary_parameter_local_field_and_return_values_are_not_callable_edges() {
    let graph = fixture(
        r#"
        impl EncoderBackend for Writer { fn plan(&self, coverage: u8) {
            let coverage = coverage; let result = Body { coverage }; return result;
        } }
        mod query { fn coverage() { CodecError::WrongFormat("unrelated".into()); } }
        fn coverage() { CodecError::WrongFormat("also unrelated".into()); }
        fn result() { CodecError::WrongFormat("not a call".into()); }
    "#,
    );
    assert!(graph.findings().is_empty());
    assert_eq!(graph.reachable().len(), 1);
}

#[test]
fn free_calls_resolve_in_the_declared_module_not_an_unrelated_same_name() {
    let graph = fixture(
        r#"
        mod writer {
            impl EncoderBackend for Writer { fn plan(&self) { coverage(); } }
            fn coverage() { safe(); } fn safe() {}
        }
        mod query { fn coverage() { CodecError::WrongFormat("wrong module".into()); } }
    "#,
    );
    assert!(graph.findings().is_empty());
    let reached = graph.reachable();
    assert_eq!(reached.len(), 3);
    assert!(reached.keys().all(|&index| !graph.functions[index]
        .scope
        .iter()
        .any(|part| part == "query")));
}

#[test]
fn qualified_associated_calls_do_not_take_a_same_named_type_in_another_module() {
    let graph = fixture(
        r#"
        mod writer { struct Body; impl Body { fn make() {} }
            impl EncoderBackend for Writer { fn plan(&self) { Body::make(); } }
        }
        mod query { struct Body; impl Body { fn make() { CodecError::WrongFormat("wrong type".into()); } } }
    "#,
    );
    assert!(graph.findings().is_empty());
    assert_eq!(graph.reachable().len(), 2);
}

#[test]
fn callback_aliases_assignments_and_parameters_keep_true_targets_and_limitations_visible() {
    let graph = fixture(
        r#"
        impl EncoderBackend for Writer { fn plan(&self) {
            let mut callback = first; let forwarded = callback; invoke(forwarded); callback = second; callback();
        } }
        fn invoke(callback: fn()) { callback(); }
        fn first() { CodecError::WrongFormat("first".into()); }
        fn second() { CodecError::WrongFormat("second".into()); }
    "#,
    );
    let found = graph.findings();
    assert!(found
        .iter()
        .any(|site| site.contains("first") && site.contains("CodecError construction")));
    assert!(found
        .iter()
        .any(|site| site.contains("second") && site.contains("CodecError construction")));
    assert!(found
        .iter()
        .any(|site| site.contains("unresolved callable value callback")));
}

#[test]
fn called_parameter_does_not_resolve_to_an_unrelated_same_named_free_function() {
    let graph = fixture(
        r#"
        impl EncoderBackend for Writer { fn plan(&self, coverage: fn()) { coverage(); } }
        fn coverage() { CodecError::WrongFormat("not the parameter".into()); }
    "#,
    );
    let found = graph.findings();
    assert_eq!(found.len(), 1);
    assert!(found[0].contains("unresolved callable value coverage"));
    assert!(!found[0].contains("CodecError construction"));
}

#[test]
fn lexical_function_and_value_shadowing_do_not_create_module_global_fallbacks() {
    let graph = fixture(
        r#"
        impl EncoderBackend for Writer { fn plan(&self) {
            fn helper() { CodecError::WrongFormat("real nested call".into()); }
            { let helper = 3; consume(helper); } helper();
        } }
        fn helper() { CodecError::WrongFormat("unrelated outer function".into()); }
        fn consume(value: u8) { let _ = value; }
    "#,
    );
    let found = graph.findings();
    assert_eq!(found.len(), 1);
    assert!(found[0].contains("$fixture::Writer::plan@"));
}

#[test]
fn imported_free_function_aliases_and_super_paths_keep_the_owned_target() {
    let graph = fixture(
        r#"
        mod helpers { pub fn emit() { CodecError::WrongFormat("real helper".into()); } }
        mod writer { use super::helpers::emit as forwarded;
            impl EncoderBackend for Writer { fn plan(&self) { forwarded(); } }
        }
        mod other { fn emit() { CodecError::WrongFormat("not imported".into()); } }
    "#,
    );
    let found = graph.findings();
    assert_eq!(found.len(), 1);
    assert!(found[0].contains("helpers::emit"));
}

#[test]
fn macro_forwarded_callback_arguments_do_not_hide_real_helper_errors() {
    let graph = fixture(
        r#"
        macro_rules! invoke { ($f:expr) => { $f() }; }
        impl EncoderBackend for Writer { fn plan(&self) { invoke!(danger); } }
        fn danger() { CodecError::WrongFormat("real callback".into()); }
    "#,
    );
    assert!(graph
        .findings()
        .iter()
        .any(|site| site.contains("danger") && site.contains("CodecError construction")));
}

#[test]
fn parent_imports_do_not_leak_into_child_modules_and_explicit_super_globs_do() {
    let graph = fixture(
        r#"
        use crate::query::coverage;
        mod writer { fn coverage() {} impl EncoderBackend for Writer { fn plan(&self) { coverage(); } } }
        mod query { pub fn coverage() { CodecError::WrongFormat("not inherited".into()); } }
    "#,
    );
    assert!(graph.findings().is_empty());
    let graph = fixture(
        r#"
        fn inherited() { CodecError::WrongFormat("explicitly imported".into()); }
        mod writer { use super::*; impl EncoderBackend for Writer { fn plan(&self) { inherited(); } } }
    "#,
    );
    assert_eq!(graph.findings().len(), 1);
}

#[test]
fn conditional_and_match_value_bindings_do_not_become_same_named_callbacks() {
    let graph = fixture(
        r#"
        impl EncoderBackend for Writer { fn plan(&self, source: Option<u8>) {
            if let Some(coverage) = source { consume(coverage); }
            match source { Some(coverage) => consume(coverage), None => {} }
        } }
        fn consume(value: u8) { let _ = value; }
        fn coverage() { CodecError::WrongFormat("not a callback".into()); }
    "#,
    );
    assert!(graph.findings().is_empty());
}

#[test]
fn unproved_symbol_resolution_is_exposed_as_a_limitation_instead_of_global_name_fallback() {
    let graph = fixture(
        r#"
        impl EncoderBackend for Writer { fn plan(&self) { unknown_reexport::emit(); } }
        mod other { fn emit() { CodecError::WrongFormat("not the unresolved symbol".into()); } }
    "#,
    );
    assert!(graph.findings().is_empty());
    let unresolved = graph.unresolved_symbols();
    assert_eq!(unresolved.len(), 1);
    assert!(unresolved[0].contains("unknown_reexport::emit"));
    assert!(unresolved[0].contains("classification remains unresolved"));
}
