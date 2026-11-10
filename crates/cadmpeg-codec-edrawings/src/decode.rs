// SPDX-License-Identifier: Apache-2.0
//! Package decode.
//!
//! Decode opens the package, classifies it from the `eModel` comment line,
//! and expands the model stream. [`crate::hsf`] walks the stream and emits the
//! bodies, definitions, and occurrences.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::report::decode::DecodeTransfer;
use cadmpeg_ir::SourceFidelity;

use crate::loss::EdrawingsLossCode;
use crate::package::Package;
use crate::stream;

/// Source attribute counting the archive members.
const ATTRIBUTE_MEMBERS: &str = "member_count";
/// Source attribute naming the model stream version.
const ATTRIBUTE_HSF_VERSION: &str = "hsf_version";
/// Source attribute counting the bytes of the inflated model stream.
const ATTRIBUTE_BODY_BYTES: &str = "model_stream_bytes";

pub(crate) fn decode<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Decoded, CodecError> {
    let package = Package::open(ctx, root)?;
    let member = package.model(ctx)?;
    let wrapper = stream::read_wrapper(member)?;
    let classification = crate::dialect::classify(Some(&wrapper.version));
    let grammar = classification.grammar();
    let mut losses: Vec<_> = package
        .recovery_loss()
        .into_iter()
        .chain(classification.loss())
        .collect();
    let mut attributes = BTreeMap::from([
        (
            cadmpeg_core::nonblank_const!(ATTRIBUTE_MEMBERS),
            package.member_count().to_string(),
        ),
        (
            cadmpeg_core::nonblank_const!(ATTRIBUTE_HSF_VERSION),
            wrapper.version.text().to_owned(),
        ),
    ]);
    let mut model_body = None;
    if !ctx.container_only() {
        match stream::parse(ctx, member) {
            Ok(model) => {
                if !model.tail_is_observed() {
                    losses.push(EdrawingsLossCode::StreamTrailingBytes.note(format!(
                        "{} bytes follow the {} byte zlib stream and were not transferred",
                        model.tail.len(),
                        model.compressed_len
                    )));
                }
                attributes.insert(
                    cadmpeg_core::nonblank_const!(ATTRIBUTE_BODY_BYTES),
                    model.body.window().len().to_string(),
                );
                model_body = Some(model.body);
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => losses.push(EdrawingsLossCode::StreamMalformed.note(format!(
                "the eModel model stream did not open ({error}); the package carries no model \
                 data"
            ))),
        }
    }
    let mut ir = CadIr::decoded(SourceMeta::classified(
        DialectLayers::of(classification.into_matched()),
        attributes,
    ));
    let mut geometry_transferred = false;
    if let Some(body) = model_body {
        geometry_transferred = crate::hsf::decode_body(ctx, body, grammar, &mut ir, &mut losses)?;
    }
    Ok(Decoded {
        ir,
        body: DecodeBody {
            losses,
            ..DecodeBody::new(DecodeTransfer::at(ctx.depth(), geometry_transferred))
        },
        source_fidelity: SourceFidelity::default(),
    })
}
