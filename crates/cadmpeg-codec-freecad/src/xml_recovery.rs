// SPDX-License-Identifier: Apache-2.0
//! Recovering element-level XML admission.
//!
//! The strict parser reads a member all or nothing. This helper splits the
//! member at element boundaries with the recovering element scanner before
//! parsing: each damaged element is given up as its own part while the
//! surviving elements still decode. The surviving bytes are reassembled from
//! the original member views, so every admitted range keeps its source
//! offsets and the strict parser still owns XML validity.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput, PartSpec};

/// A damaged element's name and its ancestor names, parent first.
pub(crate) type KindMapper = dyn Fn(&[u8], &[&[u8]]) -> PartSpec;

/// Element kinds and ancestors removed by the scanner.
pub(crate) type Damage = Vec<(
    Vec<u8>,
    Vec<Vec<u8>>,
    Option<String>,
    cadmpeg_ir::ids::UnknownId,
)>;

/// The source member and operations used for its XML recovery.
#[derive(Clone, Copy)]
pub(crate) struct XmlSource {
    pub(crate) member: &'static str,
    pub(crate) scan_operation: &'static str,
    pub(crate) parse_operation: &'static str,
}

/// Admit one XML member, recovering readable elements around damaged ones.
///
/// The scanner frames the member before parsing. When parsing fails, each
/// damaged element is blanked out of a reassembled copy while its original
/// bytes are given up as their own part.
/// The returned view is the admitted bytes and the document borrows them, so
/// source ranges stay consistent. A member with no recoverable element
/// returns the original parse error and the caller gives up the whole member.
pub(crate) fn admit_recovering<'s>(
    ctx: &DecodeContext<'s>,
    stage: &mut PartOutput,
    view: View<'s>,
    source: XmlSource,
    map_kind: &KindMapper,
    shape_error: &dyn Fn(String) -> CodecError,
) -> Result<(View<'s>, roxmltree::Document<'s>, Damage), CodecError> {
    let mut damage = Vec::new();
    let (view, xml) = cadmpeg_core::decode::parse_xml_recovering(
        ctx,
        view,
        source.member,
        source.scan_operation,
        source.parse_operation,
        shape_error,
        |extent, name, ancestors, scan| {
            let borrowed: Vec<&[u8]> = ancestors.iter().map(Vec::as_slice).collect();
            let spec = map_kind(name, &borrowed);
            let detail = format!(
                "damaged XML element <{}> cannot be admitted",
                String::from_utf8_lossy(name)
            );
            let stub = spec.unreadable_id(cadmpeg_ir::CodecFormat::Fcstd, extent)?;
            ctx.decode_part(stage, extent, spec, |_| {
                Err::<(), _>(CodecError::malformed(detail.clone()))
            })?;
            let identity = if (name == b"Object" || name == b"Feature")
                && ancestors.first().is_some_and(|parent| {
                    parent == b"ObjectData"
                        || parent == b"FeatureData"
                        || parent == b"Objects"
                        || parent == b"Features"
                }) {
                let element = scan
                    .elements()
                    .iter()
                    .find(|element| element.extent().start() == extent.start());
                let opening_end = element
                    .and_then(|element| element.framing().nth(1))
                    .map(cadmpeg_core::decode::View::start);
                if let Some(prefix) = opening_end.and_then(|end| view.child(extent.start(), end)) {
                    let header = ctx.concat_views(&[prefix, View::over_retained(b"/>")])?;
                    match std::str::from_utf8(header.window()) {
                        Ok(text) => match ctx.parse_xml(text, "recover FreeCAD object identity") {
                            Ok(xml) => xml.root_element().attribute("name").map(str::to_owned),
                            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                            Err(_) => None,
                        },
                        Err(_) => None,
                    }
                } else {
                    None
                }
            } else {
                None
            };
            damage.push((name.to_vec(), ancestors.to_vec(), identity, stub));
            Ok(())
        },
    )?;
    Ok((view, xml, damage))
}
