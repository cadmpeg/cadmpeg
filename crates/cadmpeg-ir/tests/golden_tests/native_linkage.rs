// SPDX-License-Identifier: Apache-2.0
//! Every codec golden set is covered by check 8 (#246).
//!
//! Check 8 is a data check of its own. Each codec runs it in one dedicated
//! test over its golden inputs; the golden, depth and round-trip tests decode
//! without it.

use std::path::Path;

use syn::visit::Visit;

const CHECK: &[&str] = &[
    "cadmpeg_test_support",
    "native_linkage",
    "assert_golden_native_linkage",
];
const GOLDEN_ROOTS: &[&str] = &[
    "crates/cadmpeg-codec-catia/src/golden_tests.rs",
    "crates/cadmpeg-codec-creo/src/golden_tests.rs",
    "crates/cadmpeg-codec-f3d/src/golden_tests.rs",
    "crates/cadmpeg-codec-freecad/src/golden_tests.rs",
    "crates/cadmpeg-codec-iges/src/golden_tests.rs",
    "crates/cadmpeg-codec-inventor/src/golden_tests.rs",
    "crates/cadmpeg-codec-nx/src/golden_tests.rs",
    "crates/cadmpeg-codec-pvz/src/golden_tests.rs",
    "crates/cadmpeg-codec-rhino/src/golden_tests.rs",
    "crates/cadmpeg-codec-sat/src/golden_tests.rs",
    "crates/cadmpeg-codec-sldprt/src/golden_tests.rs",
    "crates/cadmpeg-codec-step/src/golden_tests.rs",
];

/// Counts calls to the shared golden check 8 inside `#[test]` functions.
#[derive(Default)]
struct Checks {
    in_test: bool,
    tested: usize,
    untested: usize,
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

impl<'ast> Visit<'ast> for Checks {
    fn visit_item_fn(&mut self, function: &'ast syn::ItemFn) {
        let outer = self.in_test;
        self.in_test = function
            .attrs
            .iter()
            .any(|attribute| attribute.path().is_ident("test"));
        syn::visit::visit_item_fn(self, function);
        self.in_test = outer;
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let syn::Expr::Path(function) = &*call.func {
            if path_is(&function.path, CHECK) {
                if self.in_test {
                    self.tested += 1;
                } else {
                    self.untested += 1;
                }
            }
        }
        syn::visit::visit_expr_call(self, call);
    }
}

fn checks(source: &str) -> Checks {
    let mut checks = Checks::default();
    checks.visit_file(&syn::parse_file(source).expect("parse golden source"));
    checks
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
        actual, GOLDEN_ROOTS,
        "every codec golden set must be covered by check 8"
    );
    for (expected, path) in GOLDEN_ROOTS.iter().zip(files) {
        let checks = checks(&std::fs::read_to_string(path).expect("read golden source"));
        assert_eq!(
            (checks.tested, checks.untested),
            (1, 0),
            "{expected}: one #[test] runs check 8 over the golden inputs"
        );
    }
}

#[test]
fn golden_check_census_counts_only_the_shared_check_in_a_test() {
    let checked = checks(
        "#[test] fn f(){ cadmpeg_test_support::native_linkage::assert_golden_native_linkage(&codec, &inputs); }",
    );
    assert_eq!((checked.tested, checked.untested), (1, 0));
    let helper = checks(
        "fn f(){ cadmpeg_test_support::native_linkage::assert_golden_native_linkage(&codec, &inputs); }",
    );
    assert_eq!((helper.tested, helper.untested), (0, 1));
    for source in [
        "#[test] fn f(){ cadmpeg_test_support::native_linkage::assert_native_linkage(document); }",
        "#[test] fn f(){ cadmpeg_test_support::golden::decode(&codec, reader, options); }",
        "#[test] fn f(){ assert_golden_native_linkage(&codec, &inputs); }",
    ] {
        let other = checks(source);
        assert_eq!(
            (other.tested, other.untested),
            (0, 0),
            "not the shared golden check 8: {source}"
        );
    }
}
