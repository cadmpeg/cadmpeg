// SPDX-License-Identifier: Apache-2.0
//! Source identity shared by ASM-native record projections.

use crate::ids::IdFormat;

/// Source namespace of an ASM-native record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeRecordNamespace {
    namespace: String,
}

impl NativeRecordNamespace {
    /// Identify the namespace of an unqualified ASM stream.
    #[must_use]
    pub fn new(format: IdFormat<'_>) -> Self {
        Self {
            namespace: format!("{format}:asm"),
        }
    }

    pub(super) fn id(&self, kind: &str, record_index: u32) -> String {
        format!("{}:{kind}#{}", self.namespace, record_index)
    }

    pub(super) fn from_wire(id: &str, record_index: u32, kind: &str) -> Result<Self, String> {
        let suffix = format!(":{kind}#{record_index}");
        let namespace = id.strip_suffix(&suffix).ok_or_else(|| {
            format!("native id does not match {kind} record_index {record_index}")
        })?;
        Ok(Self {
            namespace: namespace.to_owned(),
        })
    }
}
