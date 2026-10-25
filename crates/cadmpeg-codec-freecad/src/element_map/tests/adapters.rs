// SPDX-License-Identifier: Apache-2.0
//! Unit adapters enter the same source helpers as complete FCStd decoding.

use crate::element_map::{adopted, parts, ParsedMap};
use crate::native::element_map::ElementMapRecord;
use crate::native::{StringTableEntry, StringTableRecord, StringTables};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};
use std::collections::BTreeMap;
use std::ops::Range;

pub(in crate::element_map) fn parse_string_table(
    bytes: &[u8],
    count: usize,
    side: bool,
) -> Result<(Vec<StringTableEntry>, Vec<Range<usize>>), CodecError> {
    let arena = DecodeArena::new();
    let (ctx, view) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())?;
    let view = ctx.name_stream(view, "table.txt")?;
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let result = ctx.decode_part(
        &mut output,
        view,
        PartSpec::new(
            PartLevel::parse(if side {
                "freecad.string_table_entry"
            } else {
                "freecad.string_hasher"
            })
            .unwrap(),
            Vec::new(),
        ),
        |stage| parts::table(&ctx, view, Some(count), side, false, None, stage),
    )?;
    if !output.losses().is_empty() {
        return Err(CodecError::malformed(output.losses()[0].message.clone()));
    }
    let result = result
        .flatten()
        .ok_or_else(|| CodecError::malformed("table callback gave up"))?;
    let mut ranges = output
        .fidelity()
        .parts()
        .values()
        .filter(|part| {
            matches!(
                part.level().as_str(),
                "freecad.string_hasher_record" | "freecad.string_table_record"
            )
        })
        .map(|part| {
            let start = usize::try_from(part.offset()).unwrap();
            let end = start + usize::try_from(part.byte_len()).unwrap();
            let text = std::str::from_utf8(&bytes[start..end]).unwrap();
            start..start + text.trim_end_matches(char::is_whitespace).len()
        })
        .collect::<Vec<_>>();
    ranges.sort_by_key(|range| range.start);
    Ok((result.entries.into_iter().flatten().collect(), ranges))
}

pub(in crate::element_map) fn parse_element_map(
    bytes: &[u8],
    side: bool,
) -> Result<ParsedMap, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, view) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())?;
    let view = ctx.name_stream(view, "map.txt")?;
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let result = ctx.decode_part(
        &mut output,
        view,
        PartSpec::new(
            PartLevel::parse(if side {
                "freecad.element_map_entry"
            } else {
                "freecad.property_value"
            })
            .unwrap(),
            Vec::new(),
        ),
        |stage| parts::map(&ctx, view, side, parts::StringIds::default(), None, stage),
    )?;
    if !output.losses().is_empty() {
        return Err(CodecError::malformed(output.losses()[0].message.clone()));
    }
    result
        .flatten()
        .ok_or_else(|| CodecError::malformed("map callback gave up"))
}

pub(in crate::element_map) fn node_text_bytes(node: roxmltree::Node<'_, '_>) -> Vec<u8> {
    node.children()
        .filter_map(|child| child.is_text().then(|| child.text()).flatten())
        .flat_map(str::bytes)
        .collect()
}

pub(super) fn parse(
    document: &[u8],
    file_version: Option<usize>,
    properties: &[crate::test_support::source_fidelity::PropertyFixture],
    entries: &[crate::test_support::source_fidelity::EntryFixture],
    policy: &DecodePolicy,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(StringTables, Vec<ElementMapRecord>), CodecError> {
    // One actual document source preserves the production source ordinal domain
    // even when a unit fixture supplies its property XML separately.
    let absent = properties
        .iter()
        .filter(|property| {
            !document
                .windows(property.source.len())
                .any(|window| window == property.source.as_bytes())
        })
        .collect::<Vec<_>>();
    let mut bytes = document.to_vec();
    if !absent.is_empty() {
        let xml = roxmltree::Document::parse(
            std::str::from_utf8(document).map_err(CodecError::malformed)?,
        )
        .map_err(CodecError::malformed)?;
        let root = xml.root_element();
        let end = root.range().end;
        let self_closing = document[..end].ends_with(b"/>");
        let insertion = if self_closing {
            end - 2
        } else {
            document[..end]
                .windows(2)
                .rposition(|window| window == b"</")
                .ok_or_else(|| CodecError::malformed("test Document has no closing tag"))?
        };
        let mut added = Vec::new();
        if self_closing {
            added.push(b'>');
        }
        for property in absent {
            added.extend_from_slice(property.source.as_bytes());
        }
        if self_closing {
            added.extend_from_slice(b"</Document>");
        }
        bytes.splice(insertion..if self_closing { end } else { insertion }, added);
    }
    let document_len = bytes.len();
    let property_ranges = properties
        .iter()
        .map(|property| {
            let start = bytes
                .windows(property.source.len())
                .position(|window| window == property.source.as_bytes())
                .ok_or_else(|| CodecError::malformed("test property has no source extent"))?;
            Ok((property, start..start + property.source.len()))
        })
        .collect::<Result<Vec<_>, CodecError>>()?;
    let mut entry_ranges = Vec::new();
    for entry in entries {
        let start = bytes.len();
        bytes.extend_from_slice(&entry.source);
        entry_ranges.push((entry.name().to_owned(), start..bytes.len()));
    }
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, policy)?;
    let entry_views = entry_ranges
        .into_iter()
        .map(|(name, range)| (name, root.child(range.start, range.end).unwrap()))
        .collect::<BTreeMap<_, _>>();
    let context = adopted::Context::new(&entry_views, file_version);
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let source = crate::xml_framing::XmlSource::new(&ctx, root.child(0, document_len).unwrap())?;
    let mut callbacks = Vec::new();
    for (index, element) in source.elements().iter().enumerate() {
        let name = source.name(index)?;
        if !matches!(
            name,
            "StringHasher" | "StringHasher2" | "ElementMap" | "ElementMap2" | "Part"
        ) {
            continue;
        }
        let property = property_ranges
            .iter()
            .find(|(_, range)| {
                range.start <= element.extent().start() && element.extent().end() <= range.end
            })
            .map(|(property, _)| &property.property);
        if property.is_some_and(|property| property.type_name != "Part::PropertyPartShape") {
            continue;
        }
        if name == "Part" && property.is_none() {
            continue;
        }
        callbacks.push((
            !matches!(name, "StringHasher" | "StringHasher2"),
            index,
            property,
        ));
    }
    callbacks.sort_by_key(|(consumer, index, _)| (*consumer, *index));
    let mut previous = BTreeMap::<String, Vec<crate::native::ValueRecord>>::new();
    for (_, index, property) in callbacks {
        let level = if property.is_some() {
            "freecad.property_value"
        } else {
            "freecad.string_hasher"
        };
        let held = property
            .and_then(|property| previous.get(&property.id))
            .map_or(&[][..], Vec::as_slice);
        let value = ctx
            .decode_part(
                &mut output,
                source.extent(index)?,
                source.spec(index, level)?,
                |stage| decode_value(&ctx, &source, index, property, held, &context, stage),
            )?
            .flatten();
        if let (Some(property), Some(value)) = (property, value) {
            previous.entry(property.id.clone()).or_default().push(value);
        }
    }
    output.prune_dependencies(&ctx)?;
    losses.extend_from_slice(output.losses());
    let namespace = output.native().namespace("fcstd");
    let mut tables = namespace
        .map(|native| native.arena_as::<StringTableRecord>("string_tables"))
        .transpose()?
        .unwrap_or_default();
    tables.sort_by_key(|table| table.index);
    let maps = namespace
        .map(|native| native.arena_as::<ElementMapRecord>("element_maps"))
        .transpose()?
        .unwrap_or_default();
    Ok((StringTables::try_from(tables)?, maps))
}

/// Unit callers retain an actual Part value only after its authentic helper commits.
pub(in crate::element_map) fn decode_value<'source>(
    ctx: &DecodeContext<'source>,
    source: &crate::xml_framing::XmlSource<'source>,
    index: usize,
    property: Option<&crate::native::PropertyRecord>,
    previous: &[crate::native::ValueRecord],
    context: &adopted::Context<'_, 'source>,
    stage: &mut PartOutput,
) -> Result<Option<crate::native::ValueRecord>, CodecError> {
    adopted::decode(ctx, source, index, property, Some(previous), context, stage)?;
    if source.name(index)? != "Part" {
        return Ok(None);
    }
    let property = property.ok_or_else(|| CodecError::malformed("test Part has no property"))?;
    ctx.record(&property.id, source.extent(index)?, || {
        Ok(Some(crate::native::ValueRecord {
            item_index: None,
            link_index: None,
            tag: "Part".into(),
            order: 0,
            parent: None,
            attributes: source.attributes(ctx, index)?,
            decoded_json: None,
            text: None,
        }))
    })
}

pub(in crate::element_map) fn direct_element_map(
    node: roxmltree::Node<'_, '_>,
) -> Result<(), CodecError> {
    let property = crate::test_support::source_fidelity::PropertyFixture::new(
        crate::native::PropertyRecord {
            dynamic: None,
            id: "fcstd:test:property#Shape".into(),
            owner: "fcstd:test:object#Shape".into(),
            name: "Shape".into(),
            type_name: "Part::PropertyPartShape".into(),
            family: crate::native::PropertyFamily::Unknown,
            status: None,
            body: Some(crate::native::PropertyBody::Persisted {
                values: Vec::new().into(),
                links: Vec::new(),
                side_entries: Vec::new(),
            }),
            order: 0,
        },
        &node.document().input_text()[node.range()],
    );
    let mut losses = Vec::new();
    parse(
        b"<Document/>",
        Some(1),
        &[property],
        &[],
        &DecodePolicy::service(),
        &mut losses,
    )?;
    if let Some(loss) = losses.first() {
        Err(CodecError::malformed(loss.message.clone()))
    } else {
        Ok(())
    }
}

pub(in crate::element_map) fn parse_legacy_element_map(
    marker: roxmltree::Node<'_, '_>,
    file_version: Option<usize>,
    entry_data: &std::collections::HashMap<&str, &[u8]>,
    _owner: &str,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<(), CodecError> {
    let parent = marker.parent().unwrap();
    let property = crate::test_support::source_fidelity::PropertyFixture::new(
        crate::native::PropertyRecord {
            dynamic: None,
            id: "fcstd:test:property#Shape".into(),
            owner: "fcstd:test:object#Shape".into(),
            name: "Shape".into(),
            type_name: "Part::PropertyPartShape".into(),
            family: crate::native::PropertyFamily::Unknown,
            status: None,
            body: Some(crate::native::PropertyBody::Persisted {
                values: Vec::new().into(),
                links: Vec::new(),
                side_entries: Vec::new(),
            }),
            order: 0,
        },
        &parent.document().input_text()[parent.range()],
    );
    let entries = entry_data
        .iter()
        .map(|(name, bytes)| super::legacy_entry(name, bytes))
        .collect::<Vec<_>>();
    let start = losses.len();
    parse(
        b"<Document/>",
        file_version,
        &[property],
        &entries,
        &DecodePolicy::service(),
        losses,
    )?;
    if let Some(loss) = losses.get(start) {
        Err(CodecError::malformed(loss.message.clone()))
    } else {
        Ok(())
    }
}

pub(super) fn hasher_parts(document: &[u8]) -> Result<PartOutput, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, view) = DecodeContext::from_root_bytes(document, &arena, &DecodePolicy::service())?;
    let source = crate::xml_framing::XmlSource::new(&ctx, view)?;
    let entries = BTreeMap::new();
    let context = adopted::Context::new(&entries, Some(1));
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    for (index, element) in source.elements().iter().enumerate() {
        if !matches!(source.name(index)?, "StringHasher" | "StringHasher2") {
            continue;
        }
        let mut parent = element.parent();
        let mut property = None;
        while let Some(owner) = parent {
            if source.name(owner)? == "Property" {
                property = Some(crate::native::PropertyRecord {
                    dynamic: None,
                    id: format!("fcstd:test:property#{owner}"),
                    owner: "fcstd:test:object#Shape".into(),
                    name: source
                        .peek_attribute(&ctx, owner, "name")?
                        .unwrap_or_default(),
                    type_name: source
                        .peek_attribute(&ctx, owner, "type")?
                        .unwrap_or_default(),
                    family: crate::native::PropertyFamily::Geometry,
                    status: None,
                    body: None,
                    order: owner,
                });
                break;
            }
            parent = source.elements()[owner].parent();
        }
        let level = if property.is_some() {
            "freecad.property_value"
        } else {
            "freecad.string_hasher"
        };
        ctx.decode_part(
            &mut output,
            source.extent(index)?,
            source.spec(index, level)?,
            |stage| {
                adopted::decode(
                    &ctx,
                    &source,
                    index,
                    property.as_ref(),
                    None,
                    &context,
                    stage,
                )
            },
        )?;
    }
    Ok(output)
}
