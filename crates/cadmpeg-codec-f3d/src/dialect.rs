// SPDX-License-Identifier: Apache-2.0
//! F3D dialect identity: which registry row a Fusion archive is, and how it was
//! admitted.
//!
//! The `*LossCode` template: the enum is internal, registry-generated
//! [`DialectId`] constants are the boundary, `F3dDialect::matched` is the one
//! construction path, and the vocabulary is closed.
//!
//! # Two grammars, one enum, and one recovery row
//!
//! A Fusion ZIP takes one of two document-wide parse strategies, chosen before
//! anything semantic is read (`crate::container::scan`):
//!
//! - A root `Manifest.dat` selects the binary top-level manifest grammar. The
//!   version field selects nothing: every readable version is parsed with the
//!   `3-2-0-0` layout, whose anchors (`FusionDocType`, `.f3d`, and two
//!   hyphenated GUIDs) decide whether that layout fits. A document that
//!   declares `3-2-0-0` and parses is `f3d:manifest-3-2-0-0`. A document that
//!   declares another version and still parses is `f3d:unknown`, read with a
//!   strategy its own declaration does not name.
//! - No root `Manifest.dat`, but `Manifest.json`, `DesignDescription.json`, and
//!   a root-level `*.f3d` member, selects the F3Z multi-document grammar. That
//!   branch reads no version field at all, so `f3d:f3z-multi-document` is an
//!   identity row with an unbounded interior.
//!
//! The two identity rows are [`Admission::Admitted`]: each is parsed with the
//! strategy its own row declares. [`F3dDialect::Unknown`] is the mandatory
//! totality row and it is
//! [`Admission::Unverified`], using `f3d:manifest-3-2-0-0` as the
//! strategy applied to it, with [`dialect_loss`] charging
//! `source.dialect-unverified` on exactly that admission. Refusal stays
//! structural: a manifest whose bytes do not fit the anchors is refused by
//! `crate::manifest::parse_top_level`, and no version is on an allowlist.
//!
use cadmpeg_core::dialect::{
    Admission, DialectId, DialectLayers, DialectMatch, Grammar, LayerInstance,
};
use cadmpeg_core::target::TargetDescriptor;
use cadmpeg_ir::report::LossNote;
use std::collections::BTreeMap;

use crate::loss::F3dLossCode;
use crate::manifest::TOP_LEVEL_MANIFEST_VERSION;

include!("dialect/registry_ids.rs");

/// The one dialect this writer synthesizes.
///
/// `manifest::write` pins the top-level manifest version to
/// `TOP_LEVEL_MANIFEST_VERSION`, so a generated archive can be no other row.
/// The multi-document F3Z row is reachable only by replaying a retained
/// archive, which is preservation, not synthesis.
pub(crate) const TARGETS: &[TargetDescriptor] = &[TargetDescriptor {
    id: F3dDialect::Manifest3200.id(),
    aliases: &["3-2-0-0"],
}];

/// Key of the top-level `Manifest.dat` version field in
/// [`DialectMatch::declared`], recorded as the manifest cursor read it.
///
/// The value is the length-prefixed ASCII field at the head of the manifest.
/// `crate::manifest::parse_top_level` reads it and parses on regardless, so the
/// recorded value is whatever the document declared. It is the discriminant
/// between the identity row and the recovery row, and it is what names the
/// generation the bytes came from.
pub(crate) const DECLARED_TOP_LEVEL_MANIFEST_VERSION: &str = "top_level_manifest_version";

/// Key of the root-level `*.f3d` member names in [`DialectMatch::declared`],
/// comma-separated and sorted by archive path.
///
/// An F3Z archive declares no version anywhere: the branch that identifies it
/// tests for the absence of `Manifest.dat` and the presence of `Manifest.json`,
/// `DesignDescription.json`, and at least one root-level `*.f3d`. The first
/// three are constants and carry no information about the document. The
/// root-level member names are the one part of that discriminant the source
/// authored, and each is recorded verbatim as the archive spells it.
pub(crate) const DECLARED_ROOT_DOCUMENT_MEMBERS: &str = "root_document_members";

/// Key of the containing F3Z member path in an attached member layer.
///
/// Absent for a standalone F3D document. Unlike [`DialectMatch::instance`],
/// this is presentation provenance rather than a uniqueness key.
pub(crate) const DECLARED_ARCHIVE_MEMBER: &str = "archive_member";

/// Separator between root-level member names in
/// [`DECLARED_ROOT_DOCUMENT_MEMBERS`].
const MEMBER_SEPARATOR: &str = ",";

/// One row of `docs/dialects.toml` under the `f3d` namespace.
///
/// Three variants is still an enum: the drift test against the registry is what
/// the type is for, and it holds at three rows exactly as it holds at
/// twenty-two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum F3dDialect {
    /// Root `Manifest.dat` whose version, kind, and extension fields equal
    /// `3-2-0-0`, `FusionDocType`, and `.f3d`.
    Manifest3200,
    /// No root `Manifest.dat`; the F3Z manifest set plus a root-level `*.f3d`.
    F3zMultiDocument,
    /// Mandatory totality row: a top-level manifest that declares a version
    /// this codec does not know, and that the `3-2-0-0` layout parsed anyway.
    ///
    /// The document is read, so the row carries a match. The strategy applied
    /// to it is the one [`Self::Manifest3200`] declares, which the document's
    /// own declaration does not name, so the admission is
    /// [`Admission::Unverified`] and [`dialect_loss`] charges the
    /// recovery.
    Unknown,
}

impl F3dDialect {
    /// Every dialect identity this enum can name.
    #[cfg(test)]
    pub(crate) const ALL: [Self; 3] = [Self::Manifest3200, Self::F3zMultiDocument, Self::Unknown];

    /// The registry-generated id for this variant.
    pub(crate) const fn id(self) -> DialectId {
        match self {
            Self::Manifest3200 => F3D_MANIFEST_3_2_0_0,
            Self::F3zMultiDocument => F3D_F3Z_MULTI_DOCUMENT,
            Self::Unknown => F3D_UNKNOWN,
        }
    }

    /// Classifies a document archive from the version its top-level manifest
    /// declared.
    ///
    /// `version` is the field `crate::manifest::parse_top_level` read, not the
    /// constant it compared against. Reaching here means the `3-2-0-0` layout
    /// parsed the whole manifest, so the version decides only which row names
    /// that reading: its own, or the recovery row.
    pub(crate) fn classify_document(version: &str) -> DialectMatch {
        let mut declared = BTreeMap::new();
        declared.insert(
            DECLARED_TOP_LEVEL_MANIFEST_VERSION.to_owned(),
            version.to_owned(),
        );
        let dialect = if version == TOP_LEVEL_MANIFEST_VERSION {
            Self::Manifest3200
        } else {
            Self::Unknown
        };
        dialect.matched(declared)
    }

    /// Classifies a multi-document F3Z archive from its root-level `*.f3d`
    /// member names, sorted by archive path.
    ///
    /// The row declares a filename-presence discriminant and no version, so a
    /// document that reaches here was read with exactly the strategy its row
    /// declares: [`Admission::Admitted`].
    pub(crate) fn classify_f3z(root_document_members: &[&str]) -> DialectMatch {
        let mut declared = BTreeMap::new();
        declared.insert(
            DECLARED_ROOT_DOCUMENT_MEMBERS.to_owned(),
            root_document_members.join(MEMBER_SEPARATOR),
        );
        Self::F3zMultiDocument.matched(declared)
    }

    /// The one [`DialectMatch`] construction path in this codec, so a
    /// classification bug and the report can never disagree.
    fn matched(self, declared: BTreeMap<String, String>) -> DialectMatch {
        match self {
            Self::Manifest3200 | Self::F3zMultiDocument => DialectMatch::admitted(self.id()),
            Self::Unknown => {
                DialectMatch::unverified(self.id(), Grammar::of(&Self::Manifest3200.id()))
            }
        }
        .with_declared(declared)
    }
}

/// Classify the document and every kernel carrier without refusing on a layer
/// identity collision. Returns the layer set and any recoverable
/// classification loss.
pub(crate) fn classify_layers(
    scan: &crate::container::ContainerScan<'_>,
) -> (DialectLayers, Vec<LossNote>) {
    let mut layers = DialectLayers::of(scan.kind.dialect().clone());
    let mut losses = Vec::new();
    for layer in kernel_layers(scan) {
        let format = layer.format().to_owned();
        let instance = layer.instance().unwrap_or("unidentified").to_owned();
        if layers.insert(layer).is_err() {
            losses.push(F3dLossCode::DialectLayerCollision.note(format!(
                "the document produced a duplicate {format} dialect layer at instance {instance}; the later layer was omitted"
            )));
        }
    }
    (layers, losses)
}

/// Dialect-derived losses implied by a report's final classified layers.
pub(crate) fn dialect_losses(layers: &DialectLayers) -> Vec<LossNote> {
    let mut losses = layers
        .iter()
        .filter(|matched| matched.format() == FORMAT)
        .filter_map(dialect_loss)
        .collect::<Vec<_>>();
    losses.extend(
        layers
            .iter()
            .filter(|matched| matched.format() == cadmpeg_asm::dialect::FORMAT)
            .filter_map(kernel_dialect_loss),
    );
    losses
}

/// The dialect-unverified loss for a classified layer.
///
/// Returns a loss exactly for an unverified or residual admission. This reads
/// the admission rather than reclassifying, so the note and reported state
/// come from one value.
pub(crate) fn dialect_loss(matched: &DialectMatch) -> Option<LossNote> {
    let strategy = match matched.admission() {
        Admission::Admitted | Admission::Refused => return None,
        Admission::Unverified { using } => {
            format!("{}:{}", matched.format(), using.as_str())
        }
        Admission::Residual => "the residual parser path, which names no declared grammar".into(),
    };
    let version = matched
        .declared()
        .get(DECLARED_TOP_LEVEL_MANIFEST_VERSION)
        .map_or("(none)", String::as_str);
    let message = format!(
        "the top-level manifest declares version {version:?}, which no dialect row of \
                 this codec names, so no declared identity was verified. The document is read on \
                 {strategy}: every field after the version was parsed with that layout. The layout \
                 fitting is consistency, not a declaration."
    );
    let message = archive_member_message(matched, &message);
    Some(F3dLossCode::SourceDialectUnverified.note(message))
}

/// Kernel dialect layers from the binary and text B-rep streams.
pub(crate) fn kernel_layers(scan: &crate::container::ContainerScan<'_>) -> Vec<DialectMatch> {
    let text_names = crate::container::text_brep_names(scan);
    let instance = if scan.breps.len() + text_names.len() > 1 {
        LayerInstance::Tagged
    } else {
        LayerInstance::Sole
    };
    let mut matches = Vec::new();
    for brep in &scan.breps {
        let header = brep
            .kernel
            .as_ref()
            .map(crate::container::KernelFraming::as_header_ref)
            .unwrap_or(cadmpeg_asm::dialect::KernelHeaderRef::Unknown);
        matches.push(cadmpeg_asm::dialect::classify_layer(
            header, &brep.name, instance,
        ));
    }
    for name in text_names {
        let parsed = scan
            .entry_bytes(name)
            .ok()
            .and_then(|bytes| cadmpeg_asm::sat::parse(bytes).ok());
        let matched = match parsed.as_ref() {
            Some(stream) => {
                let header = stream.header.as_kernel_header();
                let reference = match stream.terminator {
                    cadmpeg_asm::sat::Terminator::Asm => {
                        cadmpeg_asm::dialect::KernelHeaderRef::TextAsm(&header)
                    }
                    cadmpeg_asm::sat::Terminator::Acis => {
                        cadmpeg_asm::dialect::KernelHeaderRef::TextAcis(&header)
                    }
                };
                cadmpeg_asm::dialect::classify_layer(reference, name, instance)
            }
            None => cadmpeg_asm::dialect::classify_layer(
                cadmpeg_asm::dialect::KernelHeaderRef::Unknown,
                name,
                instance,
            ),
        };
        matches.push(matched);
    }
    matches
}

/// The recovery loss a kernel layer charges, if it recovered.
pub(crate) fn kernel_dialect_loss(matched: &DialectMatch) -> Option<LossNote> {
    match matched.admission() {
        Admission::Refused => {
            let carrier = matched
                .declared()
                .get(cadmpeg_asm::dialect::DECLARED_CARRIER)
                .map_or("an unnamed carrier", String::as_str);
            let message = format!(
                "kernel carrier {carrier} could not be framed for dialect inspection; its retained \
                 source bytes remain available"
            );
            let message = archive_member_message(matched, &message);
            return Some(F3dLossCode::KernelCarrierUnparseable.note(message));
        }
        Admission::Admitted | Admission::Unverified { .. } | Admission::Residual => {}
    }
    let carrier = matched
        .declared()
        .get(cadmpeg_asm::dialect::DECLARED_CARRIER)
        .map_or("an unnamed carrier", String::as_str);
    let message = cadmpeg_asm::dialect::unverified_message(
        &format!("the kernel carrier {carrier}"),
        matched,
    )?;
    let message = archive_member_message(matched, &message);
    Some(F3dLossCode::KernelDialectUnverified.note(message))
}

fn archive_member_message(matched: &DialectMatch, message: &str) -> String {
    match matched.declared().get(DECLARED_ARCHIVE_MEMBER) {
        Some(member) => format!("archive member {member}: {message}"),
        None => message.to_owned(),
    }
}

#[cfg(test)]
mod tests;
