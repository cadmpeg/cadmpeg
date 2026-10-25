// SPDX-License-Identifier: Apache-2.0
//! Canonical polygon identities keep their cardinality and declared order.
use super::*;

impl Generated {
    pub(super) fn push_polygon(
        &mut self,
        table: usize,
        id: &str,
        row: String,
    ) -> Result<usize, CodecError> {
        let lane = match table {
            3 => 0,
            4 => 1,
            _ => {
                return Err(CodecError::malformed(
                    "canonical polygon has an invalid table",
                ))
            }
        };
        if let Some((existing_table, index)) = self.polygon_ids.get(id) {
            if *existing_table != table {
                return Err(CodecError::malformed(
                    "polygon identity occurs in two tables",
                ));
            }
            if self.tables[table].get(index - 1) == Some(&row) {
                return Ok(*index);
            }
            // A second, edited neutral use is real current geometry. It gets
            // an independent row while the first use keeps the canonical slot.
            return Ok(self.push(table, row));
        }
        // Only the actual serializer's forward native_ref association can
        // identify an existing slot. Equal-valued unrelated rows prove nothing.
        let slot = self
            .polygon_sources
            .get(id)
            .and_then(|slots| {
                slots.iter().copied().find(|slot| {
                    table == 3
                        && self.tables[table].get(slot - 1) == Some(&row)
                        && !self
                            .polygon_ids
                            .values()
                            .any(|(other, claimed)| *other == table && claimed == slot)
                })
            })
            .unwrap_or_else(|| {
                self.tables[table].push(row);
                self.tables[table].len()
            });
        self.polygon_ids.insert(id.to_owned(), (table, slot));
        self.polygon_order[lane].push(slot);
        Ok(slot)
    }

    pub(super) fn order_polygons(&mut self) -> Result<(), CodecError> {
        let mut maps: [BTreeMap<usize, usize>; 2] = Default::default();
        for (lane, table) in [(0, 3), (1, 4)] {
            let mut order = std::mem::take(&mut self.polygon_order[lane]);
            order.extend((1..=self.tables[table].len()).filter(|slot| {
                !self
                    .polygon_ids
                    .values()
                    .any(|(other, claimed)| *other == table && claimed == slot)
            }));
            let old = std::mem::take(&mut self.tables[table]);
            for (position, slot) in order.into_iter().enumerate() {
                let row = old.get(slot - 1).ok_or_else(|| {
                    CodecError::malformed("canonical polygon slot exceeds emitted table")
                })?;
                maps[lane].insert(slot, position + 1);
                self.tables[table].push(row.clone());
            }
        }
        for shape in &mut self.shapes {
            *shape = remap_edge(shape, &maps, self.version)?;
        }
        Ok(())
    }
}

// Rewrite only explicit polygon operands in the selected text grammar.
fn remap_edge(
    shape: &str,
    maps: &[BTreeMap<usize, usize>; 2],
    version: u8,
) -> Result<String, CodecError> {
    let mut tokens = Vec::new();
    let mut at = 0;
    for token in shape.split_ascii_whitespace() {
        let start = shape[at..]
            .find(token)
            .ok_or_else(|| CodecError::malformed("generated Edge token is absent"))?
            + at;
        at = start + token.len();
        tokens.push((token, start..at));
    }
    if tokens.first().map(|(token, _)| *token) != Some("Ed") {
        return Ok(shape.to_owned());
    }
    let mut position = 5;
    let mut replacements = Vec::new();
    loop {
        let kind = tokens
            .get(position)
            .ok_or_else(|| CodecError::malformed("generated Edge representation is cut short"))?
            .0
            .parse::<usize>()
            .map_err(|_| CodecError::malformed("generated Edge representation kind is invalid"))?;
        position += 1;
        if kind == 0 {
            break;
        }
        let (width, lane, count) = match kind {
            1 => (4, 0, 0),
            2 => (if version == 2 { 9 } else { 5 }, 0, 0),
            3 => (if version == 2 { 11 } else { 7 }, 0, 0),
            4 => (5, 0, 0),
            5 => (2, 0, 1),
            6 => (3, 1, 1),
            7 => (4, 1, 2),
            _ => {
                return Err(CodecError::malformed(
                    "generated Edge representation kind is unknown",
                ))
            }
        };
        let end = position
            .checked_add(width)
            .filter(|end| *end <= tokens.len())
            .ok_or_else(|| {
                CodecError::malformed("generated Edge representation operands are cut short")
            })?;
        for (text, range) in &tokens[position..position + count] {
            let original = text
                .parse::<usize>()
                .map_err(|_| CodecError::malformed("generated polygon reference is invalid"))?;
            let index = maps[lane].get(&original).ok_or_else(|| {
                CodecError::malformed("generated polygon reference has no emitted row")
            })?;
            replacements.push((range.clone(), index.to_string()));
        }
        position = end;
    }
    let mut output = shape.to_owned();
    for (range, index) in replacements.into_iter().rev() {
        output.replace_range(range, &index);
    }
    Ok(output)
}

#[cfg(test)]
mod tests;
