use quote::ToTokens;
use std::{fs, path::Path};
use syn::{Attribute, Block, ImplItem, Item, Stmt, TraitItem};

fn is_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        let value = a.meta.to_token_stream().to_string().replace(' ', "");
        value == "test" || value == "cfg(test)"
    })
}
fn emit(path: &Path, context: &str, name: &syn::Ident, block: &Block) {
    print!("\x1e{}\t{}\t{}\t{}", path.display(), context, name, block.to_token_stream());
    for stmt in &block.stmts {
        if let Stmt::Item(item) = stmt { walk(path, &name.to_string(), item); }
    }
}
fn walk(path: &Path, context: &str, item: &Item) {
    match item {
        Item::Fn(value) if !is_test(&value.attrs) => emit(path, context, &value.sig.ident, &value.block),
        Item::Mod(value) if !is_test(&value.attrs) && !["tests", "test_support", "test_only", "golden_tests", "integration_tests"].contains(&value.ident.to_string().as_str()) => {
            if let Some((_, items)) = &value.content { for item in items { walk(path, &value.ident.to_string(), item); } }
        }
        Item::Impl(value) if !is_test(&value.attrs) => {
            let context = format!("impl {}", value.self_ty.to_token_stream());
            for member in &value.items {
                if let ImplItem::Fn(method) = member {
                    if !is_test(&method.attrs) { emit(path, &context, &method.sig.ident, &method.block); }
                }
            }
        }
        Item::Trait(value) if !is_test(&value.attrs) => {
            for member in &value.items {
                if let TraitItem::Fn(method) = member {
                    if !is_test(&method.attrs) {
                        if let Some(block) = &method.default { emit(path, &value.ident.to_string(), &method.sig.ident, block); }
                    }
                }
            }
        }
        _ => {}
    }
}
fn directory(path: &Path) {
    let mut files = fs::read_dir(path).unwrap().map(|v| v.unwrap().path()).collect::<Vec<_>>();
    files.sort();
    for path in files {
        let name = path.file_stem().unwrap().to_str().unwrap();
        if ["tests", "test_support", "test_only", "golden_tests", "integration_tests"].contains(&name) { continue; }
        if path.is_dir() { directory(&path); }
        else if path.extension().is_some_and(|v| v == "rs") {
            let source = fs::read_to_string(&path).unwrap();
            match syn::parse_file(&source) {
                Ok(file) => for item in &file.items { walk(&path, "", item); },
                Err(error) => panic!("{}: {}", path.display(), error),
            }
        }
    }
}
fn main() { directory(Path::new(&std::env::args().nth(1).unwrap())); }
