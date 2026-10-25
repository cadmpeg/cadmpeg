// SPDX-License-Identifier: Apache-2.0
//! Actual root uses determine whether a projected topology record exists.
use super::*;

impl PayloadSource<'_> {
    pub(super) fn root_view(&self) -> Result<View<'_>, CodecError> {
        let range = self.marks.root.as_ref().ok_or_else(|| {
            CodecError::malformed("topology producer requires a readable root use")
        })?;
        window(self.source, range.start, range.end)
    }
    fn count(&self, table: u8) -> Result<usize, CodecError> {
        let range = self
            .marks
            .markers
            .get(&table)
            .ok_or_else(|| CodecError::malformed("root use requires a readable table count"))?;
        let source = window(self.source, range.start, range.end)?;
        let name = TABLES
            .get(usize::from(table))
            .ok_or_else(|| CodecError::malformed("root count has an invalid table"))?;
        if self.binary {
            BinaryCursor {
                view: source,
                census_marks: vec![],
                representation_marks: vec![],
            }
            .section_count(name)
        } else {
            let (tokens, ranges) = text_tokens(source);
            let mut field = cursor(&tokens, &ranges, source);
            if field.next("root table selector")? != *name {
                return Err(CodecError::malformed(
                    "root table count differs from its selector",
                ));
            }
            field.count("root table count", 1_000_000)
        }
    }
    pub(super) fn roots(&self) -> Result<Vec<TextShapeUse>, CodecError> {
        let source = self.root_view()?;
        if self.binary {
            let mut cursor = BinaryCursor {
                view: source,
                census_marks: vec![],
                representation_marks: vec![],
            };
            let shape = cursor.i32("root shape")?;
            let location = cursor.i32("root location")?;
            let orientation = cursor.i32("root orientation")?;
            if (shape, location, orientation) == (-1, -1, -1) {
                return Ok(Vec::new());
            }
            let shape = checked_binary_reference(shape, self.count(7)?, false, "root shape")?;
            let location = if location == 0 {
                0
            } else {
                checked_binary_reference(location, self.count(0)?, true, "root location")?
            };
            Ok(vec![TextShapeUse {
                shape,
                location: location.into(),
                orientation: binary_orientation(orientation)?,
            }])
        } else {
            let (tokens, ranges) = text_tokens(source);
            let mut cursor = cursor(&tokens, &ranges, source);
            let mut roots = Vec::new();
            while !cursor.is_empty() {
                let token = cursor.next("root shape use")?;
                if token == "*" {
                    break;
                }
                let sign = token
                    .get(..1)
                    .ok_or_else(|| CodecError::malformed("root orientation is not ASCII"))?;
                let number = token
                    .get(1..)
                    .ok_or_else(|| CodecError::malformed("root shape token is not ASCII"))?;
                let orientation = match sign {
                    "+" => TextOrientation::Forward,
                    "-" => TextOrientation::Reversed,
                    "i" => TextOrientation::Internal,
                    "e" => TextOrientation::External,
                    _ => return Err(CodecError::malformed("root orientation is invalid")),
                };
                let count = self.count(7)?;
                let reverse = number.parse::<usize>().map_err(CodecError::malformed)?;
                if reverse == 0 || reverse > count {
                    return Err(CodecError::malformed(
                        "root shape reference is out of bounds",
                    ));
                }
                let shape = count
                    .checked_add(1)
                    .and_then(|count| count.checked_sub(reverse))
                    .ok_or_else(|| CodecError::malformed("root shape reference overflows"))?;
                let location = cursor.count("root location", 1_000_000)?;
                if location != 0 && location > self.count(0)? {
                    return Err(CodecError::malformed("root location is out of bounds"));
                }
                roots.push(TextShapeUse {
                    shape,
                    location: location.into(),
                    orientation,
                });
            }
            if !cursor.is_empty() {
                return Err(CodecError::malformed("tokens follow root terminator"));
            }
            Ok(roots)
        }
    }
    pub(super) fn reaches(
        &self,
        payload: &ShapePayloadRecord,
        shape: usize,
    ) -> Result<bool, CodecError> {
        let roots = self.root_values(payload)?;
        let mut pending = roots
            .into_iter()
            .map(|(root, _)| root.shape)
            .collect::<Vec<_>>();
        let mut seen = std::collections::BTreeSet::new();
        while let Some(current) = pending.pop() {
            if !seen.insert(current) {
                continue;
            }
            if current == shape {
                return Ok(true);
            }
            let value = self.shape(payload, current)?;
            pending.extend(value.children.iter().map(|child| child.shape));
        }
        Ok(false)
    }
}
