// SPDX-License-Identifier: Apache-2.0
//! Every golden decoder audits its actual document before snapshot projection.

use std::path::{Path, PathBuf};

use proc_macro2::{TokenStream, TokenTree};
use syn::visit::Visit;

const DECODE: &[&str] = &["cadmpeg_test_support", "golden", "decode"];
const ROUTES: &[(&str, usize)] = &[
    ("crates/cadmpeg-codec-catia/src/golden_tests.rs", 2),
    ("crates/cadmpeg-codec-creo/src/golden_tests.rs", 1),
    ("crates/cadmpeg-codec-f3d/src/golden_tests.rs", 3),
    ("crates/cadmpeg-codec-f3d/src/golden_tests/damaged.rs", 0),
    (
        "crates/cadmpeg-codec-f3d/src/golden_tests/source_representation.rs",
        0,
    ),
    ("crates/cadmpeg-codec-freecad/src/golden_tests.rs", 3),
    ("crates/cadmpeg-codec-iges/src/golden_tests.rs", 2),
    ("crates/cadmpeg-codec-inventor/src/golden_tests.rs", 2),
    ("crates/cadmpeg-codec-nx/src/golden_tests.rs", 3),
    ("crates/cadmpeg-codec-rhino/src/golden_tests.rs", 2),
    ("crates/cadmpeg-codec-rhino/src/golden_tests/damaged.rs", 0),
    ("crates/cadmpeg-codec-sat/src/golden_tests.rs", 2),
    ("crates/cadmpeg-codec-sldprt/src/golden_tests.rs", 6),
    ("crates/cadmpeg-codec-step/src/golden_tests.rs", 2),
];

#[derive(Default)]
struct Routes {
    checked: usize,
    owned_golden: bool,
    unaudited: Vec<String>,
}

fn is_decode(name: &str) -> bool {
    matches!(name, "decode" | "decode_with_context" | "decode_impl")
}

fn path_is(path: &syn::Path, expected: &[&str]) -> bool {
    path.leading_colon.is_none()
        && path.segments.len() == expected.len()
        && path
            .segments
            .iter()
            .zip(expected)
            .all(|(segment, expected)| segment.ident == *expected)
}

impl<'ast> Visit<'ast> for Routes {
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if is_decode(&call.method.to_string()) {
            self.unaudited.push(call.method.to_string());
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(function) = &*call.func {
            if path_is(&function.path, DECODE)
                || (self.owned_golden && path_is(&function.path, &["crate", "golden", "decode"]))
            {
                self.checked += 1;
            } else if function
                .path
                .segments
                .last()
                .is_some_and(|segment| is_decode(&segment.ident.to_string()))
            {
                self.unaudited.push("direct decode function".into());
            }
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_macro(&mut self, call: &'ast syn::Macro) {
        self.macro_tokens(call.tokens.clone());
    }
}

impl Routes {
    fn macro_tokens(&mut self, tokens: TokenStream) {
        let tokens: Vec<_> = tokens.into_iter().collect();
        for (index, token) in tokens.iter().enumerate() {
            if let TokenTree::Group(group) = token {
                self.macro_tokens(group.stream());
            }
            let TokenTree::Ident(name) = token else {
                continue;
            };
            if !is_decode(&name.to_string()) {
                continue;
            }
            let canonical = index >= 6
                && tokens[index - 6..=index]
                    .iter()
                    .map(ToString::to_string)
                    .eq([
                        "cadmpeg_test_support",
                        ":",
                        ":",
                        "golden",
                        ":",
                        ":",
                        "decode",
                    ]
                    .into_iter()
                    .map(str::to_owned));
            let qualified = (index > 0
                && matches!(&tokens[index - 1], TokenTree::Punct(punct) if punct.as_char() == '.'))
                || (index >= 2
                    && tokens[index - 2..index].iter().all(
                        |token| matches!(token, TokenTree::Punct(punct) if punct.as_char() == ':'),
                    ));
            let called = matches!(tokens.get(index + 1), Some(TokenTree::Group(group)) if group.delimiter() == proc_macro2::Delimiter::Parenthesis);
            if canonical {
                self.checked += 1;
            } else if qualified || called {
                self.unaudited.push("decode inside macro".into());
            }
        }
    }
}

fn rust_files(dir: &Path, output: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read golden source directory") {
        let path = entry.expect("golden source entry").path();
        if path.is_dir() {
            rust_files(&path, output);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            output.push(path);
        }
    }
}

#[test]
fn every_golden_has_inward_native_links_and_no_native_backlinks() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates")
        .parent()
        .expect("workspace");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root.join("crates")).expect("crate inventory") {
        let path = entry.expect("crate entry").path();
        if !path
            .file_name()
            .expect("crate name")
            .to_string_lossy()
            .starts_with("cadmpeg-codec-")
        {
            continue;
        }
        let source = path.join("src/golden_tests.rs");
        if source.is_file() {
            files.push(source);
        }
        let children = path.join("src/golden_tests");
        if children.is_dir() {
            rust_files(&children, &mut files);
        }
    }
    files.sort_by(|left, right| left.as_os_str().cmp(right.as_os_str()));
    let actual: Vec<_> = files
        .iter()
        .map(|path| {
            path.strip_prefix(root)
                .expect("workspace source")
                .to_str()
                .expect("UTF8 source path")
        })
        .collect();
    assert_eq!(
        actual,
        ROUTES.iter().map(|(path, _)| *path).collect::<Vec<_>>(),
        "every golden source must have a reviewed route census"
    );
    for ((expected, count), path) in ROUTES.iter().zip(files) {
        let source = std::fs::read_to_string(path).expect("read golden source");
        let file = syn::parse_file(&source).expect("parse golden source");
        let mut routes = Routes::default();
        routes.visit_file(&file);
        assert!(
            routes.unaudited.is_empty(),
            "{expected}: {:?}",
            routes.unaudited
        );
        assert_eq!(
            routes.checked, *count,
            "{expected}: every existing decode stays routed through the live audit"
        );
    }
    for (file, count) in [("depth.rs", 1), ("roundtrip.rs", 3)] {
        let helper = syn::parse_file(
            &std::fs::read_to_string(root.join("crates/cadmpeg-test-support/src").join(file))
                .expect("shared golden helper"),
        )
        .expect("parse shared helper");
        let mut routes = Routes {
            owned_golden: true,
            ..Routes::default()
        };
        routes.visit_file(&helper);
        assert!(
            routes.unaudited.is_empty(),
            "{file}: every decoded document must be audited"
        );
        assert_eq!(
            routes.checked, count,
            "{file}: existing decode sites stay on the owning crate's checked route"
        );
    }
}

#[test]
fn golden_route_census_refuses_method_associated_and_macro_bypasses() {
    for source in [
        "fn f(){ codec.decode(reader, options); }",
        "fn f(){ Codec::decode(&codec, reader, options); }",
        "fn f(){ codec.decode_with_context(ctx, root); }",
        "fn f(){ codec.decode_impl(ctx, root); }",
        "fn f(){ assert!(codec.decode(reader, options).is_ok()); }",
        "fn f(){ make!({ Codec::decode(&codec, reader, options) }); }",
    ] {
        let mut routes = Routes::default();
        routes.visit_file(&syn::parse_file(source).expect("Rust route control"));
        assert!(!routes.unaudited.is_empty(), "bypass accepted: {source}");
    }
    let mut routes = Routes::default();
    routes.visit_file(
        &syn::parse_file(
            "fn f(){ cadmpeg_test_support::golden::decode(&codec, reader, options); }",
        )
        .expect("canonical route"),
    );
    assert_eq!(routes.checked, 1);
    assert!(routes.unaudited.is_empty());
    let mut routes = Routes::default();
    routes.visit_file(
        &syn::parse_file("fn f(){ json!({\"decode\": decode}); }")
            .expect("ordinary decode variable"),
    );
    assert!(
        routes.unaudited.is_empty(),
        "a snapshot field value is not a decode call"
    );
}

fn named(expr: &syn::Expr, name: &str) -> bool {
    matches!(expr, syn::Expr::Path(path) if path.qself.is_none() && path.path.is_ident(name))
}

fn audits_before_return(function: &syn::ItemFn) -> bool {
    let [syn::Stmt::Local(local), syn::Stmt::Expr(audit, Some(_)), syn::Stmt::Expr(result, None)] =
        function.block.stmts.as_slice()
    else {
        return false;
    };
    let syn::Pat::Ident(binding) = &local.pat else {
        return false;
    };
    if binding.ident != "decoded" {
        return false;
    }
    let Some(initializer) = &local.init else {
        return false;
    };
    let syn::Expr::Try(tried) = &*initializer.expr else {
        return false;
    };
    let syn::Expr::MethodCall(decode) = &*tried.expr else {
        return false;
    };
    if decode.method != "decode"
        || !named(&decode.receiver, "codec")
        || decode.args.len() != 2
        || !named(&decode.args[0], "reader")
        || !named(&decode.args[1], "options")
    {
        return false;
    }
    let syn::Expr::Call(audit) = audit else {
        return false;
    };
    let syn::Expr::Path(function) = &*audit.func else {
        return false;
    };
    if !path_is(
        &function.path,
        &["crate", "native_linkage", "assert_native_linkage"],
    ) || audit.args.len() != 1
    {
        return false;
    }
    let syn::Expr::MethodCall(document) = &audit.args[0] else {
        return false;
    };
    if document.method != "ir" || !document.args.is_empty() || !named(&document.receiver, "decoded")
    {
        return false;
    }
    let syn::Expr::Call(result) = result else {
        return false;
    };
    let syn::Expr::Path(ok) = &*result.func else {
        return false;
    };
    ok.path.is_ident("Ok") && result.args.len() == 1 && named(&result.args[0], "decoded")
}

#[test]
fn shared_decode_checks_the_actual_returned_document_before_exposure() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates")
        .parent()
        .expect("workspace");
    let source = std::fs::read_to_string(root.join("crates/cadmpeg-test-support/src/golden.rs"))
        .expect("shared golden source");
    let file = syn::parse_file(&source).expect("parse shared golden source");
    let functions: Vec<_> = file
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Fn(function) if function.sig.ident == "decode" => Some(function),
            _ => None,
        })
        .collect();
    assert_eq!(functions.len(), 1);
    assert!(
        audits_before_return(functions[0]),
        "one decode, exact live IR audit, then return the same result"
    );
    for source in [
        "fn decode(){ let decoded=codec.decode(reader,options)?; Ok(decoded) }",
        "fn decode(){ let decoded=codec.decode(reader,options)?; crate::native_linkage::assert_native_linkage(other.ir()); Ok(decoded) }",
        "fn decode(){ let decoded=codec.decode(reader,options)?; crate::native_linkage::assert_native_linkage(decoded.ir()); Ok(other) }",
        "fn decode(){ let decoded=codec.decode(reader,options)?; return Ok(decoded); crate::native_linkage::assert_native_linkage(decoded.ir()); }",
    ] {
        assert!(!audits_before_return(&syn::parse_str::<syn::ItemFn>(source).expect("route control")), "invalid proof: {source}");
    }
}
