// SPDX-License-Identifier: Apache-2.0
//! Framing and identity decode for outer `7C05` entity-table records.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One lossless token in a nested `7C07` entity-value payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum ValueField {
    /// `0x32` followed by a fixed-width entity identity.
    EntityReference {
        /// Stored entity identity.
        entity_id: u32,
        /// Byte offset within the value payload.
        offset: usize,
    },
    /// `0xE6` followed by the exact IEEE-754 binary64 bits.
    Binary64 {
        /// Stored little-endian binary64 bits.
        bits: u64,
        /// Byte offset within the value payload.
        offset: usize,
    },
    /// Compact unsigned atom.
    Atom {
        /// Decoded unsigned value.
        value: u32,
        /// Stored width, one or two bytes.
        width: u8,
        /// Byte offset within the value payload.
        offset: usize,
    },
    /// `0xFE` value terminator.
    Terminator {
        /// Byte offset within the value payload.
        offset: usize,
    },
    /// One byte outside the assigned fixed-width and compact token forms.
    Literal {
        /// Exact stored byte.
        value: u8,
        /// Byte offset within the value payload.
        offset: usize,
    },
}

/// One length-closed `7C05` entity-table record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityRecord {
    /// Byte offset of the `7C05` marker.
    pub pos: usize,
    /// Total framed byte length.
    pub total_len: usize,
    /// Byte between the `7C05` length and nested `7C06` marker.
    pub lead: u8,
    /// Stored nested `7C06` length.
    pub definition_len: u32,
    /// Exact definition prefix before the `0xEA` identity delimiter.
    pub definition_prefix: Vec<u8>,
    /// Stored entity identity.
    pub entity_id: u32,
    /// Exact definition bytes after the identity.
    pub definition_suffix: Vec<u8>,
    /// Stored nested `7C07` total length.
    pub value_len: u32,
    /// Exact nested `7C07` payload.
    pub value_payload: Vec<u8>,
    /// Lossless tokens in the nested `7C07` payload.
    pub value_fields: Vec<ValueField>,
    /// Exact bytes after the nested `7C07` frame.
    pub record_suffix: Vec<u8>,
}

/// Parse every maximal contiguous run of length-closed `7C05` records.
#[must_use]
pub fn parse_runs(data: &[u8]) -> Vec<Vec<EntityRecord>> {
    let candidates = data
        .windows(2)
        .enumerate()
        .filter(|(_, marker)| *marker == [0x7c, 0x05])
        .filter_map(|(pos, _)| parse_candidate(data, pos))
        .collect::<Vec<_>>();
    let roots = candidates
        .iter()
        .filter(|candidate| {
            !candidates.iter().any(|outer| {
                outer.pos < candidate.pos
                    && outer.pos.checked_add(outer.total_len).is_some_and(|end| {
                        candidate
                            .pos
                            .checked_add(candidate.total_len)
                            .is_some_and(|candidate_end| candidate_end <= end)
                    })
            })
        })
        .cloned()
        .collect::<Vec<_>>();

    roots
        .into_iter()
        .fold(Vec::<Vec<EntityRecord>>::new(), |mut runs, record| {
            if runs
                .last()
                .and_then(|run| run.last())
                .is_some_and(|last| last.pos.checked_add(last.total_len) == Some(record.pos))
            {
                runs.last_mut()
                    .expect("a final record implies a final run")
                    .push(record);
            } else {
                runs.push(vec![record]);
            }
            runs
        })
        .into_iter()
        .filter(|run| {
            run.windows(2)
                .all(|pair| pair[0].entity_id < pair[1].entity_id)
        })
        .collect()
}

fn parse_candidate(data: &[u8], pos: usize) -> Option<EntityRecord> {
    let total_len = usize::try_from(u32_le(data, pos.checked_add(2)?)?).ok()?;
    let end = pos.checked_add(total_len)?;
    if total_len < 19
        || end > data.len()
        || data.get(pos.checked_add(6)?)? > &0x02
        || data.get(pos.checked_add(7)?..pos.checked_add(9)?)? != [0x7c, 0x06]
    {
        return None;
    }

    let lead = *data.get(pos + 6)?;
    let definition_len = u32_le(data, pos + 9)?;
    let definition_len_usize = usize::try_from(definition_len).ok()?;
    let definition_end = pos.checked_add(7)?.checked_add(definition_len_usize)?;
    if definition_len_usize < 11 || definition_end > end {
        return None;
    }
    let definition_start = pos + 13;
    let mut at = definition_start;
    while at < definition_end {
        match data[at] {
            0xea => break,
            0x32 => at = at.checked_add(5)?,
            _ => at += 1,
        }
    }
    let identity_end = at.checked_add(5)?;
    if identity_end > definition_end
        || data.get(definition_end..definition_end.checked_add(2)?)? != [0x7c, 0x07]
    {
        return None;
    }
    let entity_id = u32_le(data, at + 1)?;
    let value_len = u32_le(data, definition_end + 2)?;
    let value_len_usize = usize::try_from(value_len).ok()?;
    let value_end = definition_end.checked_add(value_len_usize)?;
    if entity_id == 0 || value_len_usize < 6 || value_end > end {
        return None;
    }

    Some(EntityRecord {
        pos,
        total_len,
        lead,
        definition_len,
        definition_prefix: data[definition_start..at].to_vec(),
        entity_id,
        definition_suffix: data[identity_end..definition_end].to_vec(),
        value_len,
        value_payload: data[definition_end + 6..value_end].to_vec(),
        value_fields: tokenize_value(&data[definition_end + 6..value_end]),
        record_suffix: data[value_end..end].to_vec(),
    })
}

pub(crate) fn tokenize_value(payload: &[u8]) -> Vec<ValueField> {
    let mut fields = Vec::new();
    let mut at = 0;
    while at < payload.len() {
        let offset = at;
        if payload[at] == 0x32 && at + 5 <= payload.len() {
            fields.push(ValueField::EntityReference {
                entity_id: u32::from_le_bytes(
                    payload[at + 1..at + 5]
                        .try_into()
                        .expect("checked entity-reference extent"),
                ),
                offset,
            });
            at += 5;
        } else if payload[at] == 0xe6 && at + 9 <= payload.len() {
            fields.push(ValueField::Binary64 {
                bits: u64::from_le_bytes(
                    payload[at + 1..at + 9]
                        .try_into()
                        .expect("checked binary64 extent"),
                ),
                offset,
            });
            at += 9;
        } else if (0x80..=0xd0).contains(&payload[at]) {
            fields.push(ValueField::Atom {
                value: u32::from(payload[at] - 0x80),
                width: 1,
                offset,
            });
            at += 1;
        } else if (0xd1..=0xe4).contains(&payload[at]) && at + 2 <= payload.len() {
            fields.push(ValueField::Atom {
                value: u32::from(payload[at] - 0xd1) * 256 + u32::from(payload[at + 1]) + 1,
                width: 2,
                offset,
            });
            at += 2;
        } else if payload[at] == 0xfe {
            fields.push(ValueField::Terminator { offset });
            at += 1;
        } else {
            fields.push(ValueField::Literal {
                value: payload[at],
                offset,
            });
            at += 1;
        }
    }
    fields
}

fn u32_le(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::{parse_runs, tokenize_value, ValueField};

    fn record(prefix: &[u8], entity_id: u32) -> Vec<u8> {
        let mut bytes = vec![0x7c, 0x05, 0, 0, 0, 0, 0, 0x7c, 0x06];
        bytes.extend_from_slice(
            &u32::try_from(prefix.len() + 12)
                .expect("bounded test definition")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(prefix);
        bytes.push(0xea);
        bytes.extend_from_slice(&entity_id.to_le_bytes());
        bytes.push(0xaa);
        bytes.extend_from_slice(&[0x7c, 0x07, 7, 0, 0, 0, 0xfe, 0xbb]);
        let len = u32::try_from(bytes.len()).expect("bounded test record");
        bytes[2..6].copy_from_slice(&len.to_le_bytes());
        bytes
    }

    #[test]
    fn fixed_width_definition_atom_does_not_terminate_at_embedded_ea() {
        let prefix = [0x32, 0xea, 0, 0, 0, 0x11];
        let records = record(&prefix, 37);
        let runs = parse_runs(&records);
        let [run] = runs.as_slice() else {
            panic!("one entity-table run");
        };

        assert_eq!(run[0].definition_prefix, prefix);
        assert_eq!(run[0].entity_id, 37);
        assert_eq!(run[0].definition_suffix, [0xaa]);
        assert_eq!(run[0].value_len, 7);
        assert_eq!(run[0].value_payload, [0xfe]);
        assert_eq!(run[0].record_suffix, [0xbb]);
    }

    #[test]
    fn entity_table_runs_require_strictly_increasing_identities() {
        let mut records = record(&[0x11], 3);
        records.extend(record(&[0x12], 2));

        assert!(parse_runs(&records).is_empty());
    }

    #[test]
    fn value_tokens_hide_markers_inside_fixed_width_fields() {
        let payload = [
            0x32, 0xe6, 0, 0, 0, 0xe6, 0x32, 0, 0, 0, 0, 0, 0xfe, 0, 0x81, 0xfe,
        ];

        assert_eq!(
            tokenize_value(&payload),
            vec![
                ValueField::EntityReference {
                    entity_id: 0xe6,
                    offset: 0,
                },
                ValueField::Binary64 {
                    bits: 0x00fe_0000_0000_0032,
                    offset: 5,
                },
                ValueField::Atom {
                    value: 1,
                    width: 1,
                    offset: 14,
                },
                ValueField::Terminator { offset: 15 },
            ]
        );
    }

    #[test]
    fn truncated_fixed_width_value_forms_remain_lossless() {
        assert_eq!(
            tokenize_value(&[0x32, 1, 2]),
            vec![
                ValueField::Literal {
                    value: 0x32,
                    offset: 0,
                },
                ValueField::Literal {
                    value: 1,
                    offset: 1,
                },
                ValueField::Literal {
                    value: 2,
                    offset: 2,
                },
            ]
        );
        assert_eq!(
            tokenize_value(&[0xe6, 3, 4, 0xfe]),
            vec![
                ValueField::Literal {
                    value: 0xe6,
                    offset: 0,
                },
                ValueField::Literal {
                    value: 3,
                    offset: 1,
                },
                ValueField::Literal {
                    value: 4,
                    offset: 2,
                },
                ValueField::Terminator { offset: 3 },
            ]
        );
    }
}
