// SPDX-License-Identifier: Apache-2.0
//! Giving up one part a decoder cannot read.
//!
//! A part is a unit the format frames by itself: an archive entry, a
//! compressed stream, a record with its own length or terminator, a section,
//! an XML element. A decoder that cannot read a part gives up that part only:
//! one `unknowns` stub stands for it, linked to the innermost record that
//! contains it, its bytes are retained, and one `record.unreadable` loss
//! names it. Nothing else in the document describes the failure.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

use crate::document::CadIr;
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
    .with_strict_floor(Some(Severity::Error))
    .into()
}

/// One part as its decoder names it before reading it.
#[derive(Debug, Clone)]
pub struct Part<'a> {
    codec: &'static str,
    kind: &'static str,
    stream: Option<StreamName>,
    extent: View<'a>,
    containing: Option<Identity>,
    dropped: Vec<Identity>,
}

impl<'a> Part<'a> {
    /// A part of `kind` whose bytes are `extent`, in the root stream of
    /// `codec`'s source.
    #[must_use]
    pub fn new(codec: &'static str, kind: &'static str, extent: View<'a>) -> Self {
        Self {
            codec,
            kind,
            stream: None,
            extent,
            containing: None,
            dropped: Vec::new(),
        }
    }

    /// Places the part in a named stream. `extent`'s offsets are offsets in
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

    /// The id of the part's stub, minted from codec, part kind, stream, and
    /// offset.
    ///
    /// # Errors
    ///
    /// [`CodecError::Malformed`] when the codec name cannot open an id.
    pub fn id(&self) -> Result<UnknownId, CodecError> {
        let stream = self.stream.as_ref().map_or("", StreamName::as_str);
        let mut address = Vec::new();
        for field in [self.kind.as_bytes(), stream.as_bytes()] {
            address.extend_from_slice(&u64_from_index(field.len()).to_be_bytes());
            address.extend_from_slice(field);
        }
        address.extend_from_slice(&self.offset().to_be_bytes());
        UnknownId::mint(format!(
            "{}:source:part#{}",
            self.codec,
            crate::hash::sha256_hex(&address)
        ))
        .map_err(CodecError::malformed)
    }

    fn offset(&self) -> u64 {
        u64_from_index(self.extent.start())
    }
}

/// A part its decoder gave up: the stub, the retained bytes, and the loss.
#[derive(Debug)]
pub struct GivenUpPart {
    codec: &'static str,
    stub: NativeUnknownRecord,
    retained: RetainedSourceRecord,
    loss: LossNote,
}

impl GivenUpPart {
    /// The stub's id.
    #[must_use]
    pub fn id(&self) -> &UnknownId {
        &self.stub.id
    }

    /// The `record.unreadable` loss that names the part.
    #[must_use]
    pub fn loss(&self) -> &LossNote {
        &self.loss
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
        } = self;
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
            .entry("unknowns".to_owned())
            .or_default();
        let at = arena.partition_point(|existing| existing.id() < record.id());
        arena.insert(at, record);
        Ok(loss)
    }
}

/// The outcome of one part.
#[derive(Debug)]
pub enum PartOutcome<T> {
    /// The decoder read the part.
    Decoded(T),
    /// The decoder gave the part up.
    GivenUp(Box<GivenUpPart>),
}

/// Gives up the parts a decoder cannot read.
pub trait DecodeParts {
    /// Decodes one part. A failure of the part's own, including a
    /// Part-scoped resource refusal, gives the part up.
    ///
    /// # Errors
    ///
    /// The errors fatal to the session: a Session-scoped refusal, a refusal
    /// already in force, or an I/O error.
    fn decode_part<T>(
        &self,
        part: Part<'_>,
        decode: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<PartOutcome<T>, CodecError>;

    /// Gives up `part` after its decode failed with `error`.
    ///
    /// # Errors
    ///
    /// `error` itself when it is fatal to the session, a refusal in force,
    /// or a resource refusal for the retained bytes.
    fn give_up_part(&self, part: Part<'_>, error: CodecError) -> Result<GivenUpPart, CodecError>;
}

impl DecodeParts for DecodeContext<'_> {
    fn decode_part<T>(
        &self,
        part: Part<'_>,
        decode: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<PartOutcome<T>, CodecError> {
        match self.run_part(decode)? {
            Ok(value) => Ok(PartOutcome::Decoded(value)),
            Err(error) => self
                .give_up_part(part, error)
                .map(|given_up| PartOutcome::GivenUp(Box::new(given_up))),
        }
    }

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
        let bytes = part.extent.window();
        let length = u64_from_index(bytes.len());
        self.retain_bytes(length, "given-up part source bytes")?;
        let mut data = self.alloc_filled(bytes.len(), 0_u8, "given-up part source bytes")?;
        data.copy_from_slice(bytes);
        let owner = part
            .stream
            .clone()
            .map_or(SourceOwner::Root, SourceOwner::from);
        let retained =
            RetainedSourceRecord::from_bytes(owner, part.offset(), RetainedBytes::Inline { data })?;
        let mut dropped: Vec<&str> = part.dropped.iter().map(Identity::as_str).collect();
        dropped.sort_unstable();
        dropped.dedup();
        let stream = part.stream.as_ref().map_or("", StreamName::as_str);
        let message = format!(
            "part {} stream={stream:?} offset={} length={length} record={id}: {error}; dropped=[{}]",
            part.kind,
            part.offset(),
            dropped.join(", ")
        );
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
            stub: NativeUnknownRecord {
                id,
                links: part.containing.into_iter().collect(),
            },
            retained,
            loss,
        })
    }
}

#[cfg(test)]
mod tests;
