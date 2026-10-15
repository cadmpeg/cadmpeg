// SPDX-License-Identifier: Apache-2.0
//! Admission of independent appended graphs with combined validation for shared graphs.

use super::native_unknown_order;
use crate::annotations::Annotations;
use crate::appearance::{Appearance, AppearanceBinding};
use crate::attributes::SourceAttribute;
use crate::document::{CadIr, Model};
use crate::drawings::Drawing;
use crate::features::{
    DesignConfiguration, DesignParameter, Feature, FeatureInputTopology, FeatureResultTopology,
};
use crate::geometry::{pcurve::Pcurve, Curve, ProceduralCurve, ProceduralSurface, Surface};
use crate::native::{NativeConvertError, NativeRecord};
use crate::presentation::{PresentationDocument, ViewPresentation};
use crate::products::{AssemblyJoint, Occurrence, ProductDefinition};
use crate::report::check::{Check, ValidationReport};
use crate::schema::EntitySchema;
use crate::semantic_annotations::SemanticAnnotation;
use crate::sketches::{
    Sketch, SketchConstraint, SketchEntity, SpatialSketch, SpatialSketchConstraint,
    SpatialSketchEntity,
};
use crate::spreadsheets::Spreadsheet;
use crate::subd::SubdSurface;
use crate::tessellation::Tessellation;
use crate::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Shell, Vertex};
use crate::unknown::UnknownRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) enum CandidateScope {
    Appended,
    Detached,
}

/// Reusable admission facts for an exclusively owned, append-only document.
/// Shared neutral references use combined validation. Independent graphs use
/// the same validators on the appended records and their native dependencies.
#[derive(Debug)]
pub(crate) struct AdmittedState {
    identities: BTreeMap<String, bool>,
    pub(crate) lengths: [usize; crate::schema::EntityKind::ALL.len()],
    pub(crate) annotations_admitted: bool,
    dirty_unknowns: BTreeSet<usize>,
}

impl AdmittedState {
    pub(crate) fn build(
        ir: &CadIr,
        format: &str,
        unknowns: &[UnknownRecord],
    ) -> Result<Self, CodecError> {
        let mut state = Self {
            identities: BTreeMap::new(),
            lengths: lengths(&ir.model),
            annotations_admitted: true,
            dirty_unknowns: BTreeSet::new(),
        };
        {
            macro_rules! add_neutral {
                ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                    for entity in &ir.model.$field { insert(&mut state.identities, entity.identity(), false); }
                )*};
            }
            crate::document::arena_registry!(add_neutral);
            for (namespace, arenas) in &ir.native.0 {
                for (arena, records) in arenas.arenas() {
                    if namespace == format && arena == "unknowns" {
                        continue;
                    }
                    for record in records {
                        insert(&mut state.identities, record.id(), true);
                    }
                }
            }
            for record in unknowns {
                insert(&mut state.identities, record.id().as_str(), true);
            }
            Ok::<_, CodecError>(())
        }?;
        Ok(state)
    }

    pub(crate) fn dirty_unknown(&mut self, position: usize) {
        self.dirty_unknowns.insert(position);
    }

    /// Validate a combined append, restoring every model arena before returning.
    pub(crate) fn probe(
        &mut self,
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        (format, unknowns): (&str, &[UnknownRecord]),
        changes: &Annotations,
        allowed: &[Check],
        (starts, scope): (
            [usize; crate::schema::EntityKind::ALL.len()],
            CandidateScope,
        ),
    ) -> Result<Option<Result<ValidationReport, NativeConvertError>>, CodecError> {
        let mut selected = (|| {
            let mut selected = BTreeMap::new();
            macro_rules! add_selected {
                ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                    let start = starts[<$ty as EntitySchema>::KIND.index()];
                    for entity in ir.model.$field.get(start..).ok_or_else(|| CodecError::malformed("admitted prefix was removed"))? {
                        insert(&mut selected, entity.identity(), false);
                    }
                )*};
            }
            crate::document::arena_registry!(add_selected);
            Ok::<_, CodecError>(selected)
        })()?;
        let mut closed = (matches!(scope, CandidateScope::Appended) || ir.native.0.is_empty())
            && !ir.model.has_feature_regeneration_parents();
        macro_rules! check_selected {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                for entity in &ir.model.$field[starts[<$ty as EntitySchema>::KIND.index()]..] {
                    if lookup(&self.identities, entity.identity()).is_some() { closed = false; }
                    entity.visit_references(ctx, &mut |target| {
                        if lookup(&selected, target).is_none() {
                            match lookup(&self.identities, target) {
                                Some(true) => { insert(&mut selected, target, true); }
                                _ => closed = false,
                            }
                        }
                        Ok(())
                    })?;
                }
            )*};
        }
        (|| {
            crate::document::arena_registry!(check_selected);
            Ok::<_, CodecError>(())
        })()?;
        if !closed {
            return Ok(None);
        }
        for &position in &self.dirty_unknowns {
            let record = unknowns
                .get(position)
                .ok_or_else(|| CodecError::malformed("changed source record is absent"))?;
            if let Err(error) = native_unknown_order(std::slice::from_ref(record)) {
                return Ok(Some(Err(error)));
            }
            for target in record.links() {
                if lookup(&self.identities, target).is_none() && lookup(&selected, target).is_none()
                {
                    return Ok(None);
                }
            }
        }
        // Include changed annotation owners even when the candidate adds no geometry.
        if allowed.contains(&Check::Annotations) {
            for id in changes.provenance.keys().chain(changes.exactness().keys()) {
                // Native placeholders omit source fields. Their annotations need
                // the complete source record for field-path diagnostics.
                if lookup(&selected, id) != Some(false) {
                    return Ok(None);
                }
            }
        }
        let mut candidate = CadIr::empty();
        candidate.tolerances = ir.tolerances;
        (|| {
            let mut dependencies = Vec::new();
            for (id, native) in &selected {
                if *native {
                    let id = crate::ids::Identity::new(id.to_owned())
                        .map_err(|error| CodecError::malformed(error.to_string()))?;
                    dependencies.push(NativeRecord::from_identity(id, None));
                }
            }
            if !dependencies.is_empty() {
                let arena = "unknowns".to_owned();
                let namespace = format.to_owned();
                let mut arenas = crate::native::NativeNamespace::default();
                arenas.arenas_mut().insert(arena, dependencies);
                candidate.native.0.insert(namespace, arenas);
            }
            Ok::<_, CodecError>(())
        })()?;
        macro_rules! take_appended {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                candidate.model.$field.extend(ir.model.$field.drain(starts[<$ty as EntitySchema>::KIND.index()]..));
            )*};
        }
        crate::document::arena_registry!(take_appended);
        let result = if allowed.contains(&Check::Annotations) {
            super::admit_with_annotations(ctx, &candidate, changes, allowed, Vec::new())
        } else {
            super::admit(ctx, &candidate, allowed, Vec::new())
        };
        // No fallible operation may precede restoration after the split.
        ir.model.append(candidate.model);
        result.map(|report| Some(Ok(report)))
    }

    pub(crate) fn rewind(
        &mut self,
        model: &Model,
        checkpoint: &crate::draft::ModelCheckpoint,
    ) -> Result<(), CodecError> {
        macro_rules! remove_suffix {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                let slot = <$ty as EntitySchema>::KIND.index();
                let start = self.lengths[slot].min(checkpoint.arena_len::<$ty>());
                for entity in model.$field.get(start..self.lengths[slot]).ok_or_else(|| CodecError::malformed("admitted suffix was removed before invalidation"))? {
                    self.identities.remove(entity.identity());
                }
                self.lengths[slot] = start;
            )*};
        }
        crate::document::arena_registry!(remove_suffix);
        Ok(())
    }

    pub(crate) fn accept(&mut self, ir: &CadIr) -> Result<(), CodecError> {
        (|| {
            macro_rules! accept_appended {
                ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                    for entity in ir.model.$field.get(self.lengths[<$ty as EntitySchema>::KIND.index()]..).ok_or_else(|| CodecError::malformed("admitted prefix was removed"))? { insert(&mut self.identities, entity.identity(), false); }
                )*};
            }
            crate::document::arena_registry!(accept_appended);
            Ok::<_, CodecError>(())
        })()?;
        self.lengths = lengths(&ir.model);
        self.dirty_unknowns.clear();
        Ok(())
    }

    /// Annotation admission errors concern unresolved owners. Field-path
    /// findings are warnings and do not change admission. A core-only route
    /// can retain these facts without adding annotation checks to its gate.
    pub(crate) fn annotation_owners_resolve(&self, annotations: &Annotations) -> bool {
        for id in annotations
            .provenance
            .keys()
            .chain(annotations.exactness().keys())
        {
            if lookup(&self.identities, id).is_none() {
                return false;
            }
        }
        true
    }
}

fn lengths(model: &Model) -> [usize; crate::schema::EntityKind::ALL.len()] {
    macro_rules! lengths {
        ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {[$(model.$field.len()),*]};
    }
    crate::document::arena_registry!(lengths)
}

fn lookup(ids: &BTreeMap<String, bool>, id: &str) -> Option<bool> {
    ids.get(id).copied()
}

fn insert(ids: &mut BTreeMap<String, bool>, id: &str, native: bool) {
    if !ids.contains_key(id) {
        ids.insert(id.to_owned(), native);
    }
}

#[cfg(test)]
mod tests;
