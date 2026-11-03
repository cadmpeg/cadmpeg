// SPDX-License-Identifier: Apache-2.0
//! The layer of a decode: the root view, or the root layer of a package.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;

use crate::detect::Header;
use crate::loss::UsdLossCode;
use crate::text::syntax::Layer;

/// A classified layer.
pub(crate) struct Source<'a> {
    pub(crate) header: Header,
    pub(crate) layers: DialectLayers,
    pub(crate) losses: Vec<LossNote>,
    /// The layer bytes.
    pub(crate) layer: View<'a>,
    pub(crate) package: Option<super::Package<'a>>,
}

fn layer_header(bytes: &[u8]) -> Result<Header, CodecError> {
    crate::detect::header(bytes)
        .ok_or_else(|| CodecError::WrongFormat("no OpenUSD layer header".to_owned()))
}

/// Classifies `root` as a layer or a package.
pub(crate) fn source<'a>(
    ctx: &DecodeContext<'a>,
    root: View<'a>,
) -> Result<Source<'a>, CodecError> {
    if super::is_archive(root.window()) {
        let package = super::open(ctx, root)?;
        let header = layer_header(package.root.window())?;
        let (matched, loss) = crate::dialect::classify(&header).admit()?;
        let layers = DialectLayers::of(crate::dialect::package(&package.root_name))
            .with(matched.with_instance(package.root_name.clone()))?;
        return Ok(Source {
            header,
            layers,
            losses: loss.into_iter().collect(),
            layer: package.root,
            package: Some(package),
        });
    }
    let header = layer_header(root.window())?;
    let (matched, loss) = crate::dialect::classify(&header).admit()?;
    Ok(Source {
        header,
        layers: DialectLayers::of(matched),
        losses: loss.into_iter().collect(),
        layer: root,
        package: None,
    })
}

/// Parses the layer of `source`, charging the values a USDC layer holds that
/// were not read.
pub(crate) fn parse<'a>(
    ctx: &DecodeContext<'a>,
    source: &mut Source<'a>,
) -> Result<Layer, CodecError> {
    match source.header {
        Header::Text { .. } => crate::text::grammar::parse_layer(ctx, source.layer.window()),
        Header::Binary { .. } => {
            let (layer, unread) = crate::usdc::read(ctx, source.layer)?;
            if !unread.is_empty() {
                source
                    .losses
                    .push(UsdLossCode::CrateValueNotRead.note(format!(
                        "USDC values were not read: {}",
                        unread.into_iter().collect::<Vec<_>>().join(", ")
                    )));
            }
            Ok(layer)
        }
    }
}
