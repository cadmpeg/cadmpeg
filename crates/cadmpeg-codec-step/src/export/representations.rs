// SPDX-License-Identifier: Apache-2.0

use super::Builder;
use crate::writer::{refs, string, Ref};

impl Builder<'_> {
    pub(super) fn emit_shape_representation(
        &mut self,
        name: &str,
        items: Vec<Ref>,
        placements: Vec<Ref>,
        context: Ref,
    ) -> Ref {
        let (sets, items): (Vec<_>, Vec<_>) = items
            .into_iter()
            .partition(|item| self.surface_set_items.contains(item));
        if sets.is_empty() {
            return self.emit_topology_representation(name, items, placements, context);
        }
        let sets = self.emitter.emit(
            "GEOMETRICALLY_BOUNDED_SURFACE_SHAPE_REPRESENTATION",
            &format!("{},{},{context}", string(name), refs(&sets)),
        );
        // `representation.items` is SET [1:?]: with nothing left beside the
        // sets, the bounded-surface representation is the representation
        // itself (a legal `map_usage` of a body placement's map).
        if items.is_empty() && placements.is_empty() {
            return sets;
        }
        let representation = self.emit_topology_representation(name, items, placements, context);
        self.emitter.emit(
            "SHAPE_REPRESENTATION_RELATIONSHIP",
            &format!("'','',{representation},{sets}"),
        );
        representation
    }

    fn emit_topology_representation(
        &mut self,
        name: &str,
        items: Vec<Ref>,
        placements: Vec<Ref>,
        context: Ref,
    ) -> Ref {
        let (mut solids, mut other): (Vec<_>, Vec<_>) = items
            .into_iter()
            .partition(|item| self.advanced_brep_items.contains(item));
        if solids.is_empty() {
            other.extend(placements);
            return self.emitter.emit(
                "SHAPE_REPRESENTATION",
                &format!("{},{},{context}", string(name), refs(&other)),
            );
        }
        solids.extend(placements);
        let representation = self.emitter.emit(
            "ADVANCED_BREP_SHAPE_REPRESENTATION",
            &format!("{},{},{context}", string(name), refs(&solids)),
        );
        if !other.is_empty() {
            let other = self.emitter.emit(
                "SHAPE_REPRESENTATION",
                &format!("{},{},{context}", string(name), refs(&other)),
            );
            self.emitter.emit(
                "SHAPE_REPRESENTATION_RELATIONSHIP",
                &format!("'','',{representation},{other}"),
            );
        }
        representation
    }
}
