// SPDX-License-Identifier: Apache-2.0
//! Source locations and reasons for bodies excluded from decode checking.
use crate::types;
use rustc_hir::def::DefKind;
use rustc_middle::ty::TyCtxt;
use rustc_span::def_id::{DefId, LocalDefId};

pub(super) struct Body {
    pub(super) path: String,
    pub(super) line: usize,
    pub(super) name: String,
    pub(super) reason: &'static str,
    pub(super) eligible: bool,
}

fn serialization(tcx: TyCtxt<'_>, mut id: DefId) -> bool {
    while let Some(parent) = tcx.opt_parent(id) {
        if matches!(tcx.def_kind(parent), DefKind::Impl { of_trait: true }) {
            return types::serde_serialize(tcx, tcx.impl_trait_ref(parent).skip_binder().def_id);
        }
        id = parent;
    }
    false
}

pub(super) fn body(tcx: TyCtxt<'_>, owner: LocalDefId) -> Option<Body> {
    if !matches!(tcx.def_kind(owner), DefKind::Fn | DefKind::AssocFn | DefKind::Closure)
        || types::derived(tcx, owner.to_def_id()) {
        return None;
    }
    let position = tcx.sess.source_map().lookup_char_pos(tcx.def_span(owner).source_callsite().lo());
    let path = position.file.name.prefer_local_unconditionally().to_string();
    let path = std::env::current_dir().ok().and_then(|root| {
        std::path::Path::new(&path).strip_prefix(root).ok().map(|path| path.display().to_string())
    }).unwrap_or(path);
    let eligible = crate::production(tcx, owner.to_def_id());
    let reason = if serialization(tcx, owner.to_def_id()) {
        "serialization-only"
    } else if !eligible {
        "test-only"
    } else if path.split('/').any(|part| matches!(part, "encode" | "encoder" | "encode.rs" | "encoder.rs"))
        || tcx.opt_item_name(owner.to_def_id()).is_some_and(|name| matches!(name.as_str(), "encode" | "encode_impl" | "export")) {
        "encoder-only"
    } else if path.split('/').any(|part| matches!(part, "write" | "writer" | "write.rs" | "writer.rs")) {
        "writer-only"
    } else {
        "no path from a decode entry point"
    };
    Some(Body { path, line: position.line, name: tcx.def_path_str(owner), reason, eligible })
}
