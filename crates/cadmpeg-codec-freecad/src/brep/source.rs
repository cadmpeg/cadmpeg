// SPDX-License-Identifier: Apache-2.0
//! Source-owned text and binary exact-shape schedules.

use super::records::{BrepRow, BrepValue};
use super::source_rows;
use super::*;
pub(super) mod census;
pub(crate) mod projection;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};

pub(super) const TABLES: [&str; 8] = [
    "Locations",
    "Curve2ds",
    "Curves",
    "Polygon3D",
    "PolygonOnTriangulations",
    "Surfaces",
    "Triangulations",
    "TShapes",
];
const TEXT_LEVELS: [&str; 8] = [
    "freecad.brep_text_location",
    "freecad.brep_text_curve2d",
    "freecad.brep_text_curve",
    "freecad.brep_text_polygon3d",
    "freecad.brep_text_polygon_on_triangulation",
    "freecad.brep_text_surface",
    "freecad.brep_text_triangulation",
    "freecad.brep_text_tshape",
];

struct ShapeProperty<'a> {
    id: &'a str,
    type_name: &'a str,
    values: &'a [crate::native::ValueRecord],
}
impl<'a> From<&'a PropertyRecord> for ShapeProperty<'a> {
    fn from(value: &'a PropertyRecord) -> Self {
        Self {
            id: &value.id,
            type_name: &value.type_name,
            values: value.values(),
        }
    }
}
impl<'a> From<&'a crate::native::GuiPropertyRecord> for ShapeProperty<'a> {
    fn from(value: &'a crate::native::GuiPropertyRecord) -> Self {
        Self {
            id: &value.id,
            type_name: &value.type_name,
            values: &value.values,
        }
    }
}

pub(crate) fn decode_payloads<'source>(
    ctx: &crate::property_reader::PropertyReader<'_, '_>,
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
    sources: &crate::entry_sources::EntrySources<'source>,
    output: &mut PartOutput,
    projection: &mut projection::ProjectionSources<'source>,
) -> Result<Vec<ShapePayloadRecord>, CodecError> {
    decode_properties(
        ctx,
        properties.iter().map(ShapeProperty::from),
        entries,
        sources,
        output,
        projection,
    )
}

pub(crate) fn decode_gui_payloads<'source>(
    ctx: &crate::property_reader::PropertyReader<'_, '_>,
    properties: &[crate::native::GuiPropertyRecord],
    entries: &[EntryRecord],
    sources: &crate::entry_sources::EntrySources<'source>,
    output: &mut PartOutput,
    projection: &mut projection::ProjectionSources<'source>,
) -> Result<Vec<ShapePayloadRecord>, CodecError> {
    decode_properties(
        ctx,
        properties.iter().map(ShapeProperty::from),
        entries,
        sources,
        output,
        projection,
    )
}

fn decode_properties<'a, 'source>(
    ctx: &crate::property_reader::PropertyReader<'_, '_>,
    properties: impl IntoIterator<Item = ShapeProperty<'a>>,
    entries: &[EntryRecord],
    sources: &crate::entry_sources::EntrySources<'source>,
    output: &mut PartOutput,
    projection: &mut projection::ProjectionSources<'source>,
) -> Result<Vec<ShapePayloadRecord>, CodecError> {
    let by_name = entries
        .iter()
        .map(|entry| (entry.name(), entry))
        .collect::<BTreeMap<_, _>>();
    let mut payloads = Vec::new();
    for property in properties.into_iter().filter(|property| {
        matches!(
            property.type_name,
            "Part::PropertyPartShape" | "Part::PropertyTopoShapeList"
        )
    }) {
        for value in property.values.iter().filter(|value| {
            (property.type_name == "Part::PropertyPartShape"
                && value.parent.is_none()
                && value.tag == "Part")
                || (property.type_name == "Part::PropertyTopoShapeList" && value.tag == "TopoShape")
        }) {
            let Some(name) = value.attributes.get("file").filter(|name| !name.is_empty()) else {
                continue;
            };
            let Some(entry) = by_name.get(name.as_str()) else {
                continue;
            };
            let Some(source) = sources.optional_view(entry) else {
                continue;
            };
            let id = native::native_child_id("shape-payload", property.id, name);
            let binary = name
                .rsplit_once('.')
                .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("bin"));
            let level = if binary {
                "freecad.brep_binary"
            } else {
                "freecad.brep_text"
            };
            let part_spec = spec(level, Vec::new())?;
            let part_spec = match sources.parent(entry) {
                Some(parent) => part_spec.with_parent(parent),
                None => part_spec,
            };
            let mut marks = projection::RowMarks::default();
            if let Some(Some(payload)) = ctx.decode_part(output, source, part_spec, |stage| {
                let payload = if source.window().is_empty() {
                    ctx.record(&id, source, || Ok(()))?;
                    Some(ShapePayload::Empty)
                } else if binary {
                    binary_shape_marked(ctx, source, &id, stage, &mut marks)?
                        .map(|(facts, version)| ShapePayload::Binary { facts, version })
                } else {
                    text_shape_marked(ctx, source, &id, stage, &mut marks)?
                        .map(|(facts, version)| ShapePayload::Text { facts, version })
                };
                let Some(payload) = payload else {
                    return Ok(None);
                };
                let payload = ShapePayloadRecord {
                    id: id.clone(),
                    property: property.id.to_owned(),
                    entry: entry.id().to_owned(),
                    rows: Vec::new(),
                    payload,
                };
                let payload = super::records::with_rows(ctx, stage.native(), payload)?;
                stage.insert_native(ctx, "shape_payloads", payload.clone())?;
                Ok(Some(payload))
            })? {
                projection.insert(&payload, source, binary, marks);
                payloads.push(payload);
            }
        }
    }
    Ok(payloads)
}

fn text_version(mut header: View<'_>) -> Result<TextTopologyVersion, CodecError> {
    let line = std::str::from_utf8(header.req_take(header.window().len())?)
        .map_err(|_| CodecError::malformed("text B-rep header is not UTF-8"))?
        .trim();
    match line {
        "CASCADE Topology V1, (c) Matra-Datavision" => Ok(TextTopologyVersion::V1),
        "CASCADE Topology V2, (c) Matra-Datavision" => Ok(TextTopologyVersion::V2),
        "CASCADE Topology V3, (c) Open Cascade" => Ok(TextTopologyVersion::V3),
        _ => Err(CodecError::malformed("unsupported text B-rep header")),
    }
}
fn binary_version(header: View<'_>) -> Result<BinaryTopologyVersion, CodecError> {
    let prefix = b"Open CASCADE Topology V";
    let start = header
        .window()
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map(|offset| header.start() + offset)
        .ok_or_else(|| CodecError::malformed("unsupported binary B-rep header"))?;
    let width = prefix.len().min(header.end() - start);
    let mut literal = window(header, start, start + width)?;
    if literal.req_take(width)? != prefix {
        return Err(CodecError::malformed("unsupported binary B-rep header"));
    }
    let mut digit = window(header, start + width, start + width + 1)?;
    let value = digit
        .req_u8()?
        .checked_sub(b'0')
        .ok_or_else(|| CodecError::malformed("unsupported binary B-rep header"))?;
    BinaryTopologyVersion::try_from(value).map_err(CodecError::Malformed)
}

fn spec(level: &str, framing: Vec<std::ops::Range<u64>>) -> Result<PartSpec, CodecError> {
    Ok(PartSpec::new(
        PartLevel::parse(level)
            .ok_or_else(|| CodecError::malformed("B-rep level is absent from Appendix C"))?,
        framing,
    ))
}

fn header_framing(header: View<'_>, prefix: &[u8]) -> Vec<std::ops::Range<u64>> {
    let mut framing = Vec::new();
    if header.window().last() == Some(&b'\n') {
        framing.push(u64_from_index(header.end() - 1)..u64_from_index(header.end()));
    }
    if let Some(position) = header
        .window()
        .windows(prefix.len())
        .position(|window| window == prefix)
    {
        let digit = header.start() + position + prefix.len();
        if digit < header.end() {
            framing.push(u64_from_index(digit)..u64_from_index(digit + 1));
        }
    }
    framing.sort_by_key(|range| range.start);
    framing
}

fn window(source: View<'_>, start: usize, end: usize) -> Result<View<'_>, CodecError> {
    source
        .child(start, end)
        .ok_or_else(|| CodecError::malformed("B-rep part exceeds its source member"))
}

fn cursor<'a>(
    tokens: &'a [&'a str],
    ranges: &'a [std::ops::Range<usize>],
    source: View<'a>,
) -> TokenCursor<'a> {
    TokenCursor {
        tokens,
        index: 0,
        source: Some(source),
        ranges: Some(ranges),
        discriminator: None,
        census_marks: Vec::new(),
        representation_marks: Vec::new(),
    }
}

fn consumed(source: View<'_>, cursor: &TokenCursor<'_>) -> Result<usize, CodecError> {
    cursor
        .index
        .checked_sub(1)
        .and_then(|index| cursor.ranges.and_then(|ranges| ranges.get(index)))
        .map_or(Ok(0), |range| {
            range
                .end
                .checked_sub(source.start())
                .ok_or_else(|| CodecError::malformed("B-rep consumed range overflows"))
        })
}

fn text_tokens(source: View<'_>) -> (Vec<&str>, Vec<std::ops::Range<usize>>) {
    let mut tokens = Vec::new();
    let mut ranges = Vec::new();
    let bytes = source.window();
    let mut start = 0;
    let mut marker_count = false;
    while start < bytes.len() {
        if bytes[start].is_ascii_whitespace() {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        // The Appendix C terminator cannot occur in numeric TShape content.
        // Its preserved byte remains a boundary when neighboring content is bad.
        if let Some(width) = text_marker_width(&bytes[start..]) {
            end = start + width;
            marker_count = true;
        } else if marker_count && bytes[start].is_ascii_digit() {
            // The published framing is the decimal count itself, not its
            // following separator. A damaged separator cannot consume it.
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            marker_count = false;
        } else if bytes[start] != b'*' {
            marker_count = false;
            while end < bytes.len()
                && !bytes[end].is_ascii_whitespace()
                && bytes[end] != b'*'
                && text_marker_width(&bytes[end..]).is_none()
            {
                end += 1;
            }
        }
        tokens.push(std::str::from_utf8(&bytes[start..end]).unwrap_or("\u{fffd}"));
        ranges.push(source.start() + start..source.start() + end);
        start = end;
    }
    (tokens, ranges)
}

fn text_marker_width(bytes: &[u8]) -> Option<usize> {
    TABLES.iter().find_map(|name| {
        let suffix = bytes.strip_prefix(name.as_bytes())?;
        suffix
            .first()
            .is_none_or(u8::is_ascii_whitespace)
            .then_some(name.len())
    })
}

#[derive(Clone, Default)]
struct Accepted {
    facts: ShapeSet,
    shapes: BTreeMap<usize, usize>,
}

impl Accepted {
    fn indices(&self) -> remap::Indices {
        let mut indices = remap::Indices::default();
        for (table, values) in &self.facts.indices {
            for (offset, source) in values.iter().copied().enumerate() {
                indices.bind(*table, source, offset + 1);
            }
        }
        indices
    }
    fn pcurve_marks(
        &self,
        value: &BrepValue,
        marks: Vec<(usize, std::ops::Range<usize>)>,
    ) -> Vec<(usize, std::ops::Range<usize>)> {
        let BrepValue::TShape(shapes) = value else {
            return marks;
        };
        let Some(TextTShape {
            geometry: TextTShapeGeometry::Edge {
                representations, ..
            },
            ..
        }) = shapes.first()
        else {
            return marks;
        };
        let indices = self.indices();
        let mut retained = 0;
        let mut result = Vec::new();
        for (original, representation) in representations.iter().enumerate() {
            if indices.edge(&mut representation.clone()).is_none() {
                continue;
            }
            if let Some((_, range)) = marks.iter().find(|(index, _)| *index == original) {
                result.push((retained, range.clone()));
            }
            retained += 1;
        }
        result
    }
    fn admit(&mut self, index: usize, value: &BrepValue) -> Result<(), CodecError> {
        match value {
            BrepValue::Location(value) => self.facts.locations.push(value.clone()),
            BrepValue::Curve2d(value) => self.facts.curve2ds.push(value.clone()),
            BrepValue::Curve(value) => self.facts.curves.push(value.clone()),
            BrepValue::Polygon3d(value) => self.facts.polygons3d.push(value.clone()),
            BrepValue::PolygonOnTriangulation(value) => {
                self.facts.polygons_on_triangulations.push(value.clone())
            }
            BrepValue::Surface(value) => self.facts.surfaces.push(value.clone()),
            BrepValue::Triangulation(value) => self.facts.triangulations.push(value.clone()),
            BrepValue::TShape(values) => {
                let resolved = self
                    .indices()
                    .resolve(BrepValue::TShape(values.clone()))
                    .ok_or_else(|| {
                        CodecError::malformed("B-rep topology row has a missing required target")
                    })?;
                let BrepValue::TShape(resolved) = resolved else {
                    return Err(CodecError::malformed(
                        "B-rep topology row changed its value type",
                    ));
                };
                let mut shape = resolved
                    .first()
                    .cloned()
                    .ok_or_else(|| CodecError::malformed("B-rep topology row has no shape"))?;
                shape.children = shape
                    .children
                    .into_iter()
                    .filter_map(|mut child| {
                        child.shape = *self.shapes.get(&child.shape)?;
                        Some(child)
                    })
                    .collect();
                let mut candidate = self.facts.clone();
                candidate.tshapes.0.push(shape.clone());
                candidate.validate().map_err(CodecError::Malformed)?;
                self.facts.tshapes.0.push(shape);
                self.shapes.insert(index, self.facts.tshapes.len());
            }
            BrepValue::Roots(values) => {
                let resolved = self
                    .indices()
                    .resolve(BrepValue::Roots(values.clone()))
                    .ok_or_else(|| {
                        CodecError::malformed("B-rep root has a missing required target")
                    })?;
                let BrepValue::Roots(values) = resolved else {
                    return Err(CodecError::malformed("B-rep root changed its value type"));
                };
                self.facts.roots.extend(values.iter().filter_map(|value| {
                    let shape = *self.shapes.get(&value.shape)?;
                    (value.location.index() <= self.facts.locations.len()).then_some(TextShapeUse {
                        shape,
                        ..value.clone()
                    })
                }));
            }
        }
        self.facts
            .indices
            .entry(value.table_order())
            .or_default()
            .push(index);
        Ok(())
    }
}

#[cfg(test)]
fn text_shape(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    id: &str,
    output: &mut PartOutput,
) -> Result<Option<(ShapeSet, TextTopologyVersion)>, CodecError> {
    text_shape_marked(
        ctx,
        source,
        id,
        output,
        &mut projection::RowMarks::default(),
    )
}

fn text_shape_marked(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    id: &str,
    output: &mut PartOutput,
    marks: &mut projection::RowMarks,
) -> Result<Option<(ShapeSet, TextTopologyVersion)>, CodecError> {
    let (header_start, header_end) = source
        .window()
        .split_inclusive(|byte| *byte == b'\n')
        .scan(source.start(), |position, line| {
            let start = *position;
            *position += line.len();
            Some((start, *position, line))
        })
        .find(|(_, _, line)| !line.iter().all(u8::is_ascii_whitespace))
        .map(|(start, end, _)| (start, end))
        .ok_or_else(|| CodecError::malformed("text B-rep has no topology header"))?;
    let header = window(source, header_start, header_end)?;
    let version = ctx.decode_part(
        output,
        header,
        spec(
            "freecad.brep_text_header",
            header_framing(header, b"CASCADE Topology V"),
        )?,
        |_stage| {
            let (version, owner) = ctx.record_owner(id, source, || text_version(header))?;
            let (census_version, census_owner) =
                ctx.record_owner(&census_id(id), source, || text_version(header))?;
            Ok((version, owner, census_version, census_owner))
        },
    )?;
    let Some((version, owner, census_version, census_owner)) = version else {
        return Ok(None);
    };
    let mut census = census::Delta::default();
    let result = (|| {
        let (tokens, ranges) = text_tokens(source);
        if tokens.iter().filter(|token| **token == "CASCADE").count() != 1 {
            return Err(CodecError::malformed(
                "text B-rep has duplicate topology headers",
            ));
        }
        let mut markers = Vec::new();
        let mut counts = BTreeMap::new();
        let mut prior = 0;
        for name in TABLES {
            let positions = tokens
                .iter()
                .enumerate()
                .filter(|(_, token)| **token == name)
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if positions.len() != 1 || positions[0] < prior {
                return Err(CodecError::malformed(
                    "text B-rep table markers are missing, duplicated or out of order",
                ));
            }
            let position = positions[0];
            prior = position + 1;
            let count = tokens
                .get(position + 1)
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|count| *count <= 1_000_000);
            markers.push((position, count));
        }
        let mut accepted = Accepted::default();
        let mut root_start = None;
        let mut root_offset = None;
        for (table, (position, count)) in markers.iter().copied().enumerate() {
            let start = ranges[position].start;
            let content_start = position + 2;
            let end_token = if table + 1 < TABLES.len() {
                markers[table + 1].0
            } else if let Some(count) = count {
                let mut found = 0;
                let mut end = content_start;
                for (index, token) in tokens.iter().enumerate().skip(content_start) {
                    if found == count {
                        break;
                    }
                    end = index + 1;
                    if *token == "*" {
                        found += 1;
                    }
                }
                if count == 0 {
                    end = content_start;
                }
                if found == count {
                    root_start = Some(end);
                }
                end
            } else {
                tokens.len()
            };
            let end = if table == 7 && root_start.is_some() {
                let end = end_token
                    .checked_sub(1)
                    .and_then(|index| ranges.get(index))
                    .map_or(source.end(), |range| range.end);
                root_offset = Some(end);
                end
            } else {
                ranges
                    .get(end_token)
                    .map_or(source.end(), |range| range.start)
            };
            let extent = window(source, start, end)?;
            let marker_end = ranges
                .get(position + 1)
                .map_or(ranges[position].end, |range| range.end);
            let framing_end = count.map_or(ranges[position].end, |_| marker_end);
            let mut table_accepted = accepted.clone();
            let mut table_marks = projection::RowMarks::default();
            let mut table_census = census.clone();
            let result = ctx.decode_part(
                output,
                extent,
                spec(
                    "freecad.brep_text_table",
                    vec![u64_from_index(start)..u64_from_index(framing_end)],
                )?,
                |stage| {
                    let marker_extent = window(source, start, marker_end)?;
                    let count = ctx.record_fragment(&owner, marker_extent, || {
                        let marker_range = ranges.get(position..position + 2).ok_or_else(|| {
                            CodecError::malformed("text B-rep table has no count")
                        })?;
                        let marker_tokens =
                            tokens.get(position..position + 2).ok_or_else(|| {
                                CodecError::malformed("text B-rep table has no count")
                            })?;
                        let mut marker = cursor(marker_tokens, marker_range, source);
                        if marker.next("B-rep table selector")? != TABLES[table] {
                            return Err(CodecError::malformed("invalid text B-rep table selector"));
                        }
                        marker.count("B-rep table count", 1_000_000)
                    })?;
                    counts.insert(TABLES[table].to_owned(), count);
                    table_accepted.facts.admit_table(table);
                    table_marks.counts.insert(
                        u8::try_from(table)
                            .map_err(|_| CodecError::malformed("B-rep table index exceeds u8"))?,
                        count,
                    );
                    table_marks.markers.insert(
                        u8::try_from(table)
                            .map_err(|_| CodecError::malformed("B-rep table index exceeds u8"))?,
                        start..marker_end,
                    );
                    let body_tokens = tokens
                        .get(content_start..end_token)
                        .ok_or_else(|| CodecError::malformed("invalid text B-rep table bounds"))?;
                    let body_ranges = &ranges[content_start..end_token];
                    let mut token_index = 0;
                    let mut stopped = false;
                    for index in 1..=count {
                        let row_start = body_ranges
                            .get(token_index)
                            .map_or(end, |range| range.start);
                        let tail = window(source, row_start, end)?;
                        let row_id = records::row_id(id, TABLES[table], index);
                        if table == 7 {
                            let terminator = body_tokens[token_index..]
                                .iter()
                                .position(|token| *token == "*");
                            let row_token_end = terminator
                                .map(|offset| token_index + offset + 1)
                                .unwrap_or(body_tokens.len());
                            let row_end = row_token_end
                                .checked_sub(1)
                                .and_then(|index| body_ranges.get(index))
                                .map_or(end, |range| range.end);
                            let row_extent = window(source, row_start, row_end)?;
                            let result = ctx.decode_part(
                                stage,
                                row_extent,
                                spec(
                                    TEXT_LEVELS[table],
                                    terminator
                                        .map(|_| {
                                            vec![
                                                u64_from_index(row_end - 1)
                                                    ..u64_from_index(row_end),
                                            ]
                                        })
                                        .unwrap_or_default(),
                                )?,
                                |_row_stage| {
                                    ctx.record_fragment(&owner, row_extent, || {
                                        let mut cursor = cursor(
                                            &body_tokens[token_index..row_token_end],
                                            &body_ranges[token_index..row_token_end],
                                            source,
                                        );
                                        let value = BrepValue::TShape(
                                            vec![source_rows::text_tshape(
                                                &mut cursor,
                                                index,
                                                count,
                                                &counts,
                                                version.number(),
                                                &table_accepted.shapes,
                                            )?]
                                            .into(),
                                        );
                                        let representations = table_accepted.pcurve_marks(
                                            &value,
                                            cursor.representation_marks.clone(),
                                        );
                                        table_accepted.admit(index, &value)?;
                                        let delta = census::capture(
                                            ctx,
                                            &census_owner,
                                            row_extent,
                                            &cursor.census_marks,
                                            false,
                                        )?;
                                        Ok((delta, representations))
                                    })
                                },
                            )?;
                            let failed = result.is_none();
                            if let Some((delta, representations)) = result {
                                table_census.apply(delta);
                                table_marks.rows.insert((7, index), row_start..row_end);
                                for (representation, range) in representations {
                                    table_marks.pcurves.insert((index, representation), range);
                                }
                            }
                            token_index = row_token_end;
                            if failed && terminator.is_none() {
                                stopped = true;
                                break;
                            }
                        } else {
                            let result = ctx.decode_sequential_part(
                                stage,
                                tail,
                                spec(TEXT_LEVELS[table], Vec::new())?,
                                |row_stage| {
                                    let ((row, used_tokens, delta), used) = ctx
                                        .sequential_record_fragment(&owner, tail, || {
                                            let mut cursor = cursor(
                                                &body_tokens[token_index..],
                                                &body_ranges[token_index..],
                                                source,
                                            );
                                            let value = if table == 3 || table == 4 {
                                                let (value, _, polygon_owner) = ctx.sequential_record_owner(&row_id, tail, || {
                                                    let value = text_value(
                                                        &mut cursor, table, index, &table_accepted.facts, version.number(),
                                                    )?;
                                                    Ok((value, consumed(tail, &cursor)?))
                                                })?;
                                                let admitted = ctx.record_fragment(&polygon_owner, header, || text_version(header))?;
                                                if admitted != version {
                                                    return Err(CodecError::malformed("polygon source header dispatch differs from its admitted text version"));
                                                }
                                                value
                                            } else {
                                                text_value(
                                                    &mut cursor,
                                                    table,
                                                    index,
                                                    &table_accepted.facts,
                                                    version.number(),
                                                )?
                                            };
                                            let used = consumed(tail, &cursor)?;
                                            let extent =
                                                window(source, row_start, row_start + used)?;
                                            let delta = census::capture(
                                                ctx,
                                                &census_owner,
                                                extent,
                                                &cursor.census_marks,
                                                false,
                                            )?;
                                            Ok((
                                                (
                                                    BrepRow::new(id, TABLES[table], index, value),
                                                    cursor.index,
                                                    delta,
                                                ),
                                                used,
                                            ))
                                        })?;
                                    insert_polygon(ctx, row_stage, row.clone())?;
                                    Ok(((row, used_tokens, delta), used))
                                },
                            )?;
                            let Some((row, used_tokens, delta)) = result else {
                                stopped = true;
                                break;
                            };
                            table_accepted.admit(index, &row.value)?;
                            table_census.apply(delta);
                            let last = body_ranges.get(token_index + used_tokens - 1).ok_or_else(
                                || CodecError::malformed("accepted source row end is absent"),
                            )?;
                            table_marks
                                .rows
                                .insert((table as u8, index), row_start..last.end);
                            token_index += used_tokens;
                        }
                    }
                    if !stopped && token_index != body_tokens.len() {
                        return Err(CodecError::malformed(
                            "text B-rep table contains trailing tokens",
                        ));
                    }
                    Ok(())
                },
            )?;
            if result.is_some() {
                accepted = table_accepted;
                census = table_census;
                marks.counts.extend(table_marks.counts);
                marks.markers.extend(table_marks.markers);
                marks.rows.extend(table_marks.rows);
                marks.pcurves.extend(table_marks.pcurves);
            } else {
                counts.remove(TABLES[table]);
            }
        }
        if root_start == Some(tokens.len()) {
            accepted.facts.roots.admit_empty();
        }
        if let Some(position) = root_start.filter(|position| *position < tokens.len()) {
            let start = root_offset.unwrap_or(ranges[position].start);
            let extent = window(source, start, source.end())?;
            let framing = tokens[position..]
                .iter()
                .zip(&ranges[position..])
                .filter(|(token, _)| **token == "*")
                .map(|(_, range)| u64_from_index(range.start)..u64_from_index(range.end))
                .collect();
            let root = ctx.decode_part(
                output,
                extent,
                spec("freecad.brep_text_root", framing)?,
                |_stage| {
                    ctx.record_fragment(&owner, extent, || {
                        let mut cursor = cursor(&tokens[position..], &ranges[position..], source);
                        let mut roots = Vec::new();
                        while !cursor.is_empty() {
                            if cursor.peek()? == Some("*") {
                                cursor.next("root terminator")?;
                                break;
                            }
                            let shape_count = counts.get("TShapes").copied().ok_or_else(|| {
                                CodecError::malformed("root use has no readable topology count")
                            })?;
                            let root = parse_shape_use(&mut cursor, shape_count, &counts)?;
                            if accepted.shapes.contains_key(&root.shape)
                                && root.location.index() <= accepted.facts.locations.len()
                            {
                                roots.push(root);
                            }
                        }
                        if !cursor.is_empty() {
                            return Err(CodecError::malformed("tokens follow root terminator"));
                        }
                        let value = BrepValue::Roots(roots);
                        accepted.admit(1, &value)?;

                        Ok(())
                    })
                },
            )?;
            if root.is_some() {
                marks.root = Some(start..source.end());
            }
        }
        accepted.facts.declared_counts = counts;
        Ok(Some((accepted.facts, version)))
    })();
    if result.is_ok() {
        output.insert_native(
            ctx,
            "carrier_census",
            census.finish(id, false, census_version.number()),
        )?;
    }
    result
}

fn text_value(
    cursor: &mut TokenCursor<'_>,
    table: usize,
    index: usize,
    facts: &ShapeSet,
    version: u8,
) -> Result<BrepValue, CodecError> {
    Ok(match table {
        0 => BrepValue::Location(source_rows::text_location(
            cursor,
            index - 1,
            &facts.locations,
        )?),
        1 => BrepValue::Curve2d(parse_curve2d(cursor, 0, index)?),
        2 => BrepValue::Curve(parse_curve(cursor, 0, index)?),
        3 => BrepValue::Polygon3d(source_rows::text_polygon3d(cursor)?),
        4 => BrepValue::PolygonOnTriangulation(source_rows::text_indexed_polygon(cursor)?),
        5 => BrepValue::Surface(parse_surface(cursor, 0, index)?),
        6 => BrepValue::Triangulation(source_rows::text_triangulation(cursor, version)?),
        _ => return Err(CodecError::malformed("invalid text B-rep table")),
    })
}

#[cfg(test)]
fn binary_shape(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    id: &str,
    output: &mut PartOutput,
) -> Result<Option<(ShapeSet, BinaryTopologyVersion)>, CodecError> {
    binary_shape_marked(
        ctx,
        source,
        id,
        output,
        &mut projection::RowMarks::default(),
    )
}

fn binary_shape_marked(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    id: &str,
    output: &mut PartOutput,
    marks: &mut projection::RowMarks,
) -> Result<Option<(ShapeSet, BinaryTopologyVersion)>, CodecError> {
    let mut cursor = BinaryCursor {
        view: source,
        census_marks: Vec::new(),
        representation_marks: Vec::new(),
    };
    let mut end = source.start();
    for line in source.window().split_inclusive(|byte| *byte == b'\n') {
        end += line.len();
        if !line.iter().all(u8::is_ascii_whitespace) {
            break;
        }
    }
    let header = window(source, source.start(), end)?;
    cursor.view = window(source, end, source.end())?;
    let version = ctx.decode_part(
        output,
        header,
        spec(
            "freecad.brep_binary_header",
            header_framing(header, b"Open CASCADE Topology V"),
        )?,
        |_stage| {
            let (version, owner) = ctx.record_owner(id, source, || binary_version(header))?;
            let (census_version, census_owner) =
                ctx.record_owner(&census_id(id), source, || binary_version(header))?;
            Ok((version, owner, census_version, census_owner))
        },
    )?;
    let Some((version, owner, census_version, census_owner)) = version else {
        return Ok(None);
    };
    let mut census = census::Delta::default();
    let result = (|| {
        let mut accepted = Accepted::default();
        for (table, name) in TABLES.into_iter().enumerate() {
            // Complete empty separator lines only locate the next marker.
            // Their bytes remain B1, rather than becoming semantic reads.
            let mut marker_start = cursor.view.position();
            for line in source.window()[marker_start - source.start()..]
                .split_inclusive(|byte| *byte == b'\n')
            {
                if !line.iter().all(u8::is_ascii_whitespace) {
                    break;
                }
                marker_start = marker_start
                    .checked_add(line.len())
                    .ok_or_else(|| CodecError::malformed("binary marker offset overflows"))?;
            }
            cursor.view = window(source, marker_start, source.end())?;
            let selector_start = cursor.view.position();
            let tail = window(source, selector_start, source.end())?;
            // #224 comment6376511913 / republished Appendix C row 92: every
            // actual marker is a sequential record, including an N=0 marker.
            let count = ctx.decode_sequential_part(
                output,
                tail,
                spec("freecad.brep_binary_record", Vec::new())?,
                |_stage| {
                    let (count, used) = ctx.sequential_record_fragment(&owner, tail, || {
                        let count = cursor.section_count(name)?;
                        Ok((count, cursor.view.position() - selector_start))
                    })?;
                    Ok((count, used))
                },
            )?;
            let Some(count) = count else {
                return Ok(Some((accepted.facts, version)));
            };
            accepted.facts.admit_table(table);
            marks.counts.insert(
                u8::try_from(table)
                    .map_err(|_| CodecError::malformed("B-rep table index exceeds u8"))?,
                count,
            );
            marks.markers.insert(
                u8::try_from(table)
                    .map_err(|_| CodecError::malformed("B-rep table index exceeds u8"))?,
                selector_start..cursor.view.position(),
            );
            for index in 1..=count {
                let start = cursor.view.position();
                let tail = window(source, start, source.end())?;
                let row_id = records::row_id(id, name, index);
                cursor.census_marks.clear();
                cursor.representation_marks.clear();
                let row = ctx.decode_sequential_part(
                    output,
                    tail,
                    spec("freecad.brep_binary_record", Vec::new())?,
                    |stage| {
                        let ((row, delta), used) =
                            ctx.sequential_record_fragment(&owner, tail, || {
                                let value = if table == 3 || table == 4 {
                                    let (value, _, polygon_owner) = ctx.sequential_record_owner(&row_id, tail, || {
                                        let value = binary_value(
                                            &mut cursor, table, index, count, &accepted.facts, version.number(),
                                        )?;
                                        Ok((value, cursor.view.position() - start))
                                    })?;
                                    let admitted = ctx.record_fragment(&polygon_owner, header, || binary_version(header))?;
                                    if admitted != version {
                                        return Err(CodecError::malformed("polygon source header dispatch differs from its admitted binary version"));
                                    }
                                    value
                                } else {
                                    binary_value(
                                        &mut cursor,
                                        table,
                                        index,
                                        count,
                                        &accepted.facts,
                                        version.number(),
                                    )?
                                };
                                let row = BrepRow::new(id, name, index, value);
                                let mut candidate = Accepted {
                                    facts: accepted.facts.clone(),
                                    shapes: accepted.shapes.clone(),
                                };
                                candidate.admit(index, &row.value)?;
                                let used = cursor.view.position() - start;
                                let extent = window(source, start, start + used)?;
                                let delta = census::capture(
                                    ctx,
                                    &census_owner,
                                    extent,
                                    &cursor.census_marks,
                                    true,
                                )?;
                                Ok(((row, delta), used))
                            })?;
                        insert_polygon(ctx, stage, row.clone())?;
                        Ok(((row, delta), used))
                    },
                )?;
                let Some((row, delta)) = row else {
                    return Ok(Some((accepted.facts, version)));
                };
                accepted.admit(index, &row.value)?;
                census.apply(delta);
                marks
                    .rows
                    .insert((table as u8, index), start..cursor.view.position());
                for (representation, range) in cursor.representation_marks.drain(..) {
                    marks.pcurves.insert((index, representation), range);
                }
            }
        }
        if cursor.remaining() > 12 {
            return Err(CodecError::malformed(
                "binary B-rep root cannot both follow the last TShape and end the member in twelve bytes",
            ));
        }
        if cursor.remaining() == 0 {
            accepted.facts.roots.admit_empty();
        }
        if cursor.remaining() != 0 {
            let extent = window(source, cursor.view.position(), source.end())?;
            let root = ctx.decode_part(
                output,
                extent,
                spec("freecad.brep_binary_root", Vec::new())?,
                |_stage| {
                    ctx.record_fragment(&owner, extent, || {
                        if cursor.remaining() != 12 {
                            return Err(CodecError::malformed(
                                "binary B-rep root record has invalid length",
                            ));
                        }
                        let shape = cursor.i32("binary root shape")?;
                        let location = cursor.i32("binary root location")?;
                        let orientation = cursor.i32("binary root orientation")?;
                        let roots = if (shape, location, orientation) == (-1, -1, -1) {
                            Vec::new()
                        } else {
                            vec![TextShapeUse {
                                shape: checked_binary_reference(
                                    shape,
                                    accepted.facts.tshapes.len(),
                                    false,
                                    "root shape",
                                )?,
                                location: checked_binary_reference(
                                    location,
                                    accepted.facts.locations.len(),
                                    true,
                                    "root location",
                                )?
                                .into(),
                                orientation: binary_orientation(orientation)?,
                            }]
                        };
                        let value = BrepValue::Roots(roots);
                        accepted.admit(1, &value)?;

                        Ok(())
                    })
                },
            )?;
            if root.is_some() {
                marks.root = Some(extent.start()..extent.end());
            }
        }
        Ok(Some((accepted.facts, version)))
    })();
    if result.is_ok() {
        output.insert_native(
            ctx,
            "carrier_census",
            census.finish(id, true, census_version.number()),
        )?;
    }
    result
}

fn binary_value(
    cursor: &mut BinaryCursor<'_>,
    table: usize,
    index: usize,
    count: usize,
    facts: &ShapeSet,
    version: u8,
) -> Result<BrepValue, CodecError> {
    Ok(match table {
        0 => BrepValue::Location(source_rows::binary_location(
            cursor,
            index - 1,
            &facts.locations,
        )?),
        1 => BrepValue::Curve2d(parse_binary_curve2d(cursor, 0)?),
        2 => BrepValue::Curve(parse_binary_curve(cursor, 0)?),
        3 => BrepValue::Polygon3d(source_rows::binary_polygon3d(cursor)?),
        4 => BrepValue::PolygonOnTriangulation(source_rows::binary_indexed_polygon(cursor)?),
        5 => BrepValue::Surface(parse_binary_surface(cursor, 0)?),
        6 => BrepValue::Triangulation(source_rows::binary_triangulation(cursor, version)?),
        7 => BrepValue::TShape(
            vec![parse_binary_tshape(
                cursor,
                version,
                index,
                count,
                BinaryGeometryCounts {
                    curves: facts.curves.len(),
                    curves2d: facts.curve2ds.len(),
                    surfaces: facts.surfaces.len(),
                    locations: facts.locations.len(),
                    polygons3d: facts.polygons3d.len(),
                    indexed_polygons: facts.polygons_on_triangulations.len(),
                    triangulations: facts.triangulations.len(),
                },
            )?]
            .into(),
        ),
        _ => return Err(CodecError::malformed("invalid binary B-rep table")),
    })
}

fn census_id(owner: &str) -> String {
    native::native_child_id("carrier-census", owner, "families")
}

fn insert_polygon(
    ctx: &DecodeContext<'_>,
    stage: &mut PartOutput,
    row: BrepRow,
) -> Result<(), CodecError> {
    match &row.value {
        BrepValue::Polygon3d(value) => stage.insert_native(
            ctx,
            "polygons3d",
            records::polygons::PolygonRecord {
                id: row.id,
                value: value.clone(),
            },
        ),
        BrepValue::PolygonOnTriangulation(value) => stage.insert_native(
            ctx,
            "polygons_on_triangulations",
            records::polygons::PolygonRecord {
                id: row.id,
                value: value.clone(),
            },
        ),
        _ => Ok(()),
    }
}

#[cfg(test)]
fn parse_fixture<T>(
    bytes: &[u8],
    parse: impl FnOnce(&DecodeContext<'_>, View<'_>, &mut PartOutput) -> Result<Option<T>, CodecError>,
) -> Result<T, CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, source) = DecodeContext::from_root_bytes(bytes, &arena, &policy)?;
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let result = parse(&ctx, source, &mut output)?;
    if let Some(loss) = output.losses().first() {
        return Err(CodecError::malformed(&loss.message));
    }
    result.ok_or_else(|| CodecError::malformed("B-rep header was unreadable"))
}

#[cfg(test)]
pub(crate) fn parse_text(bytes: &[u8]) -> Result<(ShapeSet, TextTopologyVersion), CodecError> {
    parse_fixture(bytes, |ctx, source, output| {
        text_shape(ctx, source, "fcstd:native:shape-payload#fixture", output)
    })
}

#[cfg(test)]
pub(super) fn parse_binary_prefix(
    bytes: &[u8],
) -> Result<(ShapeSet, BinaryTopologyVersion), CodecError> {
    parse_fixture(bytes, |ctx, source, output| {
        binary_shape(ctx, source, "fcstd:native:shape-payload#fixture", output)
    })
}

#[cfg(test)]
pub(crate) fn parse_payloads(
    original: &crate::property_reader::PropertyReader<'_, '_>,
    properties: &[PropertyRecord],
    entries: &[EntryRecord],
    sources: &crate::entry_sources::EntrySources<'_>,
) -> Result<Vec<ShapePayloadRecord>, CodecError> {
    let mut xml = String::from("<Properties>");
    for property in properties {
        let fragment = original.property_xml(property)?;
        let tree = original.parse_xml(&fragment, "B-rep property fixture")?;
        let element = tree.root_element();
        xml.push('<');
        xml.push_str(element.tag_name().name());
        for (name, value) in [
            ("name", property.name.as_str()),
            ("type", property.type_name.as_str()),
        ] {
            xml.push_str(&format!(
                " {name}=\"{}\"",
                crate::writer::xml::escaped(value)
            ));
        }
        for attribute in element
            .attributes()
            .filter(|attribute| !matches!(attribute.name(), "name" | "type"))
        {
            xml.push_str(&format!(
                " {}=\"{}\"",
                attribute.name(),
                crate::writer::xml::escaped(attribute.value())
            ));
        }
        xml.push('>');
        for child in element.children().filter(roxmltree::Node::is_element) {
            xml.push_str(&fragment[child.range()]);
        }
        xml.push_str("</");
        xml.push_str(element.tag_name().name());
        xml.push('>');
    }
    xml.push_str("</Properties>");
    let xml_len = xml.len();
    let mut bytes = xml.into_bytes();
    let mut locations = Vec::new();
    for entry in entries {
        let start = bytes.len();
        bytes.extend_from_slice(sources.view(entry)?.window());
        locations.push((entry.name().to_owned(), start, bytes.len()));
    }
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(
        &bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )?;
    let source = crate::xml_framing::XmlSource::new(
        &ctx,
        root.child(0, xml_len)
            .ok_or_else(|| CodecError::malformed("property fixture exceeds source"))?,
    )?;
    let mut entry_views = BTreeMap::new();
    for (name, start, end) in locations {
        entry_views.insert(
            name.clone(),
            ctx.register_named_slice(
                root,
                cadmpeg_core::decode::ByteRange {
                    start: u64_from_index(start),
                    end: u64_from_index(end),
                },
                &name,
            )?,
        );
    }
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Fcstd);
    let mut parsed = Vec::new();
    for (index, original) in source.children(0).zip(properties) {
        let property = ctx.record(&original.id, source.extent(index)?, || {
            let mut values = Vec::new();
            for child in source.children(index) {
                fixture_values(&ctx, &source, child, None, &mut values)?;
            }
            if original.type_name == "Part::PropertyPartShape"
                && values
                    .iter()
                    .filter(|value| value.parent.is_none() && value.tag == "Part")
                    .count()
                    > 1
            {
                return Err(CodecError::malformed(
                    "shape property has multiple direct Part carriers",
                ));
            }
            let mut property = original.clone();
            if source.name(index)? != "_Property" {
                property.body = Some(crate::native::PropertyBody::Persisted {
                    values: values.into(),
                    links: Vec::new(),
                    side_entries: Vec::new(),
                });
            }
            Ok(property)
        })?;
        parsed.push(property);
    }
    let reader = crate::property_reader::PropertyReader::typed(&ctx);
    let sources = crate::entry_sources::EntrySources::new(&ctx, &entry_views)?;
    let result = decode_payloads(
        &reader,
        &parsed,
        entries,
        &sources,
        &mut output,
        &mut projection::ProjectionSources::default(),
    )?;
    if let Some(loss) = output.losses().first() {
        return Err(CodecError::malformed(&loss.message));
    }
    Ok(result)
}

#[cfg(test)]
fn fixture_values(
    ctx: &DecodeContext<'_>,
    source: &crate::xml_framing::XmlSource<'_>,
    index: usize,
    parent: Option<usize>,
    values: &mut Vec<crate::native::ValueRecord>,
) -> Result<(), CodecError> {
    let mut name = source
        .elements()
        .get(index)
        .ok_or_else(|| CodecError::malformed("fixture value lacks a source node"))?
        .name();
    let width = name.window().len();
    let tag = std::str::from_utf8(name.req_take(width)?)
        .map_err(|_| CodecError::malformed("fixture value tag is not UTF-8"))?
        .rsplit(':')
        .next()
        .unwrap_or("")
        .to_owned();
    let attributes = crate::persistence::source::value_attributes(ctx, source, index, false)?;
    let text = source.text(ctx, index)?;
    let order = values.len();
    values.push(crate::native::ValueRecord {
        item_index: None,
        link_index: None,
        tag,
        order,
        parent,
        attributes,
        decoded_json: None,
        text: (!text.is_empty()).then_some(text),
    });
    for child in source.children(index) {
        fixture_values(ctx, source, child, Some(order), values)?;
    }
    Ok(())
}

pub(crate) mod remap;
#[cfg(test)]
mod tests;
