// SPDX-License-Identifier: Apache-2.0
//! Identity of the format that owns entity IDs decoded from an ASM stream.

use cadmpeg_ir::ids::{
    Identity, IdentityComponent, IdentityKey, IdentityNamespace, StaticIdentityComponent,
};

/// A validated static format component of an ASM entity identity.
///
/// ASM callers use format literals owned by the codec. Keeping the proof in
/// this private-field wrapper prevents an arbitrary string from entering the
/// identity namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IdFormat {
    proof: StaticIdentityComponent,
}

impl IdFormat {
    /// Construct a format from a component whose grammar was admitted during
    /// const evaluation.
    #[must_use]
    pub const fn from_static(proof: StaticIdentityComponent) -> Self {
        Self { proof }
    }

    /// Admit a format literal, returning `None` when its grammar is invalid.
    #[must_use]
    pub const fn from_literal(value: &'static str) -> Option<Self> {
        match StaticIdentityComponent::new(value) {
            Some(proof) => Some(Self { proof }),
            None => None,
        }
    }

    /// Construct the namespace used by BREP identities.
    #[must_use]
    pub(crate) fn brep_namespace(self, kind: &IdentityComponent) -> IdentityNamespace {
        let format = IdentityComponent::from_static(self.proof);
        let scope = cadmpeg_ir::identity_component!("brep");
        IdentityNamespace::from_components(&format, &scope, kind)
    }

    /// Compose an identity under a static BREP namespace.
    #[must_use]
    pub(crate) fn brep_identity(
        self,
        kind: &IdentityComponent,
        key: impl Into<IdentityKey>,
    ) -> Identity {
        Identity::compose(&self.brep_namespace(kind), key)
    }
}

impl std::fmt::Display for IdFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(IdentityComponent::from_static(self.proof).as_str())
    }
}

/// Admit an ASM format literal at compile time.
///
/// The literal is checked while the expansion's `const` proof is evaluated;
/// runtime text must enter through a typed source admission path instead of
/// pretending to be a static format.
#[macro_export]
macro_rules! asm_format {
    ($value:literal) => {{
        const STATIC_FORMAT: $crate::ids::IdFormat =
            match $crate::ids::IdFormat::from_literal($value) {
                Some(format) => format,
                None => panic!("ASM identity format literal has invalid grammar"),
            };
        STATIC_FORMAT
    }};
}

/// Compose a typed id under `format:brep:<kind>#<key>`.
macro_rules! brep_id {
    ($format:expr, $type:ty, $kind:literal, $key:expr) => {{
        <$type>::compose(
            &$format.brep_namespace(&cadmpeg_ir::identity_component!($kind)),
            $key,
        )
    }};
}

pub(crate) use brep_id;

/// Build a key from literal and numeric parts without reopening its grammar.
macro_rules! brep_key {
    ($value:literal, $($rest:tt)+) => {{
        cadmpeg_ir::identity_key!($value).then($crate::ids::brep_key!($($rest)+))
    }};
    ($value:literal) => {
        cadmpeg_ir::identity_key!($value)
    };
    ($value:expr, $($rest:tt)+) => {{
        cadmpeg_ir::ids::IdentityKey::from($value).then($crate::ids::brep_key!($($rest)+))
    }};
    ($value:expr) => {
        cadmpeg_ir::ids::IdentityKey::from($value)
    };
}

pub(crate) use brep_key;
