// SPDX-License-Identifier: Apache-2.0
//! Framing and identity decode for outer `7C05` entity-table records.

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
    /// Exact bytes after the identity through the `7C05` frame end.
    pub tail: Vec<u8>,
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
    let definition_start = pos + 13;
    let mut at = definition_start;
    while at < end {
        match data[at] {
            0xea => break,
            0x32 => at = at.checked_add(5)?,
            _ => at += 1,
        }
    }
    let identity_end = at.checked_add(5)?;
    if identity_end > end {
        return None;
    }
    let entity_id = u32_le(data, at + 1)?;
    let tail = data.get(identity_end..end)?.to_vec();
    if entity_id == 0 || !tail.windows(2).any(|marker| marker == [0x7c, 0x07]) {
        return None;
    }

    Some(EntityRecord {
        pos,
        total_len,
        lead,
        definition_len,
        definition_prefix: data[definition_start..at].to_vec(),
        entity_id,
        tail,
    })
}

fn u32_le(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::parse_runs;

    fn record(prefix: &[u8], entity_id: u32) -> Vec<u8> {
        let mut bytes = vec![0x7c, 0x05, 0, 0, 0, 0, 0, 0x7c, 0x06];
        bytes.extend_from_slice(
            &u32::try_from(prefix.len() + 5)
                .expect("bounded test definition")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(prefix);
        bytes.push(0xea);
        bytes.extend_from_slice(&entity_id.to_le_bytes());
        bytes.extend_from_slice(&[0x7c, 0x07, 6, 0, 0, 0]);
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
    }

    #[test]
    fn entity_table_runs_require_strictly_increasing_identities() {
        let mut records = record(&[0x11], 3);
        records.extend(record(&[0x12], 2));

        assert!(parse_runs(&records).is_empty());
    }
}
