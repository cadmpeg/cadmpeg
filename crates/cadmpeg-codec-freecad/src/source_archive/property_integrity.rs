// SPDX-License-Identifier: Apache-2.0
//! Given-up persistence elements agree with recovering source boundaries.

use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::SourceFidelity;

pub(super) fn validate<'source>(
    ctx: &DecodeContext<'source>,
    archive: &ArchiveSnapshot<'source>,
    fidelity: &SourceFidelity,
) -> Result<(), CodecError> {
    if !fidelity.parts().values().any(persistence_failure) {
        return Ok(());
    }
    let document = archive.open(ctx, "Document.xml")?;
    let source = crate::xml_framing::XmlSource::new(ctx, document)?;
    let mut levels = ctx.alloc_filled(
        source.elements().len(),
        None,
        "FreeCAD XML source integrity",
    )?;
    for (index, element) in source.elements().iter().enumerate() {
        let parent = element.parent().and_then(|parent| levels[parent]);
        let name = source.name(index)?;
        levels[index] = match (parent, name) {
            (None, "Document") if element.parent().is_none() => Some("freecad.document"),
            (Some("freecad.document"), "Objects" | "Features") => Some("freecad.objects"),
            (Some("freecad.objects"), "ObjectDeps") => Some("freecad.object_deps"),
            (Some("freecad.object_deps"), "Dep") => Some("freecad.object_dep"),
            (Some("freecad.objects"), "Object" | "Feature") => Some("freecad.object_declaration"),
            (Some("freecad.document"), "ObjectData" | "FeatureData") => {
                Some("freecad.object_data_section")
            }
            (Some("freecad.object_data_section"), "Object" | "Feature") => {
                Some("freecad.object_data")
            }
            (Some("freecad.object_data"), "Extensions") => Some("freecad.extensions"),
            (Some("freecad.extensions"), "Extension") => Some("freecad.extension"),
            (
                Some("freecad.document" | "freecad.object_data" | "freecad.extension"),
                "Properties",
            ) => Some("freecad.properties"),
            (Some("freecad.properties"), "Property" | "_Property") => Some("freecad.property"),
            (Some("freecad.property"), _) => Some("freecad.property_value"),
            (Some("freecad.property_value" | "freecad.value_element"), _) => {
                Some("freecad.value_element")
            }
            (Some("freecad.document"), "StringHasher" | "StringHasher2") => {
                Some("freecad.string_hasher")
            }
            _ => None,
        };
    }
    let capture = ctx.capture_part_extent(document)?;
    for (id, part) in fidelity
        .parts()
        .iter()
        .filter(|(_, part)| persistence_failure(part))
    {
        if part.stream() != capture.stream() {
            continue;
        }
        let matched = source
            .elements()
            .iter()
            .enumerate()
            .find(|(index, element)| {
                levels[*index] == Some(part.level().as_str())
                    && u64_from_index(element.extent().start()) == part.offset()
                    && u64_from_index(element.extent().window().len()) == part.byte_len()
            })
            .map(|(_, element)| element.extent())
            .ok_or_else(|| {
                CodecError::malformed(
                    "given-up persistence extent disagrees with its source hierarchy",
                )
            })?;
        let layout = fidelity.record_layout().get(id.as_str()).ok_or_else(|| {
            CodecError::malformed("given-up persistence element has no source layout")
        })?;
        let retained = fidelity.retained_record(id.as_str()).ok_or_else(|| {
            CodecError::malformed("given-up persistence element has no source bytes")
        })?;
        if layout.stream() != capture.stream()
            || retained.stream() != capture.stream()
            || layout.offset() != part.offset()
            || retained.offset() != part.offset()
            || layout.byte_len() != part.byte_len()
            || retained.byte_len() != part.byte_len()
            || retained.data() != Some(matched.window())
        {
            return Err(CodecError::malformed(
                "given-up persistence source disagrees with its original element",
            ));
        }
    }
    Ok(())
}

fn persistence_failure(part: &cadmpeg_ir::source_fidelity::parts::PartRecord) -> bool {
    matches!(part.state(), PartState::GivenUp {})
        && matches!(
            part.level().as_str(),
            "freecad.document"
                | "freecad.objects"
                | "freecad.object_deps"
                | "freecad.object_dep"
                | "freecad.object_declaration"
                | "freecad.object_data_section"
                | "freecad.object_data"
                | "freecad.extensions"
                | "freecad.extension"
                | "freecad.properties"
                | "freecad.property"
                | "freecad.property_value"
                | "freecad.value_element"
                | "freecad.string_hasher"
        )
}
