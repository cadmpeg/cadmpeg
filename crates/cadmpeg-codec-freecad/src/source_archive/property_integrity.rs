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
                    "freecad.objects"
                        | "freecad.object_declaration"
                        | "freecad.object_data_section"
                        | "freecad.object_data"
                        | "freecad.properties"
                        | "freecad.property"
                        | "freecad.property_value"
                )
        })
    };
    if failures().next().is_none() {
        return Ok(());
    }
    let document = archive.open(ctx, "Document.xml")?;
    let xml = crate::container::admit_document(ctx, document.window())?;
    let nodes = || {
        let root = xml.root_element();
        let containers = root
            .children()
            .filter(|node| node.has_tag_name("Properties"))
            .chain(
                root.children()
                    .filter(|node| {
                        node.has_tag_name("ObjectData") || node.has_tag_name("FeatureData")
                    })
                    .flat_map(|section| {
                        section.children().filter(|node| {
                            node.has_tag_name("Object") || node.has_tag_name("Feature")
                        })
                    })
                    .flat_map(|object| {
                        object
                            .children()
                            .filter(|node| node.has_tag_name("Properties"))
                    }),
            );
        let properties = containers.flat_map(|container| {
            std::iter::once(("freecad.properties", container)).chain(
                container
                    .children()
                    .filter(|node| crate::persistence::document_properties::selected(*node))
                    .flat_map(|node| {
                        std::iter::once(("freecad.property", node)).chain(
                            node.children()
                                .filter(roxmltree::Node::is_element)
                                .map(|value| ("freecad.property_value", value)),
                        )
                    }),
            )
        });
        let declarations = root
            .children()
            .filter(|node| node.has_tag_name("Objects") || node.has_tag_name("Features"))
            .flat_map(|container| {
                std::iter::once(("freecad.objects", container)).chain(
                    container
                        .children()
                        .filter(|node| node.has_tag_name("Object") || node.has_tag_name("Feature"))
                        .map(|node| ("freecad.object_declaration", node)),
                )
            });
        let data = root
            .children()
            .filter(|node| node.has_tag_name("ObjectData") || node.has_tag_name("FeatureData"))
            .flat_map(|container| {
                std::iter::once(("freecad.object_data_section", container)).chain(
                    container
                        .children()
                        .filter(|node| node.has_tag_name("Object") || node.has_tag_name("Feature"))
                        .map(|node| ("freecad.object_data", node)),
                )
            });
        declarations.chain(data).chain(properties)
    };
    let count = nodes()
        .try_fold(0usize, |sum, _| sum.checked_add(1))
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
    for (index, (level, node)) in nodes().enumerate() {
        let extent = crate::fidelity::relative_xml_extent(document, node.range())?;
        let capture = ctx.capture_part_extent(extent)?;
        actual[index] = (level, capture.offset(), capture.byte_len(), extent);
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
