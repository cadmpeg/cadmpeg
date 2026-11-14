// SPDX-License-Identifier: Apache-2.0
//! History bodies: producer and consumer features, and the closure of each
//! nest under the topology checks.

use std::collections::HashSet;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::document::CadIr;
use crate::features::FeatureId;
use crate::history_body::HistoryBody;
use crate::index::identities::IdentitySet;
use crate::index::ModelIndex;
use crate::report::check::{Check, Finding};
use crate::report::Severity;
use crate::schema::EntitySchema;

use super::geometry_consistency::check_edge_endpoint_consistency;
use super::identity_order::check_identity_and_order;
use super::record_finding;
use super::referential_integrity::check_typed_references;
use super::topology::graphs::{
    check_coedge_pairing, check_shell_connectivity, check_wire_topology,
};
use super::topology::{check_references, check_topology_tolerances};

/// Identities of every record of a nest.
fn nest_identities(model: &crate::document::Model) -> Vec<&str> {
    fn ids<T: EntitySchema>(records: &[T]) -> impl Iterator<Item = &str> {
        records.iter().map(EntitySchema::identity)
    }
    ids(&model.bodies)
        .chain(ids(&model.regions))
        .chain(ids(&model.shells))
        .chain(ids(&model.faces))
        .chain(ids(&model.loops))
        .chain(ids(&model.coedges))
        .chain(ids(&model.edges))
        .chain(ids(&model.vertices))
        .chain(ids(&model.surfaces))
        .chain(ids(&model.curves))
        .chain(ids(&model.pcurves))
        .chain(ids(&model.points))
        .collect()
}

/// Run the topology checks over one nest as if it were a model of its own.
/// A reference that leaves the nest is a failed reference check there.
fn check_nest(
    ctx: &DecodeContext<'_>,
    body: &HistoryBody,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut nest = CadIr::empty();
    nest.model = body.topology.to_model();
    let index = ModelIndex::build(&nest);
    let mut nested = Vec::new();
    check_identity_and_order(&index, &mut nested)?;
    check_references(ctx, &nest, &index, &mut nested)?;
    check_typed_references(ctx, &nest, &index, &mut nested)?;
    check_coedge_pairing(&nest, &mut nested)?;
    check_shell_connectivity(&nest, &mut nested);
    check_wire_topology(&nest, &mut nested);
    check_edge_endpoint_consistency(ctx, &nest, &mut nested)?;
    check_topology_tolerances(&nest, &mut nested);
    for mut finding in nested {
        finding.message = format!("history body nest: {}", finding.message);
        finding.related.insert(0, body.id.as_str().to_owned());
        findings.push(finding);
    }
    Ok(())
}

pub(super) fn check_history_bodies(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    ids: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    if ir.model.history_bodies.is_empty() {
        return Ok(());
    }
    let features: HashSet<&str> = ir
        .model
        .features
        .iter()
        .map(|feature| feature.id.as_str())
        .collect();
    let mut owners = HashSet::new();
    for body in &ir.model.history_bodies {
        let owner = body.id.as_str();
        for (role, feature) in [
            ("produced_by", &body.produced_by),
            ("consumed_by", &body.consumed_by),
        ] {
            if let Some(feature) = feature.as_ref().map(FeatureId::as_str) {
                if !features.contains(feature) {
                    record_finding(
                        findings,
                        Check::ReferentialIntegrity,
                        Severity::Error,
                        Some(owner),
                        format_args!("history body {role} {feature} names no feature"),
                    );
                }
            }
        }
        check_nest(ctx, body, findings)?;
        let model = body.topology.to_model();
        for id in nest_identities(&model) {
            if IdentitySet::contains(ids, id) {
                record_finding(
                    findings,
                    Check::Identity,
                    Severity::Error,
                    Some(owner),
                    format_args!("history body record {id} repeats a top-level identity"),
                );
            } else if !owners.insert(id.to_owned()) {
                record_finding(
                    findings,
                    Check::Identity,
                    Severity::Error,
                    Some(owner),
                    format_args!(
                        "history body record {id} repeats an identity of another history body"
                    ),
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
