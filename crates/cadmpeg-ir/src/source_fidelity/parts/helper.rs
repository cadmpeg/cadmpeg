// SPDX-License-Identifier: Apache-2.0
//! Append-only part output and the shared DecodeContext extension helper.

use std::collections::BTreeSet;

use crate::schema::rewrite::typed::RewriteIdentities;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{de::DeserializeOwned, Serialize};

mod captures;
mod dependencies;
mod finalization;
mod parser;
pub use captures::CapturedRecordLayout;
pub use dependencies::PartDependencyPruning;
pub use finalization::FinalizedPartOutput;
pub use parser::ParserPartOutput;

use super::{PartLevel, PartRecord, PartSpec, PartState};
use crate::codec::DecodeLossCode;
use crate::document::Model;
use crate::draft::ArenaEntity;
use crate::ids::UnknownId;
use crate::native::{Native, NativeConvertError, NativeRecord};
use crate::provenance::{CodecFormat, SourceProvenance, StreamName};
use crate::report::loss::LossNote;
use crate::source_fidelity::{RecordLayout, RetainedBytes, RetainedSourceRecord, SourceFidelity};
use crate::NativeUnknownRecord;

/// Append-only output staged by independently framed decode parts.
///
/// A child has no mutable access to previously committed sibling records.
/// Dropping its stage discards records, layouts, retained bytes and diagnostics.
#[derive(Debug)]
pub struct PartOutput {
    codec: CodecFormat,
    model: Model,
    native: Native,
    fidelity: SourceFidelity,
    losses: Vec<LossNote>,
    ids: BTreeSet<String>,
    native_owners: std::collections::BTreeMap<String, dependencies::NativeOwner>,
    failures: Vec<dependencies::FailureHome>,
    // A part's stage moves its records into the parent output on commit.
    // Their storage is charged once, when the root output receives them.
    staged: bool,
}

impl PartOutput {
    /// Start one codec's partial decode output.
    pub fn new(codec: CodecFormat) -> Self {
        Self {
            codec,
            model: Model::default(),
            native: Native::default(),
            fidelity: SourceFidelity::default(),
            losses: Vec::new(),
            ids: BTreeSet::new(),
            native_owners: std::collections::BTreeMap::new(),
            failures: Vec::new(),
            staged: false,
        }
    }
    fn stage(codec: CodecFormat) -> Self {
        Self {
            staged: true,
            ..Self::new(codec)
        }
    }
    /// Borrow successfully staged model records.
    pub fn model(&self) -> &Model {
        &self.model
    }
    /// Borrow successfully staged native records.
    pub fn native(&self) -> &Native {
        &self.native
    }
    /// Borrow source layouts, retained images and the part tree.
    pub fn fidelity(&self) -> &SourceFidelity {
        &self.fidelity
    }
    /// Add annotations and source layouts only to this stage.
    pub fn fidelity_mut(&mut self) -> &mut SourceFidelity {
        &mut self.fidelity
    }
    /// Borrow staged decode diagnostics.
    pub fn losses(&self) -> &[LossNote] {
        &self.losses
    }
    /// Stage a successful part's diagnostic under the same session budget.
    pub fn push_loss(&mut self, ctx: &DecodeContext<'_>, loss: LossNote) -> Result<(), CodecError> {
        ctx.retain_bytes(u64_from_index(loss.message.len()), "part diagnostic text")?;
        reserve_output(ctx, self.staged, &mut self.losses, 1)?;
        self.losses.push(loss);
        Ok(())
    }
    /// Insert an owned neutral entity. Variable-size payloads are admitted by
    /// their parser before insertion; this charges the entity and arena storage.
    pub fn insert_model<T: ArenaEntity>(
        &mut self,
        ctx: &DecodeContext<'_>,
        entity: T,
    ) -> Result<(), CodecError> {
        self.admit_id(ctx, entity.identity())?;
        reserve_output(ctx, self.staged, T::arena_mut(&mut self.model), 1)?;
        ctx.charge_entities(1, "part neutral entity")?;
        T::arena_mut(&mut self.model).push(entity);
        Ok(())
    }
    /// Insert one typed native record without replacing an existing arena.
    pub fn insert_native<T: Serialize + DeserializeOwned + RewriteIdentities>(
        &mut self,
        ctx: &DecodeContext<'_>,
        arena: &str,
        value: T,
    ) -> Result<(), CodecError> {
        let mut records =
            crate::native::arena_from(ctx, std::iter::once(Ok::<T, NativeConvertError>(value)))?;
        let record = records
            .pop()
            .ok_or_else(|| CodecError::malformed("native part record was not formed"))?;
        self.admit_id(ctx, record.id())?;
        self.register_native_owner::<T>(ctx, record.id())?;
        ctx.charge_entities(1, "part native entity")?;
        let staged = self.staged;
        let destination = self
            .native
            .namespace_mut(self.codec.as_str())
            .arenas_mut()
            .entry(arena.to_owned())
            .or_default();
        reserve_output(ctx, staged, destination, 1)?;
        destination.push(record);
        Ok(())
    }
    fn admit_id(&mut self, ctx: &DecodeContext<'_>, id: &str) -> Result<(), CodecError> {
        if self.ids.contains(id) {
            return Err(CodecError::malformed(format_args!(
                "duplicate part record {id}"
            )));
        }
        ctx.retain_bytes(
            u64_from_index(id.len())
                .checked_add(u64_from_index(size_of::<String>()))
                .ok_or_else(|| CodecError::malformed("part identity storage overflows"))?,
            "part identity index",
        )?;
        self.ids.insert(id.to_owned());
        Ok(())
    }
    fn preflight(&self, other: &Self) -> Result<(), CodecError> {
        if let Some(id) = self.ids.intersection(&other.ids).next() {
            return Err(CodecError::malformed(format_args!(
                "duplicate part record {id}"
            )));
        }
        for id in other.fidelity.parts.keys() {
            if self.fidelity.parts.contains_key(id) {
                return Err(CodecError::malformed("duplicate staged part identity"));
            }
        }
        for id in other.fidelity.record_layout.keys() {
            if self.fidelity.record_layout.contains_key(id) {
                return Err(CodecError::malformed("duplicate part record layout"));
            }
        }
        for id in other.fidelity.retained_records.keys() {
            if self.fidelity.retained_records.contains_key(id) {
                return Err(CodecError::malformed("duplicate retained part record"));
            }
        }
        for key in other.fidelity.metadata.keys() {
            if self.fidelity.metadata.contains_key(key) {
                return Err(CodecError::malformed("duplicate part metadata key"));
            }
        }
        for id in other
            .fidelity
            .annotations
            .provenance
            .keys()
            .chain(other.fidelity.annotations.exactness().keys())
        {
            if self.fidelity.annotations.provenance.contains_key(id)
                || self.fidelity.annotations.exactness().contains_key(id)
            {
                return Err(CodecError::malformed("duplicate part annotation"));
            }
        }
        Ok(())
    }
    fn append(&mut self, ctx: &DecodeContext<'_>, mut other: Self) -> Result<(), CodecError> {
        self.preflight(&other)?;
        let staged = self.staged;
        macro_rules! reserve_model {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => { $(reserve_output(ctx, staged, &mut self.model.$field, other.model.$field.len())?;)* };
        }
        crate::document::arena_registry!(reserve_model);
        self.fidelity.append(other.fidelity)?;
        self.model.append(other.model);
        for (format, mut namespace) in other.native.0 {
            let destination = self.native.namespace_mut(format).arenas_mut();
            for (arena, mut records) in std::mem::take(namespace.arenas_mut()) {
                let arena = destination.entry(arena).or_default();
                reserve_output(ctx, staged, arena, records.len())?;
                arena.append(&mut records);
            }
        }
        self.native_owners.append(&mut other.native_owners);
        for failure in &mut other.failures {
            failure.loss_index = failure
                .loss_index
                .checked_add(self.losses.len())
                .ok_or_else(|| CodecError::malformed("part diagnostic index overflows"))?;
        }
        reserve_output(ctx, staged, &mut self.failures, other.failures.len())?;
        self.failures.append(&mut other.failures);
        self.ids.append(&mut other.ids);
        reserve_output(ctx, staged, &mut self.losses, other.losses.len())?;
        self.losses.append(&mut other.losses);
        Ok(())
    }
}

/// Decode one Appendix C part while keeping its failure in one shared home.
pub trait DecodeParts {
    /// A failed callback contributes one unknown stub, exact retained bytes and
    /// one record.unreadable loss. A successful callback commits its stage.
    fn decode_part<T>(
        &self,
        output: &mut PartOutput,
        extent: View<'_>,
        spec: PartSpec,
        decode: impl FnOnce(&mut PartOutput) -> Result<T, CodecError>,
    ) -> Result<Option<T>, CodecError>;

    /// Run a real parser callback without access to a semantic record stage.
    /// Only the helper can add failure stubs. Actual nested `record` attempts
    /// still seed the shared dependency closure on a failed part.
    fn decode_parser_part<T>(
        &self,
        output: &mut ParserPartOutput,
        extent: View<'_>,
        spec: PartSpec,
        decode: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<Option<T>, CodecError>;

    /// Run nested syntax parts within their genuine parent transaction.
    /// The callback can stage only parser parts, not semantic records.
    fn decode_parser_scope<T>(
        &self,
        output: &mut ParserPartOutput,
        extent: View<'_>,
        spec: PartSpec,
        decode: impl FnOnce(&mut ParserPartOutput) -> Result<T, CodecError>,
    ) -> Result<Option<T>, CodecError>;
}

impl DecodeParts for DecodeContext<'_> {
    fn decode_parser_part<T>(
        &self,
        output: &mut ParserPartOutput,
        extent: View<'_>,
        spec: PartSpec,
        decode: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<Option<T>, CodecError> {
        self.decode_parser_scope(output, extent, spec, |_stage| decode())
    }

    fn decode_parser_scope<T>(
        &self,
        output: &mut ParserPartOutput,
        extent: View<'_>,
        spec: PartSpec,
        decode: impl FnOnce(&mut ParserPartOutput) -> Result<T, CodecError>,
    ) -> Result<Option<T>, CodecError> {
        self.decode_part(&mut output.0, extent, spec, |stage| {
            let mut parser = ParserPartOutput(PartOutput::stage(stage.codec));
            let result = decode(&mut parser);
            *stage = parser.0;
            result
        })
    }

    fn decode_part<T>(
        &self,
        output: &mut PartOutput,
        extent: View<'_>,
        spec: PartSpec,
        decode: impl FnOnce(&mut PartOutput) -> Result<T, CodecError>,
    ) -> Result<Option<T>, CodecError> {
        // Framing bytes come from the format's header fields, which damaged
        // input can corrupt. A framing error is a failure of this part only.
        let framing = spec.validate(extent);
        let framing_valid = framing.is_ok();
        let mut staged = PartOutput::stage(output.codec);
        let outcome = self.part_transaction(extent, || {
            framing?;
            let value = decode(&mut staged)?;
            output.preflight(&staged)?;
            Ok(value)
        })?;
        let (capture, result) = outcome.into_parts();
        let id = part_id(
            self,
            output.codec,
            spec.level,
            capture.stream(),
            capture.offset(),
        )?;
        if output.fidelity.parts.contains_key(&id) || staged.fidelity.parts.contains_key(&id) {
            return Err(CodecError::malformed("duplicate part invocation identity"));
        }
        self.retain_bytes(
            u64_from_index(size_of::<PartRecord>())
                .checked_add(u64_from_index(capture.stream().len()))
                .and_then(|len| len.checked_add(u64_from_index(id.as_str().len())))
                .ok_or_else(|| CodecError::malformed("part tree storage overflows"))?,
            "part tree node",
        )?;
        for node in staged.fidelity.parts.values_mut() {
            if node.parent.is_none() {
                self.retain_bytes(u64_from_index(id.as_str().len()), "part parent identity")?;
                node.parent = Some(id.clone());
            }
        }
        let state = if result.is_ok() {
            PartState::Decoded {}
        } else {
            PartState::GivenUp {}
        };
        // Invalid framing intervals are not recorded; the whole extent is then
        // content of the given-up part.
        let spec = if framing_valid {
            spec
        } else {
            PartSpec::new(spec.level, Vec::new())
        };
        let record = PartRecord::captured(&capture, spec, state);
        match result {
            Ok(value) => {
                staged.fidelity.parts.insert(id, record);
                output.append(self, staged)?;
                Ok(Some(value))
            }
            Err(error) => {
                let mut dropped = Vec::new();
                reserve(self, &mut dropped, staged.ids.len())?;
                for dropped_id in &staged.ids {
                    self.retain_bytes(u64_from_index(dropped_id.len()), "dropped part identity")?;
                    dropped.push(dropped_id.clone());
                }
                let mut dropped_set = dropped.into_iter().collect::<BTreeSet<_>>();
                for attempted in capture.attempted_record_ids() {
                    if !dropped_set.contains(attempted) {
                        self.retain_bytes(
                            u64_from_index(attempted.len()),
                            "discarded parser record identity",
                        )?;
                        dropped_set.insert(attempted.clone());
                    }
                }
                for failure in &staged.failures {
                    for id in &failure.dropped {
                        if !dropped_set.contains(id) {
                            self.retain_bytes(
                                u64_from_index(id.len()),
                                "discarded descendant identity",
                            )?;
                            dropped_set.insert(id.clone());
                        }
                    }
                }
                let mut dropped = Vec::new();
                reserve(self, &mut dropped, dropped_set.len())?;
                dropped.extend(dropped_set);
                let mut parts = std::mem::take(&mut staged.fidelity.parts);
                for node in parts.values_mut() {
                    self.retain_bytes(u64_from_index(id.as_str().len()), "covering part identity")?;
                    node.state = PartState::Covered { by: id.clone() };
                }
                parts.insert(id.clone(), record);
                // All staged model/native/retention/layout/loss state is dropped.
                drop(staged);
                let mut replacement = PartOutput::stage(output.codec);
                let links = capture
                    .record_id()
                    .map(|id| {
                        crate::ids::Identity::new(id.to_owned()).map_err(CodecError::malformed)
                    })
                    .transpose()?
                    .into_iter()
                    .collect::<Vec<_>>();
                let stub = NativeUnknownRecord {
                    id: id.clone(),
                    links,
                };
                replacement.admit_id(self, id.as_str())?;
                replacement.register_native_owner::<NativeUnknownRecord>(self, id.as_str())?;
                self.charge_entities(1, "given-up part stub")?;
                let native = NativeRecord::from(&stub);
                self.retain_bytes(
                    u64_from_index(native.text_len()),
                    "given-up part native stub",
                )?;
                let mut unknowns = Vec::new();
                reserve_output(self, true, &mut unknowns, 1)?;
                unknowns.push(native);
                replacement
                    .native
                    .namespace_mut(output.codec.as_str())
                    .arenas_mut()
                    .insert("unknowns".to_owned(), unknowns);
                let length = extent.window().len();
                self.retain_bytes(u64_from_index(length), "given-up part source bytes")?;
                let mut bytes = self.alloc_filled(length, 0u8, "given-up part source bytes")?;
                bytes.copy_from_slice(extent.window());
                let retained = RetainedSourceRecord::from_bytes(
                    capture.stream().to_owned(),
                    capture.offset(),
                    RetainedBytes::Inline { data: bytes },
                )?;
                replacement
                    .fidelity
                    .insert_retained_record(id.clone(), retained)?;
                self.retain_bytes(
                    u64_from_index(size_of::<RecordLayout>())
                        .checked_add(u64_from_index(capture.stream().len()))
                        .and_then(|len| len.checked_add(u64_from_index(id.as_str().len())))
                        .ok_or_else(|| CodecError::malformed("part layout storage overflows"))?,
                    "given-up part layout",
                )?;
                replacement.fidelity.record_layout.insert(
                    id.as_str().to_owned(),
                    RecordLayout::from_part_capture(&capture),
                );
                replacement.fidelity.parts = parts;
                let prefix = format!(
                    "{} stream={} offset={} length={} record={} error={}",
                    replacement.fidelity.parts[&id].level.as_str(),
                    capture.stream(),
                    capture.offset(),
                    capture.byte_len(),
                    capture.record_id().unwrap_or("none"),
                    error
                );
                let message = format!(
                    "{prefix} dropped={}",
                    serde_json::to_string(&dropped).map_err(CodecError::malformed)?
                );
                let mut provenance = SourceProvenance::in_stream(
                    output.codec.as_str(),
                    StreamName::try_from(capture.stream().to_owned())
                        .map_err(CodecError::malformed)?,
                    capture.offset(),
                );
                // The loss names the part's span (#217 §5.6); an empty part has none.
                if capture.byte_len() != 0 {
                    provenance = provenance.with_byte_len(capture.byte_len())?;
                }
                replacement.push_loss(
                    self,
                    DecodeLossCode::RecordUnreadable
                        .note(message)
                        .with_provenance(provenance),
                )?;
                replacement.record_failure(self, id, dropped, prefix)?;
                output.append(self, replacement)?;
                Ok(None)
            }
        }
    }
}

impl SourceFidelity {
    /// Install every completed record capture of `ctx` as a record layout.
    ///
    /// Core builds each capture's reads; this and
    /// [`Self::install_mapped_record_captures`] are the only paths from a
    /// capture to `record_layout`. `fields` receives each record id and its
    /// new layout before insertion, to add the record's `pointers` and
    /// `payloads`; it has no access to the reads. A capture made inside a
    /// record batch, a duplicate record id, or a `fields` failure is an
    /// error, and no layout is then inserted. Equal capture ids can exist in
    /// different batches, so only the batch-aware mapper installs them.
    pub fn install_record_captures(
        &mut self,
        ctx: &DecodeContext<'_>,
        mut fields: impl FnMut(&str, &mut RecordLayout) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        let mut staged = SourceFidelity::default();
        for capture in ctx.take_record_captures()? {
            if capture.batch_id().is_some() {
                return Err(CodecError::malformed(
                    "a batched record capture needs install_mapped_record_captures",
                ));
            }
            let (id, mut layout) = RecordLayout::from_capture(capture);
            fields(&id, &mut layout)?;
            staged.insert_record_layout(id, layout)?;
        }
        self.append(staged)
    }
}

pub(super) fn part_id(
    ctx: &DecodeContext<'_>,
    codec: CodecFormat,
    level: PartLevel,
    stream: &str,
    offset: u64,
) -> Result<UnknownId, CodecError> {
    let length = level
        .as_str()
        .len()
        .checked_add(stream.len())
        .and_then(|len| len.checked_add(24))
        .ok_or_else(|| CodecError::malformed("part identity source length overflows"))?;
    ctx.retain_bytes(u64_from_index(length), "part address identity")?;
    let mut source = Vec::new();
    source.try_reserve_exact(length).map_err(|_| {
        cadmpeg_core::decode::ResourceLimit::allocation_failed(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            u64_from_index(length),
            "part address identity",
        )
    })?;
    source.extend_from_slice(&u64_from_index(level.as_str().len()).to_be_bytes());
    source.extend_from_slice(level.as_str().as_bytes());
    source.extend_from_slice(&u64_from_index(stream.len()).to_be_bytes());
    source.extend_from_slice(stream.as_bytes());
    source.extend_from_slice(&offset.to_be_bytes());
    let id = format!(
        "{}:source:part#{}",
        codec.as_str(),
        crate::hash::sha256_hex(&source)
    );
    UnknownId::try_from(id).map_err(CodecError::malformed)
}

/// Reserve output storage; a stage's storage is charged by the root output.
fn reserve_output<T>(
    ctx: &DecodeContext<'_>,
    staged: bool,
    values: &mut Vec<T>,
    additional: usize,
) -> Result<(), CodecError> {
    if !staged {
        return reserve(ctx, values, additional);
    }
    let bytes = additional
        .checked_mul(size_of::<T>())
        .ok_or_else(|| CodecError::malformed("part stage storage overflows"))?;
    values.try_reserve_exact(additional).map_err(|_| {
        cadmpeg_core::decode::ResourceLimit::allocation_failed(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            u64_from_index(bytes),
            "part stage storage",
        )
        .into()
    })
}

fn reserve<T>(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<T>,
    additional: usize,
) -> Result<(), CodecError> {
    let required = values
        .len()
        .checked_add(additional)
        .ok_or_else(|| CodecError::malformed("part output capacity overflows"))?;
    if required > values.capacity() {
        let bytes = required
            .abs_diff(values.capacity())
            .checked_mul(size_of::<T>())
            .ok_or_else(|| CodecError::malformed("part output storage overflows"))?;
        ctx.retain_bytes(u64_from_index(bytes), "part output storage")?;
        values.try_reserve_exact(additional).map_err(|_| {
            cadmpeg_core::decode::ResourceLimit::allocation_failed(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                ctx.policy().limits.max_retained_bytes,
                u64_from_index(bytes),
                "part output storage",
            )
        })?;
    }
    Ok(())
}
