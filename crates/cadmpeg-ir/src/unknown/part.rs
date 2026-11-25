// SPDX-License-Identifier: Apache-2.0
//! Giving up one part a decoder cannot read.
//!
//! A part is a unit the format frames by itself: an archive entry, a
//! compressed stream, a record with its own length or terminator, a section,
//! an XML element. A decoder that cannot read a part gives up that part only:
//! one `unknowns` stub stands for it, linked to the innermost record that
//! contains it, its bytes are retained, and one `record.unreadable` loss
//! names it. Nothing else in the document describes the failure.

use std::collections::BTreeSet;

use cadmpeg_core::decode::{u64_from_index, ArenaClass, DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::document::CadIr;
use crate::hash::digest::Sha256Digest;
use crate::ids::{Identity, UnknownId};
use crate::native::NativeRecord;
use crate::provenance::{SourceOwner, SourceProvenance, StreamName};
use crate::report::loss::{LossKind, LossNote, LossTaxonomy, NamespacedLossKind};
use crate::report::Severity;
use crate::source_fidelity::{RetainedBytes, RetainedSourceRecord, SourceFidelity};
use crate::unknown::NativeUnknownRecord;

/// The `record.unreadable` loss: one per given-up part.
#[must_use]
pub fn record_unreadable_kind() -> LossKind {
    NamespacedLossKind::new(
        crate::loss_namespace!("decode"),
        "record.unreadable",
        LossTaxonomy::DecodeDiagnostic,
    )
    .into()
}

/// One part as its decoder names it before reading it.
#[derive(Debug, Clone)]
pub struct Part<'a> {
    codec: &'static str,
    kind: &'static str,
    name: Option<String>,
    stream: Option<StreamName>,
    offset: u64,
    bytes: &'a [u8],
    holds: BTreeSet<ArenaClass>,
    containing: Option<Identity>,
    dropped: Vec<Identity>,
    lost: Vec<String>,
    consequence: Option<String>,
    digest_only: bool,
}

impl<'a> Part<'a> {
    /// A part of `kind` whose bytes are `extent`, in the root stream of
    /// `codec`'s source, holding content of the classes in `holds`.
    #[must_use]
    pub fn new(
        codec: &'static str,
        kind: &'static str,
        extent: View<'a>,
        holds: impl IntoIterator<Item = ArenaClass>,
    ) -> Self {
        Self::at(
            codec,
            kind,
            u64_from_index(extent.start()),
            extent.window(),
            holds,
        )
    }

    /// A part of `kind` whose `bytes` start at `offset` in the root stream
    /// of `codec`'s source, holding content of the classes in `holds`.
    ///
    /// The classes are those the part's bytes can hold. The retained bytes of
    /// a given-up part are marked `untyped` with them.
    #[must_use]
    pub fn at(
        codec: &'static str,
        kind: &'static str,
        offset: u64,
        bytes: &'a [u8],
        holds: impl IntoIterator<Item = ArenaClass>,
    ) -> Self {
        Self {
            codec,
            kind,
            name: None,
            stream: None,
            offset,
            bytes,
            holds: holds.into_iter().collect(),
            containing: None,
            dropped: Vec::new(),
            lost: Vec::new(),
            consequence: None,
            digest_only: false,
        }
    }

    /// Gives the name the source stores for the part, such as an archive
    /// entry's path, for the loss to report.
    #[must_use]
    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Places the part in a named stream. The part's offset is an offset in
    /// that stream.
    #[must_use]
    pub fn in_stream(mut self, stream: StreamName) -> Self {
        self.stream = Some(stream);
        self
    }

    /// Links the part to the innermost record that contains it.
    #[must_use]
    pub fn contained_in(mut self, record: Identity) -> Self {
        self.containing = Some(record);
        self
    }

    /// Names records the decoder drops because they need this part.
    #[must_use]
    pub fn dropping(mut self, records: impl IntoIterator<Item = Identity>) -> Self {
        self.dropped.extend(records);
        self
    }

    /// Names entries removed from records the decoder keeps because the
    /// entries need this part. Each text names the record and the entry it
    /// lost.
    #[must_use]
    pub fn losing_entries(mut self, entries: impl IntoIterator<Item = String>) -> Self {
        self.lost.extend(entries);
        self
    }

    /// States what else the document lacks because the part was given up,
    /// such as content the part would have placed.
    #[must_use]
    pub fn costing(mut self, consequence: impl Into<String>) -> Self {
        self.consequence = Some(consequence.into());
        self
    }

    /// Keeps only the length and digest of the part's bytes, for a decoder
    /// that already retains them in a record holding the whole stream.
    #[must_use]
    pub fn digest_only(mut self) -> Self {
        self.digest_only = true;
        self
    }

    /// The id of the part's stub, minted from codec, part kind, stream, and
    /// offset.
    ///
    /// # Errors
    ///
    /// [`CodecError::Malformed`] when the codec name cannot open an id.
    pub fn id(&self) -> Result<UnknownId, CodecError> {
        part_id(
            self.codec,
            &part_address(self.kind, self.stream.as_ref(), self.offset),
        )
    }

    fn offset(&self) -> u64 {
        self.offset
    }
}

/// The address a part's stub id is minted from: its kind, stream, and
/// offset, each length-prefixed.
fn part_address(kind: &str, stream: Option<&StreamName>, offset: u64) -> Vec<u8> {
    let stream = stream.map_or("", StreamName::as_str);
    let mut address = Vec::new();
    for field in [kind.as_bytes(), stream.as_bytes()] {
        address.extend_from_slice(&u64_from_index(field.len()).to_be_bytes());
        address.extend_from_slice(field);
    }
    address.extend_from_slice(&offset.to_be_bytes());
    address
}

/// The address the stub id of a record given up after the decode is minted
/// from: its kind and its path in the document, each length-prefixed, after
/// a tag no length of a part's kind takes.
fn record_address(kind: &str, path: &str) -> Vec<u8> {
    let mut address = (u64::MAX - 1).to_be_bytes().to_vec();
    for field in [kind.as_bytes(), path.as_bytes()] {
        address.extend_from_slice(&u64_from_index(field.len()).to_be_bytes());
        address.extend_from_slice(field);
    }
    address
}

fn part_id(codec: &str, address: &[u8]) -> Result<UnknownId, CodecError> {
    UnknownId::mint(format!(
        "{codec}:source:part#{}",
        crate::hash::sha256_hex(address)
    ))
    .map_err(CodecError::malformed)
}

/// A part its decoder gave up: the stub, the retained bytes, and the loss.
#[derive(Debug, Clone)]
pub struct GivenUpPart {
    codec: &'static str,
    kind: &'static str,
    name: Option<String>,
    stream: Option<StreamName>,
    offset: u64,
    stub: NativeUnknownRecord,
    retained: RetainedSourceRecord,
    loss: LossNote,
    /// The loss message before the records the give-up drops.
    head: String,
    tail: Option<Tail>,
}

/// The records a give-up drops and marks and the entries it loses, which
/// its loss names after the head.
#[derive(Debug, Clone, Default)]
struct Tail {
    dropped: Vec<String>,
    marked: Option<Vec<String>>,
    /// The outcome of each stored state that the release changed.
    stated: Vec<String>,
    lost: Vec<String>,
}

/// The loss message of a part: `head`, then the lists of `tail`.
fn message(head: &str, tail: Option<&Tail>) -> String {
    let Some(tail) = tail else {
        return head.to_owned();
    };
    let mut message = format!("{head}; dropped=[{}]", tail.dropped.join(", "));
    if let Some(marked) = &tail.marked {
        message.push_str("; marked=[");
        message.push_str(&marked.join(", "));
        message.push(']');
    }
    for stated in &tail.stated {
        message.push_str("; ");
        message.push_str(stated);
    }
    if !tail.lost.is_empty() {
        message.push_str("; lost=[");
        message.push_str(&tail.lost.join(", "));
        message.push(']');
    }
    message
}

/// Puts the ids of `more` into the sorted set `ids`.
fn join_sorted(ids: &mut Vec<String>, more: impl IntoIterator<Item = String>) {
    ids.extend(more);
    ids.sort_unstable();
    ids.dedup();
}

impl GivenUpPart {
    /// The stub id of a record the decode wrapper gives up after the decode,
    /// minted from codec, the record's kind, and its path in the document,
    /// which stand for the part's kind, stream, and offset.
    ///
    /// # Errors
    ///
    /// [`CodecError::Malformed`] when the codec name cannot open an id.
    pub(crate) fn record_id(codec: &str, kind: &str, path: &str) -> Result<UnknownId, CodecError> {
        part_id(codec, &record_address(kind, path))
    }

    /// A record the decode wrapper gives up after the decode, as a part: its
    /// kind is the record's, its stream is the record's path in the
    /// document, its bytes are the record's canonical JSON `text`, kept
    /// `untyped` with `classes`, and `loss` names it. It sits in no record,
    /// so its stub links to the document.
    ///
    /// # Errors
    ///
    /// [`CodecError::Malformed`] when `path` is empty or the codec name
    /// cannot open an id.
    pub(crate) fn of_record(
        codec: &'static str,
        kind: &'static str,
        path: &str,
        text: Vec<u8>,
        classes: impl IntoIterator<Item = ArenaClass>,
        loss: LossNote,
    ) -> Result<Self, CodecError> {
        let stream = StreamName::try_from(path.to_owned())
            .map_err(|_| CodecError::malformed("a given-up record has an empty path"))?;
        let retained = RetainedSourceRecord::from_bytes(
            SourceOwner::from(stream.clone()),
            0,
            RetainedBytes::Inline { data: text },
        )?
        .with_untyped(classes);
        Ok(Self {
            codec,
            kind,
            name: None,
            stream: Some(stream),
            offset: 0,
            stub: NativeUnknownRecord {
                id: Self::record_id(codec, kind, path)?,
                links: Vec::new(),
            },
            retained,
            head: loss.message.clone(),
            tail: None,
            loss,
        })
    }

    #[doc = "Coalesce repeated readings of the same source part, retaining every containing link."]
    #[doc = "Returns a malformed error when equal part identities carry different source images or losses."]
    pub fn coalesce(mut parts: Vec<Self>) -> Result<Vec<Self>, CodecError> {
        parts.sort_by(|left, right| left.id().cmp(right.id()));
        let mut conflict = false;
        parts.dedup_by(|next, previous| {
            if next.id() != previous.id() {
                return false;
            }
            if next.codec != previous.codec
                || next.kind != previous.kind
                || next.stream != previous.stream
                || next.offset != previous.offset
                || next.retained != previous.retained
                || next.loss != previous.loss
            {
                conflict = true;
                return false;
            }
            previous.stub.links.append(&mut next.stub.links);
            true
        });
        if conflict {
            Err(CodecError::malformed(
                "repeated given-up part has contradictory source images or losses",
            ))
        } else {
            for part in &mut parts {
                part.stub.links.sort();
                part.stub.links.dedup();
            }
            Ok(parts)
        }
    }

    /// The stub's id.
    #[must_use]
    pub fn id(&self) -> &UnknownId {
        &self.stub.id
    }

    /// The part's kind, as its decoder named it.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        self.kind
    }

    /// The name the source stores for the part, where its decoder read one.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// The stream that holds the part, where its decoder named one.
    #[must_use]
    pub const fn stream(&self) -> Option<&StreamName> {
        self.stream.as_ref()
    }

    /// Restates a part of a document that the stream `container` of an
    /// outer document holds, for a decoder that reads the nested document
    /// into the outer one. The part's stream becomes `container`, followed
    /// by `/` and the part's own stream where it has one; its offset stays
    /// an offset in that stream. The stub id is minted from `container` and
    /// the part's own address, so it equals no id a part of the outer
    /// document takes. The loss is restated with the new stream and id.
    ///
    /// # Errors
    ///
    /// [`CodecError::Malformed`] when the stream name or the id cannot be
    /// formed, or a resource refusal for the restated loss text.
    pub fn nested_in(
        self,
        ctx: &DecodeContext<'_>,
        container: &StreamName,
    ) -> Result<Self, CodecError> {
        let Self {
            codec,
            kind,
            name,
            stream,
            offset,
            mut stub,
            retained,
            mut loss,
            mut head,
            tail,
        } = self;
        let nested = match &stream {
            Some(inner) => {
                StreamName::try_from(format!("{}/{}", container.as_str(), inner.as_str()))
                    .map_err(CodecError::malformed)?
            }
            None => container.clone(),
        };
        let mut address = u64::MAX.to_be_bytes().to_vec();
        address.extend_from_slice(&u64_from_index(container.as_str().len()).to_be_bytes());
        address.extend_from_slice(container.as_str().as_bytes());
        address.extend_from_slice(&part_address(kind, stream.as_ref(), offset));
        let id = part_id(codec, &address)?;
        if loss.code == record_unreadable_kind() {
            restate_record(&mut head, stub.id.as_str(), id.as_str());
        }
        let old_field = format!(
            " stream={:?} offset=",
            stream.as_ref().map_or("", StreamName::as_str)
        );
        if let Some(at) = head.find(&old_field) {
            head.replace_range(
                at..at + old_field.len(),
                &format!(" stream={:?} offset=", nested.as_str()),
            );
        }
        loss.message = message(&head, tail.as_ref());
        ctx.retain_bytes(
            u64_from_index(loss.message.len()),
            "nested given-up part loss",
        )?;
        loss.provenance =
            Some(SourceProvenance::in_stream(codec, nested.clone(), offset).with_tag(kind));
        stub.id = id;
        Ok(Self {
            codec,
            kind,
            name,
            stream: Some(nested.clone()),
            offset,
            stub,
            retained: retained.with_owner(nested),
            loss,
            head,
            tail,
        })
    }

    /// A copy of the `record.unreadable` loss that names the part, for a
    /// report that does not commit it.
    #[must_use]
    pub fn loss(&self) -> LossNote {
        self.loss.clone()
    }

    /// Names in the loss the records that a release drops and marks because
    /// they need the part's record, and the `stated` outcome of each stored
    /// state it changed, for a decoder that releases them after it gave the
    /// part up (§3.3).
    ///
    /// # Errors
    ///
    /// A resource refusal for the restated loss text.
    pub(crate) fn name_released(
        &mut self,
        ctx: &DecodeContext<'_>,
        dropped: impl IntoIterator<Item = String>,
        marked: impl IntoIterator<Item = String>,
        stated: impl IntoIterator<Item = String>,
    ) -> Result<(), CodecError> {
        let tail = self.tail.get_or_insert_with(Tail::default);
        join_sorted(&mut tail.dropped, dropped);
        join_sorted(tail.marked.get_or_insert_with(Vec::new), marked);
        tail.stated.extend(stated);
        let restated = message(&self.head, Some(tail));
        ctx.retain_bytes(u64_from_index(restated.len()), "given-up part loss")?;
        self.loss.message = restated;
        Ok(())
    }

    /// Links the stub to the innermost record that contains the part, for a
    /// decoder that creates that record after it gave the part up.
    #[must_use]
    pub fn contained_in(mut self, record: Identity) -> Self {
        self.stub.links = vec![record];
        self
    }

    /// Adds the stub to the codec's native `unknowns` arena of `ir` and the
    /// part's bytes to `fidelity`, and returns the loss for the decoder's
    /// report. On failure `ir` and `fidelity` are unchanged.
    ///
    /// # Errors
    ///
    /// A resource refusal for the stub's text, or [`CodecError::Malformed`]
    /// when the stub's id is already present.
    pub fn commit(
        self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        fidelity: &mut SourceFidelity,
    ) -> Result<LossNote, CodecError> {
        let Self {
            codec,
            stub,
            retained,
            loss,
            ..
        } = self;
        let codec = crate::native::namespace_codec(codec)?;
        let record = NativeRecord::from(&stub);
        let taken = ir.native.0.values().any(|namespace| {
            namespace
                .arenas()
                .values()
                .flatten()
                .any(|existing| existing.id() == record.id())
        });
        if taken || fidelity.retained_record(stub.id.as_str()).is_some() {
            return Err(CodecError::malformed(format_args!(
                "given-up part {} is already present",
                stub.id
            )));
        }
        ctx.retain_bytes(u64_from_index(record.text_len()), "given-up part stub")?;
        fidelity.insert_retained_record(stub.id, retained)?;
        let arena = ir
            .native
            .namespace_mut(codec)
            .arenas_mut()
            .entry(cadmpeg_core::nonblank_literal!("unknowns"))
            .or_default();
        let at = arena.partition_point(|existing| existing.id() < record.id());
        arena.insert(at, record);
        Ok(loss)
    }
}

/// Restates the `record.unreadable` loss of the part whose stub id changed
/// from `from` to `to`, as when a decoder composing several documents
/// rescopes one document's records. Returns whether `loss` names `from`.
pub fn restate_loss(loss: &mut LossNote, from: &str, to: &str) -> bool {
    loss.code == record_unreadable_kind() && restate_record(&mut loss.message, from, to)
}

/// Restates the `record=` field of the loss text `text` from `from` to
/// `to`. Returns whether `text` names `from`.
fn restate_record(text: &mut String, from: &str, to: &str) -> bool {
    let field = format!(" record={from}: ");
    let Some(at) = text.find(&field) else {
        return false;
    };
    text.replace_range(at..at + field.len(), &format!(" record={to}: "));
    true
}

/// Links the committed stub `id` in `codec`'s `unknowns` arena to the
/// innermost record that contains its part, replacing any link it had, for a
/// decoder that defines that record after the commit, as when a decoder
/// composing several documents defines the member that holds the part.
/// Returns whether `ir` holds the stub.
///
/// # Errors
///
/// [`CodecError::Malformed`] when `id` cannot name a stub.
pub fn link_committed(
    ir: &mut CadIr,
    codec: &str,
    id: &str,
    record: Identity,
) -> Result<bool, CodecError> {
    let Some(arena) = ir
        .native
        .0
        .get_mut(codec)
        .and_then(|namespace| namespace.arenas_mut().get_mut("unknowns"))
    else {
        return Ok(false);
    };
    let Some(stub) = arena.iter_mut().find(|stub| stub.id() == id) else {
        return Ok(false);
    };
    *stub = NativeRecord::from(&NativeUnknownRecord {
        id: UnknownId::mint(id.to_owned()).map_err(CodecError::malformed)?,
        links: vec![record],
    });
    Ok(true)
}

/// Gives up the parts a decoder cannot read.
pub trait DecodeParts {
    /// Gives up `part` after its decode, run inside
    /// [`DecodeContext::run_part`], failed with `error`.
    ///
    /// # Errors
    ///
    /// `error` itself when it is fatal to the session, a refusal in force,
    /// or a resource refusal for the retained bytes.
    fn give_up_part(&self, part: Part<'_>, error: CodecError) -> Result<GivenUpPart, CodecError>;
}

impl DecodeParts for DecodeContext<'_> {
    fn give_up_part(&self, part: Part<'_>, error: CodecError) -> Result<GivenUpPart, CodecError> {
        if let Some(limit) = self.resource_refusal() {
            return Err(limit.into());
        }
        if let CodecError::Io(_)
        | CodecError::ResourceLimit(cadmpeg_core::decode::ResourceLimit {
            scope: cadmpeg_core::decode::LimitScope::Session,
            ..
        }) = error
        {
            return Err(error);
        }
        let id = part.id()?;
        let bytes = part.bytes;
        let length = u64_from_index(bytes.len());
        let retained_bytes = if part.digest_only {
            RetainedBytes::Digest {
                byte_len: length,
                sha256: Sha256Digest::digest(bytes),
            }
        } else {
            self.retain_bytes(length, "given-up part source bytes")?;
            let mut data = self.alloc_filled(bytes.len(), 0_u8, "given-up part source bytes")?;
            data.copy_from_slice(bytes);
            RetainedBytes::Inline { data }
        };
        let owner = part
            .stream
            .clone()
            .map_or(SourceOwner::Root, SourceOwner::from);
        let retained = RetainedSourceRecord::from_bytes(owner, part.offset(), retained_bytes)?
            .with_untyped(part.holds.iter().copied());
        let mut dropped: Vec<&str> = part.dropped.iter().map(Identity::as_str).collect();
        dropped.sort_unstable();
        dropped.dedup();
        let stream = part.stream.as_ref().map_or("", StreamName::as_str);
        let name = part
            .name
            .as_ref()
            .map(|name| format!(" {name:?}"))
            .unwrap_or_default();
        let consequence = part
            .consequence
            .as_ref()
            .map(|consequence| format!("; {consequence}"))
            .unwrap_or_default();
        let head = format!(
            "part {}{name} stream={stream:?} offset={} length={length} record={id}: {error}{consequence}",
            part.kind,
            part.offset(),
        );
        let tail = Tail {
            dropped: dropped.into_iter().map(str::to_owned).collect(),
            marked: None,
            stated: Vec::new(),
            lost: part.lost.clone(),
        };
        let message = message(&head, Some(&tail));
        self.retain_bytes(u64_from_index(message.len()), "given-up part loss")?;
        let provenance = match part.stream.clone() {
            Some(stream) => SourceProvenance::in_stream(part.codec, stream, part.offset()),
            None => SourceProvenance::root(part.codec, part.offset()),
        }
        .with_tag(part.kind);
        let loss = LossNote::new(record_unreadable_kind(), message)
            .with_severity(Severity::Error)
            .with_provenance(provenance);
        Ok(GivenUpPart {
            codec: part.codec,
            kind: part.kind,
            name: part.name,
            stream: part.stream,
            offset: part.offset,
            stub: NativeUnknownRecord {
                id,
                links: part.containing.into_iter().collect(),
            },
            retained,
            loss,
            head,
            tail: Some(tail),
        })
    }
}

#[cfg(test)]
mod tests;
