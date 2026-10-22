// SPDX-License-Identifier: Apache-2.0
//! Failed document property parts agree with their actual XML source nodes.

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::SourceFidelity;

pub(super) fn validate<'source>(
    ctx: &DecodeContext<'source>,
    archive: &ArchiveSnapshot<'source>,
    fidelity: &SourceFidelity,
) -> Result<(), CodecError> {
    let failures = || {
        fidelity.parts().iter().filter(|(_, part)| {
            part.state() == &PartState::GivenUp {}
                && matches!(
                    part.level().as_str(),
                    "freecad.properties" | "freecad.property"
                )
        })
    };
    if failures().next().is_none() {
        return Ok(());
    }
    let document = archive.open(ctx, "Document.xml")?;
    let xml = crate::container::admit_document(ctx, document.window())?;
    let count = xml
        .root_element()
        .children()
        .filter(|node| node.has_tag_name("Properties"))
        .try_fold(0usize, |sum, container| {
            sum.checked_add(1)?.checked_add(
                container
                    .children()
                    .filter(|node| crate::persistence::document_strings::selected(*node))
                    .count(),
            )
        })
        .ok_or_else(|| CodecError::malformed("property source inventory count overflows"))?;
    let bytes = count
        .checked_mul(size_of::<(&str, u64, u64, View<'_>)>())
        .ok_or_else(|| CodecError::malformed("property source inventory storage overflows"))?;
    ctx.retain_bytes(u64_from_index(bytes), "FreeCAD property source integrity")?;
    let mut actual = ctx.alloc_filled(
        count,
        ("", 0, 0, document),
        "FreeCAD property source integrity",
    )?;
    let mut index = 0;
    for container in xml
        .root_element()
        .children()
        .filter(|node| node.has_tag_name("Properties"))
    {
        for (level, node) in std::iter::once(("freecad.properties", container)).chain(
            container
                .children()
                .filter(|node| crate::persistence::document_strings::selected(*node))
                .map(|node| ("freecad.property", node)),
        ) {
            let extent = crate::fidelity::relative_xml_extent(document, node.range())?;
            let capture = ctx.capture_part_extent(extent)?;
            actual[index] = (level, capture.offset(), capture.byte_len(), extent);
            index += 1;
        }
    }
    actual.sort_unstable_by_key(|entry| (entry.0, entry.1, entry.2));
    for (id, part) in failures() {
        let key = (part.level().as_str(), part.offset(), part.byte_len());
        let index = actual
            .binary_search_by_key(&key, |entry| (entry.0, entry.1, entry.2))
            .map_err(|_| {
                CodecError::malformed("given-up property extent disagrees with original XML")
            })?;
        let extent = actual[index].3;
        let capture = ctx.capture_part_extent(extent)?;
        let layout = fidelity
            .record_layout()
            .get(id.as_str())
            .ok_or_else(|| CodecError::malformed("given-up property has no source layout"))?;
        let retained = fidelity
            .retained_record(id.as_str())
            .ok_or_else(|| CodecError::malformed("given-up property has no source bytes"))?;
        if part.stream() != capture.stream()
            || layout.stream() != capture.stream()
            || retained.stream() != capture.stream()
            || layout.offset() != capture.offset()
            || retained.offset() != capture.offset()
            || layout.byte_len() != capture.byte_len()
            || retained.byte_len() != capture.byte_len()
            || retained.data() != Some(extent.window())
        {
            return Err(CodecError::malformed(
                "given-up property source disagrees with original XML",
            ));
        }
    }
    Ok(())
}
