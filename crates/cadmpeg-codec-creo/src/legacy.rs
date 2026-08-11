// SPDX-License-Identifier: Apache-2.0
//! Structural grammar for legacy ASCII persistence records.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

/// One unique `@<name> <id> <type-code>` declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeDeclaration {
    /// Attribute identifier referenced by value rows in the same scope.
    pub id: u32,
    /// Attribute name without the leading `@`.
    pub name: String,
    /// Stored numeric type code.
    pub type_code: u8,
    /// Byte offset of the declaration line.
    pub offset: usize,
}

/// One `<depth> <attribute-id> <payload>` value row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeValue {
    /// Object-tree nesting depth.
    pub depth: u32,
    /// Identifier of the owning attribute declaration in the same scope.
    pub attribute_id: u32,
    /// Byte offset of the value row.
    pub offset: usize,
    /// Byte range of the payload after the second field separator.
    pub payload: Range<usize>,
    /// Contiguous source range containing immediately following `$` rows.
    pub continuation_rows: Option<Range<usize>>,
    /// Number of immediately following `$` rows.
    pub continuation_count: usize,
}

/// Declarations and values owned by one outer object or named ASCII section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    /// Complete byte extent scanned for this scope.
    pub range: Range<usize>,
    /// First declaration for each identifier, in source order.
    pub declarations: Vec<AttributeDeclaration>,
    /// Value rows whose declaration resolves uniquely in this scope.
    pub values: Vec<AttributeValue>,
    /// Numeric value rows whose identifier has no unique local declaration.
    pub unresolved_value_count: usize,
    /// Repeated local identifiers whose name or type code conflicts.
    pub conflicting_declaration_count: usize,
}

/// Structurally resolved legacy ASCII persistence scopes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Persistence {
    /// Outer persistence scope followed by each named ASCII section scope.
    pub scopes: Vec<Scope>,
}

impl Persistence {
    /// Number of unique local attribute declarations across all scopes.
    pub fn declaration_count(&self) -> usize {
        self.scopes
            .iter()
            .map(|scope| scope.declarations.len())
            .sum()
    }

    /// Number of structurally resolved value rows across all scopes.
    pub fn value_count(&self) -> usize {
        self.scopes.iter().map(|scope| scope.values.len()).sum()
    }

    /// Number of `$` continuation rows across all resolved values.
    pub fn continuation_count(&self) -> usize {
        self.scopes
            .iter()
            .flat_map(|scope| &scope.values)
            .map(|value| value.continuation_count)
            .sum()
    }

    /// Number of numeric rows without a unique declaration in their scope.
    pub fn unresolved_value_count(&self) -> usize {
        self.scopes
            .iter()
            .map(|scope| scope.unresolved_value_count)
            .sum()
    }

    /// Number of conflicting declaration identifiers across all scopes.
    pub fn conflicting_declaration_count(&self) -> usize {
        self.scopes
            .iter()
            .map(|scope| scope.conflicting_declaration_count)
            .sum()
    }
}

pub(crate) fn line(data: &[u8], start: usize) -> Option<(&[u8], usize)> {
    let bytes = data.get(start..)?;
    let relative_end = bytes.iter().position(|byte| *byte == b'\n');
    let end = relative_end.map_or(data.len(), |end| start + end);
    let next = relative_end.map_or(end, |_| end + 1);
    Some((
        data[start..end]
            .strip_suffix(b"\r")
            .unwrap_or(&data[start..end]),
        next,
    ))
}

pub(crate) fn parse_declaration(line: &[u8], offset: usize) -> Option<AttributeDeclaration> {
    let line = std::str::from_utf8(line).ok()?;
    let mut fields = line.split_ascii_whitespace();
    let name = fields.next()?.strip_prefix('@')?;
    if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_graphic()) {
        return None;
    }
    let id = fields.next()?.parse().ok()?;
    let type_code = fields.next()?.parse().ok()?;
    fields.next().is_none().then(|| AttributeDeclaration {
        id,
        name: name.to_string(),
        type_code,
        offset,
    })
}

pub(crate) fn starts_with_declaration(data: &[u8], start: usize) -> bool {
    line(data, start)
        .and_then(|(line, _)| parse_declaration(line, start))
        .is_some()
}

fn decimal(bytes: &[u8], mut offset: usize) -> Option<(u32, usize)> {
    let start = offset;
    let mut value = 0u32;
    while let Some(digit) = bytes.get(offset).filter(|byte| byte.is_ascii_digit()) {
        value = value
            .checked_mul(10)?
            .checked_add(u32::from(*digit - b'0'))?;
        offset += 1;
    }
    (offset > start).then_some((value, offset))
}

fn value(line: &[u8], line_offset: usize) -> Option<AttributeValue> {
    let (depth, after_depth) = decimal(line, 0)?;
    if line.get(after_depth) != Some(&b' ') {
        return None;
    }
    let (attribute_id, after_attribute) = decimal(line, after_depth + 1)?;
    let payload_start = if after_attribute == line.len() {
        after_attribute
    } else {
        if line.get(after_attribute) != Some(&b' ') {
            return None;
        }
        after_attribute + 1
    };
    Some(AttributeValue {
        depth,
        attribute_id,
        offset: line_offset,
        payload: line_offset + payload_start..line_offset + line.len(),
        continuation_rows: None,
        continuation_count: 0,
    })
}

fn scan_scope(data: &[u8], range: Range<usize>) -> Scope {
    let end = range.end.min(data.len());
    let range = range.start.min(end)..end;
    let mut declarations = Vec::<AttributeDeclaration>::new();
    let mut declaration_indices = BTreeMap::<u32, usize>::new();
    let mut conflicting_ids = BTreeSet::<u32>::new();
    let mut candidates = Vec::<AttributeValue>::new();
    let mut continuation_owner = None::<usize>;
    let mut next = range.start;

    while next < range.end {
        let line_offset = next;
        let Some((current, after_line)) = line(&data[..range.end], next) else {
            break;
        };
        next = after_line;
        if current == b"#END_OF_UGC" {
            break;
        }
        if current.starts_with(b"$") {
            if let Some(owner) = continuation_owner {
                let value = &mut candidates[owner];
                value
                    .continuation_rows
                    .get_or_insert(line_offset..line_offset + current.len())
                    .end = line_offset + current.len();
                value.continuation_count += 1;
            }
            continue;
        }
        continuation_owner = None;

        if let Some(declaration) = parse_declaration(current, line_offset) {
            if let Some(index) = declaration_indices.get(&declaration.id).copied() {
                let previous = &declarations[index];
                if previous.name != declaration.name || previous.type_code != declaration.type_code
                {
                    conflicting_ids.insert(declaration.id);
                }
            } else {
                declaration_indices.insert(declaration.id, declarations.len());
                declarations.push(declaration);
            }
            continue;
        }
        if let Some(value) = value(current, line_offset) {
            continuation_owner = Some(candidates.len());
            candidates.push(value);
        }
    }

    let candidate_count = candidates.len();
    candidates.retain(|value| {
        declaration_indices.contains_key(&value.attribute_id)
            && !conflicting_ids.contains(&value.attribute_id)
    });
    Scope {
        range,
        declarations,
        unresolved_value_count: candidate_count - candidates.len(),
        values: candidates,
        conflicting_declaration_count: conflicting_ids.len(),
    }
}

/// Scan independently scoped legacy ASCII record extents.
pub(crate) fn scan(data: &[u8], ranges: impl IntoIterator<Item = Range<usize>>) -> Persistence {
    Persistence {
        scopes: ranges
            .into_iter()
            .filter(|range| range.start < range.end && range.start < data.len())
            .map(|range| scan_scope(data, range))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_resolves_declarations_values_and_continuations() {
        let data = b"#P_OBJECT 6\n@root 1 0\n0 1 ->\n@matrix 2 2\n1 2 [2][2]\n\
                     $3FF,0\n$0,3FF\n#END_OF_UGC\n";

        let persistence = scan(data, std::iter::once(0..data.len()));
        let scope = &persistence.scopes[0];

        assert_eq!(scope.declarations.len(), 2);
        assert_eq!(scope.declarations[1].name, "matrix");
        assert_eq!(scope.declarations[1].type_code, 2);
        assert_eq!(scope.values.len(), 2);
        assert_eq!(&data[scope.values[0].payload.clone()], b"->");
        assert_eq!(&data[scope.values[1].payload.clone()], b"[2][2]");
        assert_eq!(scope.values[1].continuation_count, 2);
        let continuation = scope.values[1]
            .continuation_rows
            .clone()
            .expect("continuations");
        assert_eq!(&data[continuation], b"$3FF,0\n$0,3FF");
        assert_eq!(persistence.unresolved_value_count(), 0);
        assert_eq!(persistence.conflicting_declaration_count(), 0);
    }

    #[test]
    fn scan_resolves_identifiers_within_independent_scopes() {
        let data = b"@field 7 1\n1 7 4\n@other 7 2\n2 7 5\n";
        let second = data
            .windows(b"@other".len())
            .position(|window| window == b"@other")
            .expect("second scope");

        let persistence = scan(data, [0..second, second..data.len()]);

        assert_eq!(persistence.scopes.len(), 2);
        assert_eq!(persistence.declaration_count(), 2);
        assert_eq!(persistence.value_count(), 2);
        assert_eq!(persistence.conflicting_declaration_count(), 0);
    }

    #[test]
    fn scan_withholds_ambiguous_and_undeclared_values() {
        let data = b"#P_OBJECT 6\n$orphan\n@field 7 1\n@other 7 2\n1 7 4\n2 99 5\n";

        let persistence = scan(data, std::iter::once(0..data.len()));
        let scope = &persistence.scopes[0];

        assert!(scope.values.is_empty());
        assert_eq!(scope.declarations.len(), 1);
        assert_eq!(persistence.conflicting_declaration_count(), 1);
        assert_eq!(persistence.unresolved_value_count(), 2);
    }
}
