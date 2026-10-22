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
fn unproved_symbol_resolution_is_a_finding_instead_of_a_global_name_fallback() {
    let graph = fixture(
        r#"
        impl EncoderBackend for Writer { fn plan(&self) { unknown_reexport::emit(); } }
        mod other { fn emit() { CodecError::WrongFormat("not the unresolved symbol".into()); } }
    "#,
    );
    let found = graph.findings();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("unresolved workspace call unknown_reexport::emit"));
    assert!(!found[0].contains("CodecError construction"));
    assert_eq!(graph.reachable().len(), 1);
    let unresolved = graph.unresolved_symbols();
    assert_eq!(unresolved.len(), 1);
    assert!(unresolved[0].contains("unknown_reexport::emit"));
    assert!(unresolved[0].contains("classification remains unresolved"));
    let graph = fixture(
        r#"
        impl EncoderBackend for Writer { fn plan(&self) { unknown_reexport::missing(); } }
    "#,
    );
    let found = graph.findings();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("unresolved workspace call unknown_reexport::missing"));
}

#[test]
fn re_exported_renamed_and_glob_imported_paths_reach_the_defining_module() {
    let graph = fixture(
        r#"
        mod decode {
            pub use context::{Context, Spec as Renamed};
            pub use helpers::*;
            mod context {
                pub struct Context; pub struct Spec;
                impl Context { pub fn from_root() -> Self { root_limit() } }
                impl Spec { pub fn make() {} }
                fn root_limit() { ResourceLimit { scope, dimension }; }
            }
            mod helpers { pub fn emit() { CodecError::WrongFormat("glob".into()); } }
        }
        mod writer {
            use crate::decode::emit as forwarded;
            impl EncoderBackend for Writer { fn plan(&self) {
                crate::decode::Context::from_root(); crate::decode::Renamed::make(); forwarded();
            } }
        }
    "#,
    );
    let found = graph.findings();
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found
        .iter()
        .any(|site| site.contains("manufactured ResourceLimit") && site.contains("root_limit")));
    assert!(found
        .iter()
        .any(|site| site.contains("CodecError construction") && site.contains("helpers::emit")));
}

#[test]
fn types_constants_variants_and_external_paths_are_not_unresolved_calls() {
    let graph = fixture(
        r#"
        struct Wrapper(u8); enum Kind { One(u8) } const LIMIT: u8 = 1;
        impl EncoderBackend for Writer { fn plan(&self) {
            Wrapper(LIMIT); Kind::One(LIMIT); std::mem::take(value); u64::from(LIMIT);
            Vec::with_capacity(1); Some(LIMIT);
        } }
    "#,
    );
    assert!(graph.findings().is_empty(), "{:?}", graph.findings());
}

#[test]
fn self_imports_and_macro_generated_items_reach_their_definitions() {
    let graph = fixture(
        r#"
        mod helpers { pub fn emit() { CodecError::WrongFormat("self import".into()); } }
        macro_rules! accessors { ($name:ident) => { fn $name() { CodecError::WrongFormat("generated".into()); } }; }
        mod view { struct View; impl View { accessors!(read_at); } }
        mod writer { use crate::helpers::{self};
            impl EncoderBackend for Writer { fn plan(&self) { helpers::emit(); crate::view::View::read_at(); } }
        }
    "#,
    );
    let found = graph.findings();
    assert!(!found
        .iter()
        .any(|site| site.contains("unresolved workspace call")));
    assert!(found
        .iter()
        .any(|site| site.contains("CodecError construction") && site.contains("helpers::emit")));
    assert!(found
        .iter()
        .any(|site| site.contains("CodecError construction") && site.contains("accessors")));
}

#[test]
fn a_public_function_reexport_reaches_its_real_forbidden_helper() {
    let graph = fixture(
        r#"
        mod implementation { pub fn emit() { CodecError::WrongFormat("actual helper".into()); } }
        mod facade { pub use crate::implementation::emit as write; }
        impl EncoderBackend for Writer { fn plan(&self) { crate::facade::write(); } }
        mod unrelated { fn write() { CodecError::WrongFormat("unrelated".into()); } }
    "#,
    );
    let found = graph.findings();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("implementation::emit"));
    assert!(found[0].contains("CodecError construction"));
    assert_eq!(graph.reachable().len(), 2);
}

#[test]
fn chained_function_and_module_reexports_forward_into_the_owned_call_graph() {
    let graph = fixture(
        r#"
        mod implementation { pub fn emit() { CodecError::WrongFormat("actual helper".into()); } }
        mod first { pub use super::implementation::{self as body, emit as dispatch}; }
        mod second { pub use crate::first as facade; pub use crate::first::dispatch as write; }
        impl EncoderBackend for Writer { fn plan(&self) {
            crate::second::write(); crate::second::facade::body::emit();
        } }
    "#,
    );
    assert_eq!(
        graph.findings().len(),
        1,
        "one actual site on both real routes"
    );
    assert_eq!(graph.reachable().len(), 2);
    assert!(graph
        .unresolved_symbols()
        .iter()
        .all(|path| !path.contains("second::write") && !path.contains("second::facade")));
}

#[test]
fn reexported_associated_types_and_simple_type_aliases_keep_the_real_owner() {
    let graph = fixture(
        r#"
        mod implementation { pub struct Engine; impl Engine {
            pub fn emit() { CodecError::WrongFormat("actual associated helper".into()); }
        } pub type Alias = Engine; }
        mod facade { pub use crate::implementation::Alias as Writer; }
        impl EncoderBackend for Writer { fn plan(&self) { crate::facade::Writer::emit(); } }
        mod unrelated { struct Writer; impl Writer { fn emit() { CodecError::WrongFormat("wrong owner".into()); } } }
    "#,
    );
    let found = graph.findings();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("Engine::emit"));
    assert_eq!(graph.reachable().len(), 2);
}

#[test]
fn constructor_reexports_are_checked_even_without_a_function_symbol() {
    let graph = fixture(
        r#"
        mod first { pub use cadmpeg_core::CodecError::WrongFormat as fail; }
        mod second { pub use crate::first::fail as reject; }
        impl EncoderBackend for Writer { fn plan(&self) { crate::second::reject("actual constructor".into()); } }
    "#,
    );
    let found = graph.findings();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("CodecError construction"));
    assert!(found[0].contains("Writer::plan"));
    assert_eq!(
        graph.reachable().len(),
        1,
        "enum constructor has no invented body"
    );
}

#[test]
fn self_and_super_constructor_reexport_paths_retain_the_owned_binding() {
    let graph = fixture(
        r#"
        mod writer {
            mod facade { pub use cadmpeg_core::CodecError::WrongFormat as fail; }
            impl EncoderBackend for Writer { fn plan(&self) { self::facade::fail("actual constructor".into()); } }
            mod nested { impl EncoderBackend for Nested { fn plan(&self) { super::facade::fail("second actual constructor".into()); } } }
        }
    "#,
    );
    let found = graph.findings();
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found
        .iter()
        .all(|site| site.contains("CodecError construction")));
}

#[test]
fn equally_named_reexports_in_other_modules_do_not_pollute_a_safe_owned_call() {
    let graph = fixture(
        r#"
        mod safe { pub fn emit() {} }
        mod dangerous { pub fn emit() { CodecError::WrongFormat("not reached".into()); } }
        mod chosen { pub use crate::safe::emit as write; }
        mod other { pub use crate::dangerous::emit as write; }
        impl EncoderBackend for Writer { fn plan(&self) { crate::chosen::write(); } }
    "#,
    );
    assert!(graph.findings().is_empty());
    assert_eq!(graph.reachable().len(), 2);
    assert!(graph
        .reachable()
        .keys()
        .all(|&index| !graph.functions[index]
            .scope
            .iter()
            .any(|part| part == "dangerous")));
}

#[test]
fn alias_cycles_stay_explicitly_unresolved_without_borrowing_an_unrelated_target() {
    let graph = fixture(
        r#"
        mod first { pub use crate::second::emit as emit; }
        mod second { pub use crate::first::emit as emit; }
        mod unrelated { fn emit() { CodecError::WrongFormat("not the cycle target".into()); } }
        impl EncoderBackend for Writer { fn plan(&self) { crate::first::emit(); } }
    "#,
    );
    let found = graph.findings();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("unresolved workspace call crate::first::emit"));
    assert_eq!(graph.reachable().len(), 1);
    let unresolved = graph.unresolved_symbols();
    assert_eq!(unresolved.len(), 1);
    assert!(unresolved[0].contains("first::emit"));
    assert!(unresolved[0].contains("classification remains unresolved"));
}

#[test]
fn path_growing_alias_cycles_are_bounded_and_remain_an_unresolved_limitation() {
    let graph = fixture(
        r#"
        mod recursive { pub use crate::recursive::forward::forward as forward; }
        impl EncoderBackend for Writer { fn plan(&self) { crate::recursive::forward::emit(); } }
        mod unrelated { fn emit() { CodecError::WrongFormat("not a proved target".into()); } }
    "#,
    );
    let found = graph.findings();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("unresolved workspace call crate::recursive::forward::emit"));
    assert_eq!(graph.reachable().len(), 1);
    assert_eq!(graph.unresolved_symbols().len(), 1);
}

#[test]
fn conditional_reexport_bindings_conservatively_reach_both_actual_targets() {
    let graph = fixture(
        r#"
        mod first { pub fn emit() { CodecError::WrongFormat("first actual branch".into()); } }
        mod second { pub fn emit() { CodecError::WrongFormat("second actual branch".into()); } }
        mod facade {
            #[cfg(feature="first")] pub use crate::first::emit as write;
            #[cfg(not(feature="first"))] pub use crate::second::emit as write;
        }
        impl EncoderBackend for Writer { fn plan(&self) { crate::facade::write(); } }
    "#,
    );
    let found = graph.findings();
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.iter().any(|site| site.contains("first::emit")));
    assert!(found.iter().any(|site| site.contains("second::emit")));
}

#[test]
fn a_shadowing_local_value_is_not_a_reexported_error_constructor() {
    let graph = fixture(
        r#"
        mod facade { pub use cadmpeg_core::CodecError::WrongFormat as result; }
        use crate::facade::result;
        impl EncoderBackend for Writer { fn plan(&self) { let result = 5; return result; } }
    "#,
    );
    assert!(graph.findings().is_empty());
    assert_eq!(graph.reachable().len(), 1);
}

#[test]
fn repeated_self_module_aliases_resolve_when_each_step_consumes_a_real_path_segment() {
    let graph = fixture(
        r#"
        mod actual {
            pub use crate::actual as again;
            pub fn emit() { CodecError::WrongFormat("real repeated module alias".into()); }
        }
        impl EncoderBackend for Writer { fn plan(&self) { crate::actual::again::again::emit(); } }
    "#,
    );
    let found = graph.findings();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("actual::emit"));
    assert_eq!(graph.reachable().len(), 2);
    assert!(graph
        .unresolved_symbols()
        .iter()
        .all(|path| !path.contains("actual::again")));
}

#[test]
fn a_chained_renamed_encoder_backend_trait_is_an_actual_encode_root() {
    let graph = fixture(
        r#"
        mod facade { pub use cadmpeg_ir::codec::write::EncoderBackend as Backend; }
        mod public { pub use crate::facade::Backend as WriterBackend; }
        mod implementation { pub fn emit() { CodecError::WrongFormat("actual encode helper".into()); } }
        impl crate::public::WriterBackend for Writer { fn plan(&self) { crate::implementation::emit(); } }
    "#,
    );
    let roots: Vec<_> = graph
        .functions
        .iter()
        .filter(|function| graph.is_root(function))
        .collect();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].name, "plan");
    let found = graph.findings();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("implementation::emit"));
    assert!(found[0].contains("Writer::plan"));
    assert_eq!(graph.reachable().len(), 2);
}

#[test]
fn an_unrelated_trait_with_the_same_export_alias_is_not_an_encoder_root() {
    let graph = fixture(
        r#"
        mod encoder { pub use cadmpeg_ir::codec::write::EncoderBackend as Backend; }
        mod inspection { pub use other_crate::EncoderBackend as Backend; }
        impl crate::encoder::Backend for SafeWriter { fn plan(&self) {} }
        impl crate::inspection::Backend for Inspector { fn plan(&self) { CodecError::WrongFormat("not encode reachable".into()); } }
    "#,
    );
    let roots: Vec<_> = graph
        .functions
        .iter()
        .filter(|function| graph.is_root(function))
        .collect();
    assert_eq!(roots.len(), 1);
    assert!(roots[0]
        .owner
        .as_ref()
        .expect("owner")
        .iter()
        .any(|name| name == "SafeWriter"));
    assert_eq!(graph.reachable().len(), 1);
    assert!(graph.findings().is_empty());
}

#[test]
fn a_call_through_a_type_parameter_dispatches_like_a_method_call() {
    let graph = fixture(
        r#"
        trait Scan { const WIDTH: usize; fn scan(); }
        impl Scan for Part { const WIDTH: usize = 4; fn scan() { CodecError::WrongFormat("dispatched".into()); } }
        fn read<P: Scan>() { P::scan(); consume(P::WIDTH); }
        fn consume(value: usize) { let _ = value; }
        impl EncoderBackend for Writer { fn plan(&self) { read::<Part>(); } }
    "#,
    );
    let found = graph.findings();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("CodecError construction") && found[0].contains("Part::scan"));
    assert!(graph.unresolved_symbols().is_empty());
}
