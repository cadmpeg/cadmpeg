// SPDX-License-Identifier: Apache-2.0
//! Serialize borrowed iterator columns without collecting them into vectors.

use serde::ser::SerializeSeq;
use serde::Serialize;

/// A serializable sequence over a cloneable iterator.
pub struct IterWire<I>(pub I);

impl<I> Serialize for IterWire<I>
where
    I: Iterator + Clone,
    I::Item: Serialize,
{
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let values = self.0.clone();
        let (lower, upper) = values.size_hint();
        let mut sequence = serializer.serialize_seq((upper == Some(lower)).then_some(lower))?;
        for value in values {
            sequence.serialize_element(&value)?;
        }
        sequence.end()
    }
}
