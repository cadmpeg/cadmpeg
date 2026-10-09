// SPDX-License-Identifier: Apache-2.0
//! Admission of independent appended graphs with combined validation for shared graphs.

use super::native_unknown_order;
use crate::annotations::{admit_identity_work, Annotations};
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
use cadmpeg_core::decode::{u64_from_index, DecodeContext, ScopedReservation};
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
pub(crate) struct AdmittedState<'ctx> {
    identities: BTreeMap<String, bool>,
    pub(crate) lengths: [usize; crate::schema::EntityKind::ALL.len()],
    pub(crate) annotations_admitted: bool,
    dirty_unknowns: BTreeSet<usize>,
    dirty_storage: ScopedReservation<'ctx>,
    storage: ScopedReservation<'ctx>,
}

impl<'ctx> AdmittedState<'ctx> {
    pub(crate) fn build(
        ctx: &'ctx DecodeContext<'ctx>,
        ir: &CadIr,
        format: &str,
        unknowns: &[UnknownRecord],
    ) -> Result<Self, CodecError> {
        let mut state = Self {
            identities: BTreeMap::new(),
            lengths: lengths(&ir.model),
            annotations_admitted: true,
            dirty_unknowns: BTreeSet::new(),
            dirty_storage: ctx.reserve_scoped(0, "changed source record storage")?,
            storage: ctx.reserve_scoped(0, "admitted identity storage")?,
        };
        state.storage.with_storage(|| {
            macro_rules! add_neutral {
                ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                    for entity in &ir.model.$field { insert(ctx, &mut state.identities, entity.identity(), false)?; }
                )*};
            }
            crate::document::arena_registry!(add_neutral);
            for (namespace, arenas) in &ir.native.0 {
                for (arena, records) in arenas.arenas() {
                    if namespace == format && arena == "unknowns" { continue; }
                    for record in records { insert(ctx, &mut state.identities, record.id(), true)?; }
                }
            }
            for record in unknowns { insert(ctx, &mut state.identities, record.id().as_str(), true)?; }
            Ok::<_, CodecError>(())
        })?;
        Ok(state)
    }

    pub(crate) fn dirty_unknown(
        &mut self,
        ctx: &DecodeContext<'_>,
        position: usize,
    ) -> Result<(), CodecError> {
        super::mark_unknown_dirty(
            ctx,
            &mut self.dirty_storage,
            &mut self.dirty_unknowns,
            position,
            "changed source record positions",
        )?;
        Ok(())
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
        ctx.charge_work(
            u64_from_index(crate::schema::EntityKind::ALL.len()) * 4,
            "candidate admission arena scan",
        )?;
        let mut selected = ctx.with_scoped_storage("appended admission identities", || {
            let mut selected = BTreeMap::new();
            macro_rules! add_selected {
                ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                    let start = starts[<$ty as EntitySchema>::KIND.index()];
                    for entity in ir.model.$field.get(start..).ok_or_else(|| CodecError::malformed("admitted prefix was removed"))? {
                        insert(ctx, &mut selected, entity.identity(), false)?;
                    }
                )*};
            }
            crate::document::arena_registry!(add_selected);
            Ok::<_, CodecError>(selected)
        })?;
        let mut closed = (matches!(scope, CandidateScope::Appended) || ir.native.0.is_empty())
            && !ir.model.has_feature_regeneration_parents();
        macro_rules! check_selected {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                for entity in &ir.model.$field[starts[<$ty as EntitySchema>::KIND.index()]..] {
                    if lookup(ctx, &self.identities, entity.identity())?.is_some() { closed = false; }
                    entity.visit_references(ctx, &mut |target| {
                        if lookup(ctx, &selected.0, target)?.is_none() {
                            match lookup(ctx, &self.identities, target)? {
                                Some(true) => { insert(ctx, &mut selected.0, target, true)?; }
                                _ => closed = false,
                            }
                        }
                        Ok(())
                    })?;
                }
            )*};
        }
        selected.1.with_storage(|| {
            crate::document::arena_registry!(check_selected);
            Ok::<_, CodecError>(())
        })?;
        if !closed {
            return Ok(None);
        }
        for &position in &self.dirty_unknowns {
            let record = unknowns
                .get(position)
                .ok_or_else(|| CodecError::malformed("changed source record is absent"))?;
            if let Err(error) = native_unknown_order(ctx, std::slice::from_ref(record))? {
                return Ok(Some(Err(error)));
            }
            for target in record.links() {
                if lookup(ctx, &self.identities, target)?.is_none()
                    && lookup(ctx, &selected.0, target)?.is_none()
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
                if lookup(ctx, &selected.0, id)? != Some(false) {
                    return Ok(None);
                }
            }
        }
        let mut candidate = CadIr::empty();
        candidate.tolerances = ir.tolerances;
        let mut moved = ctx.reserve_scoped(0, "appended admission model storage")?;
        // Admit every split before moving any arena. Restoration only moves these
        // same records into the original, still allocated arena buffers.
        macro_rules! reserve_splits {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                let count = ir.model.$field.len() - starts[<$ty as EntitySchema>::KIND.index()];
                ctx.reserve_scoped_vec(&mut moved, &mut candidate.model.$field, count, "appended admission arena slots")?;
                ctx.charge_work(u64_from_index(count).checked_mul(u64_from_index(std::mem::size_of::<$ty>())).and_then(|n| n.checked_mul(2)).ok_or_else(|| ctx.refuse_codec_limit("appended admission moves", u64::MAX - 1, u64::MAX))?, "appended admission moves")?;
            )*};
        }
        crate::document::arena_registry!(reserve_splits);
        let mut native_storage = ctx.reserve_scoped(0, "appended admission native dependencies")?;
        native_storage.with_storage(|| {
            let mut dependencies = Vec::new();
            for (id, native) in &selected.0 {
                if *native {
                    let id = crate::ids::Identity::new(
                        ctx.copy_retained_text(id, "appended admission native identity")?,
                    )
                    .map_err(|error| CodecError::malformed(error.to_string()))?;
                    ctx.push_vec(
                        &mut dependencies,
                        NativeRecord::from_identity(id, None),
                        "appended admission native dependencies",
                    )?;
                }
            }
            if !dependencies.is_empty() {
                let arena =
                    ctx.copy_retained_text("unknowns", "appended admission native arena")?;
                let namespace =
                    ctx.copy_retained_text(format, "appended admission native namespace")?;
                let mut arenas = crate::native::NativeNamespace::default();
                ctx.insert_btree_map(
                    arenas.arenas_mut(),
                    arena,
                    dependencies,
                    "appended admission native arena",
                )?;
                ctx.insert_btree_map(
                    &mut candidate.native.0,
                    namespace,
                    arenas,
                    "appended admission native namespace",
                )?;
            }
            Ok::<_, CodecError>(())
        })?;
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
        ctx: &DecodeContext<'_>,
        model: &Model,
        checkpoint: &crate::draft::ModelCheckpoint,
    ) -> Result<(), CodecError> {
        macro_rules! remove_suffix {
            ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                let slot = <$ty as EntitySchema>::KIND.index();
                let start = self.lengths[slot].min(checkpoint.arena_len::<$ty>());
                for entity in model.$field.get(start..self.lengths[slot]).ok_or_else(|| CodecError::malformed("admitted suffix was removed before invalidation"))? {
                    admit_identity_work(ctx, self.identities.len(), entity.identity().len(), "invalidate appended admission identities")?;
                    self.identities.remove(entity.identity());
                }
                self.lengths[slot] = start;
            )*};
        }
        crate::document::arena_registry!(remove_suffix);
        Ok(())
    }

    pub(crate) fn accept(
        &mut self,
        ctx: &'ctx DecodeContext<'ctx>,
        ir: &CadIr,
    ) -> Result<(), CodecError> {
        self.storage.with_storage(|| {
            macro_rules! accept_appended {
                ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {$(
                    for entity in ir.model.$field.get(self.lengths[<$ty as EntitySchema>::KIND.index()]..).ok_or_else(|| CodecError::malformed("admitted prefix was removed"))? { insert(ctx, &mut self.identities, entity.identity(), false)?; }
                )*};
            }
            crate::document::arena_registry!(accept_appended);
            Ok::<_, CodecError>(())
        })?;
        self.lengths = lengths(&ir.model);
        self.dirty_unknowns.clear();
        self.dirty_storage = ctx.reserve_scoped(0, "changed source record storage")?;
        Ok(())
    }

    /// Annotation admission errors concern unresolved owners. Field-path
    /// findings are warnings and do not change admission. A core-only route
    /// can retain these facts without adding annotation checks to its gate.
    pub(crate) fn annotation_owners_resolve(
        &self,
        ctx: &DecodeContext<'_>,
        annotations: &Annotations,
    ) -> Result<bool, CodecError> {
        for id in annotations
            .provenance
            .keys()
            .chain(annotations.exactness().keys())
        {
            ctx.charge_work(1, "admitted annotation owner scan")?;
            if lookup(ctx, &self.identities, id)?.is_none() {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

fn lengths(model: &Model) -> [usize; crate::schema::EntityKind::ALL.len()] {
    macro_rules! lengths {
        ($($field:ident: $ty:ty, $doc:literal, [$($attribute:meta),*] $(, [$($schema_attr:meta),*])?;)*) => {[$(model.$field.len()),*]};
    }
    crate::document::arena_registry!(lengths)
}

fn lookup(
    ctx: &DecodeContext<'_>,
    ids: &BTreeMap<String, bool>,
    id: &str,
) -> Result<Option<bool>, CodecError> {
    admit_identity_work(ctx, ids.len(), id.len(), "admitted identity lookup")?;
    Ok(ids.get(id).copied())
}

fn insert(
    ctx: &DecodeContext<'_>,
    ids: &mut BTreeMap<String, bool>,
    id: &str,
    native: bool,
) -> Result<(), CodecError> {
    admit_identity_work(ctx, ids.len(), id.len(), "admitted identity lookup")?;
    if !ids.contains_key(id) {
        let id = ctx.copy_retained_text(id, "admitted identity text")?;
        ctx.insert_btree_map(ids, id, native, "admitted identity slots")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
