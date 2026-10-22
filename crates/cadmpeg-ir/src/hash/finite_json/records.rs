// SPDX-License-Identifier: Apache-2.0
//! Structural positions of arena records in the existing CADIR serializer.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecordPath {
    Root,
    Model,
    Native,
    Namespace,
    Records,
    Other,
}

impl RecordPath {
    pub(super) fn field(self, name: &str) -> Self {
        match (self, name) {
            (Self::Root, "model") => Self::Model,
            (Self::Root, "native") => Self::Native,
            (Self::Model, _) => Self::Records,
            _ => Self::Other,
        }
    }

    pub(super) const fn map_value(self) -> Self {
        match self {
            Self::Native => Self::Namespace,
            Self::Namespace => Self::Records,
            _ => Self::Other,
        }
    }
}

#[cfg(test)]
mod tests;
