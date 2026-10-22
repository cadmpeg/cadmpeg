// SPDX-License-Identifier: Apache-2.0
//! Read-only source owners and neutral links for B1 class derivation.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::document::{ArenaClass, ArenaClasses, CadIr};
use crate::ids::Identity;
use crate::schema::{EntityKind, EntitySchema};

/// A neutral record's typed native link and registered arena class.
#[derive(Debug, Clone, Copy)]
pub struct NeutralProvenance<'a> {
    id: &'a str,
    kind: EntityKind,
    class: ArenaClass,
    native_ref: Option<&'a str>,
}

impl<'a> NeutralProvenance<'a> {
    /// Neutral record identity.
    pub fn id(&self) -> &'a str {
        self.id
    }
    /// Neutral arena kind.
    pub fn kind(&self) -> EntityKind {
        self.kind
    }
    /// Class fixed by the neutral arena registry.
    pub fn class(&self) -> ArenaClass {
        self.class
    }
    /// Native record named by this neutral record.
    pub fn native_ref(&self) -> Option<&'a str> {
        self.native_ref
    }
}

/// A native record's declared owner and reserved unknown-stub links.
#[derive(Debug)]
pub struct NativeProvenance<'a> {
    id: &'a str,
    namespace: &'a str,
    arena: &'a str,
    owner: Option<Identity>,
    unknown_links: Vec<Identity>,
}

impl<'a> NativeProvenance<'a> {
    /// Native record identity.
    pub fn id(&self) -> &'a str {
        self.id
    }
    /// Source-format namespace.
    pub fn namespace(&self) -> &'a str {
        self.namespace
    }
    /// Codec-owned native arena name.
    pub fn arena(&self) -> &'a str {
        self.arena
    }
    /// Direct owner named by the native record's owner field.
    pub fn owner(&self) -> Option<&str> {
        self.owner.as_ref().map(Identity::as_str)
    }
    /// Exact related records named by a reserved unknown stub.
    pub fn unknown_links(&self) -> &[Identity] {
        &self.unknown_links
    }
    /// Whether this record is demonstrably identity-only.
    /// Other native families require their codec's typed field classification.
    pub fn identity_only(&self) -> Option<bool> {
        (self.arena == "unknowns").then_some(true)
    }
}

fn entity_kind<T: EntitySchema>(_: &T) -> EntityKind {
    T::KIND
}

impl CadIr {
    /// Visits every neutral arena through its typed schema and class registry.
    pub fn visit_neutral_provenance<'ir>(
        &'ir self,
        visitor: &mut dyn FnMut(NeutralProvenance<'ir>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        macro_rules! visit {
            ($($field:ident: $ty:ty, $class:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
                $(for record in &self.model.$field {
                    visitor(NeutralProvenance {
                        id: record.identity(),
                        kind: entity_kind(record),
                        class: crate::document::arena_class!($class),
                        native_ref: record.native_reference(),
                    })?;
                })*
            };
        }
        crate::document::classified_arena_registry!(visit);
        Ok(())
    }

    /// Visits every native namespace and arena without copying record fields.
    pub fn visit_native_provenance<'ir>(
        &'ir self,
        ctx: &DecodeContext<'_>,
        visitor: &mut dyn FnMut(NativeProvenance<'ir>) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        for (namespace, records) in &self.native.0 {
            for (arena, records) in records.arenas() {
                for record in records {
                    let unknown_links = if arena == "unknowns" {
                        record.provenance_unknown(ctx)?.links
                    } else {
                        Vec::new()
                    };
                    visitor(NativeProvenance {
                        id: record.id(),
                        namespace,
                        arena,
                        owner: record.provenance_owner()?,
                        unknown_links,
                    })?;
                }
            }
        }
        Ok(())
    }

    /// Union of the owner's own neutral class and exact reverse native links.
    pub fn provenance_classes(&self, owner: &str) -> ArenaClasses {
        let mut classes = ArenaClasses::new();
        macro_rules! collect {
            ($($field:ident: $ty:ty, $class:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
                $(if self.model.$field.iter().any(|record| {
                    record.identity() == owner || record.native_reference() == Some(owner)
                }) {
                    classes.insert(crate::document::arena_class!($class));
                })*
            };
        }
        crate::document::classified_arena_registry!(collect);
        classes
    }
}

#[cfg(test)]
mod tests;
