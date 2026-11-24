// SPDX-License-Identifier: Apache-2.0
//! Per-arena export coverage declared by every write path.
//!
//! A backend states, for each model arena, whether its payload represents the
//! arena. The sealed wrapper charges one export loss for every non-empty arena
//! the payload omits, so a drop is reported even when the backend has no code
//! path that knows about the arena.

use cadmpeg_core::decode::ArenaClass;

use crate::document::{ArenaHome, CadIr};
use crate::report::loss::LossNote;
use crate::schema::{ArenaRecord, EntityKind};

use super::loss::ExportLossCode;

/// What one write path does with one model arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArenaDisposition {
    /// The payload represents every record of the arena.
    Written,
    /// The payload may represent some records of the arena; the backend refuses
    /// the arena or charges its own specific export loss for every record it
    /// does not represent.
    Reported,
    /// The payload represents no record of the arena; the backend charges its
    /// own specific export loss when the arena is non-empty, or refuses it.
    Dropped,
    /// The payload does not represent the arena; the wrapper charges a loss
    /// when it is non-empty.
    Omitted,
    /// The payload never reads the arena. Its records are nests that hold
    /// states the other arenas do not, so no payload represents them and the
    /// wrapper charges no loss.
    Ignored,
}

impl ArenaDisposition {
    /// Whether the payload may carry a record of the arena.
    pub const fn carries(self) -> bool {
        matches!(self, Self::Written | Self::Reported)
    }
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
    /// Whether the payload may carry a record of this model arena.
    pub fn carries(&self, kind: EntityKind) -> bool {
        match self {
            Self::Complete => true,
            Self::Declared(dispositions) => dispositions.carries(kind),
        }
    }

    /// Whether the payload writes records of an arena the CADIR wire omits
    /// ([`crate::document::ArenaName::WIRE_OMITTED`]): a write that takes
    /// content from them, which a document read from CADIR JSON does not hold.
    pub(super) fn reads_wire_omitted(&self) -> bool {
        macro_rules! any_carried {
            ($($field:ident: $ty:ty, $class:ident, $home:ident;)*) => {
                |dispositions: &ArenaDispositions| false $(|| dispositions.$field.carries())*
            };
        }
        match self {
            Self::Complete => false,
            Self::Declared(dispositions) => {
                (crate::document::wire_omitted_arenas!(any_carried))(dispositions)
            }
        }
    }

    /// One loss per non-empty arena this coverage omits.
    pub(super) fn omission_losses(&self, ir: &CadIr, format: &str) -> Vec<LossNote> {
        match self {
            Self::Complete => Vec::new(),
            Self::Declared(dispositions) => dispositions.omission_losses(ir, format),
        }
    }
}

fn arena_kind<T: ArenaRecord>(_: &[T]) -> EntityKind {
    T::KIND
}

/// Whether a v10 design record restates the records of this design arena.
const fn has_v10_counterpart(kind: EntityKind) -> bool {
    matches!(
        kind,
        EntityKind::DesignScope
            | EntityKind::DesignContainer
            | EntityKind::DesignFeature
            | EntityKind::DesignSketch
            | EntityKind::DesignSketchEntity
            | EntityKind::DesignSketchConstraint
            | EntityKind::DesignSpatialSketch
            | EntityKind::DesignSpatialSketchEntity
            | EntityKind::DesignSpatialSketchConstraint
            | EntityKind::DesignParameter
            | EntityKind::DesignSpreadsheet
            | EntityKind::DesignConfiguration
    )
}

/// A disposition field of [`ArenaDispositions`]: the arenas of the `design`
/// and `process` homes are fields of its `design` and `process` members.
macro_rules! disposition {
    ($dispositions:expr, $field:ident, design) => {
        $dispositions.design.$field
    };
    ($dispositions:expr, $field:ident, process) => {
        $dispositions.process.$field
    };
    ($dispositions:expr, $field:ident, $home:ident) => {
        $dispositions.$field
    };
}

macro_rules! split_arena_dispositions {
    ([$($model:ident)*] [$($design:ident)*] [$($process:ident)*] $field:ident: design; $($rest:tt)*) => {
        split_arena_dispositions! { [$($model)*] [$($design)* $field] [$($process)*] $($rest)* }
    };
    ([$($model:ident)*] [$($design:ident)*] [$($process:ident)*] $field:ident: process; $($rest:tt)*) => {
        split_arena_dispositions! { [$($model)*] [$($design)*] [$($process)* $field] $($rest)* }
    };
    ([$($model:ident)*] [$($design:ident)*] [$($process:ident)*] $field:ident: $home:ident; $($rest:tt)*) => {
        split_arena_dispositions! { [$($model)* $field] [$($design)*] [$($process)*] $($rest)* }
    };
    ([$($model:ident)*] [$($design:ident)*] [$($process:ident)*]) => {
        /// One disposition for every arena.
        ///
        /// Every field is required and the type has no default, so a new arena
        /// fails to compile in every writer until that writer declares it.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct ArenaDispositions {
            $(
                #[doc = concat!("Disposition of the `", stringify!($model), "` arena.")]
                pub $model: ArenaDisposition,
            )*
            /// Dispositions of the arenas of the `design` home.
            pub design: DesignArenaDispositions,
            /// Dispositions of the arenas of the `process` home.
            pub process: ProcessArenaDispositions,
        }

        /// One disposition for every arena of the `process` home.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct ProcessArenaDispositions {
            $(
                #[doc = concat!("Disposition of the `process.", stringify!($process), "` arena.")]
                pub $process: ArenaDisposition,
            )*
        }

        impl ProcessArenaDispositions {
            /// One disposition for every arena of the `process` home.
            #[must_use]
            pub const fn uniform(disposition: ArenaDisposition) -> Self {
                Self { $($process: disposition,)* }
            }
        }

        /// One disposition for every arena of the `design` home.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct DesignArenaDispositions {
            $(
                #[doc = concat!("Disposition of the `design.", stringify!($design), "` arena.")]
                pub $design: ArenaDisposition,
            )*
        }

        impl DesignArenaDispositions {
            /// One disposition for every arena of the `design` home.
            #[must_use]
            pub const fn uniform(disposition: ArenaDisposition) -> Self {
                Self { $($design: disposition,)* }
            }
        }
    };
}

/// Adds the identities of the records of a `design` arena whose disposition
/// carries them to `$ids`; arenas of other homes add none.
macro_rules! written_design_ids {
    ($dispositions:expr, $ids:ident, $ir:expr, $field:ident, design) => {
        if $dispositions.design.$field.carries() {
            $ids.extend($ir.design.$field.iter().map(ArenaRecord::record_identity));
        }
    };
    ($dispositions:expr, $ids:ident, $ir:expr, $field:ident, $home:ident) => {};
}

macro_rules! declare_arena_dispositions {
    ($($field:ident: $ty:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
        split_arena_dispositions! { [] [] [] $($field: $home;)* }

        impl ArenaDispositions {
            /// Whether the payload may carry a record of this arena.
            pub fn carries(&self, kind: EntityKind) -> bool {
                [$(disposition!(self, $field, $home)),*]
                    .get(kind.index())
                    .is_none_or(|disposition| disposition.carries())
            }

            /// The label of each arena of `ir` whose records the write omits and
            /// names in an `arena.omitted` loss: a model arena by its field name,
            /// a design or process arena as `design.<field>` or `process.<field>`.
            #[must_use]
            pub fn omitted(&self, ir: &CadIr) -> Vec<&'static str> {
                self.charged_omissions(ir)
                    .into_iter()
                    .map(|(_, label, _)| label)
                    .collect()
            }

            fn omission_losses(&self, ir: &CadIr, format: &str) -> Vec<LossNote> {
                self.charged_omissions(ir)
                    .into_iter()
                    .map(|(kind, label, records)| {
                        ExportLossCode::ArenaOmitted(kind).note(format!(
                            "the {format} write does not represent model arena `{label}`: \
                             {records} record(s) omitted",
                        ))
                    })
                    .collect()
            }

            /// Each arena of `ir` whose records the write omits and charges: its
            /// kind, label and record count.
            /// The identities of the `design` records the payload writes.
            fn written_design_ids<'ir>(&self, ir: &'ir CadIr) -> std::collections::HashSet<&'ir str> {
                let mut ids = std::collections::HashSet::new();
                $(written_design_ids!(self, ids, ir, $field, $home);)*
                ids
            }

            fn charged_omissions(&self, ir: &CadIr) -> Vec<(EntityKind, &'static str, usize)> {
                // While the document holds a v10 design record, the v11 design
                // arenas with a v10 counterpart restate content the v10 path
                // writes.
                let restated = ir.arenas().holds(ArenaClass::Design, ArenaHome::Model);
                // A record of an arena the CADIR wire omits that a written
                // `design` record restates under its id is carried by that
                // record.
                let written_design = self.written_design_ids(ir);
                let mut charged = Vec::new();
                $(
                    let label = crate::document::arena_label!($field, $home);
                    // The document root is the written document itself.
                    let records = if arena_kind(&ir.$home.$field) == EntityKind::ProductDefinition {
                        crate::products::stated_definitions(&ir.model.product_definitions).count()
                    } else if crate::document::ArenaName::WIRE_OMITTED
                        .iter()
                        .any(|arena| arena.as_str() == label)
                    {
                        ir.$home.$field
                            .iter()
                            .filter(|record| !written_design.contains(record.record_identity()))
                            .count()
                    } else {
                        ir.$home.$field.len()
                    };
                    if disposition!(self, $field, $home) == ArenaDisposition::Omitted
                        && records != 0
                        && !(restated && has_v10_counterpart(arena_kind(&ir.$home.$field)))
                    {
                        charged.push((arena_kind(&ir.$home.$field), label, records));
                    }
                )*
                charged
            }
        }
    };
}

crate::document::every_arena_registry!(declare_arena_dispositions);

/// What a CADIR write does with each arena: it writes every arena but those
/// the wire omits ([`crate::document::ArenaName::WIRE_OMITTED`]), which the
/// wrapper names in an `arena.omitted` loss.
pub(super) fn cadir_dispositions() -> ArenaDispositions {
    macro_rules! written {
        ($($field:ident: $ty:ty, $home:ident, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {
            ArenaDispositions {
                $($field: ArenaDisposition::Written,)*
                design: DesignArenaDispositions::uniform(ArenaDisposition::Written),
                process: ProcessArenaDispositions::uniform(ArenaDisposition::Written),
            }
        };
    }
    macro_rules! omit {
        ($($field:ident: $ty:ty, $class:ident, $home:ident;)*) => {
            |dispositions: &mut ArenaDispositions| {
                $(dispositions.$field = ArenaDisposition::Omitted;)*
            }
        };
    }
    let mut dispositions = crate::document::arena_registry!(written);
    (crate::document::wire_omitted_arenas!(omit))(&mut dispositions);
    dispositions
}
