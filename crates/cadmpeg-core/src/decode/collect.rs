// SPDX-License-Identifier: Apache-2.0
//! Collections whose length is proven against the input.

use crate::CodecError;

use super::BoundedCount;

/// A vector that must contain exactly a count proven against input.
#[derive(Debug)]
pub struct ExactVec<T> {
    values: Vec<T>,
    capacity: usize,
}

impl<T> ExactVec<T> {
    /// Starts a vector that must hold exactly `count` values.
    #[must_use]
    pub fn new(count: BoundedCount) -> Self {
        Self {
            values: Vec::new(),
            capacity: count.get(),
        }
    }

    /// Appends one value without exceeding the bounded count.
    pub fn push(&mut self, value: T) -> Result<(), CodecError> {
        if self.values.len() == self.capacity {
            return Err(CodecError::Malformed(
                "fixed-capacity vector overflow".to_owned(),
            ));
        }
        self.values.push(value);
        Ok(())
    }

    /// Returns the values if the bounded count was filled exactly.
    pub fn finish(self) -> Result<Vec<T>, CodecError> {
        if self.values.len() == self.capacity {
            Ok(self.values)
        } else {
            Err(CodecError::malformed(format_args!(
                "fixed-capacity vector contains {} of {} values",
                self.values.len(),
                self.capacity
            )))
        }
    }
}
