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

use super::record_finding;
use super::topology::nested;

/// Identities of every record of a nest.
fn nest_identities(geometry: &crate::document::Topology) -> Vec<&str> {
    fn ids<T: EntitySchema>(records: &[T]) -> impl Iterator<Item = &str> {
        records.iter().map(EntitySchema::identity)
    }
    ids(&geometry.bodies)
        .chain(ids(&geometry.regions))
        .chain(ids(&geometry.shells))
        .chain(ids(&geometry.faces))
        .chain(ids(&geometry.loops))
        .chain(ids(&geometry.coedges))
        .chain(ids(&geometry.edges))
        .chain(ids(&geometry.vertices))
        .chain(ids(&geometry.surfaces))
        .chain(ids(&geometry.curves))
        .chain(ids(&geometry.pcurves))
        .chain(ids(&geometry.points))
        .collect()
}

/// Run the topology checks over one nest as if it were a model of its own.
/// A reference that leaves the nest is a failed reference check there.
fn check_nest(
    ctx: &DecodeContext<'_>,
    tolerances: crate::units::Tolerances,
    body: &HistoryBody,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut nested = Vec::new();
    nested::check(ctx, tolerances, body.topology.to_topology(), &mut nested)?;
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
        if body.produced_by.is_none() && body.consumed_by.is_none() {
            record_finding(
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                Some(owner),
                crate::rule!(
                    "history_bodies.history-body-names-neither-producing-nor",
                    "history body names neither a producing nor a consuming feature"
                ),
            );
        }
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
                        crate::rule!(
                            "history_bodies.history-body-names-no-feature",
                            "history body {role} {feature} names no feature"
                        ),
                    );
                }
            }
        }
        check_nest(ctx, ir.tolerances, body, findings)?;
        let geometry = body.topology.to_topology();
        for id in nest_identities(&geometry) {
            if IdentitySet::contains(ids, id) {
                record_finding(
                    findings,
                    Check::Identity,
                    Severity::Error,
                    Some(owner),
                    crate::rule!(
                        "history_bodies.history-body-record-repeats-top-level",
                        "history body record {id} repeats a top-level identity"
                    ),
                );
            } else if !owners.insert(id.to_owned()) {
                record_finding(
                    findings,
                    Check::Identity,
                    Severity::Error,
                    Some(owner),
                    crate::rule!(
                        "history_bodies.history-body-record-repeats-identity-another",
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
