// SPDX-License-Identifier: Apache-2.0
//! Target resolution: what an `FCStd` write is allowed to be.
//!
//! The one gate on this codec's write law. [`Encoder::plan`] resolves here, and
//! [`super::write_seekable`] then carries out what a
//! [`Resolution`] settled without re-deciding any of it.
//!
//! [`Encoder::plan`]: cadmpeg_ir::codec::write::Encoder::plan

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    carried_source_losses, edit_dropped, edit_unverified, target::ResolvedWrite, ArenaCoverage,
    Consumption, EncodeInput, ExportBody, PatchConsumption, WritePath,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::schema::EntityKind;
use cadmpeg_ir::SourceFidelity;

use super::{recover, write, Omissions};
use crate::dialect::FcstdDialect;
use crate::native::{DocumentFacts, EntryRecord};
use crate::persistence::Vocabulary;

/// What resolving a [`cadmpeg_ir::codec::write::target::TargetRequest`] against the source decided.
///
/// This writer has one capability. It patches the retained
/// `Document.xml` and regenerates none, so the only dialect it can deliver is
/// the one the retained document declares. Only [`resolve`] and
/// [`retained_baseline`] build one: its fields are private, so a `Resolution`
/// in hand proves that the retained archive entries hold a `Document.xml` that
/// reads as the dialect it carries. [`write_seekable`] takes that proof instead
/// of raw options, which is why it needs no target gate of its own.
///
/// [`write_seekable`]: super::write_seekable
#[derive(Debug)]
pub(super) struct Resolution<'a> {
    ir: &'a CadIr,
    namespace: &'a cadmpeg_ir::native::NativeNamespace,
    document: DocumentFacts,
    schema_version: String,
    vocabulary: Vocabulary,
    target: DialectId,
    document_xml: String,
    /// The other retained entries that read, each name once, in arena order.
    entries: Vec<EntryRecord>,
    omitted: Omissions,
}

impl<'a> Resolution<'a> {
    pub(super) const fn ir(&self) -> &'a CadIr {
        self.ir
    }

    pub(super) const fn namespace(&self) -> &'a cadmpeg_ir::native::NativeNamespace {
        self.namespace
    }

    pub(super) const fn document(&self) -> &DocumentFacts {
        &self.document
    }

    pub(super) fn schema_version(&self) -> &str {
        &self.schema_version
    }

    pub(super) const fn vocabulary(&self) -> Vocabulary {
        self.vocabulary
    }

    pub(super) const fn target(&self) -> &DialectId {
        &self.target
    }

    pub(super) fn document_xml(&self) -> &str {
        &self.document_xml
    }

    pub(super) fn entries(&self) -> &[EntryRecord] {
        &self.entries
    }

    pub(super) const fn omitted(&self) -> &Omissions {
        &self.omitted
    }
}

const BASELINE_UNAVAILABLE: &str =
    "the document holds no FCStd archive entry with a readable Document.xml, and this writer \
     regenerates no Document.xml, so the target cannot be written";
const TRANSCODE_UNAVAILABLE: &str =
    "the resolved target displaces the retained FCStd source dialect, and this writer \
     regenerates no Document.xml, so the target cannot be written";
const DIALECT_UNAVAILABLE: &str =
    "the retained Document.xml declares another schema than the resolved target, and this \
     writer regenerates no Document.xml, so the target cannot be written";

/// Resolve the request against the source, then plan the export it names.
///
/// `Explicit(id)` refuses an id outside the synthesis catalog. It is otherwise
/// the replay law's compare: the retained document is written back exactly when
/// its `Document.xml` declares `id`, and an id that displaces the recorded
/// source dialect is a transcode this writer cannot perform, refused by name
/// with the catalog.
///
/// `Inherit` asks for preservation instead. This writer repacks the retained
/// entry set and patches `Document.xml` inside it, which reproduces whatever
/// schema the source declared — schema 2 and schema 3 included, neither of which
/// is a synthesis target. There is no fall-through to the catalog default: a
/// same-format conversion never silently changes what the file is.
///
/// The source bytes are the archive entries the document's `fcstd` records
/// hold (§3.4, Paths), whatever format `ir.source` names. A document that holds
/// no such entry with a readable `Document.xml` gives this writer, which has no
/// synthesis path, nothing to write.
pub(crate) fn plan(
    input: EncodeInput<'_>,
    resolved: &ResolvedWrite<'_>,
) -> Result<ExportBody, CodecError> {
    let resolution = resolve(input.ir, resolved)?;
    let baseline = admit_unedited(input.ir, input.fidelity)?;
    finish(&resolution, input.fidelity, baseline)
}

/// Native `fcstd` arenas this writer handles itself: property values patch
/// `Document.xml`, entry payloads are repacked, and object and extension
/// declaration edits are reported as dropped.
const CARRIED_NATIVE_ARENAS: &[&str] = &["entries", "extensions", "objects", "properties"];

/// The codec digest (§3.1, Rule B) under which a `Full` decode records
/// [`uncarried_digest`] and an `FCStd` write compares it.
pub(crate) const UNCARRIED_DIGEST: &str = "fcstd_uncarried_local_sha256";

/// The digest an `FCStd` write compares to find edits it cannot write.
///
/// Covers every neutral arena, the source metadata, and every native record
/// except [`CARRIED_NATIVE_ARENAS`]. Outside those arenas the writer reads only
/// the physical ledger, to order the archive, so a change to anything this
/// digest covers does not reach the output.
pub(crate) fn uncarried_digest(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<Sha256Digest, CodecError> {
    // FCStd retains no whole-file source image, so no unknown record is
    // excluded as one.
    let digest = cadmpeg_ir::hash::document_local_sha256_without_carried(
        ctx,
        ir,
        ir.source.as_ref(),
        "fcstd",
        "",
        CARRIED_NATIVE_ARENAS,
    )?;
    Sha256Digest::try_from(digest).map_err(|_| {
        CodecError::Malformed("the FCStd uncarried digest is no SHA-256 digest".into())
    })
}

/// What [`admit_unedited`] could establish about edits since decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Baseline {
    /// The digest matches: nothing outside [`CARRIED_NATIVE_ARENAS`] changed.
    Unedited,
    /// The digest differs: an edit outside [`CARRIED_NATIVE_ARENAS`] has no
    /// serializer here and is left as the source stores it.
    Edited,
    /// The sidecar records no digest, so the document's edits are not known.
    Unrecorded,
}

/// Compare the document with the digest its decode recorded.
///
/// A document whose sidecar records no digest (no sidecar, or every decode
/// below `Full`) is [`Baseline::Unrecorded`].
fn admit_unedited(ir: &CadIr, fidelity: Option<&SourceFidelity>) -> Result<Baseline, CodecError> {
    let Some(expected) = fidelity.and_then(|fidelity| fidelity.codec_digests.get(UNCARRIED_DIGEST))
    else {
        return Ok(Baseline::Unrecorded);
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )?;
    let current = uncarried_digest(&ctx, ir)?;
    ctx.finish_session()?;
    Ok(if current == *expected {
        Baseline::Unedited
    } else {
        Baseline::Edited
    })
}

/// Write the resolved export. The sealed encoder stamps the target identity
/// and the fidelity resolution; this writer patches the retained document and
/// consumes no sidecar. Every write is a patch, so `untyped` retained records
/// in `fidelity` charge both carried-source codes. A [`Baseline::Unrecorded`]
/// document charges `export:edit.unverified`, and a [`Baseline::Edited`] one
/// charges `export:edit.dropped` naming the document, because the digest does
/// not isolate the edited record (§3.4, Paths item 4).
fn finish(
    resolution: &Resolution<'_>,
    fidelity: Option<&cadmpeg_ir::SourceFidelity>,
    baseline: Baseline,
) -> Result<ExportBody, CodecError> {
    let mut bytes = Vec::new();
    let outcome = write(&mut bytes, resolution)?;
    let write_path = WritePath::Patched {
        consumption: PatchConsumption::Independent(Consumption::NotConsumed),
    };
    let mut losses = carried_source_losses(fidelity, &write_path);
    match baseline {
        Baseline::Unedited => {}
        Baseline::Unrecorded => losses.push(edit_unverified(format!(
            "the FCStd decode sidecar records no `{UNCARRIED_DIGEST}` codec digest, so edits \
             since decode are not known; the file patches the retained Document.xml with the \
             carried native records and holds no other edit"
        ))),
        Baseline::Edited => losses.extend(edit_dropped(
            EntityKind::Feature,
            "FCStd",
            "patches the retained Document.xml with the carried native records and holds \
             the other records as the source stores them; the uncarried digest names no \
             record, so the entry names the document",
            [resolution.document().id.clone()],
        )),
    }
    losses.extend(outcome.losses);
    Ok(ExportBody {
        bytes,
        census: outcome.census,
        losses,
        write_path,
        coverage: ArenaCoverage::Complete,
        notes: outcome.notes,
    })
}

/// Decide what to write, from the request and the source.
fn resolve<'a>(ir: &'a CadIr, resolved: &ResolvedWrite<'_>) -> Result<Resolution<'a>, CodecError> {
    if resolved.displaced_source().is_some() {
        return Err(resolved.unavailable(TRANSCODE_UNAVAILABLE));
    }
    let resolution = retained_baseline(ir, resolved.target_id())?
        .ok_or_else(|| resolved.unavailable(BASELINE_UNAVAILABLE))?;
    if FcstdDialect::from_schema_version(resolution.schema_version()).id() != *resolved.target_id()
    {
        return Err(resolved.unavailable(DIALECT_UNAVAILABLE));
    }
    Ok(resolution)
}

/// The retained archive the write repacks, witnessed by its own
/// `Document.xml`.
///
/// The entries are the whole baseline: the writer never regenerates a
/// `Document.xml`, so it writes exactly when the `fcstd` entry records hold a
/// `Document.xml` that the decoder admits. Its `SchemaVersion` and
/// document facts come from those bytes, which are what the output holds. A
/// `Container` decode retains no entry. An entry record that does not read, or
/// whose name an earlier entry already takes, is left out of the archive and
/// reported. `target` is the dialect the resolution names.
pub(super) fn retained_baseline<'a>(
    ir: &'a CadIr,
    target: &DialectId,
) -> Result<Option<Resolution<'a>>, CodecError> {
    let Some(namespace) = ir.native.namespace("fcstd") else {
        return Ok(None);
    };
    let mut omitted = Omissions::default();
    let (records, unreadable) = super::read_arena::<EntryRecord>(namespace, "entries")?;
    for id in unreadable {
        omitted.push(super::UNREADABLE_ENTRY, id);
    }
    let mut names = std::collections::HashSet::new();
    let mut document_xml = None;
    let mut entries = Vec::with_capacity(records.len());
    for entry in records {
        if !names.insert(entry.name().to_owned()) {
            omitted.push(super::DUPLICATE_ENTRY, entry.id());
        } else if entry.name() == "Document.xml" {
            document_xml = Some(entry);
        } else {
            entries.push(entry);
        }
    }
    let Some(document_xml) = document_xml else {
        return Ok(None);
    };
    let Some((document, schema_version, vocabulary)) = read_declaration(document_xml.data())?
    else {
        return Ok(None);
    };
    let Ok(document_xml) = String::from_utf8(document_xml.data().to_vec()) else {
        return Ok(None);
    };
    Ok(Some(Resolution {
        ir,
        namespace,
        document,
        schema_version,
        vocabulary,
        target: target.clone(),
        document_xml,
        entries,
        omitted,
    }))
}

/// The document facts, `SchemaVersion`, and vocabulary that `bytes` declare,
/// or `None` where they do not read as a `Document.xml` the decoder admits.
fn read_declaration(
    bytes: &[u8],
) -> Result<Option<(DocumentFacts, String, Vocabulary)>, CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy)?;
    let declared = crate::container::admit_document(&ctx, bytes).and_then(|xml| {
        let (document, schema_version) = crate::container::parse_document(&xml, &mut Vec::new())?;
        let vocabulary = Vocabulary::from_declaration(&schema_version)?;
        Ok((document, schema_version, vocabulary))
    });
    match declared {
        Ok(declared) => Ok(Some(declared)),
        Err(error) => recover(error).map(|()| None),
    }
}
