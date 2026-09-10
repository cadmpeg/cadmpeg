// SPDX-License-Identifier: Apache-2.0
//! The one door that mints the identities the NX decoder creates.

use cadmpeg_ir::ids::Identity;
use std::fmt::Display;

/// The `<format>:<scope>` half of every identity the NX decoder mints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IdScope(String);

impl IdScope {
    /// The NX scope of the given name.
    pub(crate) fn native(scope: impl Display) -> Self {
        Self(format!("nx:{scope}"))
    }

    /// The scope of one Parasolid stream of the container.
    pub(crate) fn stream(stream_index: impl Display) -> Self {
        Self::native(format_args!("s{stream_index}"))
    }

    /// The scope of whole-stream container evidence.
    pub(crate) fn container() -> Self {
        Self::native("container")
    }

    /// The scope of entities the decoder synthesizes from non-topology input.
    pub(crate) fn derived() -> Self {
        Self::native("derived")
    }

    /// The scope an existing identity was minted under.
    pub(crate) fn of(prefix: &str) -> Self {
        Self(prefix.to_owned())
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

    /// Mint `<scope>:<kind>#<key>`, declining a key that leaves the grammar.
    pub(crate) fn try_id<T: From<Identity>>(&self, kind: &str, key: impl Display) -> Option<T> {
        Identity::new(format!("{}:{kind}#{key}", self.0))
            .ok()
            .map(Into::into)
    }
}

/// Mint an identity that extends an existing entity id's key with a suffix.
pub(crate) fn extended_id<T: From<Identity>>(base: &str, suffix: &str) -> T {
    Identity::new(format!("{base}:{suffix}"))
        .expect("identity grammar")
        .into()
}
