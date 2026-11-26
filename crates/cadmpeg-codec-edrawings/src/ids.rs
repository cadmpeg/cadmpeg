// SPDX-License-Identifier: Apache-2.0
//! Identities of the IR entities a package decodes to.
//!
//! Every identity is `edrawings:model:<kind>#<key>`. Product definitions are
//! keyed by their index (parts first, in order of first use, then the
//! assembly root and sub-assemblies), occurrences by the index of their tree
//! node in stream order, so that an instance without an occurrence keeps the
//! id it would have. The B-rep entities of a body take their keys from the mesh B-rep
//! module, under the scope `body<index>`.

use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};

/// The product definition at `index`.
pub(crate) fn definition(index: usize) -> ProductDefinitionId {
    ProductDefinitionId::compose(
        &cadmpeg_ir::identity_namespace!("edrawings", "model", "definition"),
        index,
    )
}

/// The occurrence of the tree node at `index` in stream order.
pub(crate) fn occurrence(index: usize) -> OccurrenceId {
    OccurrenceId::compose(
        &cadmpeg_ir::identity_namespace!("edrawings", "model", "occurrence"),
        index,
    )
}

#[cfg(test)]
mod tests {
    use super::{definition, occurrence};

    #[test]
    fn identities_follow_the_index() {
        assert_ne!(definition(0), definition(1));
        assert_eq!(occurrence(3), occurrence(3));
    }
}
