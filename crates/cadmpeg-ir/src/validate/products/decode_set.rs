// SPDX-License-Identifier: Apache-2.0
//! Decode sets (part 10, Scope and linkage; §2.2, §3.1, §8.1): the scope
//! qualification of ids, and the keys of `SourceFidelity.file_digests`.

use std::collections::BTreeSet;

use crate::document::CadIr;
use crate::products::{ProductDefinition, Storage};
use crate::report::check::{Check, Finding};
use crate::report::Severity;
use crate::source_fidelity::SourceFidelity;

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
                crate::rule!("products.entity-id-scope-key-names-no-file",
                    "entity id's scope is qualified by `{key}@`, the key of no `File` definition of the document"
                ),
            );
        }
    }
}

/// The scope of every id of a record read from a component file begins with
/// that file's definition key and `@`: the bodies, sketches, and occurrences
/// its `File` definition owns, and the root containers of its design scope
/// (§2.2, §3.1). The `File` definition is a record of the placing file.
pub(in crate::validate) fn check_component_records(ir: &CadIr, findings: &mut Vec<Finding>) {
    for definition in &ir.model.product_definitions {
        if !matches!(
            definition.storage,
            Storage::File {
                source: Some(_),
                ..
            }
        ) {
            continue;
        }
        let Some(key) = definition_key(definition) else {
            continue;
        };
        let roots = ir
            .design
            .scopes
            .iter()
            .filter(|scope| scope.definition == definition.id)
            .flat_map(|scope| {
                scope
                    .roots
                    .iter()
                    .map(crate::design::ids::ContainerId::as_str)
            });
        let owned = definition
            .bodies
            .iter()
            .map(crate::ids::BodyId::as_str)
            .chain(
                definition
                    .sketches
                    .iter()
                    .map(crate::sketches::SketchId::as_str),
            )
            .chain(
                definition
                    .occurrences
                    .iter()
                    .map(crate::ids::OccurrenceId::as_str),
            )
            .chain(roots);
        for id in owned {
            if qualifier(id) != Some(key) {
                crate::validate::record_finding(
                    findings,
                    Check::Identity,
                    Severity::Error,
                    Some(id),
                    crate::rule!("products.component-record-scope-unqualified",
                        "id of a record read from the component file of `{}` does not begin its scope with that file's key `{key}@`",
                        definition.id
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
        let mut report = |ruled: crate::report::check::Ruled<'_>| {
            crate::validate::record_finding(
                findings,
                Check::ReferentialIntegrity,
                Severity::Error,
                Some(key.as_str()),
                ruled,
            );
        };
        match storage {
            Some(Storage::File {
                source: Some(_), ..
            }) => {}
            Some(Storage::File { source: None, .. }) => report(crate::rule!(
                "products.file-digest-of-root",
                "the sidecar states a file digest for the root definition, whose file `decode_digest` covers"
            )),
            Some(Storage::Embedded | Storage::External(_)) => report(crate::rule!(
                "products.file-digest-of-no-file",
                "the sidecar states a file digest for a definition read from no file of its own"
            )),
            None => report(crate::rule!(
                "products.file-digest-of-no-definition",
                "the sidecar states a file digest for no product definition of the document"
            )),
        }
    }
}

#[cfg(test)]
mod tests;
