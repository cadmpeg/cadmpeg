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
