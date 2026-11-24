// SPDX-License-Identifier: Apache-2.0
//! Decode sets (part 10, Scope and linkage; §2.2, §3.1, §8.1): the scope
//! qualification of the ids of records read from a component file, and the
//! keys of `SourceFidelity.file_digests`.

use std::collections::BTreeSet;

use crate::document::CadIr;
use crate::products::{ProductDefinition, Storage};
use crate::report::check::{Check, Finding};
use crate::report::Severity;
use crate::source_fidelity::SourceFidelity;
use crate::validate::design::records::{DesignRecords, MemberKey};

/// The definition key of an id: the key of a `File` definition's id is the
/// text after its `#`.
fn definition_key(definition: &ProductDefinition) -> Option<&str> {
    definition.id.as_str().split_once('#').map(|(_, key)| key)
}

/// The keys of the document's `File` definitions.
pub(in crate::validate) fn file_keys(ir: &CadIr) -> BTreeSet<&str> {
    ir.model
        .product_definitions
        .iter()
        .filter(|definition| matches!(definition.storage, Storage::File { .. }))
        .filter_map(definition_key)
        .collect()
}

/// The definition key that qualifies an id's `<scope>`: the text before the
/// scope's `@`, where it holds one.
fn qualifier(id: &str) -> Option<&str> {
    let scope = id.split(':').nth(1)?;
    scope.split_once('@').map(|(key, _)| key)
}

/// Every scope that holds `@` begins with the key of a `File` definition of
/// the document (§2.2).
pub(in crate::validate) fn check_qualified_scope(
    file_keys: &BTreeSet<&str>,
    id: &str,
    findings: &mut Vec<Finding>,
) {
    if let Some(key) = qualifier(id) {
        if !file_keys.contains(key) {
            crate::validate::record_finding(
                findings,
                Check::Identity,
                Severity::Error,
                Some(id),
                format_args!(
                    "entity id's scope is qualified by `{key}@`, the key of no `File` definition of the document"
                ),
            );
        }
    }
}

/// The ids of the records read from a component file, the bodies and
/// occurrences its `File` definition owns and the features and containers
/// of its design scope, begin their `<scope>` with that definition's key and
/// `@` (§2.2, §3.1).
pub(super) fn check_component_ids(ir: &CadIr, findings: &mut Vec<Finding>) {
    let components: Vec<(&ProductDefinition, &str)> = ir
        .model
        .product_definitions
        .iter()
        .filter(|definition| {
            matches!(
                definition.storage,
                Storage::File {
                    source: Some(_),
                    ..
                }
            )
        })
        .filter_map(|definition| Some((definition, definition_key(definition)?)))
        .collect();
    if components.is_empty() {
        return;
    }
    let records = DesignRecords::new(ir);
    for (definition, key) in components {
        let scope = definition.id.as_str();
        let features = ir
            .design
            .features
            .iter()
            .map(|feature| feature.id.as_str())
            .filter(|id| records.scope_of(MemberKey::Feature(id)) == Some(scope));
        let containers = ir
            .design
            .containers
            .iter()
            .map(|container| container.id().as_str())
            .filter(|id| records.scope_of(MemberKey::Container(id)) == Some(scope));
        let owned = definition
            .bodies
            .iter()
            .map(crate::ids::BodyId::as_str)
            .chain(
                definition
                    .occurrences
                    .iter()
                    .map(crate::ids::OccurrenceId::as_str),
            )
            .chain(features)
            .chain(containers);
        for id in owned {
            if qualifier(id) != Some(key) {
                crate::validate::record_related_finding(
                    findings,
                    Check::Identity,
                    Severity::Error,
                    id,
                    &[scope],
                    format_args!(
                        "a record read from the component file of {scope} has an id whose scope does not begin with `{key}@`"
                    ),
                );
            }
        }
    }
}

/// Every key of `SourceFidelity.file_digests` names a `File` definition
/// read from a component file; the root definition has none (§8.1).
pub(in crate::validate) fn check_file_digests(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    findings: &mut Vec<Finding>,
) {
    for key in fidelity.file_digests.keys() {
        let storage = ir
            .model
            .product_definitions
            .iter()
            .find(|definition| definition.id == *key)
            .map(|definition| &definition.storage);
        let message = match storage {
            Some(Storage::File {
                source: Some(_), ..
            }) => continue,
            Some(Storage::File { source: None, .. }) => {
                "the sidecar states a file digest for the root definition, whose file `decode_digest` covers"
            }
            Some(Storage::Embedded | Storage::External(_)) => {
                "the sidecar states a file digest for a definition read from no file of its own"
            }
            None => "the sidecar states a file digest for no product definition of the document",
        };
        crate::validate::record_finding(
            findings,
            Check::ReferentialIntegrity,
            Severity::Error,
            Some(key.as_str()),
            format_args!("{message}"),
        );
    }
}

#[cfg(test)]
mod tests;
