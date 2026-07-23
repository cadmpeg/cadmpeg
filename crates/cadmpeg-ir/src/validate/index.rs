// SPDX-License-Identifier: Apache-2.0
//! Shared id lookup built once per validation run.
//!
//! [`ModelIndex`] holds one `id -> entity` map per model arena, the presence
//! set of native unknown-record ids, and the set of every entity id in the
//! document. It is generated from the same `arena_registry!` declaration as the
//! model itself, so a new arena is indexed without editing this module. Checks
//! probe the maps by id; they still iterate the arena `Vec`s for traversal, so
//! finding order never depends on a map's iteration order.
#![allow(clippy::wildcard_imports)] // Split checks share private orchestration context.

use super::*;
use crate::drawings::Drawing;
use crate::features::{DesignConfiguration, DesignParameter, FeatureInputTopology};
use crate::presentation::{PresentationDocument, ViewPresentation};
use crate::products::{AssemblyJoint, Component, Occurrence};
use crate::semantic_annotations::SemanticAnnotation;
use crate::sketches::{
    Sketch, SketchConstraint, SketchEntity, SpatialSketch, SpatialSketchConstraint,
    SpatialSketchEntity,
};
use crate::spreadsheets::Spreadsheet;
use crate::subd::SubdSurface;

macro_rules! define_model_index {
    ($( $field:ident: $element:ty, $doc:literal, [$($attribute:meta),*] => $key:expr; )*) => {
        /// Per-arena `id -> entity` maps plus the document-wide id sets, built
        /// once and probed by every reference check.
        ///
        /// Generated uniformly from `arena_registry!`, so a new arena is indexed
        /// with no edit here. Arenas whose references are checked only through
        /// [`all_ids`](Self::all_ids) (or not at all) still get a map they never
        /// read; the allow covers those until a by-id consumer needs them.
        #[allow(dead_code)]
        pub(crate) struct ModelIndex<'a> {
            $(
                #[doc = $doc]
                pub(crate) $field: std::collections::HashMap<String, &'a $element>,
            )*
            /// Ids of every native unknown record that deserializes. A namespace
            /// whose `unknowns` arena fails to convert contributes nothing here,
            /// matching the pre-index behavior.
            pub(crate) unknown_ids: std::collections::HashSet<String>,
            /// Every entity id in the document: model arenas, native records
            /// (including unknowns). Reference targets resolve against this set.
            pub(crate) all_ids: std::collections::HashSet<String>,
        }

        impl<'a> ModelIndex<'a> {
            /// Build the index for `ir` in one pass over each arena.
            pub(crate) fn build(ir: &'a CadIr) -> Self {
                let mut all_ids: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                $(
                    let key: fn(&$element) -> String = $key;
                    let $field: std::collections::HashMap<String, &'a $element> = ir
                        .model
                        .$field
                        .iter()
                        .map(|entity| {
                            let id = key(entity);
                            all_ids.insert(id.clone());
                            (id, entity)
                        })
                        .collect();
                )*
                for record in ir
                    .native
                    .0
                    .values()
                    .flat_map(|namespace| namespace.arenas.values())
                    .flatten()
                {
                    all_ids.insert(record.id.clone());
                }
                let unknown_ids = ir
                    .all_native_unknowns()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|record| record.id.0)
                    .collect();
                Self {
                    $( $field, )*
                    unknown_ids,
                    all_ids,
                }
            }

            /// Whether `id` names any entity in the document.
            pub(crate) fn contains(&self, id: &str) -> bool {
                self.all_ids.contains(id)
            }
        }
    };
}
crate::document::arena_registry!(define_model_index);
