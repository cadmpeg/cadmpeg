// SPDX-License-Identifier: Apache-2.0
//! History bodies: producer and consumer features, and the closure of each
//! nest under the topology checks.

use std::collections::{BTreeSet, HashSet};

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
use super::topology::nested::{self, NestRules};

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

/// Run `rules` over one nest as if it were a model of its own.
/// A reference that leaves the nest is a failed reference check there.
fn check_nest(
    ctx: &DecodeContext<'_>,
    ids: &ModelIndex<'_>,
    body: &HistoryBody,
    file_keys: &BTreeSet<&str>,
    rules: NestRules,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let nested = nested::check(
        ctx,
        ids.native_view(),
        body.topology.to_topology(),
        Some(file_keys),
        rules,
    )?;
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
    nests: NestRules,
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
    let model_bodies: HashSet<&str> = ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.as_str())
        .collect();
    let file_keys = super::products::decode_set::file_keys(ir);
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
        if let Some(state_of) = body.state_of.as_ref().map(crate::ids::BodyId::as_str) {
            if !model_bodies.contains(state_of) {
                record_finding(
                    findings,
                    Check::ReferentialIntegrity,
                    Severity::Error,
                    Some(owner),
                    crate::rule!(
                        "history_bodies.history-body-state-names-no-body",
                        "history body state_of {state_of} names no model body"
                    ),
                );
            }
        }
        check_nest(ctx, ids, body, &file_keys, nests, findings)?;
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
