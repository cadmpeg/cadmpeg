// SPDX-License-Identifier: Apache-2.0
//! Product array fields consume their own original header and row getters.

use super::*;
use crate::gui::payloads::sources::{ArrayHeader, PayloadSources};
use crate::native::entry_contents::EntryContents;
use cadmpeg_core::decode::RecordOwner;

struct Rows<'a> {
    header: Option<ArrayHeader<'a>>,
    contents: Option<&'a EntryContents>,
    count: Option<usize>,
}

fn rows<'a, 'source>(
    ctx: &DecodeContext<'source>,
    properties: &[&'a PropertyRecord],
    entries: &'a [crate::native::EntryRecord],
    producer: &RecordOwner<'_>,
    sources: &'a PayloadSources<'source>,
    grammar: (&str, &str, &str),
) -> Result<Rows<'a>, CodecError> {
    let (name, runtime, root) = grammar;
    let Some(property) = sole_named_property("product", properties, name)? else {
        return Ok(Rows {
            header: None,
            contents: None,
            count: Some(0),
        });
    };
    require_root(property, runtime, name, root)?;
    let contents = property.side_entries().iter().find_map(|name| {
        entries
            .iter()
            .find(|entry| entry.name() == name)
            .and_then(|entry| {
                entry.contents().iter().find(|contents| {
                    matches!(
                        (runtime, contents),
                        (
                            "App::PropertyPlacementList",
                            EntryContents::Placement { .. }
                        ) | ("App::PropertyVectorList", EntryContents::Vector { .. })
                    )
                })
            })
    });
    let header = sources.header(ctx, producer, &property.id)?;
    let count = if let Some(header) = &header {
        let count = header.count();
        let held = match contents {
            Some(EntryContents::Placement { values, .. }) => Some(values.len()),
            Some(EntryContents::Vector { values, .. }) => Some(values.len()),
            _ => None,
        };
        if held.is_some_and(|held| held != count) {
            return Err(malformed("product contents differ from their actual count"));
        }
        Some(count)
    } else {
        if contents.is_some() {
            return Err(malformed(
                "held product list header has no original supplier",
            ));
        }
        let has_pointer = property.values().iter().any(|value| {
            value
                .attributes
                .get("file")
                .is_some_and(|file| !file.is_empty())
        });
        if !has_pointer {
            Some(0)
        } else {
            integer_property(properties, "ElementCount")?
                .map(|count| usize::try_from(count).map_err(CodecError::malformed))
                .transpose()?
        }
    };
    Ok(Rows {
        header,
        contents,
        count,
    })
}

pub(super) fn placement_list<'source>(
    ctx: &DecodeContext<'source>,
    properties: &[&PropertyRecord],
    entries: &[crate::native::EntryRecord],
    producer: &RecordOwner<'_>,
    sources: &PayloadSources<'source>,
) -> Result<Option<Vec<Option<FiniteFrame>>>, CodecError> {
    let rows = rows(
        ctx,
        properties,
        entries,
        producer,
        sources,
        (
            "PlacementList",
            "App::PropertyPlacementList",
            "PlacementList",
        ),
    )?;
    rows.count
        .map(|count| {
            let mut output = ctx.alloc_filled(count, None, "FreeCAD product frame slots")?;
            if let Some(EntryContents::Placement { values, .. }) = rows.contents {
                let header = rows_header(&rows.header)?;
                for (ordinal, (slot, value)) in output.iter_mut().zip(values).enumerate() {
                    if value.is_some() {
                        *slot = Some(sources.placement(ctx, producer, ordinal, header)?);
                    }
                }
            }
            Ok(output)
        })
        .transpose()
}

fn rows_header<'a, 'source>(
    header: &'a Option<ArrayHeader<'source>>,
) -> Result<&'a ArrayHeader<'source>, CodecError> {
    header
        .as_ref()
        .ok_or_else(|| malformed("held product list row has no original header"))
}

pub(super) fn vector_list<'source>(
    ctx: &DecodeContext<'source>,
    properties: &[&PropertyRecord],
    entries: &[crate::native::EntryRecord],
    producer: &RecordOwner<'_>,
    sources: &PayloadSources<'source>,
) -> Result<Option<Vec<Option<FiniteVector<3>>>>, CodecError> {
    let rows = rows(
        ctx,
        properties,
        entries,
        producer,
        sources,
        ("ScaleList", "App::PropertyVectorList", "VectorList"),
    )?;
    rows.count
        .map(|count| {
            let mut output = ctx.alloc_filled(count, None, "FreeCAD product scale slots")?;
            if let Some(EntryContents::Vector { values, .. }) = rows.contents {
                let header = rows_header(&rows.header)?;
                for (ordinal, (slot, value)) in output.iter_mut().zip(values).enumerate() {
                    if value.is_some() {
                        *slot = Some(sources.vector(ctx, producer, ordinal, header)?);
                    }
                }
            }
            Ok(output)
        })
        .transpose()
}

fn header<'a, 'source>(
    reader: &'a crate::property_reader::PropertyReader<'_, 'source>,
    id: &str,
    property: &'a PropertyRecord,
    producer: &mut Option<RecordOwner<'source>>,
) -> Result<Option<ArrayHeader<'a>>, CodecError> {
    if let Some(producer) = producer {
        reader.source_payload_header(producer, property)
    } else if let Some((header, owner)) = reader.begin_payload_record(id, property)? {
        *producer = Some(owner);
        Ok(Some(header))
    } else {
        Ok(None)
    }
}

fn named<'a>(
    properties: &'a [PropertyRecord],
    owner: &str,
    role: &str,
) -> Option<&'a PropertyRecord> {
    properties
        .iter()
        .find(|property| property.owner == owner && property.name == role)
}

pub(super) fn occurrence_arity<'source>(
    reader: &crate::property_reader::PropertyReader<'_, 'source>,
    record: &ProductNodeRecord,
    properties: &[PropertyRecord],
    id: &str,
    expected: usize,
    producer: &mut Option<RecordOwner<'source>>,
) -> Result<usize, CodecError> {
    if !reader.has_payload_sources() || record.element_count().is_some() {
        return Ok(expected);
    }
    for role in ["PlacementList", "ScaleList"] {
        let Some(property) = named(properties, &record.object, role) else {
            continue;
        };
        if let Some(header) = header(reader, id, property, producer)? {
            let actual = NonZeroUsize::new(header.count()).unwrap_or(NonZeroUsize::MIN);
            if actual.get() != expected {
                return Err(malformed(
                    "product occurrence arity differs from its actual header",
                ));
            }
            return Ok(actual.get());
        }
    }
    Ok(expected)
}

pub(super) fn placement<'source>(
    reader: &crate::property_reader::PropertyReader<'_, 'source>,
    properties: &[PropertyRecord],
    record: &ProductNodeRecord,
    source: (&str, usize, &mut Option<RecordOwner<'source>>),
    fallback: FiniteFrame,
) -> Result<FiniteFrame, CodecError> {
    if !reader.has_payload_sources() {
        return Ok(fallback);
    }
    let (id, index, producer) = source;
    let property = named(properties, &record.object, "PlacementList")
        .ok_or_else(|| malformed("held product placement has no original property"))?;
    let header = header(reader, id, property, producer)?
        .ok_or_else(|| malformed("held product placement has no original header"))?;
    if index >= header.count() {
        return Err(malformed(
            "product placement ordinal exceeds its actual count",
        ));
    }
    reader
        .source_placement(
            producer
                .as_ref()
                .ok_or_else(|| malformed("product placement producer is absent"))?,
            property,
            index,
            &header,
        )?
        .ok_or_else(|| malformed("source product placement has no actual supplier"))
}

pub(super) fn vector<'source>(
    reader: &crate::property_reader::PropertyReader<'_, 'source>,
    properties: &[PropertyRecord],
    record: &ProductNodeRecord,
    source: (&str, usize, &mut Option<RecordOwner<'source>>),
    fallback: FiniteVector<3>,
) -> Result<FiniteVector<3>, CodecError> {
    if !reader.has_payload_sources() {
        return Ok(fallback);
    }
    let (id, index, producer) = source;
    let property = named(properties, &record.object, "ScaleList")
        .ok_or_else(|| malformed("held product scale has no original property"))?;
    let header = header(reader, id, property, producer)?
        .ok_or_else(|| malformed("held product scale has no original header"))?;
    if index >= header.count() {
        return Err(malformed("product scale ordinal exceeds its actual count"));
    }
    reader
        .source_vector(
            producer
                .as_ref()
                .ok_or_else(|| malformed("product scale producer is absent"))?,
            property,
            index,
            &header,
        )?
        .ok_or_else(|| malformed("source product scale has no actual supplier"))
}
