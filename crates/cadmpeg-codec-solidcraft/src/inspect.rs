// SPDX-License-Identifier: Apache-2.0
//! Design inspection: every member with its role, the document header, and
//! the blob audit.

use std::collections::BTreeMap;

use cadmpeg_core::container::{ContainerRole, EntryStorage, VerbatimLabel};
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::{CodecError, ContainerEntry};
use cadmpeg_ir::{ContainerKind, ContainerSummary};

use crate::document::Header;
use crate::package::DOCUMENT_MEMBER;
use crate::source::{BlobCheck, Source};

/// The entry name of a plain JSON document.
pub(crate) const JSON_DOCUMENT_ENTRY: &str = "document";
/// Entry attribute: the declared document format.
const ATTRIBUTE_FORMAT: &str = "format";
/// Entry attribute: the document name.
const ATTRIBUTE_NAME: &str = "name";
/// Entry attribute: the feature record count.
const ATTRIBUTE_FEATURES: &str = "features";
/// Entry attribute: the parameter record count.
const ATTRIBUTE_PARAMS: &str = "params";
/// Entry attribute: the blob digest verdict.
const ATTRIBUTE_DIGEST: &str = "sha256";
/// Entry attribute: the document fields that reference a blob.
const ATTRIBUTE_REFERENCED_BY: &str = "referenced_by";
/// Entry attribute: the component that a linked design supplies.
const ATTRIBUTE_COMPONENT: &str = "component";

pub(crate) fn inspect<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<ContainerSummary, CodecError> {
    let source = Source::open(ctx, root)?;
    let classification = source.classify();
    let audit = source.audit_blobs(ctx)?;
    let mut losses = source.opening_losses();
    losses.extend(classification.loss());
    losses.extend(audit.losses.iter().cloned());
    let (kind, mut entries) = match &source.package {
        Some(package) => (ContainerKind::Zip, package.entries()),
        None => (
            ContainerKind::Flat,
            vec![ContainerEntry {
                name: JSON_DOCUMENT_ENTRY.to_owned(),
                role: ContainerRole::Document,
                storage: EntryStorage::verbatim(
                    VerbatimLabel::Stored,
                    u64_from_index(source.root.window().len()),
                ),
                attributes: BTreeMap::new(),
            }],
        ),
    };
    let document_entry = if source.package.is_some() {
        DOCUMENT_MEMBER
    } else {
        JSON_DOCUMENT_ENTRY
    };
    for entry in &mut entries {
        if entry.name == document_entry {
            if let Some(header) = source.header() {
                header_attributes(header, &mut entry.attributes);
            }
        }
        if let Some(check) = audit.checks.get(&entry.name) {
            entry.attributes.insert(
                ATTRIBUTE_DIGEST.to_owned(),
                match check {
                    BlobCheck::Verified => "verified",
                    BlobCheck::Mismatch => "mismatch",
                    BlobCheck::Unreadable => "unreadable",
                }
                .to_owned(),
            );
        }
        if let Some(fields) = audit.referenced_by.get(&entry.name) {
            entry.attributes.insert(
                ATTRIBUTE_REFERENCED_BY.to_owned(),
                fields.iter().cloned().collect::<Vec<_>>().join(","),
            );
        }
    }
    if let Some(header) = source.header() {
        entries.extend(header.links.iter().map(|(component, path)| ContainerEntry {
            name: path.clone(),
            role: ContainerRole::ExternalReference,
            storage: EntryStorage::unreported(VerbatimLabel::None),
            attributes: BTreeMap::from([(ATTRIBUTE_COMPONENT.to_owned(), component.clone())]),
        }));
    }
    let notes = match source.header() {
        Some(header) => vec![format!(
            "design document with {} features and {} parameters",
            header.features, header.params
        )],
        None => Vec::new(),
    };
    Ok(ContainerSummary::classified(
        DialectLayers::of(classification.into_matched()),
        kind,
        entries,
        losses,
        notes,
    ))
}

fn header_attributes(header: &Header, attributes: &mut BTreeMap<String, String>) {
    if let Some(version) = header.tag.version() {
        attributes.insert(ATTRIBUTE_FORMAT.to_owned(), version.to_string());
    }
    if let Some(name) = &header.name {
        attributes.insert(ATTRIBUTE_NAME.to_owned(), name.clone());
    }
    attributes.insert(ATTRIBUTE_FEATURES.to_owned(), header.features.to_string());
    attributes.insert(ATTRIBUTE_PARAMS.to_owned(), header.params.to_string());
}
