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
    /// The item before the record in its node is a node that holds records.
    pub(crate) follows_node: bool,
    /// Header words at `words` and `reserved` equal `1, 1, 1, 0`.
    pub(crate) canonical_header: bool,
    /// The assembly that holds the record's pair, as an index into
    /// [`Tree::assemblies`].
    pub(crate) assembly: Option<usize>,
    pub(crate) bytes: View<'a>,
}

/// The kernel records of a `Brep Data` payload and the assemblies that hold
/// their pairs.
#[derive(Debug)]
pub(crate) struct Tree<'a> {
    /// The kernel records in stored order.
    pub(crate) records: Vec<KernelRecord<'a>>,
    /// The nodes below the scene root that hold pairs, in the order they end.
    pub(crate) assemblies: Vec<Assembly>,
}

/// A node below the scene root that holds pairs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Assembly {
    /// The assembly that holds this node, as an index into
    /// [`Tree::assemblies`].
    pub(crate) parent: Option<usize>,
}

/// One node below the scene root, in the order the nodes open.
struct Node {
    parent: Option<usize>,
    holds_pair: bool,
}

/// One open node of the walk.
struct Level {
    end: usize,
    /// The node below the scene root that the level reads, in opening order.
    node: Option<usize>,
    /// The node holds a record at any depth.
    holds_record: bool,
    /// The last closed item of the node is a node that holds records.
    after_node: bool,
}

impl Level {
    const fn new(end: usize, node: Option<usize>) -> Self {
        Self {
            end,
            node,
            holds_record: false,
            after_node: false,
        }
    }
}

/// Reads every kernel record of a `Brep Data` payload in stored order, and
/// the assemblies that hold their pairs. The node that the payload holds is
/// the scene root.
pub(crate) fn read(payload: View<'_>) -> Result<Tree<'_>, TreeError> {
    let mut cursor = payload;
    let mut levels = vec![Level::new(payload.end(), None)];
    let mut records = Vec::new();
    let mut nodes: Vec<Node> = Vec::new();
    let mut closed_nodes = Vec::new();
    loop {
        while levels
            .last()
            .is_some_and(|level| level.end == cursor.position())
        {
            let Some(closed) = levels.pop() else {
                break;
            };
            closed_nodes.extend(closed.node);
            if let Some(parent) = levels.last_mut() {
                parent.holds_record |= closed.holds_record;
                parent.after_node = closed.holds_record;
            }
        }
        let Some(end) = levels.last().map(|level| level.end) else {
            break;
        };
        let at = cursor.position();
        if View::u32_le_at(cursor.unread(), record::MARKER).is_some_and(|marker| marker != 0) {
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
            let mut record = kernel_record(whole)?;
            if let Some(level) = levels.last_mut() {
                record.follows_node = level.after_node;
                if record.follows_node {
                    if let Some(node) = level.node {
                        nodes[node].holds_pair = true;
                        record.assembly = Some(node);
                    }
                }
                level.holds_record = true;
                level.after_node = false;
            }
            records.push(record);
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
            let parent = levels.last_mut().and_then(|level| {
                level.after_node = false;
                level.node
            });
            let node = (levels.len() > 1).then(|| {
                nodes.push(Node {
                    parent,
                    holds_pair: false,
                });
                nodes.len() - 1
            });
            levels.push(Level::new(node_end, node));
        }
    }
    Ok(assemble(records, &nodes, &closed_nodes))
}

/// Numbers the nodes that hold pairs in the order they end, and refers each
/// record and each assembly to its assembly by that number.
fn assemble<'a>(mut records: Vec<KernelRecord<'a>>, nodes: &[Node], closed: &[usize]) -> Tree<'a> {
    let mut number = vec![None; nodes.len()];
    let mut order = Vec::new();
    for &node in closed.iter().filter(|&&node| nodes[node].holds_pair) {
        number[node] = Some(order.len());
        order.push(node);
    }
    let holder = |mut node: Option<usize>| {
        while let Some(at) = node {
            if number[at].is_some() {
                return number[at];
            }
            node = nodes[at].parent;
        }
        None
    };
    let assemblies = order
        .iter()
        .map(|&node| Assembly {
            parent: holder(nodes[node].parent),
        })
        .collect();
    for record in &mut records {
        record.assembly = record.assembly.and_then(|node| number[node]);
    }
    Tree {
        records,
        assemblies,
    }
}

fn kernel_record(whole: View<'_>) -> Result<KernelRecord<'_>, TreeError> {
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
        follows_node: false,
        canonical_header: words == [1, 1, 1, 0],
        assembly: None,
        bytes,
    })
}

#[cfg(test)]
mod tests;
