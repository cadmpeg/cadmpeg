// SPDX-License-Identifier: Apache-2.0
//! Field access by schema name, shared by partition and Deltas node instances.

use super::deltas::node::DeltasNode;
use super::primary::Node;
use std::ops::Range;

/// The big-endian bytes of the null real.
pub(crate) const NULL_REAL: [u8; 8] = [0xc2, 0xbc, 0x92, 0x8f, 0x99, 0x6e, 0x00, 0x00];

/// A node instance whose fields read by schema name. A pointer reads as its
/// target node index.
pub trait NodeFields {
    /// Node type.
    fn kind(&self) -> u16;
    /// Encoded node index.
    fn index(&self) -> u32;
    /// Byte range of the instance, from its type tag.
    fn range(&self) -> Range<usize>;
    /// Byte range of the named field.
    fn field_range(&self, name: &str) -> Option<Range<usize>>;
    /// The target of a scalar pointer field.
    fn pointer(&self, name: &str) -> Option<u32>;
    /// The value of a scalar integer field.
    fn integer(&self, name: &str) -> Option<u32>;
    /// The value of a scalar byte field.
    fn byte(&self, name: &str) -> Option<u8>;
    /// The value of a scalar character field.
    fn char(&self, name: &str) -> Option<u8>;
    /// The value of a scalar real field.
    fn real(&self, name: &str) -> Option<f64>;
    /// Whether a scalar real field holds the null real.
    fn is_null(&self, name: &str) -> Option<bool>;
    /// The value of a scalar vector field.
    fn vector(&self, name: &str) -> Option<[f64; 3]>;

    /// The targets of the named scalar pointer fields, in order.
    fn pointers<const N: usize>(&self, names: [&str; N]) -> Option<[u32; N]> {
        let mut out = [0; N];
        for (value, name) in out.iter_mut().zip(names) {
            *value = self.pointer(name)?;
        }
        Some(out)
    }
}

macro_rules! delegate {
    ($node:ty) => {
        impl NodeFields for $node {
            fn kind(&self) -> u16 {
                <$node>::kind(self)
            }
            fn index(&self) -> u32 {
                <$node>::index(self)
            }
            fn range(&self) -> Range<usize> {
                <$node>::range(self)
            }
            fn field_range(&self, name: &str) -> Option<Range<usize>> {
                <$node>::field_range(self, name)
            }
            fn pointer(&self, name: &str) -> Option<u32> {
                <$node>::pointer(self, name)
            }
            fn integer(&self, name: &str) -> Option<u32> {
                <$node>::integer(self, name)
            }
            fn byte(&self, name: &str) -> Option<u8> {
                <$node>::byte(self, name)
            }
            fn char(&self, name: &str) -> Option<u8> {
                <$node>::char(self, name)
            }
            fn real(&self, name: &str) -> Option<f64> {
                <$node>::real(self, name)
            }
            fn is_null(&self, name: &str) -> Option<bool> {
                <$node>::is_null(self, name)
            }
            fn vector(&self, name: &str) -> Option<[f64; 3]> {
                <$node>::vector(self, name)
            }
        }
    };
}

delegate!(Node<'_>);
delegate!(DeltasNode<'_>);
