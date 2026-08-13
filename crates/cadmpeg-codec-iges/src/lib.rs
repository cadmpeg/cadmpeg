// SPDX-License-Identifier: Apache-2.0
//! IGES Fixed ASCII codec for versions 5.1, 5.2, and 5.3.
//!
//! Support level: [L8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#support-ladder)
//! for the declared Fixed ASCII mechanical/document envelope. Bounded
//! semantic writing is an extra; the L9 gate remains open.

mod card;
mod directory;
mod entities;
mod global;
mod graph;
mod layout;
mod native;
mod parameter;
mod profile;
mod reader;
mod writer;

#[cfg(feature = "fuzzing")]
#[doc(hidden)]
pub mod fuzz;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::{CodecError, ContainerSummary};
use cadmpeg_ir::codec::{
    CodecBackend, Confidence, DecodeOptions, DecodeResult, EncodeInput, Encoder, ExportPlan,
};
use cadmpeg_ir::hash::document_local_sha256;
use cadmpeg_ir::CadIr;
use std::io::Cursor;

pub(crate) const SOURCE_IMAGE_ID: &str = "iges:file:source-image#0";

/// IGES specification version selected for semantic output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum IgesVersion {
    /// IGES 5.3 Fixed ASCII.
    #[default]
    V5_3,
    /// IGES 5.2 Fixed ASCII.
    V5_2,
    /// IGES 5.1 Fixed ASCII.
    V5_1,
}

impl IgesVersion {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::V5_1 => "5.1",
            Self::V5_2 => "5.2",
            Self::V5_3 => "5.3",
        }
    }

    pub(crate) const fn global_flag(self) -> u8 {
        match self {
            Self::V5_1 => 9,
            Self::V5_2 => 10,
            Self::V5_3 => 11,
        }
    }
}

/// Options controlling a semantic IGES write.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IgesWriteOptions {
    /// Target specification version.
    pub version: IgesVersion,
}

/// Codec for IGES files.
#[derive(Debug, Default, Clone, Copy)]
pub struct IgesCodec;

pub(crate) fn document_digest(ir: &CadIr) -> String {
    document_local_sha256(ir, "iges", SOURCE_IMAGE_ID)
}

impl CodecBackend for IgesCodec {
    fn id(&self) -> &'static str {
        "iges"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        layout::confidence(prefix)
    }

    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        let mut reader = Cursor::new(root.window());
        match layout::classify(&mut reader)? {
            representation @ (layout::Representation::CompressedAscii
            | layout::Representation::Binary) => {
                return Ok(layout::unsupported_summary(representation));
            }
            layout::Representation::Unknown => {
                return Err(CodecError::WrongFormat(
                    "unrecognized IGES representation".into(),
                ));
            }
            layout::Representation::FixedAscii => {}
        }
        ctx.charge_work(root.window().len() as u64, "iges_inspect_card_scan")?;
        let _scan_storage = ctx.reserve_scoped(
            root.window().len() as u64,
            "iges_inspect_card_storage",
            None,
        )?;
        let scan = card::scan_with_context(root.window(), Some(ctx))?;
        let global = global::parse(&scan)?;
        let directory = directory::parse(&scan)?;
        ctx.charge_entities(directory.len() as u64, "iges_inspect_directory_entries")?;
        let parameters = parameter::assemble_with_context(&scan, &directory, &global, Some(ctx))?;
        let parameter_tokens = parameters
            .iter()
            .map(|record| record.tokens.len() as u64)
            .sum();
        ctx.charge_work(parameter_tokens, "iges_inspect_parameter_parse")?;
        let references = graph::build(&directory);
        let mut summary = card::summarize(&scan);
        summary.notes.extend(global.summary_notes());
        summary.notes.extend(directory::summary_notes(&directory));
        summary.notes.extend(parameter::summary_notes(&parameters));
        summary.notes.extend(graph::summary_notes(&references));
        Ok(summary)
    }

    fn decode_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<DecodeResult, CodecError> {
        let mut source = Cursor::new(root.window());
        match layout::classify(&mut source)? {
            layout::Representation::FixedAscii => reader::decode(
                root.window(),
                DecodeOptions {
                    container_only: ctx.container_only(),
                    policy: *ctx.policy(),
                },
                ctx,
            ),
            representation @ (layout::Representation::CompressedAscii
            | layout::Representation::Binary) => Err(layout::unsupported_error(representation)),
            layout::Representation::Unknown => Err(CodecError::WrongFormat(
                "unrecognized IGES representation".into(),
            )),
        }
    }
}

/// IGES encoder with explicit target-version options.
#[derive(Debug, Clone, Copy, Default)]
pub struct IgesEncoder {
    options: IgesWriteOptions,
}

impl IgesEncoder {
    /// Construct an encoder for `options`.
    #[must_use]
    pub const fn new(options: IgesWriteOptions) -> Self {
        Self { options }
    }
}

impl Encoder for IgesEncoder {
    fn id(&self) -> &'static str {
        "iges"
    }

    fn plan<'a>(&self, input: EncodeInput<'a>) -> Result<ExportPlan<'a>, CodecError> {
        writer::plan(input, self.options)
    }
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod write_roundtrip_tests;
