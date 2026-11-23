// SPDX-License-Identifier: Apache-2.0
//! Archive member resolution of manifest references by document identity.
//!
//! A member without `RedirectionsStream.dat` names its references by the
//! absolute path of the authoring cache, which no archive member carries. The
//! archive's `DesignDescription.json` states the lineage URN and the version
//! URN of every member, so a reference resolves to the member whose pair
//! equals the reference's pair.

use std::collections::BTreeMap;

use cadmpeg_ir::products::ExternalDocument;

use super::XrefTable;
use crate::loss::F3dLossCode;
use crate::records::xref::RequiredXrefText;

/// Archive members by `(lineage URN, version URN)`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LineageMembers {
    members: BTreeMap<(String, String), Vec<String>>,
}

impl LineageMembers {
    /// Index the members of `identities` that satisfy `is_member` and state
    /// both URNs.
    pub(crate) fn new(
        identities: &BTreeMap<String, ExternalDocument>,
        is_member: impl Fn(&str) -> bool,
    ) -> Self {
        let mut members = BTreeMap::<(String, String), Vec<String>>::new();
        for (path, identity) in identities {
            let (Some(lineage), Some(version)) = (&identity.document_id, &identity.version) else {
                continue;
            };
            if is_member(path) {
                members
                    .entry((lineage.as_str().to_owned(), version.as_str().to_owned()))
                    .or_default()
                    .push(path.clone());
            }
        }
        Self { members }
    }

    /// The only member stating both URNs.
    fn unique(&self, lineage: &str, version: &str) -> Option<&str> {
        match self
            .members
            .get(&(lineage.to_owned(), version.to_owned()))?
            .as_slice()
        {
            [member] => Some(member),
            _ => None,
        }
    }
}

/// Resolve each reference path of `table` that names exactly one archive
/// member by identity, and charge one loss per resolved path. A reference
/// without identity or without exactly one match keeps its path.
pub(super) fn resolve(table: &mut XrefTable, index: &LineageMembers) {
    let mut resolved = BTreeMap::<String, String>::new();
    for reference in &table.references {
        let path = reference.relative_path.as_str();
        if resolved.contains_key(path) {
            continue;
        }
        let Some(identity) = table.documents.get(path) else {
            continue;
        };
        let (Some(lineage), Some(version)) = (&identity.document_id, &identity.version) else {
            continue;
        };
        if let Some(member) = index.unique(lineage.as_str(), version.as_str()) {
            resolved.insert(path.to_owned(), member.to_owned());
        }
    }
    for (path, member) in &resolved {
        table.identity_losses.push(F3dLossCode::XrefRedirectionFromLineage.note(format!(
            "reference {path}: resolved to archive member {member} by lineage and version URN; the document has no RedirectionsStream.dat"
        )));
        if let Some(identity) = table.documents.remove(path) {
            table.documents.entry(member.clone()).or_insert(identity);
        }
    }
    for reference in &mut table.references {
        if let Some(member) = resolved.get(reference.relative_path.as_str()) {
            if let Ok(member) = RequiredXrefText::try_from(member.clone()) {
                reference.relative_path = member;
            }
        }
    }
}
