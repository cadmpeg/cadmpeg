// SPDX-License-Identifier: Apache-2.0
//! Legacy output retains explicit topology labels when source rows are absent.

use super::{xml, BTreeMap, CodecError, FreecadLossCode, LossNote};
use crate::native::element_map::LegacyMapRecord;

pub(super) fn carrier(
    id: &str,
    rows: &[Option<LegacyMapRecord>],
    source_rows: &[Option<LegacyMapRecord>],
    entries: &mut BTreeMap<String, Vec<u8>>,
    losses: &mut Vec<LossNote>,
) -> Result<Option<String>, CodecError> {
    let absent = source_rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| row.is_none().then_some(format!("legacy_records[{index}]")))
        .collect::<Vec<_>>();
    if !absent.is_empty() {
        losses.push(cadmpeg_ir::codec::write::ExportLossCode::SourceUndecodedOmitted.note(format!(
            "element map {id} omits undecoded source positions {absent:?}; explicit topology labels are retained"
        )));
    }
    let admitted = rows.iter().flatten().collect::<Vec<_>>();
    if admitted
        .iter()
        .any(|row| row.mapped_name.is_empty() || row.mapped_name.chars().any(char::is_whitespace))
    {
        if admitted.iter().any(|row| row.string_ids.contains(&0)) {
            losses.push(FreecadLossCode::ExportNativePropertyNotTransferred.note(format!(
                "element map {id} combines whitespace names with zero string selectors, which no legacy carrier can express"
            )));
            return Ok(None);
        }
        let mut body = format!("<ElementMap count=\"{}\">", admitted.len());
        for row in admitted {
            body.push_str(&xml::element(
                "Element",
                &[
                    ("key", row.mapped_name.clone()),
                    ("value", row.indexed_name.clone()),
                    (
                        "sid",
                        row.string_ids
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(","),
                    ),
                ],
            ));
        }
        body.push_str("</ElementMap>");
        return Ok(Some(body));
    }
    let mut index = 0_usize;
    let file = loop {
        let candidate = format!("Cadmpeg.LegacyMap.{index}.txt");
        if !entries.contains_key(&candidate) {
            break candidate;
        }
        index = index.checked_add(1).ok_or_else(|| {
            CodecError::InvalidInput("legacy map output filenames exceed address space".into())
        })?;
    };
    let mut content = format!("{}\n", admitted.len());
    for row in admitted {
        content.push_str(&format!(
            "{} {} {}",
            row.indexed_name,
            row.mapped_name,
            row.string_ids.len()
        ));
        for selector in &row.string_ids {
            content.push(' ');
            content.push_str(&selector.to_string());
        }
        content.push('\n');
    }
    entries.insert(file.clone(), content.into_bytes());
    Ok(Some(xml::element("ElementMap", &[("file", file)])))
}

pub(super) fn part_attributes(
    shape: &mut String,
    version: &str,
    hasher: Option<usize>,
) -> Result<bool, CodecError> {
    let document = roxmltree::Document::parse(shape).map_err(CodecError::malformed)?;
    let mut parts = document
        .root_element()
        .children()
        .filter(|node| node.has_tag_name("Part"));
    let Some(part) = parts.next() else {
        return Ok(false);
    };
    if parts.next().is_some() {
        return Ok(false);
    }
    let mut removed = part
        .attributes()
        .filter(|attribute| {
            attribute.namespace().is_none()
                && matches!(attribute.name(), "ElementMap" | "HasherIndex")
        })
        .map(|attribute| attribute.range())
        .collect::<Vec<_>>();
    let range = part.range();
    let opening_end = shape[range.clone()]
        .find('>')
        .ok_or_else(|| CodecError::malformed("generated Part has no opening terminator"))?;
    let end = range
        .start
        .checked_add(opening_end)
        .and_then(|end| end.checked_add(1))
        .ok_or_else(|| CodecError::malformed("generated Part range overflows"))?;
    let mut opening = shape[range.start..end].to_owned();
    removed.sort_by_key(|range| std::cmp::Reverse(range.start));
    for removed in removed {
        opening.replace_range(removed.start - range.start..removed.end - range.start, "");
    }
    let insert_at = opening.len() - if opening.ends_with("/>") { 2 } else { 1 };
    let mut attributes = format!(" ElementMap=\"{}\"", xml::escaped(version));
    if let Some(hasher) = hasher {
        attributes.push_str(&format!(" HasherIndex=\"{hasher}\""));
    }
    opening.insert_str(insert_at, &attributes);
    shape.replace_range(range.start..end, &opening);
    Ok(true)
}
