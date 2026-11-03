// SPDX-License-Identifier: Apache-2.0
//! Current-entry dimensional tables for simple drilled-hole recipes.

use std::collections::BTreeMap;
use std::ops::Range;

use crate::container::ContainerScan;
use crate::feature::definitions::FeatureDimensionTable;
use crate::feature::entity::FeatureEntityTable;
use crate::feature::schema::SchemaClass;
use crate::psb;
use cadmpeg_core::decode::View;

use super::super::super::feature_history::dimensions::feature_dimension_table_complete;
use super::super::super::uniqueness::exactly_one;
use super::SimpleDrilledDimensionFamily;

const ARRAY: &[u8] = b"\xe0\x00feat_defs_911\0";
const NEXT_ARRAY: &[u8] = b"\xe0\x00feat_defs_";
const ENTRY_END: &[u8] = b"\xe3\x00\x00\xe2\x00\x18\xe5\xe2\x18\x18\x18";

fn first_entry(data: &[u8], at: usize) -> Option<(usize, u32, usize)> {
    (data.get(at..at + 3) == Some(&[0xf4, 0x06, 0xf8])).then_some(())?;
    let (_, after_count) = psb::complete_compact_int(data, at + 3)?;
    (data.get(after_count) == Some(&0xf7)).then_some(())?;
    let (_, after_class) = psb::reference_id(data, after_count + 1).ok()?;
    (data.get(after_class..after_class + 4) == Some(&[0xfb, 0xe3, 0xf7, 0x03])).then_some(())?;
    let key_at = after_class + 4;
    let (key, end) = psb::complete_compact_int(data, key_at)?;
    (data.get(end..end + 2) == Some(&[0xf6, 0x02])).then_some((key_at, key, end + 2))
}

fn entry_range(data: &[u8], key: u32, entry_limit: usize) -> Option<Range<usize>> {
    let array = exactly_one(
        data.windows(ARRAY.len())
            .enumerate()
            .filter_map(|(at, bytes)| (bytes == ARRAY).then_some(at)),
    )?;
    let after_name = array + ARRAY.len();
    let end = data[after_name..]
        .windows(NEXT_ARRAY.len())
        .position(|bytes| bytes == NEXT_ARRAY)
        .map_or(data.len(), |relative| after_name + relative);
    let data = &data[..end];
    let (first_at, first_key, first_end) = first_entry(data, after_name)?;
    let mut entries = vec![(first_at, first_key, first_end, true)];
    for at in first_end..data.len() {
        if data.get(at) != Some(&0xe3) {
            continue;
        }
        let Some((candidate, after)) = psb::complete_compact_int(data, at + 1) else {
            continue;
        };
        if data.get(after..after + 2) != Some(&[0xf6, 0x02]) {
            continue;
        }
        if entries.len() >= entry_limit {
            return None;
        }
        let preceding = &data[..at];
        let authenticated = preceding.ends_with(ENTRY_END)
            || preceding
                .strip_suffix(&[0xf4, 0x05])
                .is_some_and(|bytes| bytes.ends_with(ENTRY_END));
        entries.push((at, candidate, after + 2, authenticated));
    }
    let index = exactly_one(
        entries
            .iter()
            .enumerate()
            .filter_map(|(index, (_, candidate, _, _))| (*candidate == key).then_some(index)),
    )?;
    entries[index].3.then_some(())?;
    let end = entries.get(index + 1).map_or(data.len(), |entry| entry.0);
    let start = entries[index].2;
    let footer = exactly_one(
        data[start..end]
            .windows(ENTRY_END.len())
            .enumerate()
            .filter_map(|(offset, bytes)| (bytes == ENTRY_END).then_some(start + offset)),
    )?;
    Some(start..footer + ENTRY_END.len())
}

fn dimension_core_is_bounded(
    data: &[u8],
    range: &Range<usize>,
    row: &crate::feature::definitions::FeatureDimension,
) -> bool {
    let Some(end) = row
        .offset
        .checked_add(3)
        .and_then(|end| end.checked_add(row.value_body.len()))
        .and_then(|end| end.checked_add(row.auxiliary_body.len()))
        .filter(|end| row.offset >= range.start && *end <= range.end)
    else {
        return false;
    };
    let Some(mut view) = View::over_retained(data).child(row.offset, end) else {
        return false;
    };
    view.u8().map(u32::from) == Some(row.dimension_type)
        && view.take(row.value_body.len()) == Some(row.value_body.as_slice())
        && view.u8() == Some(row.direction_byte)
        && view.take(row.auxiliary_body.len()) == Some(row.auxiliary_body.as_slice())
        && view.u8().map(u32::from) == Some(row.external_id)
}

pub(super) fn current_dimension_table<'a>(
    scan: &'a ContainerScan<'_>,
    recipe: &FeatureEntityTable,
    family: SimpleDrilledDimensionFamily,
) -> Option<&'a FeatureDimensionTable> {
    let all_features = exactly_one(
        scan.framing
            .sections
            .iter()
            .filter(|section| section.name() == "AllFeatur"),
    )?;
    let definitions = exactly_one(
        scan.framing
            .sections
            .iter()
            .filter(|section| section.name() == "FeatDefs"),
    )?;
    let row = exactly_one(scan.features.rows.iter().filter(|row| {
        row.feature_id == recipe.feature_id
            && row.root_schema_class == Some(SchemaClass::Hole)
            && row.stream_offset == all_features.offset()
    }))?;
    (all_features.contains(recipe.offset)
        && recipe.offset >= row.body_offset
        && recipe.offset < row.body_offset.checked_add(row.body.len())?)
    .then_some(())?;
    let source = scan.framing.data.as_ref();
    let section = source.get(definitions.offset()..definitions.end())?;
    let local = entry_range(section, recipe.feature_id, scan.features.rows.len())?;
    let range = definitions.offset() + local.start..definitions.offset() + local.end;
    let depth = family.depth_external_id();
    let mut tables = BTreeMap::new();
    for definition in &scan.features.definitions {
        if definition
            .identity
            .schema_id()
            .map(std::num::NonZeroU32::get)
            != Some(911)
            || !range.contains(&definition.offset)
            || source.get(definition.offset..definition.offset + 4) != Some(b"\xe3S2D")
        {
            continue;
        }
        let Some(table) = definition.dimensions.as_ref() else {
            continue;
        };
        if !feature_dimension_table_complete(table)
            || table.rows.len() != 3
            || !range.contains(&table.offset)
            || ![(0, 2), (1, 10), (depth, 2)].into_iter().all(|(id, kind)| {
                table
                    .rows
                    .iter()
                    .filter(|row| row.external_id == id && row.dimension_type == kind)
                    .count()
                    == 1
            })
            || !table
                .rows
                .iter()
                .all(|row| dimension_core_is_bounded(source, &range, row))
        {
            continue;
        }
        match tables.entry(table.offset) {
            std::collections::btree_map::Entry::Occupied(entry) => {
                (*entry.get() == table).then_some(())?;
            }
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(table);
            }
        }
    }
    exactly_one(tables.into_values())
}

#[cfg(test)]
mod tests;
