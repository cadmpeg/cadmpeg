// SPDX-License-Identifier: Apache-2.0
//! Identities of the IR entities a package decodes to.
//!
//! Every identity is `pvz:model:<kind>#<key>`. Definition keys are
//! definition indexes, occurrence keys are occurrence paths, and the keys of
//! geometry-member entities start with the encoded member name, so one member
//! names the same entities in every decode.

use cadmpeg_ir::ids::{
    AppearanceBindingId, AppearanceId, BodyId, IdentityKey, OccurrenceId, ProductDefinitionId,
};
use cadmpeg_ir::tessellation::TessellationId;

/// Key text for one geometry member.
fn member_key(member: &str) -> IdentityKey {
    IdentityKey::encode_segment(member)
}

/// Identity scope of the B-rep records of `member`. The body the B-rep
/// writer names under this scope is [`body`] of `member`.
pub(crate) fn member_scope(member: &str) -> IdentityKey {
    member_key(member)
}

/// The definition at `index`.
pub(crate) fn definition(index: usize) -> ProductDefinitionId {
    ProductDefinitionId::compose(
        &cadmpeg_ir::identity_namespace!("pvz", "model", "definition"),
        index,
    )
}

/// The occurrence at occurrence path `path`.
pub(crate) fn occurrence(path: &str) -> OccurrenceId {
    OccurrenceId::compose(
        &cadmpeg_ir::identity_namespace!("pvz", "model", "occurrence"),
        IdentityKey::encode_segment(path),
    )
}

/// The body that holds the geometry of `member`.
pub(crate) fn body(member: &str) -> BodyId {
    BodyId::compose(
        &cadmpeg_ir::identity_namespace!("pvz", "model", "body"),
        member_key(member),
    )
}

/// Mesh `index` of `member`.
pub(crate) fn tessellation(member: &str, index: usize) -> TessellationId {
    TessellationId::compose(
        &cadmpeg_ir::identity_namespace!("pvz", "model", "tessellation"),
        member_key(member).colon(index),
    )
}

/// The appearance `member` defines for `base_index`.
pub(crate) fn appearance(member: &str, base_index: u32) -> AppearanceId {
    AppearanceId::compose(
        &cadmpeg_ir::identity_namespace!("pvz", "model", "appearance"),
        member_key(member).colon(base_index),
    )
}

/// The binding of mesh `index` of `member` to its appearance.
pub(crate) fn appearance_binding(member: &str, index: usize) -> AppearanceBindingId {
    AppearanceBindingId::compose(
        &cadmpeg_ir::identity_namespace!("pvz", "model", "appearance-binding"),
        member_key(member).colon(index),
    )
}
