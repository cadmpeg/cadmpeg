// SPDX-License-Identifier: Apache-2.0
//! Only tracked core captures and the part helper can build read attribution.
//!
//! The owners are `source_fidelity/part.rs`, the #215 part-helper tree
//! `source_fidelity/parts/**` and `cadmpeg-core/src/decode/**`. Elsewhere,
//! a layout comes from `SourceFidelity::install_record_captures` and a
//! composed layout from `SourceFidelity::prefix_record_layouts`.

use std::collections::BTreeSet;
use std::path::Path;

use syn::visit::{self, Visit};
use syn::{Expr, Pat, Type, UseTree};

const PART_HELPER: &str = "crates/cadmpeg-ir/src/source_fidelity/part.rs";
const PART_HELPER_TREE: &str = "crates/cadmpeg-ir/src/source_fidelity/parts/";

fn is_owner(path: &str) -> bool {
    path == PART_HELPER
        || path.starts_with(PART_HELPER_TREE)
        || path.starts_with("crates/cadmpeg-core/src/decode/")
}

/// `RecordLayout` functions that build `reads` from a capture or rewrite
/// their stream keys.
const READ_BUILDERS: [&str; 3] = [
    "from_capture",
    "with_stream_prefix",
    "with_stream_prefix_in",
];

#[derive(Default)]
struct Aliases {
    names: BTreeSet<String>,
}

impl Aliases {
    fn path(&self, path: &syn::Path) -> bool {
        path.segments
            .last()
            .is_some_and(|name| self.names.contains(&name.ident.to_string()))
    }

    fn ty(&self, ty: &Type) -> bool {
        match ty {
            Type::Path(ty) => self.path(&ty.path),
            Type::Reference(ty) => self.ty(&ty.elem),
            Type::Paren(ty) => self.ty(&ty.elem),
            Type::Group(ty) => self.ty(&ty.elem),
            _ => false,
        }
    }

    fn import(&mut self, tree: &UseTree) {
        match tree {
            UseTree::Path(path) => self.import(&path.tree),
            UseTree::Rename(rename) if self.names.contains(&rename.ident.to_string()) => {
                self.names.insert(rename.rename.to_string());
            }
            UseTree::Group(group) => {
                for item in &group.items {
                    self.import(item);
                }
            }
            _ => {}
        }
    }
}

impl<'ast> Visit<'ast> for Aliases {
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        self.import(&item.tree);
    }

    fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
        if self.ty(&item.ty) {
            self.names.insert(item.ident.to_string());
        }
        visit::visit_item_type(self, item);
    }
}

struct ReadConstruction<'a> {
    aliases: &'a Aliases,
    bindings: BTreeSet<String>,
    layout_impl: bool,
    violations: BTreeSet<&'static str>,
}

impl ReadConstruction<'_> {
    fn names_layout(&self, segment: &syn::PathSegment) -> bool {
        self.aliases.names.contains(&segment.ident.to_string())
    }

    fn layout(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Path(path) => path.path.get_ident().is_some_and(|name| {
                self.bindings.contains(&name.to_string()) || (name == "self" && self.layout_impl)
            }),
            Expr::Struct(value) => self.aliases.path(&value.path),
            Expr::Reference(value) => self.layout(&value.expr),
            Expr::Paren(value) => self.layout(&value.expr),
            Expr::Group(value) => self.layout(&value.expr),
            Expr::Unary(value) => self.layout(&value.expr),
            _ => false,
        }
    }

    fn reads(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Field(field) => {
                matches!(&field.member, syn::Member::Named(name) if name == "reads")
                    && self.layout(&field.base)
            }
            Expr::MethodCall(call) => call.method == "reads" && self.layout(&call.receiver),
            Expr::Paren(value) => self.reads(&value.expr),
            Expr::Group(value) => self.reads(&value.expr),
            Expr::Reference(value) => self.reads(&value.expr),
            _ => false,
        }
    }

    fn bind(&mut self, pattern: &Pat) {
        match pattern {
            Pat::Ident(name) => {
                self.bindings.insert(name.ident.to_string());
            }
            Pat::Type(typed) => self.bind(&typed.pat),
            Pat::Reference(reference) => self.bind(&reference.pat),
            _ => {}
        }
    }
}

impl<'ast> Visit<'ast> for ReadConstruction<'_> {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let previous = std::mem::take(&mut self.bindings);
        visit::visit_item_fn(self, item);
        self.bindings = previous;
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        let previous = std::mem::take(&mut self.bindings);
        visit::visit_impl_item_fn(self, item);
        self.bindings = previous;
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let previous = self.layout_impl;
        self.layout_impl = self.aliases.ty(&item.self_ty);
        visit::visit_item_impl(self, item);
        self.layout_impl = previous;
    }

    fn visit_fn_arg(&mut self, argument: &'ast syn::FnArg) {
        if let syn::FnArg::Typed(argument) = argument {
            if self.aliases.ty(&argument.ty) {
                self.bind(&argument.pat);
            }
        }
        visit::visit_fn_arg(self, argument);
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        let explicit = matches!(&local.pat, Pat::Type(typed) if self.aliases.ty(&typed.ty));
        let inferred = local
            .init
            .as_ref()
            .is_some_and(|init| self.layout(&init.expr));
        if explicit || inferred {
            self.bind(&local.pat);
        }
        visit::visit_local(self, local);
    }

    fn visit_expr_struct(&mut self, value: &'ast syn::ExprStruct) {
        if self.aliases.path(&value.path) || (value.path.is_ident("Self") && self.layout_impl) {
            self.violations
                .insert("RecordLayout construction outside the part helper");
        }
        visit::visit_expr_struct(self, value);
    }

    fn visit_pat_struct(&mut self, value: &'ast syn::PatStruct) {
        if self.aliases.path(&value.path) {
            self.violations
                .insert("RecordLayout destructuring outside the part helper");
        }
        visit::visit_pat_struct(self, value);
    }

    fn visit_expr_field(&mut self, field: &'ast syn::ExprField) {
        if matches!(&field.member, syn::Member::Named(name) if name == "reads")
            && self.layout(&field.base)
        {
            self.violations
                .insert("direct RecordLayout.reads access outside the part helper");
        }
        visit::visit_expr_field(self, field);
    }

    fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
        let segments = path.path.segments.iter().collect::<Vec<_>>();
        if let [.., owner, function] = segments.as_slice() {
            if READ_BUILDERS.contains(&function.ident.to_string().as_str())
                && (self.names_layout(owner) || (owner.ident == "Self" && self.layout_impl))
            {
                self.violations
                    .insert("RecordLayout read builder call outside the part helper");
            }
        }
        visit::visit_expr_path(self, path);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        // `RecordLayout::with_stream_prefix` takes one argument; the
        // provenance method of the same name takes a context and a prefix.
        if (call.method == "with_stream_prefix"
            && (call.args.len() == 1 || self.layout(&call.receiver)))
            || (call.method == "with_stream_prefix_in"
                && (call.args.len() == 2 || self.layout(&call.receiver)))
        {
            self.violations
                .insert("RecordLayout read builder call outside the part helper");
        }
        if (self.layout(&call.receiver) && call.method == "reads_mut")
            || (self.reads(&call.receiver)
                && matches!(
                    call.method.to_string().as_str(),
                    "insert"
                        | "clear"
                        | "append"
                        | "entry"
                        | "remove"
                        | "retain"
                        | "get_mut"
                        | "values_mut"
                        | "iter_mut"
                ))
        {
            self.violations
                .insert("mutable RecordLayout reads access outside the part helper");
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_reference(&mut self, reference: &'ast syn::ExprReference) {
        if reference.mutability.is_some() && self.reads(&reference.expr) {
            self.violations
                .insert("mutable RecordLayout reads borrow outside the part helper");
        }
        visit::visit_expr_reference(self, reference);
    }
}

fn violations(path: &str, source: &str) -> Vec<&'static str> {
    let parsed = syn::parse_file(source).expect("source must be valid Rust syntax");
    if is_owner(path) {
        return Vec::new();
    }
    let mut aliases = Aliases {
        names: BTreeSet::from(["RecordLayout".into()]),
    };
    loop {
        let count = aliases.names.len();
        aliases.visit_file(&parsed);
        if count == aliases.names.len() {
            break;
        }
    }
    let mut scanner = ReadConstruction {
        aliases: &aliases,
        bindings: BTreeSet::new(),
        layout_impl: false,
        violations: BTreeSet::new(),
    };
    scanner.visit_file(&parsed);
    scanner.violations.into_iter().collect()
}

#[test]
fn only_decode_tracking_and_the_part_helper_construct_record_reads() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let mut sources = Vec::new();
    for directory in ["crates", "tools", "scripts"] {
        super::collect_rust_sources(&root.join(directory), &mut sources).unwrap();
    }
    let mut found = Vec::new();
    for source in sources {
        let path = source
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(&source).unwrap();
        for violation in violations(&path, &text) {
            found.push(format!("{path}: {violation}"));
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn guard_controls_cover_aliases_updates_mutations_and_helper_boundaries() {
    for source in [
        "fn f(){ let _=RecordLayout{reads: Default::default(), ..existing}; }",
        "use x::RecordLayout as Layout; fn f(){ let _=Layout{..existing}; }",
        "type Alias = Second; type Second = x::RecordLayout; fn f(){let _=Alias{..existing};}",
        "fn f(layout:&mut RecordLayout){layout.reads = Default::default();}",
        "fn f(layout:&mut RecordLayout){let alias=layout; alias.reads.clear();}",
        "fn f(layout:&mut RecordLayout){let _=&mut layout.reads;}",
        "fn f(layout:&mut RecordLayout){std::mem::replace(&mut layout.reads, Default::default());}",
        "fn f(layout:&mut RecordLayout){std::mem::take(&mut layout.reads);}",
        "fn f(layout:&mut RecordLayout, other:&mut RecordLayout){std::mem::swap(&mut layout.reads,&mut other.reads);}",
        "fn f(layout:&mut RecordLayout){layout.reads_mut().clear();}",
        "fn f(layout:&mut RecordLayout){layout.reads().get_mut(\"file\");}",
        "fn f(layout:&mut RecordLayout){let RecordLayout{reads: alias,..}=layout;}",
        "impl RecordLayout {fn make()->Self {Self{..existing}}}",
        "fn f(capture:RecordCapture){let _=RecordLayout::from_capture(capture);}",
        "use x::RecordLayout as Layout; fn f(c:RecordCapture){let _=Layout::from_capture(c);}",
        "fn f(captures:Vec<RecordCapture>){let _=captures.into_iter().map(RecordLayout::from_capture);}",
        "fn f(layout:RecordLayout){let _=layout.with_stream_prefix(\"p/\");}",
        "fn f(layouts:Map){for (_, layout) in layouts {let _=layout.with_stream_prefix(prefix);}}",
        "fn f(layout:RecordLayout){let _=RecordLayout::with_stream_prefix(layout, \"p/\");}",
        "impl RecordLayout {fn g(c:RecordCapture){let _=Self::from_capture(c);}}",
        "fn f(layout:RecordLayout){let _=layout.with_stream_prefix_in(ctx, prefix);}",
        "fn f(layouts:Map){for (_, layout) in layouts {let _=layout.with_stream_prefix_in(ctx, prefix);}}",
        "use x::RecordLayout as Layout; fn f(l:Layout){let _=Layout::with_stream_prefix_in(l, ctx, prefix);}",
    ] {
        assert!(!violations("crates/a/src/decode.rs", source).is_empty(), "{source}");
        assert!(!violations("crates/a/src/tests.rs", source).is_empty(), "no test exemption: {source}");
        assert!(!violations("crates/cadmpeg-ir/src/source_fidelity/other.rs", source).is_empty(), "no helper sibling exemption: {source}");
    }
    let allowed = "fn f(layout:&RecordLayout){let _=layout.reads().get(\"file\");}";
    assert!(violations("crates/a/src/lib.rs", allowed).is_empty());
    for allowed in [
        "fn f(p:SourceProvenance){let _=p.with_stream_prefix(ctx, \"p/\");}",
        "fn f(f:&mut SourceFidelity){f.install_record_captures(ctx, |_, _| Ok(())).unwrap();}",
        "fn f(f:&mut SourceFidelity, l:Map){f.prefix_record_layouts(l, \"p/\").unwrap();}",
        "fn f(c:Other){let _=Other::from_capture(c);}",
    ] {
        assert!(
            violations("crates/a/src/lib.rs", allowed).is_empty(),
            "{allowed}"
        );
    }
    assert!(violations(
        "crates/a/src/lib.rs",
        "struct Other{reads:u8} fn f(other:Other){let _=other.reads;}"
    )
    .is_empty());
    for owner in [
        PART_HELPER,
        "crates/cadmpeg-ir/src/source_fidelity/parts/helper.rs",
        "crates/cadmpeg-ir/src/source_fidelity/parts/rescope.rs",
        "crates/cadmpeg-core/src/decode/tracking.rs",
    ] {
        assert!(violations(
            owner,
            "fn f(layout:&mut RecordLayout){layout.reads.clear();}"
        )
        .is_empty());
        assert!(violations(
            owner,
            "fn f(c:RecordCapture){let (_, l)=RecordLayout::from_capture(c); l.with_stream_prefix(\"p/\");}"
        )
        .is_empty());
    }
}
