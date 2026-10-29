// SPDX-License-Identifier: Apache-2.0
//! Property-section parts, values and preview assets.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};

use crate::native::{PropertyRecord, PropertySectionRecord};
use crate::property_set::{PropertySection, PropertySetDescriptor};

/// Metadata composition receives only a section whose native and asset stage
/// committed. A failed callback cannot change the document's metadata builder.
pub(super) fn stage<'a>(
    ctx: &DecodeContext<'a>,
    output: &mut PartOutput,
    descriptor: &PropertySetDescriptor<'a>,
    section_ordinal: usize,
    preview_ordinal: &mut usize,
) -> Result<Option<PropertySection<'a>>, CodecError> {
    let (source, fmtid) =
        crate::property_set::source_admission::section_source(descriptor, section_ordinal)?;
    let section_id = format!(
        "inventor:property:section#{}-{section_ordinal}",
        descriptor.stream.directory_id()
    );
    let stream = || descriptor.path.clone();
    ctx.decode_part(
        output,
        source,
        super::part_spec("inventor.property_section", source, stream)?,
        |section_stage| {
            let directory = crate::property_set::parse_section_directory(ctx, source)?;
            section_stage.insert_native(
                ctx,
                "property_sections",
                PropertySectionRecord {
                    id: section_id.clone(),
                    set_path: descriptor.path.clone(),
                    ordinal: super::record_ordinal(
                        section_ordinal,
                        "Inventor property section ordinal",
                    )?,
                    fmtid: super::hex(&fmtid),
                    code_page: directory.code_page,
                    offsets_ordered: directory.offsets_ordered,
                    dictionary_entries: u64_from_index(directory.names.len()),
                    property_count: u64_from_index(directory.ranges.len()),
                },
            )?;
            let crate::property_set::PropertySectionDirectory {
                mut ranges,
                names,
                code_page,
                offsets_ordered,
            } = directory;
            let mut section = PropertySection {
                fmtid,
                code_page,
                offsets_ordered,
                dictionary_entries: names.len(),
                properties: Vec::new(),
            };
            let set_name =
                if let Some((_, start, end)) = ranges.iter().find(|(id, _, _)| *id == 255) {
                    let raw = source
                        .child(source.start() + *start, source.start() + *end)
                        .ok_or_else(|| {
                            CodecError::malformed("property-set name view is outside its section")
                        })?;
                    match crate::property_set::parse_property(
                        ctx,
                        raw,
                        255,
                        names.get(&255).cloned(),
                        code_page,
                    ) {
                        Ok(property) => property.value.scalar_text(),
                        Err(CodecError::ResourceLimit(limit)) => {
                            return Err(CodecError::ResourceLimit(limit))
                        }
                        // The real property callback below owns the unreadable name;
                        // no unproven role alias is inferred from this failed probe.
                        Err(_) => None,
                    }
                } else {
                    None
                };
            let identity_matches = set_name
                .as_deref()
                .and_then(super::known_property_set_fmtid)
                .is_none_or(|expected| expected == fmtid);
            let mut accepted = Vec::new();
            ranges.sort_by_key(|(id, _, _)| *id);
            for (property_id, start, end) in ranges {
                let raw = source
                    .child(source.start() + start, source.start() + end)
                    .ok_or_else(|| CodecError::malformed("property view is outside its section"))?;
                let native_id = format!(
                    "inventor:property:value#{}-{section_ordinal}-{}",
                    descriptor.stream.directory_id(),
                    property_id
                );
                let name = names.get(&property_id).cloned().or_else(|| {
                    identity_matches
                        .then_some(set_name.as_deref())
                        .flatten()
                        .and_then(|set_name| super::built_in_property_name(set_name, property_id))
                        .map(str::to_owned)
                });
                let dependency_property = matches!(property_id, 0 | 1);
                let admitted = ctx.decode_part(
                    section_stage,
                    raw,
                    super::part_spec("inventor.property", raw, stream)?,
                    |property_stage| {
                        let property = crate::property_set::parse_property(
                            ctx,
                            raw,
                            property_id,
                            name,
                            section.code_page,
                        )?;
                        property_stage.insert_native(
                            ctx,
                            "properties",
                            PropertyRecord {
                                id: native_id.clone(),
                                set_path: descriptor.path.clone(),
                                section_ordinal: super::record_ordinal(
                                    section_ordinal,
                                    "Inventor property section ordinal",
                                )?,
                                fmtid: super::hex(&fmtid),
                                property_id: property.id,
                                name: property.name.clone(),
                                value_kind: super::property_value_kind(&property.value),
                                scalar_value: property.value.scalar_text(),
                                raw_len: u64_from_index(raw.window().len()),
                                raw_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(
                                    raw.window(),
                                ),
                            },
                        )?;
                        if super::is_preview(&fmtid, property.id, property.name.as_deref()) {
                            if let Some((bytes, media_type)) = read_preview(&property.value) {
                                let asset = super::form_preview_asset(
                                    ctx,
                                    *preview_ordinal,
                                    &native_id,
                                    bytes,
                                    media_type,
                                )?;
                                // IDs allocated to genuinely formed assets are not reused after a
                                // later failure rolls back this property or its containing section.
                                *preview_ordinal =
                                    preview_ordinal.checked_add(1).ok_or_else(|| {
                                        CodecError::malformed("preview ordinal overflows")
                                    })?;
                                property_stage.insert_model(ctx, asset)?;
                            }
                        }
                        Ok(property)
                    },
                )?;
                if let Some(property) = admitted {
                    accepted.push(property);
                } else if dependency_property {
                    return Err(CodecError::malformed(
                        "property section depends on an unreadable dictionary or code page",
                    ));
                }
            }
            if !identity_matches {
                return Err(CodecError::malformed(
                    "embedded property-set name does not match its FMTID",
                ));
            }
            section.properties = accepted;
            Ok(section)
        },
    )
}

/// Read only a blob whose preview grammar has recognized an image.
fn read_preview<'a>(
    value: &crate::property_set::PropertyValue<'a>,
) -> Option<(&'a [u8], &'static str)> {
    use crate::property_set::PropertyValue;
    let (_, media_type) = super::preview_bytes(value)?;
    let (mut source, clipboard) = match value {
        PropertyValue::Binary { value, .. } => (*value, false),
        PropertyValue::Clipboard { data, .. } => (*data, true),
        _ => return None,
    };
    let length = source.remaining();
    let bytes = source.take(length)?;
    // The recognized clipboard image wrapper contains u32 kind and four u16
    // header fields. Its payload follows those twelve validated source bytes.
    let payload = if clipboard { bytes.get(12..)? } else { bytes };
    Some((payload, media_type))
}

#[cfg(test)]
mod tests;
