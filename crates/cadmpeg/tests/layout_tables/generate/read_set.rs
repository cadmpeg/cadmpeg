// SPDX-License-Identifier: Apache-2.0
//! Resolve layout reads from Rust tokens. Literals and comments are not paths.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use proc_macro2::{Delimiter, Group, TokenStream, TokenTree};
use syn::UseTree;

/// Source reads in all builds and in builds without test-only items.
#[derive(Default)]
pub(super) struct Reads {
    all: BTreeSet<(String, String)>,
    production: BTreeSet<(String, String)>,
}

impl Reads {
    pub(super) fn contains(&self, record: &str, name: &str) -> bool {
        self.all.contains(&(record.to_string(), name.to_string()))
    }

    pub(super) fn test_only(&self, record: &str, name: &str) -> bool {
        self.contains(record, name)
            && !self
                .production
                .contains(&(record.to_string(), name.to_string()))
    }
}

fn requires_test(meta: &syn::Meta) -> bool {
    match meta {
        syn::Meta::Path(path) => path.is_ident("test"),
        syn::Meta::List(list) if list.path.is_ident("all") || list.path.is_ident("any") => {
            use syn::parse::Parser;
            let terms = syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
                .parse2(list.tokens.clone());
            terms.is_ok_and(|terms| {
                if list.path.is_ident("all") {
                    terms.iter().any(requires_test)
                } else {
                    !terms.is_empty() && terms.iter().all(requires_test)
                }
            })
        }
        _ => false,
    }
}

fn test_attribute(meta: &syn::Meta) -> bool {
    if meta.path().is_ident("test") {
        return true;
    }
    if let syn::Meta::List(list) = meta {
        return list.path.is_ident("cfg")
            && syn::parse2::<syn::Meta>(list.tokens.clone())
                .is_ok_and(|meta| requires_test(&meta));
    }
    false
}

/// Remove complete test-only items before deriving production reads.
fn production_tokens(stream: TokenStream) -> TokenStream {
    let tokens: Vec<_> = stream.into_iter().collect();
    let mut output = TokenStream::new();
    let mut index = 0;
    while index < tokens.len() {
        let start = index;
        let mut test_only = false;
        while let Some([hash, TokenTree::Group(attribute)]) = tokens.get(index..index + 2) {
            if !is_punct(hash, '#') || attribute.delimiter() != Delimiter::Bracket {
                break;
            }
            test_only |= syn::parse2::<syn::Meta>(attribute.stream())
                .is_ok_and(|meta| test_attribute(&meta));
            index += 2;
        }
        if test_only {
            while let Some(token) = tokens.get(index) {
                index += 1;
                if is_punct(token, ';')
                    || matches!(token, TokenTree::Group(group) if group.delimiter() == Delimiter::Brace)
                {
                    break;
                }
            }
            continue;
        }
        output.extend(tokens[start..index].iter().cloned());
        if let Some(token) = tokens.get(index) {
            output.extend([match token {
                TokenTree::Group(group) => TokenTree::Group(Group::new(
                    group.delimiter(),
                    production_tokens(group.stream()),
                )),
                token => token.clone(),
            }]);
            index += 1;
        }
    }
    output
}

fn test_modules(
    tokens: TokenStream,
    directory: &Path,
    inherited: bool,
    files: &mut BTreeSet<PathBuf>,
) -> Result<(), String> {
    // Only module declarations affect external test-file ownership. Parsing
    // function bodies and expressions into a full syntax tree repeats work
    // that the read scanner performs below.
    let mut tokens = tokens.into_iter().peekable();
    let mut test_only = inherited;
    while let Some(token) = tokens.next() {
        if is_punct(&token, '#') {
            if let Some(TokenTree::Group(attribute)) = tokens.peek() {
                if attribute.delimiter() == Delimiter::Bracket {
                    test_only |= syn::parse2::<syn::Meta>(attribute.stream())
                        .is_ok_and(|meta| test_attribute(&meta));
                    tokens.next();
                    continue;
                }
            }
        }
        if matches!(&token, TokenTree::Ident(ident) if ident == "pub") {
            if matches!(tokens.peek(), Some(TokenTree::Group(group)) if group.delimiter() == Delimiter::Parenthesis)
            {
                tokens.next();
            }
            continue;
        }
        if !matches!(&token, TokenTree::Ident(ident) if ident == "mod") {
            test_only = inherited;
            continue;
        }
        let Some(TokenTree::Ident(name)) = tokens.next() else {
            return Err("module declaration has no name".to_owned());
        };
        let child = directory.join(name.to_string());
        let body = tokens.next();
        if name != "layout" {
            if let Some(TokenTree::Group(group)) = body {
                if group.delimiter() != Delimiter::Brace {
                    return Err("inline module has no body".to_owned());
                }
                test_modules(group.stream(), &child, test_only, files)?;
            } else {
                let sibling = child.with_extension("rs");
                let path = if sibling.is_file() {
                    sibling
                } else {
                    child.join("mod.rs")
                };
                if path.is_file() {
                    if test_only {
                        files.insert(path.clone());
                    }
                    let source =
                        std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
                    let tokens = source
                        .parse::<TokenStream>()
                        .map_err(|error| format!("{}: {error}", path.display()))?;
                    test_modules(tokens, &child, test_only, files)?;
                }
            }
        }
        test_only = inherited;
    }
    Ok(())
}

#[derive(Clone, Default)]
struct Imports {
    paths: BTreeMap<String, Vec<String>>,
}

fn resolve(path: &[String], imports: &Imports) -> Vec<String> {
    let path = path
        .iter()
        .skip_while(|part| matches!(part.as_str(), "crate" | "self" | "super"))
        .cloned()
        .collect::<Vec<_>>();
    if let Some(bound) = path.first().and_then(|name| imports.paths.get(name)) {
        bound.iter().chain(path.iter().skip(1)).cloned().collect()
    } else {
        path
    }
}

fn bind(tree: &UseTree, prefix: &[String], imports: &mut Imports) {
    match tree {
        UseTree::Path(path) => {
            let mut prefix = prefix.to_vec();
            prefix.push(path.ident.to_string());
            bind(&path.tree, &prefix, imports);
        }
        UseTree::Name(name) => {
            let mut path = prefix.to_vec();
            if name.ident != "self" {
                path.push(name.ident.to_string());
            }
            if let Some(local) = path.last().cloned() {
                let resolved = resolve(&path, imports);
                imports.paths.insert(local, resolved);
            }
        }
        UseTree::Rename(rename) => {
            let mut path = prefix.to_vec();
            if rename.ident != "self" {
                path.push(rename.ident.to_string());
            }
            let resolved = resolve(&path, imports);
            imports.paths.insert(rename.rename.to_string(), resolved);
        }
        UseTree::Group(group) => {
            for tree in &group.items {
                bind(tree, prefix, imports);
            }
        }
        // Parent-module imports are inherited from the source module tree.
        UseTree::Glob(_) => {}
    }
}

fn is_punct(token: &TokenTree, character: char) -> bool {
    matches!(token, TokenTree::Punct(punct) if punct.as_char() == character)
}

/// Collect imports in this token scope and remove them from the read stream.
fn scope(
    tokens: TokenStream,
    inherited: &Imports,
) -> Result<(Cow<'_, Imports>, Vec<TokenTree>), String> {
    let tokens: Vec<_> = tokens.into_iter().collect();
    let mut imports = Cow::Borrowed(inherited);
    let mut body = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        if matches!(&tokens[index], TokenTree::Ident(ident) if ident == "use") {
            let end = tokens[index..]
                .iter()
                .position(|token| is_punct(token, ';'))
                .map(|offset| index + offset + 1)
                .ok_or_else(|| "use statement has no semicolon".to_string())?;
            let statement = tokens[index..end].iter().cloned().collect();
            let item: syn::ItemUse = syn::parse2(statement).map_err(|error| error.to_string())?;
            bind(&item.tree, &[], imports.to_mut());
            index = end;
        } else {
            body.push(tokens[index].clone());
            index += 1;
        }
    }
    Ok((imports, body))
}

fn scan<'a>(
    tokens: TokenStream,
    inherited: &'a Imports,
    reads: &mut BTreeSet<(String, String)>,
) -> Result<Cow<'a, Imports>, String> {
    let (imports, tokens) = scope(tokens, inherited)?;
    let mut index = 0;
    while index < tokens.len() {
        match &tokens[index] {
            TokenTree::Group(group) => {
                scan(group.stream(), &imports, reads)?;
            }
            TokenTree::Ident(ident) => {
                let mut path = vec![ident.to_string()];
                while let Some([colon_a, colon_b, TokenTree::Ident(next)]) =
                    tokens.get(index + 1..index + 4)
                {
                    if !is_punct(colon_a, ':') || !is_punct(colon_b, ':') {
                        break;
                    }
                    path.push(next.to_string());
                    index += 3;
                }
                let start = path
                    .iter()
                    .position(|part| !matches!(part.as_str(), "crate" | "self" | "super"))
                    .unwrap_or(path.len());
                let path = &path[start..];
                let (prefix, suffix) = path
                    .first()
                    .and_then(|first| imports.paths.get(first))
                    .map_or((path, &[][..]), |bound| (bound.as_slice(), &path[1..]));
                let mut parts = prefix.iter().chain(suffix.iter());
                if let (Some(module), Some(record), Some(name), None) =
                    (parts.next(), parts.next(), parts.next(), parts.next())
                {
                    if module == "layout" {
                        reads.insert((record.clone(), name.clone()));
                    }
                }
            }
            _ => {}
        }
        index += 1;
    }
    Ok(imports)
}

fn sources(directory: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(directory).map_err(|error| error.to_string())?;
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            sources(&path, paths)?;
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && path != directory.join("layout.rs")
        {
            paths.push(path);
        }
    }
    Ok(())
}

/// Imports visible through parent modules, including separate test files.
fn parent_imports(
    path: &Path,
    root: &Path,
    cache: &mut BTreeMap<PathBuf, Imports>,
) -> Result<Imports, String> {
    let mut parents = Vec::new();
    let mut directory = path.parent();
    while let Some(current) = directory {
        if current == root {
            parents.push(root.join("lib.rs"));
            break;
        }
        let sibling = current.with_extension("rs");
        let module = current.join("mod.rs");
        parents.push(if sibling.is_file() { sibling } else { module });
        directory = current.parent();
    }
    let mut imports = Imports::default();
    for parent in parents.into_iter().rev() {
        if parent == path || !parent.is_file() {
            continue;
        }
        if let Some(cached) = cache.get(&parent) {
            imports = cached.clone();
            continue;
        }
        let source = std::fs::read_to_string(&parent).map_err(|error| error.to_string())?;
        let tokens = source
            .parse::<TokenStream>()
            .map_err(|error| error.to_string())?;
        imports = scope(tokens, &imports)?.0.into_owned();
        cache.insert(parent, imports.clone());
    }
    Ok(imports)
}

/// Derive the generated items read by the owning crate's source tree.
pub(super) fn layout_reads(root: &Path) -> Result<Reads, String> {
    let mut paths = Vec::new();
    sources(root, &mut paths)?;
    paths.sort();
    let source = std::fs::read_to_string(root.join("lib.rs")).map_err(|error| error.to_string())?;
    let tokens = source
        .parse::<TokenStream>()
        .map_err(|error| error.to_string())?;
    let mut test_files = BTreeSet::new();
    test_modules(tokens, root, false, &mut test_files)?;
    let mut reads = Reads::default();
    // Each parent has one inherited import context within this source tree.
    // Save the imports found by its own scan so children need no second parse.
    let mut import_cache = BTreeMap::new();
    for path in paths {
        let result = (|| {
            let source = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
            let tokens = source
                .parse::<TokenStream>()
                .map_err(|error| error.to_string())?;
            let imports = parent_imports(&path, root, &mut import_cache)?;
            let own_imports = scan(tokens.clone(), &imports, &mut reads.all)?;
            import_cache.insert(path.clone(), own_imports.into_owned());
            if !test_files.contains(&path) {
                scan(production_tokens(tokens), &imports, &mut reads.production)?;
            }
            Ok::<(), String>(())
        })();
        result.map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(reads)
}

#[cfg(test)]
mod tests {
    use super::{layout_reads, production_tokens, scan, Imports};
    use std::collections::BTreeSet;

    fn reads(source: &str) -> Result<BTreeSet<(String, String)>, String> {
        let mut reads = BTreeSet::new();
        scan(
            source
                .parse()
                .map_err(|error: proc_macro2::LexError| error.to_string())?,
            &Imports::default(),
            &mut reads,
        )?;
        Ok(reads)
    }

    fn expected(items: &[(&str, &str)]) -> BTreeSet<(String, String)> {
        items
            .iter()
            .map(|(record, name)| (record.to_string(), name.to_string()))
            .collect()
    }

    #[test]
    fn layout_reads_separate_test_only_items() -> Result<(), String> {
        let source = "use crate::layout::header as h; h::LEN; #[cfg(test)] mod tests { h::MAGIC; } #[test] fn test() { h::OFFSET; } #[cfg(all(feature = \"x\", test))] const C: usize = h::VALUE;";
        let mut production = BTreeSet::new();
        scan(
            production_tokens(
                source
                    .parse()
                    .map_err(|error: proc_macro2::LexError| error.to_string())?,
            ),
            &Imports::default(),
            &mut production,
        )?;
        assert_eq!(production, expected(&[("header", "LEN")]));
        assert_eq!(
            reads(source)?,
            expected(&[
                ("header", "LEN"),
                ("header", "MAGIC"),
                ("header", "OFFSET"),
                ("header", "VALUE")
            ])
        );
        Ok(())
    }

    #[test]
    fn layout_reads_follow_external_test_modules_and_ignore_generated_source(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path();
        std::fs::create_dir(root.join("reader"))?;
        std::fs::write(root.join("lib.rs"), "mod reader; mod layout;")?;
        std::fs::write(
            root.join("reader.rs"),
            "use crate::layout::header as h; const C: usize = h::LEN; #[cfg(test)] mod checks;",
        )?;
        std::fs::write(
            root.join("reader/checks.rs"),
            "use super::h; const C: usize = h::MAGIC;",
        )?;
        std::fs::write(root.join("layout.rs"), "crate::layout::unread::OFFSET;")?;
        let reads = layout_reads(root)?;
        assert!(reads.contains("header", "LEN"));
        assert!(!reads.test_only("header", "LEN"));
        assert!(reads.test_only("header", "MAGIC"));
        assert!(!reads.contains("unread", "OFFSET"));
        Ok(())
    }

    #[test]
    fn external_test_modules_follow_inline_visibility_and_do_not_inherit_function_attributes(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path();
        std::fs::create_dir(root.join("reader"))?;
        std::fs::write(root.join("lib.rs"), "pub mod reader { #[cfg(test)] pub(crate) mod checks; #[test] fn test() {} pub(crate) mod production; }")?;
        std::fs::write(
            root.join("reader/checks.rs"),
            "crate::layout::header::MAGIC;",
        )?;
        std::fs::write(
            root.join("reader/production.rs"),
            "crate::layout::header::LEN;",
        )?;
        let reads = layout_reads(root)?;
        assert!(reads.test_only("header", "MAGIC"));
        assert!(reads.contains("header", "LEN"));
        assert!(!reads.test_only("header", "LEN"));
        Ok(())
    }

    #[test]
    fn parent_import_cache_keeps_sibling_aliases_and_rereads_new_invocations(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let temporary = tempfile::tempdir()?;
        let root = temporary.path();
        std::fs::write(root.join("lib.rs"), "mod first; mod second;")?;
        for (module, record) in [("first", "header"), ("second", "row")] {
            std::fs::create_dir(root.join(module))?;
            std::fs::write(
                root.join(format!("{module}.rs")),
                format!("use crate::layout::{record} as h; mod a; mod b;"),
            )?;
            for child in ["a", "b"] {
                std::fs::write(
                    root.join(module).join(format!("{child}.rs")),
                    "use super::h; const C: usize = h::LEN;",
                )?;
            }
        }
        let reads = layout_reads(root)?;
        assert!(reads.contains("header", "LEN"));
        assert!(reads.contains("row", "LEN"));
        std::fs::write(
            root.join("first.rs"),
            "use crate::layout::token as h; mod a; mod b;",
        )?;
        let reads = layout_reads(root)?;
        assert!(reads.contains("token", "LEN"));
        assert!(reads.contains("row", "LEN"));
        assert!(!reads.contains("header", "LEN"));
        Ok(())
    }

    #[test]
    fn layout_reads_resolve_reference_forms() -> Result<(), String> {
        assert_eq!(reads("use crate::layout::header; header::LEN; use crate::layout::row as entry; entry::OFFSET;")?, expected(&[("header", "LEN"), ("row", "OFFSET")]));
        assert_eq!(
            reads("use crate::layout::{header, row as entry}; header::LEN; entry::OFFSET;")?,
            expected(&[("header", "LEN"), ("row", "OFFSET")])
        );
        assert_eq!(reads("crate::layout::header::LEN; super::layout::row::OFFSET; self::layout::token::TAG; layout::header::MAGIC_VALUE;")?, expected(&[("header", "LEN"), ("row", "OFFSET"), ("token", "TAG"), ("header", "MAGIC_VALUE")]));
        assert_eq!(
            reads("use crate::{layout::{header as h, row}}; h::LEN; row::OFFSET;")?,
            expected(&[("header", "LEN"), ("row", "OFFSET")])
        );
        assert_eq!(reads("use crate::layout as offsets; offsets::header::LEN; use crate::layout::row::{OFFSET as AT}; AT;")?, expected(&[("header", "LEN"), ("row", "OFFSET")]));
        Ok(())
    }

    #[test]
    fn layout_reads_ignore_unread_records_and_literals() -> Result<(), String> {
        assert!(reads("use crate::layout::unread; // crate::layout::header::LEN\n \"crate::layout::row::OFFSET\"; r#\"layout::token::TAG\"#;")?.is_empty());
        Ok(())
    }

    #[test]
    fn layout_reads_keep_scoped_aliases_and_macro_paths() -> Result<(), String> {
        assert_eq!(reads("use crate::layout::header as h; fn f() { use crate::layout::row as h; m!(h::OFFSET); } h::LEN;")?, expected(&[("header", "LEN"), ("row", "OFFSET")]));
        assert_eq!(
            reads("use crate::layout::header as h; mod tests { use super::h; h::LEN; }")?,
            expected(&[("header", "LEN")])
        );
        Ok(())
    }
}
