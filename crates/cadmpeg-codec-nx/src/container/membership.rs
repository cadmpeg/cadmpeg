// SPDX-License-Identifier: Apache-2.0
//! Members whose count fits the `RMFastLoad` unsigned 32-bit count word.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ObjectIdMembers<T> {
    members: Vec<T>,
    count: u32,
}

impl<T> ObjectIdMembers<T> {
    pub(crate) fn new(members: Vec<T>) -> Result<Self, &'static str> {
        let count = u32::try_from(members.len()).map_err(|_| "members: count exceeds u32")?;
        Ok(Self { members, count })
    }

    pub(crate) fn count(&self) -> u32 {
        self.count
    }
    pub(crate) fn as_slice(&self) -> &[T] {
        &self.members
    }
    #[cfg(test)]
    pub(crate) fn into_vec(self) -> Vec<T> {
        self.members
    }
}
