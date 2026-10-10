// SPDX-License-Identifier: Apache-2.0
//! Per-arena export coverage declared by every write path.
//!
//! A backend states, for each model arena, whether its payload represents the
//! arena. The sealed wrapper charges one export loss for every non-empty arena
//! the payload omits, so a drop is reported even when the backend has no code
//! path that knows about the arena.

use crate::document::Model;
use crate::report::loss::LossNote;
use crate::schema::{EntityKind, EntitySchema};

use super::loss::ExportLossCode;

/// What one write path does with one model arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArenaDisposition {
    /// The payload represents every record of the arena.
    Written,
    /// The backend refuses the arena or charges its own specific export loss
    /// for every record it does not represent.
    Reported,
    /// The payload does not represent the arena; the wrapper charges a loss
    /// when it is non-empty.
    Omitted,
}

/// Which model arenas a payload carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArenaCoverage {
    /// The payload carries the whole neutral document by construction: a
    /// verbatim replay or a patch admitted only when the document is unchanged
    /// from decode, or a serialization of the whole document.
    Complete,
    /// The payload carries the arenas this per-arena declaration names.
    Declared(ArenaDispositions),
}

impl ArenaCoverage {
    /// One loss per non-empty arena this coverage omits.
    pub(super) fn omission_losses(&self, model: &Model, format: &str) -> Vec<LossNote> {
        match self {
            Self::Complete => Vec::new(),
            Self::Declared(dispositions) => dispositions.omission_losses(model, format),
        }
    }
}

fn arena_kind<T: EntitySchema>(_: &[T]) -> EntityKind {
    T::KIND
}

macro_rules! declare_arena_dispositions {
    ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        /// One disposition for every model arena.
        ///
        /// Every field is required and the type has no default, so a new arena
        /// fails to compile in every writer until that writer declares it.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct ArenaDispositions {
            $(
                #[doc = concat!("Disposition of the `", stringify!($field), "` arena.")]
                pub $field: ArenaDisposition,
            )*
        }

        impl ArenaDispositions {
            fn omission_losses(&self, model: &Model, format: &str) -> Vec<LossNote> {
                let mut losses = Vec::new();
                $(
                    if self.$field == ArenaDisposition::Omitted && !model.$field.is_empty() {
                        losses.push(
                            ExportLossCode::ArenaOmitted(arena_kind(&model.$field))
                            .note(format!(
                                "the {format} write does not represent model arena `{}`: \
                                 {} record(s) omitted",
                                stringify!($field),
                                model.$field.len(),
                            )),
                        );
                    }
                )*
                losses
            }
        }
    };
}

crate::document::arena_registry!(declare_arena_dispositions);
