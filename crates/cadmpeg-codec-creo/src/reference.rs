// SPDX-License-Identifier: Apache-2.0
//! Model-space reference entities from `MdlRefInfo`.

use crate::scalar::{self, ScalarCache};

/// One finite model-space line entity.
#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceLine {
    /// First endpoint in model coordinates.
    pub start: [f64; 3],
    /// Second endpoint in model coordinates.
    pub end: [f64; 3],
    /// Byte offset of the positional row in its section.
    pub offset: usize,
}

fn scalar_suffix(row: &[u8], count: usize, cache: &ScalarCache) -> Option<Vec<f64>> {
    (0..row.len())
        .filter_map(|start| {
            let mut cursor = start;
            let mut values = Vec::with_capacity(count);
            while values.len() < count {
                let (value, next) = if row.get(cursor) == Some(&0x18)
                    && scalar::decode_tabulated_cylinder_second_coordinate(row, cursor + 1, cache)
                        .is_some()
                {
                    (0.0, cursor + 1)
                } else {
                    scalar::decode_tabulated_cylinder_second_coordinate(row, cursor, cache)?
                };
                values.push(value);
                cursor = next;
            }
            (cursor == row.len() && values.iter().all(|value| value.is_finite()))
                .then_some((start, values))
        })
        .min_by_key(|(start, _)| *start)
        .map(|(_, values)| values)
}

/// Decode every complete positional `entity(line)` row.
pub fn lines(payload: &[u8]) -> Vec<ReferenceLine> {
    const PROTOTYPE: &[u8] = b"ent_list(line)\0";
    const LIST: &[u8] = b"\xe0\x00ent_list(";
    const INSTANCE: &[u8] = b"\xe0\x00entity(line)\0";
    const ENTITY: &[u8] = b"\xe0\x00entity(";
    const ROW_START: &[u8] = b"\xf6\xe2";

    let cache = ScalarCache::from_section(payload);
    let mut result = Vec::new();
    let mut search = 0;
    while let Some(prototype) = payload[search..]
        .windows(PROTOTYPE.len())
        .position(|window| window == PROTOTYPE)
        .map(|relative| search + relative)
    {
        let instance_search = prototype + PROTOTYPE.len();
        let prototype_end = payload[instance_search..]
            .windows(LIST.len())
            .position(|window| window == LIST)
            .map_or(payload.len(), |relative| instance_search + relative);
        let Some(instance) = payload[instance_search..prototype_end]
            .windows(INSTANCE.len())
            .position(|window| window == INSTANCE)
            .map(|relative| instance_search + relative)
        else {
            search = prototype_end.max(instance_search);
            continue;
        };
        let rows_start = instance + INSTANCE.len();
        let block_end = payload[rows_start..]
            .windows(ENTITY.len())
            .position(|window| window == ENTITY)
            .map_or(payload.len(), |relative| rows_start + relative);
        let mut starts = Vec::new();
        let mut cursor = rows_start;
        while let Some(start) = payload[cursor..block_end]
            .windows(ROW_START.len())
            .position(|window| window == ROW_START)
            .map(|relative| cursor + relative)
        {
            if starts.is_empty() || payload.get(start.wrapping_sub(1)) == Some(&0xe3) {
                starts.push(start);
            }
            cursor = start + ROW_START.len();
        }
        for (index, start) in starts.iter().copied().enumerate() {
            let end = starts.get(index + 1).map_or(block_end, |next| next - 1);
            let end = if payload.get(end.wrapping_sub(1)) == Some(&0xe3) {
                end - 1
            } else if payload.get(end) == Some(&0xe3) {
                end
            } else {
                continue;
            };
            let Some(values) = scalar_suffix(&payload[start..end], 6, &cache) else {
                continue;
            };
            result.push(ReferenceLine {
                start: values[..3].try_into().expect("three bounded coordinates"),
                end: values[3..].try_into().expect("three bounded coordinates"),
                offset: start,
            });
        }
        search = block_end.max(instance_search);
    }
    result.sort_by_key(|line| line.offset);
    result.dedup_by_key(|line| line.offset);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_complete_positional_line_rows() {
        let payload = b"ent_list(line)\0\xe0\x02end1\0\xf8\x03\x18\xdf\x1d\x84\xe8\xb0\xed\x7b\x46\x19\x87\x25\xdc\x17\x53\xfa\
            \xe0\x00entity(line)\0\xf1\xe3\xf7\x11\xf6\xe2\x02\x48\x10\x00\xeb\x10\x00\x00\x00\x00\x02\
            \x18\xdf\x1d\x84\xe8\xb0\xed\x7b\x2d\x19\x87\x25\xdc\x17\x53\xfa\
            \x18\x2d\x43\x23\xb0\x9d\x16\x1d\xaf\x2d\x19\x87\x25\xdc\x17\x53\xfa\xe3\
            \xe0\x00entity(text)\0";
        let decoded = lines(payload);
        let [line] = decoded.as_slice() else {
            panic!("one line");
        };
        assert_eq!(line.start[0], 0.0);
        assert_eq!(line.end[0], 0.0);
        assert_ne!(line.start, line.end);
    }

    #[test]
    fn withholds_incomplete_coordinate_suffix() {
        let payload =
            b"ent_list(line)\0\xe0\x00entity(line)\0\xf6\xe2\x02\x18\xe3\xe0\x00entity(text)\0";
        assert!(lines(payload).is_empty());
    }

    #[test]
    fn decodes_signed_coordinate_dictionary_line_rows() {
        let coordinates = b"\x18\x41\x93\x8a\x07\xa0\xe6\xf8\x55\x8c\x3e\x32\xfb\x7f\x13\x0b\
            \x18\x93\x27\x14\x0f\x41\xcd\xf1\x8c\x3e\x32\xfb\x7f\x13\x0b";
        assert!(scalar_suffix(coordinates, 6, &ScalarCache::from_section(coordinates)).is_some());
        let payload = b"ent_list(line)\0\xe0\x00entity(line)\0\xf1\xe3\xf7\x11\
            \xf6\xe2\x02\x48\x10\x00\xeb\x10\x00\x00\x00\x00\x02\
            \x18\x41\x93\x8a\x07\xa0\xe6\xf8\x55\x8c\x3e\x32\xfb\x7f\x13\x0b\
            \x18\x93\x27\x14\x0f\x41\xcd\xf1\x8c\x3e\x32\xfb\x7f\x13\x0b\xe3\
            \xe0\x00entity(text)\0";
        assert_eq!(lines(payload).len(), 1);
    }
}
