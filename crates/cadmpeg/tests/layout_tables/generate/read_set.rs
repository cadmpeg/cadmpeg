// SPDX-License-Identifier: Apache-2.0
//! Resolve layout reads from Rust tokens. Literals and comments are not paths.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use proc_macro2::{TokenStream, TokenTree};
use syn::UseTree;

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
fn scope(tokens: TokenStream, inherited: &Imports) -> Result<(Imports, Vec<TokenTree>), String> {
    let tokens: Vec<_> = tokens.into_iter().collect();
    let mut imports = inherited.clone();
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
            bind(&item.tree, &[], &mut imports);
            index = end;
        } else {
            body.push(tokens[index].clone());
            index += 1;
        }
    }
    Ok((imports, body))
}

fn scan(
    tokens: TokenStream,
    inherited: &Imports,
    reads: &mut BTreeSet<(String, String)>,
) -> Result<(), String> {
    let (imports, tokens) = scope(tokens, inherited)?;
    let mut index = 0;
    while index < tokens.len() {
        match &tokens[index] {
            TokenTree::Group(group) => scan(group.stream(), &imports, reads)?,
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
                let path = resolve(&path, &imports);
                if let [module, record, name] = path.as_slice() {
                    if module == "layout" {
                        reads.insert((record.clone(), name.clone()));
                    }
                }
            }
            _ => {}
        }
        index += 1;
    }
    Ok(())
}

fn sources(directory: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(directory).map_err(|error| error.to_string())?;
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            sources(&path, paths)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") && path != directory.join("layout.rs") {
            paths.push(path);
        }
    }
    Ok(())
}

/// Imports visible through parent modules, including separate test files.
fn parent_imports(path: &Path, root: &Path) -> Result<Imports, String> {
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
        let source = std::fs::read_to_string(&parent).map_err(|error| error.to_string())?;
        let tokens = source.parse::<TokenStream>().map_err(|error| error.to_string())?;
        imports = scope(tokens, &imports)?.0;
    }
    Ok(imports)
}

/// Derive the generated items read by the owning crate's source tree.
pub(super) fn layout_reads(root: &Path) -> Result<BTreeSet<(String, String)>, String> {
    let mut paths = Vec::new();
    sources(root, &mut paths)?;
    paths.sort();
    let mut reads = BTreeSet::new();
    for path in paths {
        let result = (|| {
            let source = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
            let tokens = source.parse::<TokenStream>().map_err(|error| error.to_string())?;
            scan(tokens, &parent_imports(&path, root)?, &mut reads)
        })();
        result.map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(reads)
}

#[cfg(test)]
mod tests {
    use super::{scan, Imports};
    use std::collections::BTreeSet;

    fn reads(source: &str) -> Result<BTreeSet<(String, String)>, String> {
        let mut reads = BTreeSet::new();
        scan(source.parse().map_err(|error: proc_macro2::LexError| error.to_string())?, &Imports::default(), &mut reads)?;
        Ok(reads)
    }

    fn expected(items: &[(&str, &str)]) -> BTreeSet<(String, String)> {
        items.iter().map(|(record, name)| (record.to_string(), name.to_string())).collect()
    }

    #[test]
    fn layout_reads_resolve_reference_forms() -> Result<(), String> {
        assert_eq!(reads("use crate::layout::header; header::LEN; use crate::layout::row as entry; entry::OFFSET;")?, expected(&[("header", "LEN"), ("row", "OFFSET")]));
        assert_eq!(reads("use crate::layout::{header, row as entry}; header::LEN; entry::OFFSET;")?, expected(&[("header", "LEN"), ("row", "OFFSET")]));
        assert_eq!(reads("crate::layout::header::LEN; super::layout::row::OFFSET; self::layout::token::TAG; layout::header::MAGIC_VALUE;")?, expected(&[("header", "LEN"), ("row", "OFFSET"), ("token", "TAG"), ("header", "MAGIC_VALUE")]));
        assert_eq!(reads("use crate::{layout::{header as h, row}}; h::LEN; row::OFFSET;")?, expected(&[("header", "LEN"), ("row", "OFFSET")]));
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
        assert_eq!(reads("use crate::layout::header as h; mod tests { use super::h; h::LEN; }")?, expected(&[("header", "LEN")]));
        Ok(())
    }
}
