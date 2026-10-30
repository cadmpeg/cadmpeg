// SPDX-License-Identifier: Apache-2.0
//! The structural sections of a USDC layer (spec §9.2, §9.5).
//!
//! The table of contents names the sections. TOKENS, STRINGS, FIELDS,
//! FIELDSETS, PATHS, and SPECS describe every spec of the layer as a path,
//! a spec type, and a run of fields; each field is a token and a value
//! representation.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use super::compression::{expand, Size};
use super::integers::{self, Width};
use crate::layout::{usdc_bootstrap as bootstrap, usdc_path_item, usdc_section};

/// A crate version: major, minor, and patch.
pub(super) type Version = [u8; 3];

/// The version that compresses the structural sections.
const COMPRESSED_STRUCTURE: Version = [0, 4, 0];
/// The version whose path items and specs have no leading padding.
const UNPADDED_RECORDS: Version = [0, 1, 0];
/// The terminator of a field run in FIELDSETS.
pub(super) const FIELD_RUN_END: u32 = u32::MAX;
/// Bit of a path item header: the next item is the first child.
const HAS_CHILD: u8 = 1;
/// Bit of a path item header: a sibling follows the subtree.
const HAS_SIBLING: u8 = 1 << 1;
/// Bit of a path item header: the element is a property name.
const IS_PROPERTY: u8 = 1 << 2;

/// One path of the PATHS section: its parent, and its last element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PathNode {
    /// The parent path index; `None` for the absolute root.
    pub(super) parent: Option<usize>,
    /// The token index of the last element; unused for the root.
    pub(super) element: usize,
    /// The element is a property name.
    pub(super) property: bool,
}

/// One spec of the SPECS section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Spec {
    pub(super) path: usize,
    pub(super) field_set: usize,
    pub(super) kind: u32,
}

/// One field: its name token and its value representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Field {
    pub(super) token: usize,
    pub(super) rep: u64,
}

/// The decoded structural sections of a layer.
#[derive(Debug)]
pub(super) struct Structure<'a> {
    pub(super) version: Version,
    /// The whole layer, for values at file offsets.
    pub(super) file: View<'a>,
    pub(super) tokens: Vec<String>,
    /// The token index of each string.
    pub(super) strings: Vec<usize>,
    pub(super) fields: Vec<Field>,
    /// Field indices in runs, each run ended by [`FIELD_RUN_END`].
    pub(super) field_sets: Vec<u32>,
    pub(super) paths: Vec<Option<PathNode>>,
    pub(super) specs: Vec<Spec>,
}

fn malformed(what: &str) -> CodecError {
    CodecError::malformed(format_args!("corrupt USDC layer: {what}"))
}

/// The position in `file`'s space of the layer offset `offset`.
pub(super) fn absolute(file: View<'_>, offset: usize) -> Result<usize, CodecError> {
    file.start()
        .checked_add(offset)
        .ok_or_else(|| malformed("an offset overflows"))
}

fn index(value: u64, what: &str) -> Result<usize, CodecError> {
    usize::try_from(value).map_err(|_| malformed(what))
}

/// A count read as `u64` and proven to fit the unread bytes at
/// `min_element_size` bytes an element.
fn count(view: &mut View<'_>, min_element_size: usize, what: &str) -> Result<usize, CodecError> {
    let value = view.req_u64_le()?;
    view.counted(value, min_element_size)
        .map(cadmpeg_core::decode::BoundedCount::get)
        .ok_or_else(|| malformed(what))
}

/// The table of contents: each section's name, start, and size.
fn table_of_contents(file: View<'_>) -> Result<Vec<(String, View<'_>)>, CodecError> {
    let offset = View::i64_le_at(file.window(), bootstrap::TOC_OFFSET)
        .ok_or_else(|| malformed("the bootstrap header is truncated"))?;
    let offset = usize::try_from(offset).map_err(|_| malformed("the table offset is negative"))?;
    let mut view = file;
    view.seek(absolute(file, offset)?)
        .ok_or_else(|| malformed("the table of contents is past the end"))?;
    let sections = count(
        &mut view,
        usdc_section::LEN,
        "the section count exceeds the table",
    )?;
    let mut out = Vec::new();
    for _ in 0..sections {
        let record = view.req_take(usdc_section::LEN)?;
        let name = &record[usdc_section::NAME..usdc_section::START];
        let name: Vec<u8> = name.iter().copied().take_while(|byte| *byte != 0).collect();
        let name = String::from_utf8(name).map_err(|_| malformed("a section name is not text"))?;
        let start = View::i64_le_at(record, usdc_section::START)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| malformed("a section start is negative"))?;
        let size = View::i64_le_at(record, usdc_section::SIZE)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| malformed("a section size is negative"))?;
        let end = start
            .checked_add(size)
            .ok_or_else(|| malformed("a section end overflows"))?;
        let section = file
            .child(absolute(file, start)?, absolute(file, end)?)
            .ok_or_else(|| malformed("a section is past the end"))?;
        out.push((name, section));
    }
    Ok(out)
}

fn tokens<'a>(
    ctx: &DecodeContext<'a>,
    version: Version,
    section: View<'a>,
) -> Result<Vec<String>, CodecError> {
    let mut view = section;
    let declared = view.req_u64_le()?;
    let chars = if version < COMPRESSED_STRUCTURE {
        let size = view.req_u64_le()?;
        view.req_take_child(index(size, "the token bytes exceed the address space")?)?
    } else {
        let uncompressed = index(
            view.req_u64_le()?,
            "the token bytes exceed the address space",
        )?;
        let compressed = index(
            view.req_u64_le()?,
            "the token bytes exceed the address space",
        )?;
        let compressed = view.req_take_child(compressed)?;
        expand(ctx, compressed, Size::Exact(uncompressed))?
    };
    // Each token is at least its terminating zero byte.
    let declared = chars
        .counted(declared, 1)
        .map(cadmpeg_core::decode::BoundedCount::get)
        .ok_or_else(|| malformed("the token count exceeds the token bytes"))?;
    let mut out = ctx.alloc_filled(declared, String::new(), "usdc_tokens")?;
    let mut rest = chars.window();
    for slot in &mut out {
        let end = rest
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| malformed("a token is not terminated"))?;
        *slot = String::from_utf8(rest[..end].to_vec())
            .map_err(|_| malformed("a token is not UTF-8 text"))?;
        rest = &rest[end + 1..];
    }
    ctx.retain_bytes(
        cadmpeg_core::decode::u64_from_index(chars.window().len()),
        "usdc_tokens",
    )?;
    Ok(out)
}

/// A `u64`-counted vector of `u32` values.
fn u32_vector(
    ctx: &DecodeContext<'_>,
    view: &mut View<'_>,
    what: &'static str,
) -> Result<Vec<u32>, CodecError> {
    let length = count(view, 4, what)?;
    let mut out = ctx.alloc_filled(length, 0_u32, what)?;
    for slot in &mut out {
        *slot = view.req_u32_le()?;
    }
    Ok(out)
}

/// A compressed sequence of `u32` values after its `u64` count.
fn compressed_u32<'a>(
    ctx: &DecodeContext<'a>,
    view: &mut View<'a>,
    count: usize,
) -> Result<Vec<u32>, CodecError> {
    integers::read(ctx, view, count, Width::Bits32)?
        .into_iter()
        .map(|value| {
            u32::try_from(value.rem_euclid(1 << 32)).map_err(|_| malformed("an index is negative"))
        })
        .collect()
}

/// A compressed sequence of `i32` values.
fn compressed_i32<'a>(
    ctx: &DecodeContext<'a>,
    view: &mut View<'a>,
    count: usize,
) -> Result<Vec<i64>, CodecError> {
    integers::read(ctx, view, count, Width::Bits32)
}

/// A count of compressed integers: at least two bits each in the coded
/// bytes, which the compressed bytes expand to at most 255 times.
fn compressed_count(view: &mut View<'_>, what: &str) -> Result<usize, CodecError> {
    let value = view.req_u64_le()?;
    let reach = view.remaining().checked_mul(255 * 4);
    usize::try_from(value)
        .ok()
        .filter(|value| reach.is_none_or(|reach| *value <= reach))
        .ok_or_else(|| malformed(what))
}

fn fields<'a>(
    ctx: &DecodeContext<'a>,
    version: Version,
    section: View<'a>,
) -> Result<Vec<Field>, CodecError> {
    let mut view = section;
    if version < COMPRESSED_STRUCTURE {
        // Each field is 4 bytes of padding, a token index, and a value
        // representation.
        let length = count(&mut view, 16, "the field count exceeds the section")?;
        let mut out = ctx.alloc_filled(length, Field { token: 0, rep: 0 }, "usdc_fields")?;
        for slot in &mut out {
            view.req_take(4)?;
            let token = index(u64::from(view.req_u32_le()?), "a field token")?;
            *slot = Field {
                token,
                rep: view.req_u64_le()?,
            };
        }
        return Ok(out);
    }
    let length = compressed_count(&mut view, "the field count exceeds the section")?;
    let tokens = compressed_u32(ctx, &mut view, length)?;
    let reps_size = index(view.req_u64_le()?, "the field representations")?;
    let reps = view.req_take_child(reps_size)?;
    let size = length
        .checked_mul(8)
        .ok_or_else(|| malformed("the field representations overflow"))?;
    let reps = expand(ctx, reps, Size::Exact(size))?;
    let mut reps_view = reps;
    let mut out = ctx.alloc_filled(length, Field { token: 0, rep: 0 }, "usdc_fields")?;
    for (slot, token) in out.iter_mut().zip(tokens) {
        *slot = Field {
            token: index(u64::from(token), "a field token")?,
            rep: reps_view.req_u64_le()?,
        };
    }
    Ok(out)
}

fn field_sets<'a>(
    ctx: &DecodeContext<'a>,
    version: Version,
    section: View<'a>,
) -> Result<Vec<u32>, CodecError> {
    let mut view = section;
    if version < COMPRESSED_STRUCTURE {
        return u32_vector(ctx, &mut view, "usdc_field_sets");
    }
    let length = compressed_count(&mut view, "the field-set count exceeds the section")?;
    compressed_u32(ctx, &mut view, length)
}

fn specs<'a>(
    ctx: &DecodeContext<'a>,
    version: Version,
    section: View<'a>,
) -> Result<Vec<Spec>, CodecError> {
    let mut view = section;
    let empty = Spec {
        path: 0,
        field_set: 0,
        kind: 0,
    };
    if version < COMPRESSED_STRUCTURE {
        let padded = version < UNPADDED_RECORDS;
        let size = if padded { 16 } else { 12 };
        let length = count(&mut view, size, "the spec count exceeds the section")?;
        let mut out = ctx.alloc_filled(length, empty, "usdc_specs")?;
        for slot in &mut out {
            if padded {
                view.req_take(4)?;
            }
            *slot = Spec {
                path: index(u64::from(view.req_u32_le()?), "a spec path")?,
                field_set: index(u64::from(view.req_u32_le()?), "a spec field set")?,
                kind: view.req_u32_le()?,
            };
        }
        return Ok(out);
    }
    let length = compressed_count(&mut view, "the spec count exceeds the section")?;
    let paths = compressed_u32(ctx, &mut view, length)?;
    let sets = compressed_u32(ctx, &mut view, length)?;
    let types = compressed_u32(ctx, &mut view, length)?;
    let mut out = ctx.alloc_filled(length, empty, "usdc_specs")?;
    for (index_in, slot) in out.iter_mut().enumerate() {
        *slot = Spec {
            path: index(u64::from(paths[index_in]), "a spec path")?,
            field_set: index(u64::from(sets[index_in]), "a spec field set")?,
            kind: types[index_in],
        };
    }
    Ok(out)
}

/// Sets path `index` to `node`, refusing an index outside the table or a
/// path stated twice.
fn place(
    paths: &mut [Option<PathNode>],
    index_value: usize,
    node: PathNode,
) -> Result<(), CodecError> {
    let slot = paths
        .get_mut(index_value)
        .ok_or_else(|| malformed("a path index exceeds the path table"))?;
    if slot.is_some() {
        return Err(malformed("a path index repeats"));
    }
    *slot = Some(node);
    Ok(())
}

/// The path items of versions before 0.4.0: a tree in depth-first order,
/// each item a header, followed by the offset of its sibling when it has
/// both a child and a sibling.
fn uncompressed_paths(
    file: View<'_>,
    version: Version,
    section: View<'_>,
    tokens: usize,
    paths: &mut [Option<PathNode>],
) -> Result<(), CodecError> {
    let padded = version < UNPADDED_RECORDS;
    let mut view = section;
    // Pending items: the reader position and the parent path.
    let mut stack: Vec<(usize, Option<usize>)> = vec![(view.position(), None)];
    let mut visited = 0_usize;
    while let Some((position, parent)) = stack.pop() {
        view.seek(position)
            .ok_or_else(|| malformed("a path sibling offset is past the section"))?;
        let mut parent = parent;
        loop {
            visited = visited
                .checked_add(1)
                .filter(|visited| *visited <= paths.len())
                .ok_or_else(|| malformed("the path tree has more items than paths"))?;
            if padded {
                view.req_take(4)?;
            }
            let header = view.req_take(usdc_path_item::LEN)?;
            let path_index = View::u32_le_at(header, usdc_path_item::INDEX)
                .ok_or_else(|| malformed("a path item is truncated"))?;
            let element = View::u32_le_at(header, usdc_path_item::ELEMENT_TOKEN)
                .ok_or_else(|| malformed("a path item is truncated"))?;
            let bits = header[usdc_path_item::BITS];
            let path_index = index(u64::from(path_index), "a path index")?;
            let element = index(u64::from(element), "a path element")?;
            match parent {
                None => place(
                    paths,
                    path_index,
                    PathNode {
                        parent: None,
                        element: 0,
                        property: false,
                    },
                )?,
                Some(parent) => {
                    if element >= tokens {
                        return Err(malformed("a path element names no token"));
                    }
                    place(
                        paths,
                        path_index,
                        PathNode {
                            parent: Some(parent),
                            element,
                            property: bits & IS_PROPERTY != 0,
                        },
                    )?;
                }
            }
            let child = bits & HAS_CHILD != 0;
            let sibling = bits & HAS_SIBLING != 0;
            if child {
                if sibling {
                    let offset = view.req_i64_le()?;
                    let offset = usize::try_from(offset)
                        .map_err(|_| malformed("a path sibling offset is negative"))?;
                    stack.push((absolute(file, offset)?, parent));
                }
                parent = Some(path_index);
            } else if !sibling {
                break;
            }
        }
    }
    Ok(())
}

/// The compressed path table of 0.4.0 and later: path indices, element
/// token indices (negative for a property), and jumps.
fn compressed_paths<'a>(
    ctx: &DecodeContext<'a>,
    section: View<'a>,
    tokens: usize,
    paths: &mut [Option<PathNode>],
) -> Result<(), CodecError> {
    let mut view = section;
    let encoded = compressed_count(&mut view, "the encoded path count exceeds the section")?;
    let indices = compressed_u32(ctx, &mut view, encoded)?;
    let elements = compressed_i32(ctx, &mut view, encoded)?;
    let jumps = compressed_i32(ctx, &mut view, encoded)?;
    // Pending runs: the item index and the parent path.
    let mut stack: Vec<(usize, Option<usize>)> = vec![(0, None)];
    let mut visited = 0_usize;
    while let Some((start, parent)) = stack.pop() {
        let mut current = start;
        let mut parent = parent;
        loop {
            visited = visited
                .checked_add(1)
                .filter(|visited| *visited <= encoded)
                .ok_or_else(|| malformed("the path jumps revisit an item"))?;
            let path_index = indices
                .get(current)
                .copied()
                .ok_or_else(|| malformed("a path jump leaves the table"))?;
            let path_index = index(u64::from(path_index), "a path index")?;
            match parent {
                None => place(
                    paths,
                    path_index,
                    PathNode {
                        parent: None,
                        element: 0,
                        property: false,
                    },
                )?,
                Some(parent) => {
                    let element = elements[current];
                    let token = index(element.unsigned_abs(), "a path element")?;
                    if token >= tokens {
                        return Err(malformed("a path element names no token"));
                    }
                    place(
                        paths,
                        path_index,
                        PathNode {
                            parent: Some(parent),
                            element: token,
                            property: element < 0,
                        },
                    )?;
                }
            }
            let jump = jumps[current];
            let child = jump > 0 || jump == -1;
            let sibling = jump >= 0;
            if child {
                if sibling {
                    let target = usize::try_from(jump)
                        .ok()
                        .and_then(|jump| current.checked_add(jump))
                        .ok_or_else(|| malformed("a path jump overflows"))?;
                    stack.push((target, parent));
                }
                parent = Some(path_index);
            } else if !sibling {
                break;
            }
            current = current
                .checked_add(1)
                .ok_or_else(|| malformed("a path item overflows"))?;
        }
    }
    Ok(())
}

fn paths<'a>(
    ctx: &DecodeContext<'a>,
    file: View<'a>,
    version: Version,
    section: View<'a>,
    tokens: usize,
) -> Result<Vec<Option<PathNode>>, CodecError> {
    let mut view = section;
    // Each path is at least one coded integer of each of the three
    // sequences, or one item header.
    let total = if version < COMPRESSED_STRUCTURE {
        count(
            &mut view,
            usdc_path_item::LEN,
            "the path count exceeds the section",
        )?
    } else {
        compressed_count(&mut view, "the path count exceeds the section")?
    };
    let mut out = ctx.alloc_filled(total, None, "usdc_paths")?;
    if total == 0 {
        return Ok(out);
    }
    if version < COMPRESSED_STRUCTURE {
        uncompressed_paths(file, version, view, tokens, &mut out)?;
    } else {
        compressed_paths(ctx, view, tokens, &mut out)?;
    }
    Ok(out)
}

/// Reads the bootstrap header, the table of contents, and the structural
/// sections of the USDC layer `file`.
pub(super) fn read<'a>(
    ctx: &DecodeContext<'a>,
    file: View<'a>,
) -> Result<Structure<'a>, CodecError> {
    let bytes = file.window();
    if bytes.len() < bootstrap::LEN {
        return Err(malformed("the bootstrap header is truncated"));
    }
    let version = [
        bytes[bootstrap::MAJOR],
        bytes[bootstrap::MINOR],
        bytes[bootstrap::PATCH],
    ];
    let sections = table_of_contents(file)?;
    let section = |name: &str| {
        sections
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, view)| *view)
    };
    let tokens = match section("TOKENS") {
        Some(view) => tokens(ctx, version, view)?,
        None => Vec::new(),
    };
    let strings = match section("STRINGS") {
        Some(view) => {
            let mut view = view;
            u32_vector(ctx, &mut view, "usdc_strings")?
                .into_iter()
                .map(|token| index(u64::from(token), "a string token"))
                .collect::<Result<Vec<_>, _>>()?
        }
        None => Vec::new(),
    };
    if strings.iter().any(|token| *token >= tokens.len()) {
        return Err(malformed("a string names no token"));
    }
    let fields = match section("FIELDS") {
        Some(view) => fields(ctx, version, view)?,
        None => Vec::new(),
    };
    if fields.iter().any(|field| field.token >= tokens.len()) {
        return Err(malformed("a field names no token"));
    }
    let field_sets = match section("FIELDSETS") {
        Some(view) => field_sets(ctx, version, view)?,
        None => Vec::new(),
    };
    if field_sets.iter().any(|field| {
        *field != FIELD_RUN_END
            && index(u64::from(*field), "a field index").is_ok_and(|field| field >= fields.len())
    }) {
        return Err(malformed("a field set names no field"));
    }
    let paths = match section("PATHS") {
        Some(view) => paths(ctx, file, version, view, tokens.len())?,
        None => Vec::new(),
    };
    let specs = match section("SPECS") {
        Some(view) => specs(ctx, version, view)?,
        None => Vec::new(),
    };
    for spec in &specs {
        if paths.get(spec.path).is_none_or(Option::is_none)
            || spec.field_set >= field_sets.len().max(1)
        {
            return Err(malformed("a spec names no path or field set"));
        }
    }
    Ok(Structure {
        version,
        file,
        tokens,
        strings,
        fields,
        field_sets,
        paths,
        specs,
    })
}

#[cfg(test)]
mod tests;
