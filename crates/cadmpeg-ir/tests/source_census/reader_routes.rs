// SPDX-License-Identifier: Apache-2.0
/// Semantic route checks for hand-written source readers.
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::{
    collect_rust_sources, deserialize_impl_target, flatten_tokens, is_test_module, is_test_path,
    macro_body_impl_targets, skip_meta_value, HAND_IMPLS,
};

/// One route shape a hand-written reader is allowed to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum HandReaderClass {
    /// A closed object adapter whose wire declaration refuses unknown keys.
    Wire,
    /// A scalar, byte string, fixed array, or sequence adapter.
    Keyless,
    /// An intentionally open map or canonical JSON value adapter.
    FreeForm,
    /// A general JSON value adapter with an explicit version gate.
    ValidatedValue,
}

impl HandReaderClass {
    /// The spelling used by the source census table and its diagnostics.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Wire => "wire",
            Self::Keyless => "keyless",
            Self::FreeForm => "free-form",
            Self::ValidatedValue => "validated-value",
        }
    }
}

/// One source span that emits a hand-written reader.
#[derive(Debug)]
struct HandImplSource {
    path: String,
    name: String,
    body: String,
    tokens: Vec<String>,
}

/// Return the elements in `left` that do not have a matching occurrence in
/// `right`, preserving duplicate entries. A set loses the two generic
/// `Provenance` readers and the two `$name` macro readers, which is precisely
/// the traversal gap this census must catch.
pub(super) fn multiset_difference<T: Clone + Ord>(left: &[T], right: &[T]) -> Vec<T> {
    let mut available = BTreeMap::<T, usize>::new();
    for item in right {
        *available.entry(item.clone()).or_default() += 1;
    }
    let mut difference = Vec::new();
    for item in left {
        let Some(count) = available.get_mut(item) else {
            difference.push(item.clone());
            continue;
        };
        *count -= 1;
        if *count == 0 {
            available.remove(item);
        }
    }
    difference
}

/// Whether one source attribute states a bare serde flag.
fn serde_has_flag(attrs: &[syn::Attribute], name: &str) -> bool {
    let mut stated = false;
    for attribute in attrs {
        if !attribute.path().is_ident("serde") {
            continue;
        }
        attribute
            .parse_nested_meta(|meta| {
                if meta.path.is_ident(name) {
                    stated = true;
                }
                skip_meta_value(&meta)?;
                Ok(())
            })
            .expect("every serde attribute in the wire crates parses as a nested meta list");
    }
    stated
}

/// Collect names of object declarations that reject unknown keys.
///
/// Ordinary declarations are read as syntax. Macro bodies cannot be parsed as
/// complete Rust items until expansion, so their flattened token stream is
/// read only far enough to bind each `deny_unknown_fields` attribute to its
/// following `struct` or `enum` declaration. This includes the generated
/// `ModelReadWire` and native-record wire declarations.
fn denied_wire_types(root: &Path) -> BTreeSet<String> {
    fn collect(items: &[syn::Item], found: &mut BTreeSet<String>) {
        for item in items {
            match item {
                syn::Item::Struct(declaration)
                    if serde_has_flag(&declaration.attrs, "deny_unknown_fields") =>
                {
                    found.insert(declaration.ident.to_string());
                }
                syn::Item::Enum(declaration)
                    if serde_has_flag(&declaration.attrs, "deny_unknown_fields") =>
                {
                    found.insert(declaration.ident.to_string());
                }
                syn::Item::Mod(module) => {
                    if is_test_module(module) {
                        continue;
                    }
                    if let Some((_, nested)) = &module.content {
                        collect(nested, found);
                    }
                }
                syn::Item::Macro(macro_item) => {
                    let mut flat = Vec::new();
                    flatten_tokens(&macro_item.mac.tokens, &mut flat);
                    for (index, token) in flat.iter().enumerate() {
                        if token != "deny_unknown_fields" {
                            continue;
                        }
                        let Some(relative) = flat
                            .iter()
                            .skip(index + 1)
                            .position(|candidate| candidate == "struct" || candidate == "enum")
                        else {
                            continue;
                        };
                        let declaration = index + 1 + relative;
                        let Some(name) = flat.get(declaration + 1) else {
                            continue;
                        };
                        if name == "$" {
                            if let Some(metavariable) = flat.get(declaration + 2) {
                                found.insert(format!("${metavariable}"));
                            }
                        } else {
                            found.insert(name.clone());
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let mut found = BTreeSet::new();
    for source in [
        "crates/cadmpeg-ir/src",
        "crates/cadmpeg-core/src",
        "crates/cadmpeg-asm/src",
    ] {
        let mut files = Vec::new();
        collect_rust_sources(&root.join(source), &mut files)
            .unwrap_or_else(|error| panic!("cannot collect {source}: {error}"));
        files.sort();
        for file in files {
            let relative = file
                .strip_prefix(root)
                .expect("a collected source sits under the repository root")
                .to_string_lossy()
                .replace('\\', "/");
            if is_test_path(&relative) {
                continue;
            }
            let text = std::fs::read_to_string(&file).expect("read source");
            let parsed = syn::parse_file(&text)
                .map_err(|error| format!("{relative} does not parse: {error}"))
                .expect("every source file in the wire crates parses");
            collect(&parsed.items, &mut found);
        }
    }
    found
}

/// Extract the source lines covered by a syntax item.
fn source_span_text(source: &str, span: proc_macro2::Span) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let start = span.start().line.saturating_sub(1);
    let end = span.end().line.min(lines.len());
    lines
        .get(start..end)
        .map_or_else(String::new, |lines| lines.join("\n"))
}

/// Tokenize one source span while retaining the original body for diagnostics.
fn tokenize_source_body(path: &str, name: &str, body: &str) -> Vec<String> {
    let token_stream = body
        .parse::<proc_macro2::TokenStream>()
        .unwrap_or_else(|error| panic!("{path} {name} implementation does not tokenize: {error}"));
    let mut tokens = Vec::new();
    flatten_tokens(&token_stream, &mut tokens);
    tokens
}

/// Collect every implementation or macro expansion that emits a reader.
fn collect_hand_impl_sources(
    items: &[syn::Item],
    relative: &str,
    source: &str,
    found: &mut Vec<HandImplSource>,
) {
    for item in items {
        match item {
            syn::Item::Impl(implementation) => {
                if let Some(name) = deserialize_impl_target(implementation) {
                    let body =
                        source_span_text(source, syn::spanned::Spanned::span(implementation));
                    let tokens = tokenize_source_body(relative, &name, &body);
                    found.push(HandImplSource {
                        path: relative.to_owned(),
                        name,
                        body,
                        tokens,
                    });
                }
            }
            syn::Item::Mod(module) => {
                if is_test_module(module) {
                    continue;
                }
                if let Some((_, nested)) = &module.content {
                    collect_hand_impl_sources(nested, relative, source, found);
                }
            }
            syn::Item::Macro(macro_item) => {
                let body = source_span_text(source, syn::spanned::Spanned::span(macro_item));
                let mut tokens = Vec::new();
                flatten_tokens(&macro_item.mac.tokens, &mut tokens);
                for name in macro_body_impl_targets(&macro_item.mac.tokens) {
                    found.push(HandImplSource {
                        path: relative.to_owned(),
                        name,
                        body: body.clone(),
                        tokens: tokens.clone(),
                    });
                }
            }
            _ => {}
        }
    }
}

/// Collects the source for every hand-written reader in the three wire crates.
fn hand_written_impl_sources(root: &Path) -> Vec<HandImplSource> {
    let mut found = Vec::new();
    for source in [
        "crates/cadmpeg-ir/src",
        "crates/cadmpeg-core/src",
        "crates/cadmpeg-asm/src",
    ] {
        let mut files = Vec::new();
        collect_rust_sources(&root.join(source), &mut files)
            .unwrap_or_else(|error| panic!("cannot collect {source}: {error}"));
        files.sort();
        for file in files {
            let relative = file
                .strip_prefix(root)
                .expect("a collected source sits under the repository root")
                .to_string_lossy()
                .replace('\\', "/");
            if is_test_path(&relative) {
                continue;
            }
            let text = std::fs::read_to_string(&file).expect("read source");
            let parsed = syn::parse_file(&text)
                .map_err(|error| format!("{relative} does not parse: {error}"))
                .expect("every source file in the wire crates parses");
            collect_hand_impl_sources(&parsed.items, &relative, &text, &mut found);
        }
    }
    found
}

/// Whether `tokens` contains `sequence` as an exact token sequence.
fn contains_token_sequence(tokens: &[String], sequence: &[&str]) -> bool {
    tokens.windows(sequence.len()).any(|window| {
        window
            .iter()
            .zip(sequence)
            .all(|(actual, expected)| actual == expected)
    })
}

/// Whether `tokens` contains one exact token.
fn contains_token(tokens: &[String], wanted: &str) -> bool {
    tokens.iter().any(|token| token == wanted)
}

/// Return whether a tokenized source body names one of the closed wire
/// targets. Exact tokens prevent comments and string literals from becoming a
/// false proof of a closed reader.
fn closed_wire_target(
    tokens: &[String],
    denied_types: &BTreeSet<String>,
    locally_closed: &BTreeSet<String>,
) -> Option<String> {
    denied_types
        .iter()
        .chain(locally_closed.iter())
        // `Wire` is used for several function-local adapters. Its name alone
        // cannot prove that this route's own declaration is closed; the local
        // source marker is checked separately above.
        .filter(|name| name.as_str() != "Wire" && name.as_str() != "$wire")
        .find(|name| {
            contains_token_sequence(tokens, &[name, ":", ":", "deserialize"])
                || (contains_token_sequence(tokens, &[name, ":", ":", "<"])
                    && contains_token_sequence(tokens, &[">", ":", ":", "deserialize"]))
        })
        .cloned()
}

/// Classify one reader body after tokenizing it, for focused fixture tests.
fn classify_hand_reader(
    path: &str,
    name: &str,
    body: &str,
    denied_types: &BTreeSet<String>,
    locally_closed: &BTreeSet<String>,
) -> Result<HandReaderClass, String> {
    let tokens = tokenize_source_body(path, name, body);
    classify_hand_reader_tokens(path, name, body, &tokens, denied_types, locally_closed)
}

/// Classify one tokenized reader body and prove the object route is closed.
fn classify_hand_reader_tokens(
    path: &str,
    name: &str,
    body: &str,
    tokens: &[String],
    denied_types: &BTreeSet<String>,
    locally_closed: &BTreeSet<String>,
) -> Result<HandReaderClass, String> {
    if contains_token_sequence(tokens, &["distinct_keys", ":", ":", "json_object"])
        || contains_token_sequence(tokens, &["distinct_keys", ":", ":", "btree_map"])
        || contains_token_sequence(tokens, &["deserialize_any", "JsonValueVisitor"])
    {
        return Ok(HandReaderClass::FreeForm);
    }
    if contains_token_sequence(
        tokens,
        &["serde_json", ":", ":", "Value", ":", ":", "deserialize"],
    ) && contains_token(tokens, "check_ir_version")
    {
        return Ok(HandReaderClass::ValidatedValue);
    }
    if [
        &["String", ":", ":", "deserialize"][..],
        &["f64", ":", ":", "deserialize"][..],
        &["i64", ":", ":", "deserialize"][..],
        &["u32", ":", ":", "deserialize"][..],
        &["Vec", ":", ":", "deserialize"][..],
        &["Vec", ":", ":", "<"][..],
        &["Box", ":", ":", "<"][..],
        // `flatten_tokens` descends into the array delimiter group, so its
        // opener is absent from the flattened sequence.
        &["<", "f64", ";"][..],
        &["$", "raw", ":", ":", "deserialize"][..],
        &["crate", ":", ":", "bytes", ":", ":", "deserialize"][..],
        &["deserialize_named"][..],
    ]
    .iter()
    .any(|sequence| contains_token_sequence(tokens, sequence))
    {
        return Ok(HandReaderClass::Keyless);
    }
    if contains_token_sequence(tokens, &[":", ":", "deserialize"]) {
        if contains_token(tokens, "deny_unknown_fields")
            || closed_wire_target(tokens, denied_types, locally_closed).is_some()
        {
            return Ok(HandReaderClass::Wire);
        }
        return Err(format!(
            "{path} {name} reads an object route without a denied wire target: {body}"
        ));
    }
    Err(format!(
        "{path} {name} has no recognized Deserialize route: {body}"
    ))
}

/// Compare each listed class to the route found in the implementation body.
///
/// This is the executable contract for the table: changing `NativeRecord`
/// from `free-form` to `wire`, removing a wire's denial attribute, or adding a
/// new reader shape makes this test fail at the changed source location.
pub(super) fn assert_hand_written_reader_routes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the repository root sits two levels above the crate manifest")
        .to_path_buf();
    let sources = hand_written_impl_sources(&root);
    assert!(
        !sources.is_empty(),
        "the route census found no reader bodies"
    );
    let denied_types = denied_wire_types(&root);
    assert!(
        !denied_types.is_empty(),
        "the route census found no denied wire declarations"
    );
    let locally_closed: BTreeSet<String> = sources
        .iter()
        .filter(|source| contains_token(&source.tokens, "deny_unknown_fields"))
        .map(|source| source.name.clone())
        .collect();
    let mut found = Vec::new();
    for source in sources {
        let class = classify_hand_reader_tokens(
            &source.path,
            &source.name,
            &source.body,
            &source.tokens,
            &denied_types,
            &locally_closed,
        )
        .unwrap_or_else(|reason| panic!("{reason}"));
        found.push((source.path, source.name, class.as_str().to_owned()));
    }
    let mut listed: Vec<(String, String, String)> = HAND_IMPLS
        .iter()
        .map(|(path, name, class)| ((*path).to_owned(), (*name).to_owned(), (*class).to_owned()))
        .collect();
    found.sort();
    listed.sort();
    assert_eq!(
        found, listed,
        "hand-written reader route classes disagree with HAND_IMPLS"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            classify_hand_reader(
                "fixture.rs",
                "Open",
                "OpenWire::deserialize(deserializer)?",
                &denied,
                &no_local_wire,
            ),
            Err("fixture.rs Open reads an object route without a denied wire target: OpenWire::deserialize(deserializer)?".to_owned())
        );
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
}
