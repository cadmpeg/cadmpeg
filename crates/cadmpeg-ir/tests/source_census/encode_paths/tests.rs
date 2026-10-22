// SPDX-License-Identifier: Apache-2.0
use super::source::fixture;

fn findings(source: &str) -> Vec<String> {
    fixture(source).findings()
}

#[test]
fn backend_roots_reach_free_associated_method_callback_and_recursive_calls() {
    for body in [
        "first()",
        "Helper::first()",
        "helper.first()",
        "values.map(first)",
    ] {
        let source = format!(
            r#"
            impl EncoderBackend for Writer {{ fn plan(&self) {{ {body}; }} }}
            fn first() {{ second(); }}
            impl Helper {{ fn first(&self) {{ second(); }} }}
            fn second() {{ third(); }}
            fn third() {{ second(); CodecError::InvalidInput("bad".into()); }}
        "#
        );
        let found = findings(&source);
        assert!(
            found
                .iter()
                .any(|finding| finding.contains("CodecError construction candidate")),
            "{body}: {found:?}"
        );
        assert!(found
            .iter()
            .all(|finding| finding.contains("Writer::plan") && finding.contains("via")));
        assert_eq!(
            found.len(),
            1,
            "cycles and multiple routes do not duplicate a source site"
        );
    }
}

#[test]
fn method_names_conservatively_include_each_implementation_but_unreachable_functions_do_not_count()
{
    let graph = fixture(
        r#"
        impl EncoderBackend for Writer { fn plan(&self) { receiver.emit(); } }
        impl First { fn emit(&self) { CodecError::WrongFormat("one".into()); } }
        impl Second { fn emit(&self) { CodecError::WrongFormat("two".into()); } }
        fn unreachable() { CodecError::WrongFormat("not reached".into()); }
    "#,
    );
    assert_eq!(graph.findings().len(), 2);
    assert!(!graph
        .reachable()
        .keys()
        .any(|&index| graph.functions[index].name == "unreachable"));
}

#[test]
fn test_only_items_are_removed_but_production_feature_branches_stay() {
    let graph = fixture(
        r#"
        #[cfg(test)] mod tests { impl EncoderBackend for TestWriter { fn plan(&self) { CodecError::WrongFormat("bad".into()); } } }
        #[cfg(all(test, feature="helpers"))] fn helper() { CodecError::WrongFormat("bad".into()); }
        #[test] fn helper_test() { CodecError::WrongFormat("bad".into()); }
        impl EncoderBackend for Writer { fn plan(&self) { helper(); helper_test(); feature(); } }
        #[cfg(any(test, feature="production"))] fn feature() { CodecError::WrongFormat("real".into()); }
    "#,
    );
    let found = graph.findings();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("feature"));
    assert!(!graph
        .functions
        .iter()
        .any(|function| function.name == "helper" || function.name == "helper_test"));
}

#[test]
fn aliases_self_constructors_and_imported_constructor_values_cannot_hide_sites() {
    for source in [
        "use cadmpeg_core::CodecError as Failure; impl EncoderBackend for W { fn plan(&self) { Failure::InvalidInput(\"bad\".into()); } }",
        "type Failure = Other; type Other = CodecError; impl EncoderBackend for W { fn plan(&self) { Failure::WrongFormat(\"bad\".into()); } }",
        "impl EncoderBackend for W { fn plan(&self) { use cadmpeg_core::CodecError::InvalidInput as Fail; values.map_err(Fail); } }",
        "impl EncoderBackend for W { fn plan(&self) { CodecError::make(); } } impl CodecError { fn make() { Self::InvalidInput(\"bad\".into()); } }",
        "use CodecError::*; impl EncoderBackend for W { fn plan(&self) { WrongFormat(\"bad\".into()); } }",
    ] { assert!(findings(source).iter().any(|finding| finding.contains("CodecError construction candidate")), "{source}"); }
}

#[test]
fn forbidden_spellings_io_manufacture_and_old_target_variants_are_syntax_not_text() {
    for expression in [
        "CodecError::NotImplemented(\"bad\".into())",
        "CodecError::UnsupportedContent(\"bad\".into())",
        "CodecError::Malformed(\"bad\".into())",
        "CodecError::malformed(\"bad\")",
        "CodecError::Truncated { location, operation }",
        "ResourceLimit { dimension, limit, used, additional, reason, scope }",
        "ResourceLimit::new()",
        "std::io::Error::new(kind, message)",
        "io::Error::other(message)",
        "std::io::Error::from(kind)",
        "std::io::ErrorKind::Other",
        "EncodeError::Io(error)",
        "TargetRefusalKind::ExplicitUnavailable",
        "TargetRefusalKind::InheritedUnavailable",
        "TargetRefusalKind::DefaultUnavailable",
        "TargetRefusalKind::UnrecordedSource",
    ] {
        assert!(
            !findings(&format!(
                "impl EncoderBackend for W {{ fn plan(&self) {{ let _ = {expression}; }} }}"
            ))
            .is_empty(),
            "{expression}"
        );
    }
    assert!(findings(r#"impl EncoderBackend for W { fn plan(&self) { let text="CodecError::Malformed, io::Error::other"; /* ResourceLimit::new() */ } }"#).is_empty());
    assert!(!findings("use std::io::Error as IoFault; impl EncoderBackend for W { fn plan(&self) { IoFault::other(message); } }").is_empty());
}

#[test]
fn macros_and_nested_functions_remain_in_the_graph_and_unused_bodies_do_not() {
    let source = r#"
        macro_rules! fail { () => { CodecError::WrongFormat("bad".into()) }; }
        macro_rules! unused { () => { CodecError::WrongFormat("unused".into()) }; }
        impl EncoderBackend for W {
            fn plan(&self) { fn nested() { fail!(); } nested(); }
        }
    "#;
    let graph = fixture(source);
    assert_eq!(graph.findings().len(), 1);
    assert!(!graph
        .reachable()
        .keys()
        .any(|&index| graph.functions[index].name == "unused"));
    assert!(!findings("impl EncoderBackend for W { fn plan(&self) { wrap!(CodecError::WrongFormat(\"bad\".into())); } }").is_empty());
}

#[test]
fn only_the_declared_capacity_helper_may_construct_target_capacity() {
    let source = r#"
        const CAPACITY_FIELDS: &[CapacityField] = &[CapacityField { field:"count", limit:7, source:"three bits", helper:"capacity::count" }];
        impl EncoderBackend for W { fn plan(&self) { capacity::count(); } }
        mod capacity { fn count() { EncodeError::TargetCapacity { field, limit, value }; } }
    "#;
    assert!(findings(source).is_empty());
    assert!(
        !findings(&source.replace("helper:\"capacity::count\"", "helper:\"other::count\""))
            .is_empty()
    );
    assert!(!findings("impl EncoderBackend for W { fn plan(&self) { EncodeError::TargetCapacity { field, limit, value }; } }").is_empty());
    let graph = fixture("impl EncoderBackend for W { fn plan(&self) {} } impl From<CodecError> for EncodeError { fn from(error:CodecError)->Self { todo!() } }");
    assert!(graph
        .findings()
        .iter()
        .any(|finding| finding.contains("From<CodecError> for EncodeError")));
}

#[test]
fn typed_error_wrapping_is_not_a_manufactured_resource_finding_and_catches_are_not_claimed_proved()
{
    assert!(findings("impl EncoderBackend for W { fn plan(&self) { EncodeError::InvalidInput(proof); EncodeError::ResourceLimit(session_finding); output.write_all(bytes)?; output.seek(position)?; } }").is_empty());
    let found = findings("impl EncoderBackend for W { fn plan(&self) { if let Ok(value) = decode_fidelity() { return value; } synthesize(); } } fn decode_fidelity() { CodecError::WrongFormat(\"bad\".into()); }");
    assert_eq!(found.len(), 1);
    assert!(
        found[0].contains("catch boundary not proved"),
        "a lexical catch does not prove synthesis-only error consumption"
    );
}

mod precision;
