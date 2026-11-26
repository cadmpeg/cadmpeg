// SPDX-License-Identifier: Apache-2.0
//! `Brep Data` kernel record tree.

use cadmpeg_core::decode::{u64_from_index, ParseError, View};
use cadmpeg_core::CodecError;

use crate::layout::{kernel_record_header as record, token, tree_node_header as node};

#[derive(Debug)]
pub(crate) struct TreeError(CodecError);

impl From<ParseError> for TreeError {
    fn from(error: ParseError) -> Self {
        Self(error.into())
    }
}

impl From<TreeError> for CodecError {
    fn from(error: TreeError) -> Self {
        error.0
    }
}

fn malformed(message: impl Into<String>) -> TreeError {
    TreeError(CodecError::Malformed(message.into()))
}

/// Modelling kernel named by a record header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kernel {
    Acis,
    Parasolid,
    Other(u32),
}

impl Kernel {
    fn from_tag(tag: u32) -> Self {
        match tag {
            token::KERNEL_ACIS => Self::Acis,
            token::KERNEL_PARASOLID => Self::Parasolid,
            other => Self::Other(other),
        }
    }
}

/// One kernel record of the tree.
#[derive(Debug, Clone, Copy)]
pub(crate) struct KernelRecord<'a> {
    pub(crate) kernel: Kernel,
    /// Node nesting depth; a record in the root node has depth 1.
    pub(crate) depth: usize,
    /// Header words at `words` and `reserved` equal `1, 1, 1, 0`.
    pub(crate) canonical_header: bool,
    pub(crate) bytes: View<'a>,
}

/// Reads every kernel record of a `Brep Data` payload in stored order.
pub(crate) fn read(payload: View<'_>) -> Result<Vec<KernelRecord<'_>>, TreeError> {
    let mut cursor = payload;
    let mut ends = vec![payload.end()];
    let mut records = Vec::new();
    loop {
        while ends.last() == Some(&cursor.position()) {
            ends.pop();
        }
        let Some(&end) = ends.last() else {
            break;
        };
        let at = cursor.position();
        if View::u32_le_at(cursor.unread(), record::MARKER) == Some(record::MARKER_VALUE) {
            let length = cursor.req_u32_le()?;
            let length = cursor.counted(u64::from(length), 1).ok_or_else(|| {
                malformed(format!("kernel record length {length} exceeds the payload"))
            })?;
            cursor.req_take(length.get())?;
            if cursor.position() > end {
                return Err(malformed("kernel record crosses its node end"));
            }
            let whole = payload
                .child(at, cursor.position())
                .ok_or_else(|| malformed("kernel record escapes the payload"))?;
            records.push(kernel_record(whole, ends.len() - 1)?);
        } else {
            let length = cursor.req_u64_le()?;
            let node_end = u64_from_index(at)
                .checked_add(length)
                .filter(|node_end| {
                    length >= u64_from_index(node::LEN) && *node_end <= u64_from_index(end)
                })
                .ok_or_else(|| malformed(format!("tree node length {length} is out of range")))?;
            let node_end = usize::try_from(node_end)
                .map_err(|_| malformed("tree node end does not fit memory"))?;
            ends.push(node_end);
        }
    }
    Ok(records)
}

fn kernel_record(whole: View<'_>, depth: usize) -> Result<KernelRecord<'_>, TreeError> {
    let header = whole.window();
    let at = whole.start();
    let field = |offset: usize| {
        View::u32_le_at(header, offset)
            .ok_or_else(|| malformed(format!("kernel record at {at} is shorter than its header")))
    };
    let tag = field(record::KERNEL)?;
    let words = [
        field(record::WORDS)?,
        field(record::WORDS + 4)?,
        field(record::WORDS + 8)?,
        field(record::RESERVED)?,
    ];
    let bytes = whole
        .child(at + record::LEN, whole.end())
        .ok_or_else(|| malformed(format!("kernel record at {at} is shorter than its header")))?;
    Ok(KernelRecord {
        kernel: Kernel::from_tag(tag),
        depth,
        canonical_header: words == [1, 1, 1, 0],
        bytes,
    })
}

#[cfg(test)]
mod tests;
