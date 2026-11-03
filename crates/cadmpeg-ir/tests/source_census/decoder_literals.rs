// SPDX-License-Identifier: Apache-2.0
//! Decoder string-literal scan (check 9, #246 IV.2).
//!
//! No decoder compares against a string literal that occurs in a writer of
//! the same crate but in no base fixture. Decode-side files name reading
//! (`reader`, `decode`, `parse`, `lex`, `signature`, `container`,
//! `archive`); write-side files name emission (`writer`, `export`,
//! `encode`, `emitter`, `target`). A file matching neither side is outside
//! both vocabularies; a file matching both counts as write-side, so shared
//! emission helpers cannot hide a writer spelling.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use syn::visit::Visit;

const DECODE_MARKS: &[&str] = &[
    "reader",
    "decode",
    "parse",
    "lex",
    "signature",
    "container",
    "archive",
];
const WRITE_MARKS: &[&str] = &["writer", "export", "encode", "emitter", "target"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    Decode,
    Write,
}

fn side_of(path: &str) -> Option<Side> {
    let lower = path.to_ascii_lowercase();
    let hits = |marks: &[&str]| marks.iter().any(|mark| lower.contains(mark));
    if hits(WRITE_MARKS) {
        Some(Side::Write)
    } else if hits(DECODE_MARKS) {
        Some(Side::Decode)
    } else {
        None
    }
}

struct Compared {
    literals: BTreeSet<String>,
}

impl Compared {
    fn record(&mut self, literal: &str) {
        self.literals.insert(literal.to_owned());
    }

    fn test_gated(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|attr| {
            if attr.path().is_ident("test") {
                return true;
            }
            if !attr.path().is_ident("cfg") {
                return false;
            }
            match &attr.meta {
                syn::Meta::List(list) => list
                    .tokens
                    .to_string()
                    .split(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '-')
                    .any(|word| word == "test"),
                _ => false,
            }
        })
    }

    fn direct_strings(expression: &syn::Expr, found: &mut Vec<String>) {
        if let syn::Expr::Lit(literal) = expression {
            if let syn::Lit::Str(text) = &literal.lit {
                found.push(text.value());
            }
        }
    }

    fn subtree_strings(expression: &syn::Expr, found: &mut Vec<String>) {
        struct Strings(Vec<String>);
        impl<'ast> Visit<'ast> for Strings {
            fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
                self.0.push(literal.value());
            }
        }
        let mut strings = Strings(Vec::new());
        strings.visit_expr(expression);
        found.extend(strings.0);
    }
}

impl<'ast> Visit<'ast> for Compared {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if Self::test_gated(item_attrs(item)) {
            return;
        }
        syn::visit::visit_item(self, item);
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if Self::test_gated(&item.attrs) {
            return;
        }
        syn::visit::visit_item_fn(self, item);
    }

    fn visit_expr_binary(&mut self, binary: &'ast syn::ExprBinary) {
        if matches!(binary.op, syn::BinOp::Eq(_) | syn::BinOp::Ne(_)) {
            let mut found = Vec::new();
            Self::direct_strings(&binary.left, &mut found);
            Self::direct_strings(&binary.right, &mut found);
            for literal in found {
                self.record(&literal);
            }
        }
        syn::visit::visit_expr_binary(self, binary);
    }

    fn visit_macro(&mut self, value: &'ast syn::Macro) {
        let name = value
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_default();
        if matches!(name.as_str(), "matches" | "assert_eq" | "assert_ne") {
            if let Ok(arguments) = value.parse_body_with(
                syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated,
            ) {
                let mut found = Vec::new();
                let arguments: Vec<&syn::Expr> = if name == "matches" {
                    arguments.iter().skip(1).collect()
                } else {
                    arguments.iter().take(2).collect()
                };
                for argument in arguments {
                    Self::subtree_strings(argument, &mut found);
                }
                for literal in found {
                    self.record(&literal);
                }
            }
        } else {
            // Any other macro may hide a comparison in one argument, such as
            // `assert!(kind == "sheet", "message")`. Each comma-separated
            // group that parses as an expression is visited; a bare literal
            // argument is not a comparison, so messages stay out.
            for group in split_groups(&value.tokens) {
                if let Ok(argument) = syn::parse2::<syn::Expr>(group) {
                    self.visit_expr(&argument);
                }
            }
        }
        syn::visit::visit_macro(self, value);
    }

    fn visit_pat(&mut self, pattern: &'ast syn::Pat) {
        if let syn::Pat::Lit(literal) = pattern {
            if let syn::Lit::Str(text) = &literal.lit {
                self.record(&text.value());
            }
        }
        syn::visit::visit_pat(self, pattern);
    }
}

/// Attributes of any item for test gating.
fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(item) => &item.attrs,
        syn::Item::Enum(item) => &item.attrs,
        syn::Item::ExternCrate(item) => &item.attrs,
        syn::Item::Fn(item) => &item.attrs,
        syn::Item::ForeignMod(item) => &item.attrs,
        syn::Item::Impl(item) => &item.attrs,
        syn::Item::Macro(item) => &item.attrs,
        syn::Item::Mod(item) => &item.attrs,
        syn::Item::Static(item) => &item.attrs,
        syn::Item::Struct(item) => &item.attrs,
        syn::Item::Trait(item) => &item.attrs,
        syn::Item::TraitAlias(item) => &item.attrs,
        syn::Item::Type(item) => &item.attrs,
        syn::Item::Union(item) => &item.attrs,
        syn::Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

struct WriterStrings {
    literals: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for WriterStrings {
    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        self.literals.insert(literal.value());
    }

    fn visit_item(&mut self, item: &'ast syn::Item) {
        if Compared::test_gated(item_attrs(item)) {
            return;
        }
        syn::visit::visit_item(self, item);
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if Compared::test_gated(&item.attrs) {
            return;
        }
        syn::visit::visit_item_fn(self, item);
    }

    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        // Documentation text is not a code spelling.
        if !attribute.path().is_ident("doc") {
            syn::visit::visit_attribute(self, attribute);
        }
    }
}

/// Splits macro tokens at top-level commas for expression parsing.
/// Groups arrive as single token trees, so a comma punct is top-level.
fn split_groups(tokens: &proc_macro2::TokenStream) -> Vec<proc_macro2::TokenStream> {
    let mut groups = vec![proc_macro2::TokenStream::new()];
    for token in tokens.clone() {
        if matches!(&token, proc_macro2::TokenTree::Punct(mark) if mark.as_char() == ',') {
            groups.push(proc_macro2::TokenStream::new());
        } else {
            groups.last_mut().expect("group").extend([token]);
        }
    }
    groups
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn workspace_members(root: &Path) -> Vec<String> {
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("workspace manifest");
    let members = manifest
        .split_once("members = [")
        .expect("explicit workspace members")
        .1
        .split_once(']')
        .expect("workspace member array")
        .0;
    members
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| serde_json::from_str(line.trim_end_matches(',')).ok())
        .collect()
}

/// Test files live outside both vocabularies: `tests` directories and
/// modules, `*_tests` files and directories, and `test_only` trees.
/// `test_support` builders are production code and stay in.
fn test_path(relative: &str) -> bool {
    relative.split('/').any(|part| {
        let stem = part.strip_suffix(".rs").unwrap_or(part);
        stem == "tests" || stem.ends_with("_tests") || stem == "test_only"
    })
}

/// Whether any base fixture of the crate holds the literal: everything
/// under its `tests` tree, non-code files under its `src` tree (unit-test
/// data such as `reader/geometry/tests/data/*.p21`), and the public corpus.
fn fixture_hits(root: &Path, package: &str, literal: &str) -> bool {
    let needle = literal.as_bytes();
    if needle.is_empty() {
        return true;
    }
    let mut stack = vec![root.join(package).join("tests"), root.join(package).join("src")];
    if root.join("corpus").is_dir() {
        stack.push(root.join("corpus"));
    }
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for path in entries.filter_map(Result::ok).map(|entry| entry.path()) {
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            // Code is vocabulary, not fixture.
            if path.extension().is_some_and(|ext| ext == "rs") {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            if bytes.len() > 256 * 1024 * 1024 {
                continue;
            }
            if bytes.windows(needle.len()).any(|window| window == needle) {
                return true;
            }
        }
    }
    false
}

/// Findings over the workspace: decoder-compared literals that a same-crate
/// writer also spells while no base fixture holds them.
fn scan_workspace(root: &Path) -> Vec<String> {
    let mut findings = Vec::new();
    for member in workspace_members(root) {
        if !member.starts_with("crates/cadmpeg-codec-") {
            continue;
        }
        let mut files = Vec::new();
        rust_files(&root.join(&member).join("src"), &mut files);
        let mut compared: BTreeSet<String> = BTreeSet::new();
        let mut written = BTreeSet::new();
        for file in files {
            let relative = file
                .strip_prefix(root)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            if test_path(&relative) {
                continue;
            }
            let Some(side) = side_of(&relative) else {
                continue;
            };
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let Ok(parsed) = syn::parse_file(&text) else {
                continue;
            };
            match side {
                Side::Decode => {
                    let mut visitor = Compared {
                        literals: BTreeSet::new(),
                    };
                    visitor.visit_file(&parsed);
                    compared.extend(visitor.literals);
                    let _ = relative;
                }
                Side::Write => {
                    let mut visitor = WriterStrings {
                        literals: BTreeSet::new(),
                    };
                    visitor.visit_file(&parsed);
                    written.extend(visitor.literals);
                }
            }
        }
        for literal in &compared {
            if literal.len() < 2 {
                continue;
            }
            if written.contains(literal) && !fixture_hits(root, &member, literal) {
                findings.push(format!(
                    "{member}: decoder compares {literal:?}, its writer spells it, no base fixture holds it"
                ));
            }
        }
    }
    findings.sort();
    findings.dedup();
    findings
}

#[test]
fn decoder_literals_do_not_smuggle_writer_spellings() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_owned();
    let findings = scan_workspace(&root);
    assert!(
        findings.is_empty(),
        "{} decoder literal(s) name a writer spelling with no base fixture \
         (step literals are #233 fixture work in this unit; f3d and sldprt \
         literals belong to their writer lanes):\n{}",
        findings.len(),
        findings.join("\n")
    );
}

#[cfg(test)]
mod tests {
    use super::{side_of, Compared, Side, WriterStrings};
    use std::collections::BTreeSet;
    use syn::visit::Visit;

    fn compared(source: &str) -> Vec<String> {
        let parsed = syn::parse_file(source).expect("fixture parses");
        let mut visitor = Compared {
            literals: BTreeSet::new(),
        };
        visitor.visit_file(&parsed);
        visitor.literals.into_iter().collect()
    }

    fn written(source: &str) -> Vec<String> {
        let parsed = syn::parse_file(source).expect("fixture parses");
        let mut visitor = WriterStrings {
            literals: BTreeSet::new(),
        };
        visitor.visit_file(&parsed);
        visitor.literals.into_iter().collect()
    }

    #[test]
    fn equality_matches_and_patterns_count_but_messages_do_not() {
        let found = compared(
            r#"
            fn decode(kind: &str, value: Option<&str>) -> bool {
                if kind == "solid" { return true; }
                assert_eq!(kind, "sheet");
                let _ = matches!(value, Some("open"));
                match kind { "closed" => true, _ => false }
            }
            "#,
        );
        for wanted in ["solid", "sheet", "open", "closed"] {
            assert!(found.contains(&wanted.to_owned()), "{found:?}");
        }
        let found = compared(
            r#"fn decode(kind: &str) { assert!(kind == "a", "message"); }"#,
        );
        assert!(found.contains(&"a".to_owned()), "{found:?}");
        assert!(!found.contains(&"message".to_owned()), "{found:?}");
    }

    #[test]
    fn sides_split_on_reading_and_emission_names() {
        assert_eq!(
            side_of("crates/cadmpeg-codec-step/src/reader/topology.rs"),
            Some(Side::Decode)
        );
        assert_eq!(
            side_of("crates/cadmpeg-codec-step/src/writer/target.rs"),
            Some(Side::Write)
        );
        assert_eq!(side_of("crates/cadmpeg-codec-step/src/ids.rs"), None);
    }

    #[test]
    fn writer_strings_collect_every_literal() {
        assert!(written(r#"fn write() { emit("solid"); }"#).contains(&"solid".to_owned()));
    }

    #[test]
    fn test_gated_items_are_outside_both_vocabularies() {
        let parsed = syn::parse_file(
            r#"
            #[cfg(test)]
            mod tests {
                fn helper(kind: &str) -> bool { kind == "solid" }
            }
            #[test]
            fn unit() { let _ = "sheet"; }
            fn decode(kind: &str) -> bool { kind == "kept" }
            "#,
        )
        .expect("fixture parses");
        let mut decode = Compared {
            literals: BTreeSet::new(),
        };
        decode.visit_file(&parsed);
        assert!(!decode.literals.contains("solid"), "{:?}", decode.literals);
        assert!(!decode.literals.contains("sheet"), "{:?}", decode.literals);
        assert!(decode.literals.contains("kept"), "{:?}", decode.literals);
        let mut write = WriterStrings {
            literals: BTreeSet::new(),
        };
        write.visit_file(&parsed);
        assert!(!write.literals.contains("solid"), "{:?}", write.literals);
        assert!(write.literals.contains("kept"), "{:?}", write.literals);
    }
}
