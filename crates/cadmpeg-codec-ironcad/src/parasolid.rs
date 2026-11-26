// SPDX-License-Identifier: Apache-2.0
//! Parasolid kernel records of `Brep Data`.

use cadmpeg_codec_sldprt::transmit::{self, TransmitGeometry};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};

/// Identity format of the entities that the shared transmit builder mints.
const BUILDER_FORMAT: &str = "sldprt:";

/// Decodes the bodies of one native binary Parasolid record.
pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    bytes: View<'_>,
    stream: &str,
) -> Result<TransmitGeometry, CodecError> {
    let neutral = cadmpeg_parasolid::native::to_neutral(bytes.window())?;
    ctx.retain_bytes(
        cadmpeg_core::decode::u64_from_index(neutral.len()),
        "IronCAD neutral Parasolid transmit",
    )?;
    let name = cadmpeg_ir::stream_name!("ironcad:").with_suffix(stream);
    transmit::decode(ctx, &neutral, &name)?
        .ok_or_else(|| CodecError::malformed("neutral Parasolid transmit has no header"))
}

/// Moves every identity of `decoded` into the `ironcad` namespace, under
/// `namespace` when the scene holds more than one part result.
pub(crate) fn qualify(
    ctx: &DecodeContext<'_>,
    decoded: TransmitGeometry,
    namespace: Option<&str>,
) -> Result<TransmitGeometry, CodecError> {
    let mut map = IdentityMap::new(|source: &str| {
        let suffix = source.strip_prefix(BUILDER_FORMAT).unwrap_or(source);
        Ok(match namespace {
            Some(namespace) => format!("ironcad:brep/{namespace}/{suffix}"),
            None => format!("ironcad:{suffix}"),
        })
    });
    let TransmitGeometry {
        mut geometry,
        unknowns,
        losses,
    } = decoded;
    macro_rules! rewrite {
        ($($field:ident),+ $(,)?) => {
            $(geometry.$field = geometry.$field.rewrite_identities(ctx, &mut map)?;)+
        };
    }
    rewrite!(
        bodies,
        regions,
        shells,
        faces,
        loops,
        coedges,
        edges,
        vertices,
        points,
        surfaces,
        procedural_surfaces,
        curves,
        procedural_curves,
        pcurves,
    );
    let unknowns = unknowns.rewrite_identities(ctx, &mut map)?;
    map.finish()?;
    Ok(TransmitGeometry {
        geometry,
        unknowns,
        losses,
    })
}
