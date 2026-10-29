// SPDX-License-Identifier: Apache-2.0
//! Independently framed decode parts.
//!
//! A part decodes into a stage. A successful part commits its stage into the
//! parent output. A failed part drops its stage, retains its source bytes as
//! an untyped record, and charges `record.unreadable` (#209 §4.5).

use std::collections::btree_map::Entry;

use cadmpeg_core::decode::{
    u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, SpaceId, View,
};
use cadmpeg_core::CodecError;
use serde::Serialize;

use crate::codec::DecodeLossCode;
use crate::document::{ArenaClass, ArenaClasses, Model};
use crate::draft::ArenaEntity;
use crate::ids::UnknownId;
use crate::native::Native;
use crate::provenance::{CodecFormat, SourceOwner, SourceProvenance, StreamName};
use crate::report::loss::LossNote;
use crate::source_fidelity::{RetainedBytes, RetainedSourceRecord, SourceFidelity};

/// The label, stream and content classes of one part.
#[derive(Debug, Clone)]
pub struct PartSpec {
    label: &'static str,
    stream: Option<StreamName>,
    classes: Option<ArenaClasses>,
}

impl PartSpec {
    /// A part in the root stream whose bytes can hold every requested class.
    pub const fn new(label: &'static str) -> Self {
        Self {
            label,
            stream: None,
            classes: None,
        }
    }

    /// Name the stream that contains the part's extent.
    #[must_use]
    pub fn in_stream(mut self, stream: StreamName) -> Self {
        self.stream = Some(stream);
        self
    }

    /// Narrow the classes that the part's bytes can hold.
    #[must_use]
    pub fn holding(mut self, classes: impl IntoIterator<Item = ArenaClass>) -> Self {
        self.classes = Some(classes.into_iter().collect());
        self
    }
}

/// Model, native records, source fidelity and losses staged by parts.
#[derive(Debug)]
pub struct PartOutput {
    codec: CodecFormat,
    model: Model,
    native: Native,
    fidelity: SourceFidelity,
    losses: Vec<LossNote>,
}

impl PartOutput {
    /// Start one codec's part output.
    pub fn new(codec: CodecFormat) -> Self {
        Self {
            codec,
            model: Model::default(),
            native: Native::default(),
            fidelity: SourceFidelity::default(),
            losses: Vec::new(),
        }
    }

    /// Borrow staged model records.
    pub fn model(&self) -> &Model {
        &self.model
    }

    /// Borrow staged native records.
    pub fn native(&self) -> &Native {
        &self.native
    }

    /// Borrow staged source fidelity.
    pub fn fidelity(&self) -> &SourceFidelity {
        &self.fidelity
    }

    /// Add annotations and retained records to this stage.
    pub fn fidelity_mut(&mut self) -> &mut SourceFidelity {
        &mut self.fidelity
    }

    /// Borrow staged losses.
    pub fn losses(&self) -> &[LossNote] {
        &self.losses
    }

    /// Stage one loss.
    pub fn push_loss(&mut self, ctx: &DecodeContext<'_>, loss: LossNote) -> Result<(), CodecError> {
        ctx.retain_bytes(u64_from_index(loss.message.len()), "part diagnostic text")?;
        reserve(ctx, &mut self.losses, 1)?;
        self.losses.push(loss);
        Ok(())
    }

    /// Stage one neutral entity.
    pub fn insert_model<T: ArenaEntity>(
        &mut self,
        ctx: &DecodeContext<'_>,
        entity: T,
    ) -> Result<(), CodecError> {
        ctx.charge_entities(1, "part neutral entity")?;
        ctx.retain_bytes(u64_from_index(size_of::<T>()), "part neutral entity")?;
        let arena = T::arena_mut(&mut self.model);
        reserve(ctx, arena, 1)?;
        arena.push(entity);
        Ok(())
    }

    /// Stage one typed native record in `arena`.
    pub fn insert_native<T: Serialize>(
        &mut self,
        ctx: &DecodeContext<'_>,
        arena: &str,
        value: T,
    ) -> Result<(), CodecError> {
        let records = crate::native::arena_from(
            ctx,
            std::iter::once(Ok::<T, crate::native::NativeConvertError>(value)),
        )?;
        ctx.charge_entities(1, "part native entity")?;
        let destination = self
            .native
            .namespace_mut(self.codec.as_str())
            .arenas_mut()
            .entry(arena.to_owned())
            .or_default();
        reserve(ctx, destination, records.len())?;
        destination.extend(records);
        Ok(())
    }

    /// Release the staged model, native records, fidelity and losses.
    pub fn into_parts(self) -> (Model, Native, SourceFidelity, Vec<LossNote>) {
        (self.model, self.native, self.fidelity, self.losses)
    }

    /// Move `other` into this output. On failure this output is unchanged.
    fn append(&mut self, ctx: &DecodeContext<'_>, other: Self) -> Result<(), CodecError> {
        let Self {
            model,
            native,
            fidelity,
            mut losses,
            ..
        } = other;
        macro_rules! reserve_model {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
                $(reserve(ctx, &mut self.model.$field, model.$field.len())?;)*
            };
        }
        crate::document::arena_registry!(reserve_model);
        for (format, namespace) in &native.0 {
            let Some(destination) = self.native.0.get_mut(format) else {
                continue;
            };
            for (arena, records) in namespace.arenas() {
                if let Some(existing) = destination.arenas_mut().get_mut(arena) {
                    reserve(ctx, existing, records.len())?;
                }
            }
        }
        reserve(ctx, &mut self.losses, losses.len())?;
        self.fidelity.append(fidelity)?;
        self.model.append(model);
        for (format, mut namespace) in native.0 {
            let destination = self.native.namespace_mut(format).arenas_mut();
            for (arena, mut records) in std::mem::take(namespace.arenas_mut()) {
                match destination.entry(arena) {
                    Entry::Vacant(entry) => {
                        entry.insert(records);
                    }
                    Entry::Occupied(entry) => entry.into_mut().append(&mut records),
                }
            }
        }
        self.losses.append(&mut losses);
        Ok(())
    }
}

/// Decode one independently framed part inside a boundary.
pub trait DecodeParts {
    /// Run `decode` on a fresh stage and commit the stage on success.
    ///
    /// On a part failure (a decode error, or a Part-scoped resource refusal)
    /// the stage is dropped, `extent` is retained as an untyped record, a
    /// `record.unreadable` loss is staged in `output`, and the result is
    /// `Ok(None)`. A Session-scoped refusal or an I/O error is returned.
    fn decode_part<T>(
        &self,
        output: &mut PartOutput,
        extent: View<'_>,
        spec: PartSpec,
        decode: impl FnOnce(&mut PartOutput) -> Result<T, CodecError>,
    ) -> Result<Option<T>, CodecError>;
}

impl DecodeParts for DecodeContext<'_> {
    fn decode_part<T>(
        &self,
        output: &mut PartOutput,
        extent: View<'_>,
        spec: PartSpec,
        decode: impl FnOnce(&mut PartOutput) -> Result<T, CodecError>,
    ) -> Result<Option<T>, CodecError> {
        if extent.location().space != SpaceId::ROOT && spec.stream.is_none() {
            return Err(CodecError::malformed(format_args!(
                "part {} outside the root stream does not name its stream",
                spec.label
            )));
        }
        let codec = output.codec;
        let result = self.run_part(|| {
            let mut stage = PartOutput::new(codec);
            let value = decode(&mut stage)?;
            if let Some(limit) = self.resource_refusal() {
                return Err(limit.into());
            }
            output.append(self, stage)?;
            Ok(value)
        })?;
        match result {
            Ok(value) => Ok(Some(value)),
            Err(error) => {
                give_up(self, output, extent, &spec, &error)?;
                Ok(None)
            }
        }
    }
}

fn give_up(
    ctx: &DecodeContext<'_>,
    output: &mut PartOutput,
    extent: View<'_>,
    spec: &PartSpec,
    error: &CodecError,
) -> Result<(), CodecError> {
    let offset = u64_from_index(extent.start());
    let bytes = extent.window();
    let stream = spec.stream.as_ref().map_or("", StreamName::as_str);
    let id = part_id(output.codec, spec.label, stream, offset)?;
    let classes = spec
        .classes
        .clone()
        .unwrap_or_else(|| ctx.depth().required_scope().classes().collect());
    let retained = u64_from_index(bytes.len())
        .checked_add(u64_from_index(id.as_str().len()))
        .ok_or_else(|| CodecError::malformed("given-up part size overflows"))?;
    ctx.retain_bytes(retained, "given-up part source bytes")?;
    let mut data = ctx.alloc_filled(bytes.len(), 0_u8, "given-up part source bytes")?;
    data.copy_from_slice(bytes);
    let owner = spec
        .stream
        .clone()
        .map_or(SourceOwner::Root, SourceOwner::from);
    let record = RetainedSourceRecord::from_bytes(owner, offset, RetainedBytes::Inline { data })?
        .with_untyped(classes);
    output.fidelity.insert_retained_record(id, record)?;
    let length = u64_from_index(bytes.len());
    let mut provenance = match &spec.stream {
        Some(stream) => SourceProvenance::in_stream(output.codec.as_str(), stream.clone(), offset),
        None => SourceProvenance::root(output.codec.as_str(), offset),
    };
    if length != 0 {
        provenance = provenance.with_byte_len(length)?;
    }
    let message = format!(
        "part {} stream={stream:?} offset={offset} length={length}: {error}",
        spec.label
    );
    output.push_loss(
        ctx,
        DecodeLossCode::RecordUnreadable
            .note(message)
            .with_provenance(provenance),
    )
}

/// The retained-record identity of a given-up part, derived from its address.
fn part_id(
    codec: CodecFormat,
    label: &str,
    stream: &str,
    offset: u64,
) -> Result<UnknownId, CodecError> {
    let mut address = Vec::new();
    for field in [label.as_bytes(), stream.as_bytes()] {
        address.extend_from_slice(&u64_from_index(field.len()).to_be_bytes());
        address.extend_from_slice(field);
    }
    address.extend_from_slice(&offset.to_be_bytes());
    UnknownId::try_from(format!(
        "{}:source:part#{}",
        codec.as_str(),
        crate::hash::sha256_hex(&address)
    ))
    .map_err(CodecError::malformed)
}

/// Reserve room for `additional` values without aborting on allocation failure.
fn reserve<T>(
    ctx: &DecodeContext<'_>,
    values: &mut Vec<T>,
    additional: usize,
) -> Result<(), CodecError> {
    let bytes = additional
        .checked_mul(size_of::<T>())
        .ok_or_else(|| CodecError::malformed("part output storage overflows"))?;
    values.try_reserve(additional).map_err(|_| {
        ResourceLimit::allocation_failed(
            ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            u64_from_index(bytes),
            "part output storage",
        )
        .into()
    })
}

#[cfg(test)]
mod tests;
