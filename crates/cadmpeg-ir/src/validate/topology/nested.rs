use std::collections::BTreeSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::document::{CadIr, Topology};
use crate::index::ModelIndex;
use crate::report::check::Finding;
use crate::validate::geometry_consistency::check_edge_endpoint_consistency;
use crate::validate::identity_order::check_identity_and_order;
use crate::validate::referential_integrity::check_typed_references;

/// Checks `geometry` as a model of its own. `file_keys` are the keys of the
/// `File` definitions of the document that holds it.
pub(in crate::validate) fn check(
    ctx: &DecodeContext<'_>,
    geometry: Topology,
    file_keys: &BTreeSet<&str>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("nested topology validation")?;
    let mut nest = CadIr::empty();
    nest.geometry = geometry;
    let index = ModelIndex::build(&nest);
    check_identity_and_order(&index, file_keys, findings)?;
    super::check_references(ctx, &nest, &index, findings)?;
    check_typed_references(ctx, &nest, &index, findings)?;
    super::graphs::check_coedge_pairing(&nest, findings)?;
    super::graphs::check_shell_connectivity(&nest, findings);
    super::graphs::check_wire_topology(&nest, findings);
    check_edge_endpoint_consistency(ctx, &nest, findings)?;
    super::check_topology_tolerances(&nest, findings);
    Ok(())
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
        let error = super::check(
            &ctx,
            crate::document::Topology::default(),
            &std::collections::BTreeSet::new(),
            &mut Vec::new(),
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
        let unqualified = |file_keys: &std::collections::BTreeSet<&str>| {
            let mut findings = Vec::new();
            super::check(&ctx, topology.clone(), file_keys, &mut findings).unwrap();
            findings
                .iter()
                .filter(|finding| {
                    finding.rule.as_str() == "products.entity-id-scope-key-names-no-file"
                })
                .count()
        };
        assert_eq!(
            unqualified(&std::collections::BTreeSet::from(["member.f3d"])),
            0
        );
        assert_eq!(unqualified(&std::collections::BTreeSet::new()), 1);
    }
}
