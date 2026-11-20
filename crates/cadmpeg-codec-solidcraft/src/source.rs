// SPDX-License-Identifier: Apache-2.0
//! An opened design file: its encoding, its document, and its blob audit.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use crate::dialect::{DialectClassification, Encoding};
use crate::document::{Document, Header};
use crate::loss::SolidcraftLossCode;
use crate::package::{Marker, MemberRole, Package, DOCUMENT_MEMBER};

/// ZIP local file header signature: the first bytes of every container.
const CONTAINER_MAGIC: &[u8; 4] = b"PK\x03\x04";

/// The verdict on one blob member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlobCheck {
    /// The expanded bytes hash to the digest in the name.
    Verified,
    /// The expanded bytes hash to another digest.
    Mismatch,
    /// The member does not expand.
    Unreadable,
}

/// The cross-check of blob references against blob members.
#[derive(Debug, Default)]
pub(crate) struct BlobAudit {
    /// The verdict on each referenced blob member.
    pub(crate) checks: BTreeMap<String, BlobCheck>,
    /// The fields that reference each blob member.
    pub(crate) referenced_by: BTreeMap<String, BTreeSet<String>>,
    /// The losses of the audit.
    pub(crate) losses: Vec<LossNote>,
}

/// An opened design file.
pub(crate) struct Source<'a> {
    /// The root bytes.
    pub(crate) root: View<'a>,
    /// The ZIP package, for a container.
    pub(crate) package: Option<Package<'a>>,
    /// The design document, when the file holds one that parses.
    pub(crate) document: Option<Document>,
    /// Why the design document did not parse.
    pub(crate) document_error: Option<CodecError>,
}

impl<'a> Source<'a> {
    /// Opens the root as a container or a plain JSON document.
    pub(crate) fn open(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Self, CodecError> {
        if root.window().starts_with(CONTAINER_MAGIC) {
            let package = Package::open(ctx, root)?;
            let (document, document_error) =
                if *package.marker() != Marker::Drawing && package.contains(DOCUMENT_MEMBER) {
                    match package
                        .member(ctx, DOCUMENT_MEMBER)
                        .and_then(|view| crate::document::parse(ctx, view))
                    {
                        Ok(document) => (Some(document), None),
                        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                        Err(error) => (None, Some(error)),
                    }
                } else {
                    (None, None)
                };
            return Ok(Self {
                root,
                package: Some(package),
                document,
                document_error,
            });
        }
        let document = crate::document::parse(ctx, root)?;
        Ok(Self {
            root,
            package: None,
            document: Some(document),
            document_error: None,
        })
    }

    /// The encoding discriminant.
    pub(crate) fn encoding(&self) -> Encoding<'_> {
        match &self.package {
            Some(package) => Encoding::Container(package.marker()),
            None => Encoding::Json,
        }
    }

    /// The document header, when the document parsed.
    pub(crate) fn header(&self) -> Option<&Header> {
        self.document.as_ref().map(|document| &document.header)
    }

    /// Classifies the file.
    pub(crate) fn classify(&self) -> DialectClassification {
        crate::dialect::classify(self.encoding(), self.header())
    }

    /// The losses of opening: directory recovery and an unreadable document.
    pub(crate) fn opening_losses(&self) -> Vec<LossNote> {
        let mut losses: Vec<LossNote> = self
            .package
            .as_ref()
            .and_then(Package::recovery_loss)
            .into_iter()
            .collect();
        if let Some(error) = &self.document_error {
            losses.push(SolidcraftLossCode::DocumentUnreadable.note(format!(
                "document.json does not parse as a design document ({error}); its member is \
                 retained without decoding"
            )));
        }
        losses
    }

    /// Checks every blob reference against the blob members.
    pub(crate) fn audit_blobs(&self, ctx: &DecodeContext<'a>) -> Result<BlobAudit, CodecError> {
        let mut audit = BlobAudit::default();
        let Some(header) = self.header() else {
            return Ok(audit);
        };
        let mut malformed = Vec::new();
        let mut missing = Vec::new();
        for reference in &header.blobs {
            let Some(blob) = &reference.blob else {
                malformed.push(format!("{} = {:?}", reference.field, reference.target));
                continue;
            };
            let present = self
                .package
                .as_ref()
                .is_some_and(|package| package.contains(&reference.target));
            if !present {
                missing.push(format!("{} = {:?}", reference.field, reference.target));
                continue;
            }
            audit
                .referenced_by
                .entry(reference.target.clone())
                .or_default()
                .insert(reference.field.clone());
            if audit.checks.contains_key(&reference.target) {
                continue;
            }
            let Some(package) = &self.package else {
                continue;
            };
            let check = match package.member(ctx, &reference.target) {
                Ok(view) => {
                    if cadmpeg_ir::hash::sha256_hex(view.window()) == blob.digest {
                        BlobCheck::Verified
                    } else {
                        BlobCheck::Mismatch
                    }
                }
                Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                Err(_) => BlobCheck::Unreadable,
            };
            if check != BlobCheck::Verified {
                audit
                    .losses
                    .push(SolidcraftLossCode::BlobDigestMismatch.note(format!(
                        "blob member {:?} {}; its payload is not the one its name pins",
                        reference.target,
                        match check {
                            BlobCheck::Unreadable => "does not expand",
                            _ => "hashes to another SHA-256",
                        }
                    )));
            }
            audit.checks.insert(reference.target.clone(), check);
        }
        if !malformed.is_empty() {
            audit
                .losses
                .push(SolidcraftLossCode::BlobReferenceMalformed.note(format!(
                    "{} blob references are not blobs/<sha256>.<extension> names: {}",
                    malformed.len(),
                    malformed.join(", ")
                )));
        }
        if !missing.is_empty() {
            audit
                .losses
                .push(SolidcraftLossCode::BlobMissing.note(format!(
                    "{} blob references name members that the file does not hold: {}",
                    missing.len(),
                    missing.join(", ")
                )));
        }
        if let Some(package) = &self.package {
            let unreferenced: Vec<&str> = package
                .roles()
                .iter()
                .filter(|(name, role)| {
                    matches!(role, MemberRole::Blob(_)) && !audit.referenced_by.contains_key(*name)
                })
                .map(|(name, _)| name.as_str())
                .collect();
            if !unreferenced.is_empty() {
                audit
                    .losses
                    .push(SolidcraftLossCode::BlobUnreferenced.note(format!(
                        "{} blob members are referenced by no document field: {}",
                        unreferenced.len(),
                        unreferenced.join(", ")
                    )));
            }
        }
        Ok(audit)
    }
}
