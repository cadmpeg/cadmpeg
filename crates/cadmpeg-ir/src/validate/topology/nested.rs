use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::document::{CadIr, Topology};
use crate::index::ModelIndex;
use crate::native::view::NativeView;
use crate::report::check::Finding;
use crate::validate::geometry_consistency::check_edge_endpoint_consistency;
use crate::validate::identity_order::check_identity_and_order;
use crate::validate::referential_integrity::check_typed_references;

/// The rules a nest check runs on a topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::validate) enum NestRules {
    /// Coedge pairing, shell connectivity, wire topology, edge ends, and
    /// topology tolerances: the rules that give up a decoded history body.
    Admission,
    /// Every rule of part 10 on one topology. With `caches`, each procedural
    /// curve is also compared with its solved carrier (Caches, K2).
    All {
        /// Whether procedural curves are compared with their solved carriers.
        caches: bool,
    },
}

/// The findings of `rules` on one `Topology` that `document` holds outside
/// its current geometry: a resolved state, a history body's nest, or a
/// witness body. The topology is checked as the geometry of a document of
/// its own, at `document`'s tolerances (part 10, Coincidence). A nest
/// `closed` over its own references, with the keys of the `File` definitions
/// of `document`, also has its identities, their order, and every reference
/// of its records checked.
pub(in crate::validate) fn check(
    ctx: &DecodeContext<'_>,
    document: NativeView<'_>,
    topology: Topology,
    closed: Option<&BTreeSet<&str>>,
    rules: NestRules,
) -> Result<Vec<Finding>, CodecError> {
    let _depth = ctx.enter_nested("nested topology validation")?;
    let mut alone = CadIr::empty();
    alone.fragment = document.ir.fragment;
    alone.tolerances = document.ir.tolerances;
    alone.geometry = topology;
    let index = ModelIndex::build(&alone);
    let mut findings = Vec::new();
    if let Some(file_keys) = closed {
        check_identity_and_order(&index, file_keys, &mut findings)?;
        super::super::check_references(ctx, &alone, &index, &mut findings)?;
        check_typed_references(ctx, &alone, &index, &mut findings)?;
    }
    match rules {
        NestRules::Admission => {
            super::graphs::check_coedge_pairing(&alone, &mut findings)?;
            super::graphs::check_shell_connectivity(&alone, &mut findings);
            super::graphs::check_wire_topology(&alone, &mut findings);
            check_edge_endpoint_consistency(ctx, &alone, &mut findings)?;
            super::check_topology_tolerances(&alone, &mut findings);
        }
        NestRules::All { caches } => {
            super::super::check_topology_rules(ctx, &index, document, caches, &mut findings)?;
        }
    }
    Ok(findings)
}

#[cfg(test)]
mod tests {
    #[test]
    fn nested_topology_validation_propagates_the_callers_depth_limit() {
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_recursion_depth = 0;
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let ir = crate::document::CadIr::empty();
        let error = super::check(
            &ctx,
            crate::native::view::NativeView::new(&ir, None),
            crate::document::Topology::default(),
            Some(&std::collections::BTreeSet::new()),
            super::NestRules::All { caches: true },
        )
        .unwrap_err();
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    }

    #[test]
    fn a_nest_reads_the_file_keys_of_the_document_that_holds_it() {
        let topology = crate::document::Topology {
            bodies: vec![crate::topology::Body {
                unstated: None,
                id: "f3d:member.f3d@brep:entity#1".try_into().unwrap(),
                kind: crate::topology::BodyKind::Solid,
                transform: None,
                name: None,
                color: None,
                visible: None,
                native_ref: None,
            }],
            ..crate::document::Topology::default()
        };
        let ctx = cadmpeg_core::decode::test_support::service_decode_context();
        let ir = crate::document::CadIr::empty();
        let unqualified = |file_keys: &std::collections::BTreeSet<&str>| {
            super::check(
                &ctx,
                crate::native::view::NativeView::new(&ir, None),
                topology.clone(),
                Some(file_keys),
                super::NestRules::All { caches: true },
            )
            .unwrap()
            .iter()
            .filter(|finding| finding.rule.as_str() == "products.entity-id-scope-key-names-no-file")
            .count()
        };
        assert_eq!(
            unqualified(&std::collections::BTreeSet::from(["member.f3d"])),
            0
        );
        assert_eq!(unqualified(&std::collections::BTreeSet::new()), 1);
    }
}
