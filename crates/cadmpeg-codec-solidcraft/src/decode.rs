// SPDX-License-Identifier: Apache-2.0
//! Design decode.
//!
//! Decode opens the file, classifies it, reads the document header into the
//! source metadata, and checks every blob reference. The final state of a
//! container's model cache decodes into neutral topology. Each container member,
//! or the whole plain JSON document, is retained as an unknown record of the
//! `solidcraft` native namespace, so every byte survives; each family of
//! content that is retained without decoding is named by a loss.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{u64_from_index, DecodeContext, DecodeDepth, View};
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::ids::{IdentityKey, UnknownId};
use cadmpeg_ir::report::decode::DecodeTransfer;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::SourceFidelity;

use crate::dialect::FORMAT;
use crate::document::Header;
use crate::inspect::JSON_DOCUMENT_ENTRY;
use crate::loss::SolidcraftLossCode;
use crate::package::{BlobKind, Marker, MemberRole, Package, DOCUMENT_MEMBER};
use crate::source::Source;

/// Source attribute: the document name.
const ATTRIBUTE_DOCUMENT_NAME: &str = "document_name";
/// Source attribute: the document length unit.
const ATTRIBUTE_UNITS: &str = "units";
/// Source attribute: the feature record count.
const ATTRIBUTE_FEATURES: &str = "feature_records";
/// Source attribute: the parameter record count.
const ATTRIBUTE_PARAMS: &str = "parameter_records";

pub(crate) fn decode<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Decoded, CodecError> {
    let source = Source::open(ctx, root)?;
    let classification = source.classify();
    let audit = source.audit_blobs(ctx)?;
    let mut losses = source.opening_losses();
    losses.extend(classification.loss());
    losses.extend(audit.losses);
    let mut attributes = BTreeMap::new();
    if let Some(header) = source.header() {
        if let Some(name) = &header.name {
            attributes.insert(
                cadmpeg_core::nonblank_const!(ATTRIBUTE_DOCUMENT_NAME),
                name.clone(),
            );
        }
        if let Some(units) = &header.units {
            attributes.insert(
                cadmpeg_core::nonblank_const!(ATTRIBUTE_UNITS),
                units.clone(),
            );
        }
        attributes.insert(
            cadmpeg_core::nonblank_const!(ATTRIBUTE_FEATURES),
            header.features.to_string(),
        );
        attributes.insert(
            cadmpeg_core::nonblank_const!(ATTRIBUTE_PARAMS),
            header.params.to_string(),
        );
        losses.extend(document_losses(header));
    }
    match &source.package {
        Some(package) => losses.extend(package_losses(package, source.header().is_some())),
        None => losses.push(SolidcraftLossCode::ModelCacheAbsent.note(
            "a plain JSON design carries no model cache: its geometry exists only as the \
             evaluation of its feature history",
        )),
    }
    let mut ir = CadIr::decoded(SourceMeta::classified(
        DialectLayers::of(classification.into_matched()),
        attributes,
    ));
    let mut annotations = Annotations::default();
    let mut geometry_transferred = false;
    let mut outputs = BTreeMap::new();
    if let (Some(package), Some(_)) = (&source.package, source.header()) {
        if !ctx.container_only() && has_cache(package) {
            let document = package.member(ctx, DOCUMENT_MEMBER)?;
            let appearances = source
                .document
                .as_ref()
                .and_then(|document| document.object.get("appearances"));
            let outcome = crate::cache::decode(
                ctx,
                package,
                document,
                appearances,
                &mut ir,
                &mut annotations,
            )?;
            geometry_transferred = outcome.bodies > 0;
            losses.extend(outcome.losses);
            outputs = outcome.outputs;
        }
    }
    if let Some(document) = &source.document {
        // An earlier format reads its records through the migrations to the
        // current one, which this decoder does not apply: its history stays
        // retained.
        let current = document
            .header
            .tag
            .version()
            .is_some_and(|version| version >= crate::dialect::CURRENT_FORMAT);
        if !current && document.header.features > 0 {
            losses.push(SolidcraftLossCode::HistoryUndecoded.note(format!(
                "{} feature records of an earlier document format are retained without \
                 decoding: their format is read through migrations that this decoder does not \
                 apply",
                document.header.features
            )));
        } else if ctx.depth() >= DecodeDepth::Design {
            let outcome = crate::history::decode(ctx, &document.object, &outputs, &mut ir)?;
            losses.extend(outcome.losses);
        }
    }
    ir.finalize();
    let mut source_fidelity =
        SourceFidelity::with_annotations(annotations, Sha256Digest::digest(root.window()));
    if !ctx.container_only() {
        let records = retained_records(ctx, &source)?;
        source_fidelity.attach_native_unknown_records(&mut ir, FORMAT, records, ctx)?;
    }
    Ok(Decoded {
        ir,
        body: DecodeBody {
            losses,
            ..DecodeBody::new(DecodeTransfer::at(ctx.depth(), geometry_transferred))
        },
        source_fidelity,
    })
}

/// The losses for the document content that this decoder retains without
/// decoding.
fn document_losses(header: &Header) -> Vec<LossNote> {
    let mut losses = Vec::new();
    if header.params > 0 {
        losses.push(SolidcraftLossCode::ParametersUndecoded.note(format!(
            "{} parameter records are retained without decoding",
            header.params
        )));
    }
    if !header.sections.is_empty() {
        losses.push(SolidcraftLossCode::DocumentSectionUndecoded.note(format!(
            "document sections retained without decoding: {}",
            header.sections.join(", ")
        )));
    }
    losses
}

/// Whether the container holds both members of a model cache.
fn has_cache(package: &Package<'_>) -> bool {
    package.contains(crate::cache::MANIFEST_MEMBER)
        && package.contains(crate::cache::PAYLOAD_MEMBER)
}

/// The losses for the container members that this decoder retains without
/// decoding.
fn package_losses(package: &Package<'_>, has_document: bool) -> Vec<LossNote> {
    let mut losses = Vec::new();
    let mut payloads: BTreeMap<BlobKind, usize> = BTreeMap::new();
    let mut cache = false;
    let mut retained = Vec::new();
    for (name, role) in package.roles() {
        match role {
            MemberRole::Blob(blob) => *payloads.entry(blob.kind).or_default() += 1,
            MemberRole::ModelManifest | MemberRole::ModelBodies => cache = true,
            MemberRole::Preview
            | MemberRole::Thumbnail
            | MemberRole::ThumbnailKey
            | MemberRole::Cache
            | MemberRole::Plugin
            | MemberRole::Provenance
            | MemberRole::Other => retained.push(name.as_str()),
            MemberRole::Marker | MemberRole::Document | MemberRole::Drawing => {}
        }
    }
    if *package.marker() == Marker::Drawing {
        losses.push(
            SolidcraftLossCode::DrawingUndecoded
                .note("the container holds a drawing document, retained without decoding"),
        );
    } else if has_document && !cache {
        losses.push(SolidcraftLossCode::ModelCacheAbsent.note(
            "the container carries no model cache: its geometry exists only as the \
             evaluation of its feature history",
        ));
    }
    if !payloads.is_empty() {
        losses.push(SolidcraftLossCode::PayloadUndecoded.note(format!(
            "blob payloads retained without decoding: {}",
            payloads
                .iter()
                .map(|(kind, count)| format!("{count} {}", kind.extension()))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    if !retained.is_empty() {
        losses.push(SolidcraftLossCode::EntryRetained.note(format!(
            "members retained without decoding: {}",
            retained.join(", ")
        )));
    }
    losses
}

/// One unknown record per container member, or one for the whole plain JSON
/// document, each holding its expanded bytes.
fn retained_records(
    ctx: &DecodeContext<'_>,
    source: &Source<'_>,
) -> Result<Vec<UnknownRecord>, CodecError> {
    let namespace = cadmpeg_ir::identity_namespace!("solidcraft", "native", "entry");
    let mut records = Vec::new();
    let mut seen = BTreeSet::new();
    let mut retain = |name: &str, bytes: &[u8]| -> Result<(), CodecError> {
        if !seen.insert(name.to_owned()) {
            return Ok(());
        }
        ctx.retain_bytes(u64_from_index(bytes.len()), "retain SolidCraft member")?;
        records.push(UnknownRecord::retained(
            UnknownId::compose(&namespace, IdentityKey::encode_segment(name)),
            0,
            bytes.to_vec(),
            Vec::new(),
        ));
        Ok(())
    };
    match &source.package {
        Some(package) => {
            for name in package.roles().keys() {
                let view = match package.member(ctx, name) {
                    Ok(view) => view,
                    Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                    Err(_) => package.stored(name)?,
                };
                retain(name, view.window())?;
            }
        }
        None => retain(JSON_DOCUMENT_ENTRY, source.root.window())?,
    }
    Ok(records)
}
