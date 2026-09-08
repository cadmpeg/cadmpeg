// SPDX-License-Identifier: Apache-2.0
//! Members whose count fits the `RMFastLoad` unsigned 32-bit count word.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObjectIdMembers<T>(Vec<T>);

impl<T> ObjectIdMembers<T> {
    pub(crate) fn new(members: Vec<T>) -> Result<Self, &'static str> {
        u32::try_from(members.len()).map_err(|_| "members: count exceeds u32")?;
        Ok(Self(members))
    }

    pub(crate) fn count(&self) -> u32 {
        self.0.len() as u32
    }
    pub(crate) fn as_slice(&self) -> &[T] {
        &self.0
    }
    pub(crate) fn into_vec(self) -> Vec<T> {
        self.0
    }
}
