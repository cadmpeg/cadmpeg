// SPDX-License-Identifier: Apache-2.0
//! Parasolid kernel records of `Brep Data`.

use cadmpeg_codec_sldprt::transmit::{self, TransmitGeometry};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::rewrite::typed::{IdentityMap, RewriteIdentities};
use cadmpeg_ir::topology::Color;
use cadmpeg_parasolid::records::node_type::{ATTRIBUTE, FACE, REAL_VALUES};
use cadmpeg_parasolid::records::primary::Node;
use cadmpeg_parasolid::records::PrimaryRecords;
use std::collections::{BTreeMap, BTreeSet};

/// Identity format of the entities that the shared transmit builder mints.
const BUILDER_FORMAT: &str = "sldprt:";

/// Identifier of the system colour attribute definition.
const COLOUR: &[u8] = b"SDL/TYSA_COLOUR";

/// Identity prefix of the faces that the transmit builder mints.
const BUILDER_FACE: &str = "sldprt:brep:face#";

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
    let mut decoded = transmit::decode(ctx, &neutral, &name)?
        .ok_or_else(|| CodecError::malformed("neutral Parasolid transmit has no header"))?;
    let colours = face_colours(&neutral)?;
    for face in &mut decoded.geometry.faces {
        if face.color.is_some() {
            continue;
        }
        face.color = face
            .id
            .as_str()
            .strip_prefix(BUILDER_FACE)
            .and_then(|index| index.parse::<u32>().ok())
            .and_then(|index| colours.get(&index).copied());
    }
    Ok(decoded)
}

/// The colour of each FACE node whose attribute chain holds a
/// `SDL/TYSA_COLOUR` attribute (`parasolid.md` §6), keyed by node index.
fn face_colours(neutral: &[u8]) -> Result<BTreeMap<u32, Color>, CodecError> {
    let Some(primary) = PrimaryRecords::read(neutral)? else {
        return Ok(BTreeMap::new());
    };
    let nodes: BTreeMap<u32, Node<'_>> = primary.nodes().map(|node| (node.index(), node)).collect();
    let mut colours = BTreeMap::new();
    for face in nodes.values().filter(|node| node.kind() == FACE) {
        let mut next = face.pointer("attributes_features");
        let mut visited = BTreeSet::new();
        while let Some(index) = next.filter(|index| visited.insert(*index)) {
            let Some(attribute) = nodes.get(&index).filter(|node| node.kind() == ATTRIBUTE) else {
                break;
            };
            if let Some(colour) = colour(&nodes, attribute) {
                colours.insert(face.index(), colour);
                break;
            }
            next = attribute.pointer("next");
        }
    }
    Ok(colours)
}

/// The RGB colour of `attribute` when its definition is the system colour.
fn colour(nodes: &BTreeMap<u32, Node<'_>>, attribute: &Node<'_>) -> Option<Color> {
    let definition = nodes.get(&attribute.pointer("definition")?)?;
    let identifier = nodes.get(&definition.pointer("identifier")?)?;
    if identifier.chars("string")?.as_slice() != COLOUR {
        return None;
    }
    let field = attribute.pointer_list("fields")?;
    let values = nodes
        .get(field.first()?)
        .filter(|node| node.kind() == REAL_VALUES)?
        .reals("values")?;
    let [r, g, b] = values.as_slice() else {
        return None;
    };
    Color::new(*r, *g, *b, 1.0)
}

/// Moves every identity of `decoded` into the `ironcad` namespace, under
/// `namespace` when the scene holds more than one part result.
pub(crate) fn qualify(
    ctx: &DecodeContext<'_>,
    decoded: TransmitGeometry,
    namespace: Option<&str>,
) -> Result<TransmitGeometry, CodecError> {
    let prefix = identity_prefix(namespace);
    let mut map = IdentityMap::new(|source: &str| {
        let suffix = source.strip_prefix(BUILDER_FORMAT).unwrap_or(source);
        Ok(format!("{prefix}{suffix}"))
    });
    let TransmitGeometry {
        geometry,
        unknowns,
        losses,
    } = decoded;
    let geometry = geometry.rewrite_identities(ctx, &mut map)?;
    let unknowns = unknowns.rewrite_identities(ctx, &mut map)?;
    map.finish()?;
    Ok(TransmitGeometry {
        geometry,
        unknowns,
        losses,
    })
}

/// The identity prefix that replaces the builder's format, under `namespace`
/// when the scene holds more than one part result.
pub(crate) fn identity_prefix(namespace: Option<&str>) -> String {
    match namespace {
        Some(namespace) => format!("ironcad:brep/{namespace}/"),
        None => "ironcad:".to_owned(),
    }
}

/// `message` with each builder identity prefix replaced by `prefix`.
pub(crate) fn rewrite_message(message: &str, prefix: &str) -> String {
    message.replace(BUILDER_FORMAT, prefix)
}
