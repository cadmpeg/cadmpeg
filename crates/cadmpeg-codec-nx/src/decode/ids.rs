// SPDX-License-Identifier: Apache-2.0
//! The one door that mints the identities the NX decoder creates.

use cadmpeg_ir::ids::Identity;
use std::fmt::Display;

/// The `<format>:<scope>` half of every identity the NX decoder mints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IdScope(String);

impl IdScope {
    /// The scope of one Parasolid stream of the container.
    pub(crate) fn stream(stream_index: usize) -> Self {
        Self(format!("nx:s{stream_index}"))
    }

    /// The scope of whole-stream container evidence.
    pub(crate) fn container() -> Self {
        Self("nx:container".to_owned())
    }

    /// The scope of entities the decoder synthesizes from non-topology input.
    pub(crate) fn derived() -> Self {
        Self("nx:derived".to_owned())
    }

    /// Borrow the scope prefix.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// Mint `<scope>:<kind>#<key>`.
    pub(crate) fn id<T: From<Identity>>(&self, kind: &str, key: impl Display) -> T {
        Identity::new(format!("{}:{kind}#{key}", self.0))
            .expect("identity grammar")
            .into()
    }
}
