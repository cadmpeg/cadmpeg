// SPDX-License-Identifier: Apache-2.0
//! Read-only IGES 5.3 Fixed ASCII codec.
//!
//! Support level: [L8](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#support-ladder)
//! for the declared Fixed ASCII mechanical/document envelope.

mod card;
mod directory;
mod entities;
mod global;
mod graph;
mod layout;
mod loss;
mod native;
mod parameter;
mod profile;
mod reader;

use cadmpeg_ir::codec::{
    Codec, CodecError, Confidence, ContainerSummary, DecodeOptions, DecodeResult,
};
use cadmpeg_ir::decode::{DecodeContext, View};
use std::io::Cursor;

/// Codec for IGES files.
#[derive(Debug, Default, Clone, Copy)]
pub struct IgesCodec;

impl Codec for IgesCodec {
    fn id(&self) -> &'static str {
        "iges"
    }

    fn detect(&self, prefix: &[u8]) -> Confidence {
        layout::confidence(prefix)
    }

    fn inspect_impl(
        &self,
        _ctx: &DecodeContext<'_>,
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
        let scan = card::scan(&mut reader)?;
        let global = global::parse(&scan)?;
        let directory = directory::parse(&scan)?;
        let parameters = parameter::assemble(&scan, &directory, &global)?;
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
                &mut source,
                DecodeOptions {
                    container_only: ctx.container_only(),
                    policy: *ctx.policy(),
                },
            ),
            representation @ (layout::Representation::CompressedAscii
            | layout::Representation::Binary) => Err(layout::unsupported_error(representation)),
            layout::Representation::Unknown => Err(CodecError::WrongFormat(
                "unrecognized IGES representation".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests;
