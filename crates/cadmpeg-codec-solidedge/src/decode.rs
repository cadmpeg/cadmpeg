// SPDX-License-Identifier: Apache-2.0
//! Solid Edge document decode.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::SourceMeta;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::report::decode::DecodeTransfer;
use cadmpeg_ir::{CadIr, SourceFidelity};

use crate::{container, dialect, loss::SolidEdgeLossCode};

pub(crate) fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
    let snapshot = container::open(ctx, root)?;
    let mut ir = CadIr::empty();
    ir.source = Some(SourceMeta::classified(
        dialect::layers(),
        std::collections::BTreeMap::default(),
    ));
    let mut body = DecodeBody::new(DecodeTransfer::at(ctx.depth(), false));
    body.losses.extend(
        snapshot.deviations().map(|deviation| {
            SolidEdgeLossCode::ContainerNoncanonical.note(deviation.description())
        }),
    );
    if !ctx.container_only() {
        body.losses
            .push(SolidEdgeLossCode::BodiesPending.note("Solid Edge body decoding is pending"));
    }
    Ok(Decoded {
        ir,
        body,
        source_fidelity: SourceFidelity::new(Sha256Digest::digest(root.window())),
    })
}
