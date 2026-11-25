use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::document::{CadIr, Topology};
use crate::index::ModelIndex;
use crate::report::check::Finding;
use crate::units::Tolerances;
use crate::validate::geometry_consistency::check_edge_endpoint_consistency;
use crate::validate::identity_order::check_identity_and_order;
use crate::validate::referential_integrity::check_typed_references;

/// The rules of one nested topology, checked as the geometry of a document
/// of its own at its document's `tolerances` (part 10, Coincidence).
pub(in crate::validate) fn check(
    ctx: &DecodeContext<'_>,
    tolerances: Tolerances,
    geometry: Topology,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let _depth = ctx.enter_nested("nested topology validation")?;
    let mut nest = CadIr::empty();
    nest.tolerances = tolerances;
    nest.geometry = geometry;
    let index = ModelIndex::build(&nest);
    check_identity_and_order(&index, findings)?;
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
            crate::units::Tolerances::default(),
            crate::document::Topology::default(),
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    }
}
