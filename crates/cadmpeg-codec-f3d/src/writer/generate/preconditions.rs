// SPDX-License-Identifier: Apache-2.0
//! Source-less pre-write validators for the neutral `CadIr` and its F3D native
//! extension.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::native::F3dNative;
use crate::records::{
    PersistentDesignLink, PersistentSubentityTag, SegmentType, SketchCurveGeometry,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::attributes::AttributeTarget;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

use super::attributes::source_less_body_key;
use super::records::validate_dynamic_class_tag;
pub(crate) fn validate_source_less_procedural_carriers(target: &CadIr) -> Result<(), CodecError> {
    let mut surface_owners = BTreeSet::new();
    for procedural in &target.model.procedural_surfaces {
        let owner = target
            .model
            .procedural_surface_owner(&procedural.id)
            .ok_or_else(|| {
                CodecError::InvalidInput(format!(
                    "procedural surface {} has no unique carrier",
                    procedural.id
                ))
            })?;
        if !surface_owners.insert(owner) {
            return Err(CodecError::InvalidInput(format!(
                "surface {} has multiple procedural constructions",
                owner
            )));
        }
        let surface = target
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *owner)
            .ok_or_else(|| {
                CodecError::InvalidInput(format!(
                    "procedural surface {} references missing carrier {}",
                    procedural.id, owner
                ))
            })?;
        match &surface.geometry {
            SurfaceGeometry::Procedural {
                cache: Some(geometry),
                ..
            } if matches!(
                geometry.as_ref(),
                SurfaceGeometry::Nurbs(_) | SurfaceGeometry::Unknown { .. }
            ) => {}
            SurfaceGeometry::Procedural {
                construction,
                cache: None,
            } if *construction == procedural.id => {}
            SurfaceGeometry::Procedural { construction, .. } => {
                return Err(CodecError::InvalidInput(format!(
                    "surface {} links construction {construction} but is produced by {}",
                    surface.id, procedural.id
                )));
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "source-less F3D procedural surface {} cannot retain its construction on analytic carrier {}",
                    procedural.id, surface.id
                )));
            }
        }
    }

    let mut curve_owners = BTreeSet::new();
    for procedural in &target.model.procedural_curves {
        let owner = target
            .model
            .procedural_curve_owner(&procedural.id)
            .ok_or_else(|| {
                CodecError::InvalidInput(format!(
                    "procedural curve {} has no unique carrier",
                    procedural.id
                ))
            })?;
        if !curve_owners.insert(owner) {
            return Err(CodecError::InvalidInput(format!(
                "curve {} has multiple procedural constructions",
                owner
            )));
        }
        let curve = target
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *owner)
            .ok_or_else(|| {
                CodecError::InvalidInput(format!(
                    "procedural curve {} references missing carrier {}",
                    procedural.id, owner
                ))
            })?;
        match &curve.geometry {
            CurveGeometry::Procedural {
                cache: Some(geometry),
                ..
            } if matches!(geometry.as_ref(), CurveGeometry::Nurbs(_)) => {}
            CurveGeometry::Procedural {
                construction,
                cache: None,
            } if *construction == procedural.id && procedural.cache_fit_tolerance().is_none() => {}
            CurveGeometry::Procedural { construction, .. } => {
                return Err(CodecError::InvalidInput(format!(
                    "curve {} links construction {construction} but is produced by {} or carries a cache fit",
                    curve.id, procedural.id
                )));
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "source-less F3D procedural curve {} cannot retain its construction on carrier {}",
                    procedural.id, curve.id
                )));
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_source_less_topology_tolerances(
    target: &CadIr,
    native: &F3dNative,
) -> Result<(), CodecError> {
    if let Some(face) = target
        .model
        .faces
        .iter()
        .find(|face| face.tolerance.is_some())
    {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize face {} tolerance losslessly",
            face.id
        )));
    }
    if let Some(edge) = target.model.edges.iter().find(|edge| {
        edge.tolerance
            .is_some_and(|tolerance| !tolerance.is_finite() || tolerance < 0.0)
    }) {
        return Err(CodecError::InvalidInput(format!(
            "F3D edge {} tolerance must be finite and nonnegative",
            edge.id
        )));
    }
    let tolerant = native
        .tolerant_coedge_parameters
        .iter()
        .filter(|parameters| {
            matches!(
                parameters.extension,
                cadmpeg_asm::brep::records::TolerantCoedgeExtension::EmbeddedCurve {
                    target: None,
                    ..
                }
            )
        })
        .map(|parameters| parameters.coedge.as_str())
        .collect::<std::collections::HashSet<_>>();
    if let Some(coedge) = target
        .model
        .coedges
        .iter()
        .find(|coedge| coedge.use_curve.is_some() && !tolerant.contains(coedge.id.as_str()))
    {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D coedge {} use curve lacks a cache-local tolerant extension",
            coedge.id
        )));
    }
    Ok(())
}

pub(crate) fn validate_source_less_auxiliary_geometry(target: &CadIr) -> Result<(), CodecError> {
    if let Some(tessellation) = target.model.tessellations.first() {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize neutral tessellation {} losslessly",
            tessellation.id
        )));
    }
    if let Some(surface) = target
        .model
        .surfaces
        .iter()
        .find(|surface| surface.source_object.is_some())
    {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize source-object association on surface {} losslessly",
            surface.id
        )));
    }
    if let Some(curve) = target
        .model
        .curves
        .iter()
        .find(|curve| curve.source_object.is_some())
    {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize source-object association on curve {} losslessly",
            curve.id
        )));
    }
    Ok(())
}

pub(crate) fn validate_source_less_recipes(native: &F3dNative) -> Result<(), CodecError> {
    if native
        .construction_recipes
        .windows(2)
        .any(|pair| pair[0].record_index > pair[1].record_index)
    {
        return Err(CodecError::InvalidInput(
            "F3D construction recipes must be ordered by record index".into(),
        ));
    }
    let mut group_counts = HashMap::new();
    for recipe in &native.construction_recipes {
        let expected = group_counts
            .entry((recipe.kind, recipe.design_id.as_ref().map(|field| field.value.as_str())))
            .or_insert(0u32);
        if recipe.recipe_index != *expected {
            return Err(CodecError::InvalidInput(format!(
                "F3D construction recipe {} has noncontiguous group index {}",
                recipe.id, recipe.recipe_index
            )));
        }
        *expected += 1;
    }
    Ok(())
}

fn source_less_design_record_type<'a>(
    native: &'a F3dNative,
    class_tag: &str,
    record_index: u32,
    record_kind: &str,
) -> Result<&'a SegmentType, CodecError> {
    validate_dynamic_class_tag(class_tag, record_kind)?;
    let type_ordinal = class_tag
        .parse::<usize>()
        .ok()
        .and_then(|class_tag| class_tag.checked_sub(256))
        .ok_or_else(|| {
            CodecError::InvalidInput(format!(
                "F3D {record_kind} class tag {class_tag} is below the dynamic type range"
            ))
        })?;
    let design_type = native.design_types.get(type_ordinal).ok_or_else(|| {
        CodecError::InvalidInput(format!(
            "F3D {record_kind} class tag {class_tag} is outside the Design type table"
        ))
    })?;
    if !design_type.entity_ids.contains(&u64::from(record_index)) {
        return Err(CodecError::InvalidInput(format!(
            "F3D {record_kind} {record_index} is not registered by class tag {class_tag}"
        )));
    }
    Ok(design_type)
}

fn design_type_matches(design_type: &SegmentType, expected: (&str, u32, &str)) -> bool {
    design_type.type_guid.eq_ignore_ascii_case(expected.0)
        && design_type.version == expected.1
        && design_type.module == expected.2
}

pub(crate) fn validate_source_less_sketch_graph(native: &F3dNative) -> Result<(), CodecError> {
    let sketch_owners = native
        .design_entity_headers
        .iter()
        .filter(|header| header.in_sketch_module())
        .map(|header| header.entity_suffix)
        .collect::<BTreeSet<_>>();
    let root_indices = native
        .design_entity_headers
        .iter()
        .filter(|header| header.in_sketch_module())
        .flat_map(|header| header.reference_indices.iter().copied())
        .collect::<BTreeSet<_>>();
    let mut typed_indices = BTreeMap::<u32, &str>::new();
    let mut typed_records = Vec::new();
    for (record_index, id, class_tag) in native
        .sketch_points
        .iter()
        .map(|record| {
            (
                record.record_index,
                record.id.as_str(),
                record.class_tag.as_str(),
            )
        })
        .chain(native.sketch_curve_identities.iter().map(|record| {
            (
                record.record_index,
                record.id.as_str(),
                record.class_tag.as_str(),
            )
        }))
        .chain(native.sketch_relations.iter().map(|record| {
            (
                record.record_index,
                record.id.as_str(),
                record.class_tag.as_str(),
            )
        }))
        .chain(native.sketch_texts.iter().map(|record| {
            (
                record.record_index,
                record.id.as_str(),
                record.class_tag.as_str(),
            )
        }))
    {
        if let Some(before) = typed_indices.insert(record_index, id) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch records {before} and {id} share record index {record_index}"
            )));
        }
        typed_records.push((record_index, class_tag));
    }
    for (record_index, class_tag) in typed_records {
        source_less_design_record_type(native, class_tag, record_index, "sketch record")?;
    }
    let mut geometry_owners = BTreeMap::new();
    let mut point_companions = BTreeSet::new();
    let curve_indices = native
        .sketch_curve_identities
        .iter()
        .map(|curve| curve.record_index)
        .collect::<BTreeSet<_>>();
    for point in &native.sketch_points {
        let point_type = source_less_design_record_type(
            native,
            &point.class_tag,
            point.record_index,
            "sketch point",
        )?;
        if !design_type_matches(
            point_type,
            crate::design::decode::sketch::CURRENT_SKETCH_POINT_TYPE,
        ) {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D sketch point {} requires the current point record type",
                point.id
            )));
        }
        if point.record_form.class_version() != point_type.version
            || !matches!(
                point.record_form,
                crate::records::SketchPointRecordForm::Version11 { .. }
            )
        {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D sketch point {} requires the version-11 member sequence",
                point.id
            )));
        }
        if point
            .persistent_id()
            .is_none_or(|persistent_id| persistent_id == 0)
        {
            return Err(CodecError::InvalidInput(format!(
                "source-less F3D sketch point {} has no persistent identity",
                point.id
            )));
        }
        let owner_reference = point.owner_reference.ok_or_else(|| {
            CodecError::InvalidInput(format!(
                "F3D sketch point {} has no direct sketch owner",
                point.id
            ))
        })?;
        if !sketch_owners.contains(&u64::from(owner_reference)) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch point {} references missing sketch owner {owner_reference}",
                point.id
            )));
        }
        let owner_type_count = native
            .design_types
            .iter()
            .filter(|design_type| {
                design_type.entity_ids.contains(&u64::from(owner_reference))
                    && design_type.type_guid.eq_ignore_ascii_case(
                        crate::design::decode::sketch::SKETCH_CONTAINER_TYPE_GUID,
                    )
            })
            .count();
        if owner_type_count != 1 {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch point {} owner {owner_reference} does not have one sketch-container type registration",
                point.id
            )));
        }
        if point.paired_reference == point.record_index
            || typed_indices.contains_key(&point.paired_reference)
            || !point_companions.insert(point.paired_reference)
        {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch point {} has a conflicting companion record {}",
                point.id, point.paired_reference
            )));
        }
        let companion_type = native
            .design_types
            .iter()
            .filter(|design_type| {
                design_type
                    .entity_ids
                    .contains(&u64::from(point.paired_reference))
                    && design_type_matches(
                        design_type,
                        crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE,
                    )
            })
            .count();
        if companion_type != 1 {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch point {} companion {} does not have one current companion type registration",
                point.id, point.paired_reference
            )));
        }
        let companion = point.companion.as_ref().ok_or_else(|| {
            CodecError::InvalidInput(format!(
                "source-less F3D sketch point {} has no inverse companion",
                point.id
            ))
        })?;
        if companion.reference_encoding
            != crate::records::SketchPointCompanionReferenceEncoding::SameSegment
        {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D sketch point {} companion requires same-segment references",
                point.id
            )));
        }
        let mut incident_curves = BTreeSet::new();
        for curve in &companion.incident_curves {
            if !incident_curves.insert(*curve) {
                return Err(CodecError::InvalidInput(format!(
                    "F3D sketch point {} companion repeats curve {curve}",
                    point.id
                )));
            }
            if !curve_indices.contains(curve) {
                return Err(CodecError::InvalidInput(format!(
                    "F3D sketch point {} companion references missing curve {curve}",
                    point.id
                )));
            }
        }
        geometry_owners.insert(point.record_index, owner_reference);
    }
    for curve in &native.sketch_curve_identities {
        let curve_type = source_less_design_record_type(
            native,
            &curve.class_tag,
            curve.record_index,
            "sketch curve",
        )?;
        let expected_type = match curve.geometry.as_ref() {
            Some(SketchCurveGeometry::Line { .. }) => {
                crate::design::decode::sketch::CURRENT_SKETCH_LINE_TYPE
            }
            Some(SketchCurveGeometry::Arc { .. }) => {
                crate::design::decode::sketch::CURRENT_SKETCH_CIRCULAR_TYPE
            }
            Some(SketchCurveGeometry::Nurbs { .. }) => {
                crate::design::decode::sketch::CURRENT_SKETCH_NURBS_TYPE
            }
            None => {
                return Err(CodecError::NotImplemented(format!(
                    "source-less F3D sketch curve {} has no writable geometry",
                    curve.id
                )))
            }
        };
        if !design_type_matches(curve_type, expected_type) {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D sketch curve {} requires its current geometry record type",
                curve.id
            )));
        }
        let owner_reference = curve.owner_reference.ok_or_else(|| {
            CodecError::InvalidInput(format!(
                "F3D sketch curve {} has no direct sketch owner",
                curve.id
            ))
        })?;
        if !sketch_owners.contains(&u64::from(owner_reference)) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch curve {} references missing sketch owner {owner_reference}",
                curve.id
            )));
        }
        geometry_owners.insert(curve.record_index, owner_reference);
    }
    for relation in &native.sketch_relations {
        if !root_indices.contains(&relation.record_index) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch relation {} is not reachable from a sketch header",
                relation.id
            )));
        }
        if !sketch_owners.contains(&u64::from(relation.owner_reference)) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch relation {} references missing sketch owner {}",
                relation.id, relation.owner_reference
            )));
        }
        for member in relation.all_member_indices() {
            if geometry_owners
                .get(&member)
                .is_some_and(|owner| *owner != relation.owner_reference)
            {
                return Err(CodecError::InvalidInput(format!(
                    "F3D sketch relation {} owner disagrees with geometry record {member}",
                    relation.id
                )));
            }
        }
    }
    for text in &native.sketch_texts {
        if !root_indices.contains(&text.record_index) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch text {} is not reachable from a sketch header",
                text.id
            )));
        }
        if !sketch_owners.contains(&u64::from(text.owner_reference)) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch text {} references missing sketch owner {}",
                text.id, text.owner_reference
            )));
        }
    }
    let mut reachable_headers = root_indices;
    for relation in &native.sketch_relations {
        reachable_headers.extend(relation.all_member_indices());
    }
    let mut explicit_headers = BTreeSet::new();
    for header in &native.design_record_headers {
        if !explicit_headers.insert(header.record_index) {
            return Err(CodecError::InvalidInput(format!(
                "multiple F3D Design record headers use index {}",
                header.record_index
            )));
        }
        if typed_indices.contains_key(&header.record_index) {
            return Err(CodecError::InvalidInput(format!(
                "F3D Design record header {} shadows a typed sketch record",
                header.id
            )));
        }
        if !reachable_headers.contains(&header.record_index) {
            return Err(CodecError::InvalidInput(format!(
                "F3D Design record header {} is unreachable from the sketch graph",
                header.id
            )));
        }
    }
    Ok(())
}

pub(crate) fn validate_source_less_design_ownership(native: &F3dNative) -> Result<(), CodecError> {
    let mut parameter_indices = BTreeSet::new();
    let mut parameter_ordinals = BTreeSet::new();
    for parameter in &native.design_parameters {
        let expected_discriminator =
            crate::design::decode::parameters::design_parameter_discriminator(
                &parameter.source_kind,
            );
        if parameter.family_discriminator.map(|value| value.value) != Some(expected_discriminator) {
            return Err(CodecError::InvalidInput(format!(
                "F3D Design parameter {} has discriminator {:?}, expected {expected_discriminator} for {}",
                parameter.id, parameter.family_discriminator.map(|value| value.value), parameter.source_kind
            )));
        }
        validate_dynamic_class_tag(&parameter.class_tag, "Design parameter")?;
        if parameter.kind() != crate::records::DesignParameterKind::User
            || parameter.source_kind != "User Parameter"
            || parameter.owner_record_index().is_some()
        {
            return Err(CodecError::NotImplemented(
                "source-less F3D owned Design parameter records are not writable".into(),
            ));
        }
        if parameter.expression.is_empty()
            || parameter.name.is_empty()
            || parameter.unit.as_ref().is_some_and(String::is_empty)
            || !parameter.evaluated_value.is_finite()
        {
            return Err(CodecError::InvalidInput(format!(
                "F3D Design parameter {} has an invalid document parameter value",
                parameter.id
            )));
        }
        if !parameter_indices.insert(parameter.record_index)
            || !parameter_ordinals.insert(parameter.source_ordinal)
        {
            return Err(CodecError::InvalidInput(format!(
                "F3D Design parameter {} duplicates a record index or source ordinal",
                parameter.id
            )));
        }
    }
    let mut types_by_guid = BTreeMap::new();
    let mut entity_types = BTreeMap::new();
    let mut entity_modules = BTreeMap::new();
    for design_type in &native.design_types {
        if types_by_guid
            .insert(design_type.type_guid.as_str(), design_type)
            .is_some()
        {
            return Err(CodecError::InvalidInput(format!(
                "duplicate F3D Design type GUID: {}",
                design_type.type_guid
            )));
        }
        for entity_id in &design_type.entity_ids {
            if let Some(before) = entity_types.insert(*entity_id, design_type.type_guid.as_str()) {
                return Err(CodecError::InvalidInput(format!(
                    "F3D Design entity {entity_id} is registered by both type {before} and type {}",
                    design_type.type_guid
                )));
            }
            entity_modules.insert(*entity_id, design_type.module.clone());
        }
    }
    // A base type need not be registered by the same segment, so an unresolved
    // base GUID is legal; a resolved chain must still terminate.
    for design_type in &native.design_types {
        if design_type.base_type_guid.as_deref() == Some(design_type.type_guid.as_str()) {
            return Err(CodecError::InvalidInput(format!(
                "F3D Design type {} is its own base type",
                design_type.id
            )));
        }
        let mut ancestors = BTreeSet::new();
        let mut cursor = design_type;
        while let Some(base) = cursor
            .base_type_guid
            .as_deref()
            .and_then(|base| types_by_guid.get(base))
        {
            if !ancestors.insert(base.type_guid.as_str()) {
                return Err(CodecError::InvalidInput(format!(
                    "F3D Design type hierarchy contains a cycle at {}",
                    base.type_guid
                )));
            }
            cursor = base;
        }
    }
    for header in &native.design_entity_headers {
        let suffix = header
            .entity_id
            .rsplit('_')
            .next()
            .and_then(|suffix| suffix.parse::<u64>().ok());
        if suffix != Some(header.entity_suffix) {
            return Err(CodecError::InvalidInput(format!(
                "F3D Design header {} entity id conflicts with suffix {}",
                header.id, header.entity_suffix
            )));
        }
        let owned_module = entity_modules.get(&header.entity_suffix).cloned();
        if header.module != owned_module {
            return Err(CodecError::InvalidInput(format!(
                "F3D Design header {} module conflicts with MetaStream ownership",
                header.id
            )));
        }
        if header.in_sketch_module() {
            // `record_reference` is absent on the sentinel (no-base-record)
            // reference-list form. The writer derives the list count from the
            // references because decoded source streams are merged into one
            // canonical Design stream.
        } else if header.record_reference.is_some()
            || header.declared_reference_count.is_some()
            || !header.reference_indices.is_empty()
        {
            return Err(CodecError::InvalidInput(format!(
                "F3D non-sketch Design header {} carries discarded sketch references",
                header.id
            )));
        }
    }
    Ok(())
}

/// Proof that [`validate_source_less_design_bindings`] ran against the borrowed
/// `F3dNative`. The private field keeps construction inside this module, so an
/// encoder that reads binding-validated fields (material physical tokens) cannot
/// be reached without the check having run on the very native it will read.
#[derive(Clone, Copy)]
pub(crate) struct DesignBindingsValidated<'a> {
    native: &'a F3dNative,
}

impl<'a> DesignBindingsValidated<'a> {
    /// The native whose design bindings were validated.
    pub(super) fn native(self) -> &'a F3dNative {
        self.native
    }
}

pub(crate) fn validate_source_less_design_bindings(
    native: &F3dNative,
) -> Result<DesignBindingsValidated<'_>, CodecError> {
    let mut by_key = BTreeMap::new();
    let mut by_suffix = BTreeMap::new();
    let mut insert = |key: u64, suffix: u64, id: &str| -> Result<(), CodecError> {
        if by_key
            .insert(key, suffix)
            .is_some_and(|before| before != suffix)
            || by_suffix
                .insert(suffix, key)
                .is_some_and(|before| before != key)
        {
            return Err(CodecError::InvalidInput(format!(
                "F3D Design body binding {id} conflicts with the body-map key/suffix bijection"
            )));
        }
        Ok(())
    };
    if let Some(assignment) = native.design_material_assignments.first() {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D material assignment {} requires a typed body-presentation B-rep and scene graph",
            assignment.id
        )));
    }
    for visibility in &native.body_visibilities {
        insert(
            visibility.asm_body_key,
            visibility.entity_suffix,
            &visibility.id,
        )?;
    }
    Ok(DesignBindingsValidated { native })
}

pub(crate) fn validate_source_less_history_graph(
    target: &CadIr,
    native: &F3dNative,
) -> Result<(), CodecError> {
    let Some(namespace) = target.native.namespace("f3d") else {
        return Ok(());
    };
    let stored_count = |arena: &str| namespace.arenas.get(arena).map_or(0, Vec::len);
    for arena in [
        "asm_histories",
        "asm_delta_states",
        "asm_bulletin_boards",
        "asm_entity_changes",
        "asm_history_records",
    ] {
        if let Some(records) = namespace.arenas.get(arena) {
            let unique = records
                .iter()
                .map(cadmpeg_ir::NativeRecord::id)
                .collect::<BTreeSet<_>>();
            if unique.len() != records.len() {
                return Err(CodecError::InvalidInput(format!(
                    "F3D {arena} contains duplicate record ids"
                )));
            }
        }
    }
    let states = native
        .asm_histories
        .iter()
        .flat_map(|history| &history.states)
        .collect::<Vec<_>>();
    let boards = states
        .iter()
        .flat_map(|state| &state.bulletin_boards)
        .collect::<Vec<_>>();
    let changes = boards.iter().flat_map(|board| &board.changes).count();
    let records = states.iter().flat_map(|state| &state.records).count();
    let reconstructed = [
        ("asm_histories", native.asm_histories.len()),
        ("asm_delta_states", states.len()),
        ("asm_bulletin_boards", boards.len()),
        ("asm_entity_changes", changes),
        ("asm_history_records", records),
    ];
    if reconstructed
        .iter()
        .any(|(arena, count)| stored_count(arena) != *count)
    {
        return Err(CodecError::InvalidInput(
            "F3D ASM history graph contains orphaned or ambiguously parented records".into(),
        ));
    }
    for state in states {
        for record in &state.records {
            if record.raw_bytes.is_empty() {
                return Err(CodecError::InvalidInput(format!(
                    "F3D ASM history record {} has an empty native payload",
                    record.id
                )));
            }
        }
    }
    for history in &native.asm_histories {
        match history.preamble {
            Some(preamble)
                if history
                    .states
                    .first()
                    .is_some_and(|state| state.state_id == preamble.stream_size)
                    && preamble.history_entry_count >= 0 => {}
            Some(_) => {
                return Err(CodecError::InvalidInput(format!(
                    "F3D history {} requires head state_id == stream_size and nonnegative history_entry_count",
                    history.id
                )));
            }
            None => {}
        }
    }
    if native
        .asm_histories
        .iter()
        .any(|history| !crate::history::graph_is_coherent(history))
    {
        return Err(CodecError::InvalidInput(
            "F3D ASM history graph is not a coherent doubly linked state chain".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_source_less_design_links(
    target: &CadIr,
    native: &F3dNative,
    attributes: &super::attributes::AttributeIndex<'_>,
) -> Result<(), CodecError> {
    if let Some(sentinel) = native.mesh_surface_sentinels.first() {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize mesh-surface sentinel {} without its retained ASM record",
            sentinel.id()
        )));
    }
    let coedges = target
        .model
        .coedges
        .iter()
        .map(|coedge| &coedge.id)
        .collect::<BTreeSet<_>>();
    let mut linked_coedges = BTreeSet::new();
    for link in &native.sketch_curve_links {
        // Only a coedge-owned link is regenerated; a link on any other owner is
        // reported as a source-fidelity loss instead of refusing the write.
        let AttributeTarget::Coedge(coedge) = &link.target else {
            continue;
        };
        if !coedges.contains(coedge) {
            return Err(CodecError::InvalidInput(format!(
                "F3D sketch-curve link {} targets a missing coedge {}",
                link.id, coedge.0
            )));
        }
        if !linked_coedges.insert(coedge) {
            return Err(CodecError::InvalidInput(format!(
                "source-less F3D generation supports one sketch-curve link per coedge: {}",
                coedge.0
            )));
        }
    }

    let bodies = target
        .model
        .bodies
        .iter()
        .map(|item| &item.id)
        .collect::<BTreeSet<_>>();
    let faces = target
        .model
        .faces
        .iter()
        .map(|item| &item.id)
        .collect::<BTreeSet<_>>();
    let edges = target
        .model
        .edges
        .iter()
        .map(|item| &item.id)
        .collect::<BTreeSet<_>>();
    let mut groups: BTreeMap<String, Vec<&PersistentDesignLink>> = BTreeMap::new();
    for link in &native.persistent_design_links {
        let target_key = match &link.target {
            cadmpeg_ir::attributes::AttributeTarget::Body(id) if bodies.contains(id) => {
                Some(id.0.clone())
            }
            _ => None,
        };
        let Some(target_key) = target_key else {
            return Err(CodecError::InvalidInput(format!(
                "F3D persistent design link {} has an unsupported or missing target",
                link.id
            )));
        };
        if link.design_id.is_empty() || !link.design_id.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(CodecError::InvalidInput(format!(
                "F3D persistent body link {} has an invalid design id",
                link.id
            )));
        }
        groups.entry(target_key).or_default().push(link);
    }
    let mut subentity_groups: BTreeMap<(u8, String), Vec<&PersistentSubentityTag>> =
        BTreeMap::new();
    for tag in &native.persistent_subentity_tags {
        let target_key = match &tag.target {
            cadmpeg_ir::attributes::AttributeTarget::Face(id) if faces.contains(id) => {
                Some((2, id.0.clone()))
            }
            cadmpeg_ir::attributes::AttributeTarget::Edge(id) if edges.contains(id) => {
                Some((1, id.0.clone()))
            }
            _ => None,
        };
        let Some(target_key) = target_key else {
            return Err(CodecError::InvalidInput(format!(
                "F3D persistent subentity tag {} has an unsupported or missing target",
                tag.id
            )));
        };
        if tag.token.is_empty() {
            return Err(CodecError::InvalidInput(format!(
                "F3D persistent subentity tag {} requires a token",
                tag.id
            )));
        }
        subentity_groups.entry(target_key).or_default().push(tag);
    }
    for (target, mut tags) in subentity_groups {
        tags.sort_by_key(|tag| tag.ordinal);
        for (ordinal, tag) in tags.iter().enumerate() {
            if tag.ordinal != ordinal as u32 {
                return Err(CodecError::InvalidInput(format!(
                    "F3D persistent subentity tags for {target:?} require contiguous ordinals"
                )));
            }
        }
    }
    for (target, mut links) in groups {
        links.sort_by_key(|link| link.ordinal);
        for (ordinal, link) in links.iter().enumerate() {
            if link.ordinal != ordinal as u32 || link.is_current != (ordinal + 1 == links.len()) {
                return Err(CodecError::InvalidInput(format!(
                    "F3D persistent design links for {target:?} require contiguous ordinals and only the final link current"
                )));
            }
        }
    }

    let coedge_ids = target
        .model
        .coedges
        .iter()
        .map(|coedge| &coedge.id)
        .collect::<BTreeSet<_>>();
    let coedge_by_id = target
        .model
        .coedges
        .iter()
        .map(|coedge| (coedge.id.as_str(), coedge))
        .collect::<std::collections::HashMap<_, _>>();
    let curve_by_id = target
        .model
        .curves
        .iter()
        .map(|curve| (curve.id.as_str(), curve))
        .collect::<std::collections::HashMap<_, _>>();
    let mut tolerant_coedges = BTreeSet::new();
    for parameters in &native.tolerant_coedge_parameters {
        if !coedge_ids.contains(&parameters.coedge) {
            return Err(CodecError::InvalidInput(format!(
                "F3D tolerant-coedge metadata {} targets missing coedge {}",
                parameters.id(),
                parameters.coedge
            )));
        }
        if !tolerant_coedges.insert(&parameters.coedge) {
            return Err(CodecError::InvalidInput(format!(
                "multiple F3D tolerant-coedge records target {}",
                parameters.coedge
            )));
        }
        if parameters
            .parameter_range
            .iter()
            .any(|value| !value.is_finite())
        {
            return Err(CodecError::InvalidInput(format!(
                "F3D tolerant-coedge metadata {} has non-finite parameters",
                parameters.id()
            )));
        }
        match &parameters.extension {
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::None
            | cadmpeg_asm::brep::records::TolerantCoedgeExtension::Empty { target: None } => {}
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::EmbeddedCurve {
                target: None,
                parameter_range,
                ..
            } => {
                let coedge = coedge_by_id
                    .get(parameters.coedge.as_str())
                    .copied()
                    .expect("validated tolerant-coedge target");
                let use_curve = coedge.use_curve.as_ref().ok_or_else(|| {
                    CodecError::InvalidInput(format!(
                        "F3D tolerant-coedge extension {} has no use curve",
                        parameters.id()
                    ))
                })?;
                let curve = curve_by_id
                    .get(use_curve.curve.as_str())
                    .copied()
                    .ok_or_else(|| {
                        CodecError::InvalidInput(format!(
                            "F3D tolerant-coedge extension {} references missing use curve {}",
                            parameters.id(),
                            use_curve.curve
                        ))
                    })?;
                if !matches!(curve.geometry, CurveGeometry::Nurbs(_)) {
                    return Err(CodecError::NotImplemented(format!(
                        "source-less F3D tolerant-coedge extension {} requires a NURBS use curve",
                        parameters.id()
                    )));
                }
                let effective_range = parameter_range.unwrap_or(parameters.parameter_range);
                if effective_range.iter().any(|value| !value.is_finite())
                    || use_curve.parameter_range != effective_range
                {
                    return Err(CodecError::InvalidInput(format!(
                        "F3D tolerant-coedge extension {} has an inconsistent use-curve parameter range",
                        parameters.id()
                    )));
                }
            }
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::Empty { target: Some(_) }
            | cadmpeg_asm::brep::records::TolerantCoedgeExtension::Reference { .. }
            | cadmpeg_asm::brep::records::TolerantCoedgeExtension::EmbeddedCurve {
                target: Some(_),
                ..
            } => {
                return Err(CodecError::NotImplemented(format!(
                    "source-less F3D cannot relocate tolerant-coedge extension {}",
                    parameters.id()
                )));
            }
        }
    }

    let vertices = target
        .model
        .vertices
        .iter()
        .map(|item| &item.id)
        .collect::<BTreeSet<_>>();
    let shells = target
        .model
        .shells
        .iter()
        .map(|item| &item.id)
        .collect::<BTreeSet<_>>();
    let body_by_id = target
        .model
        .bodies
        .iter()
        .enumerate()
        .map(|(ordinal, body)| (body.id.as_str(), (ordinal, body)))
        .collect::<std::collections::HashMap<_, _>>();
    let vertex_by_id = target
        .model
        .vertices
        .iter()
        .map(|vertex| (vertex.id.as_str(), vertex))
        .collect::<std::collections::HashMap<_, _>>();
    let edge_by_id = target
        .model
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<std::collections::HashMap<_, _>>();
    let shell_by_id = target
        .model
        .shells
        .iter()
        .map(|shell| (shell.id.as_str(), shell))
        .collect::<std::collections::HashMap<_, _>>();
    let face_by_id = target
        .model
        .faces
        .iter()
        .map(|face| (face.id.as_str(), face))
        .collect::<std::collections::HashMap<_, _>>();
    macro_rules! validate_unique_targets {
        ($items:expr, $field:ident, $valid:expr, $label:literal) => {
            validate_unique_targets!($items, $field, $valid, $label, id());
        };
        ($items:expr, $field:ident, $valid:expr, $label:literal, $id:ident $( $call:tt )?) => {{
            let mut seen = BTreeSet::new();
            for item in $items {
                if !$valid.contains(&item.$field) {
                    return Err(CodecError::InvalidInput(format!(
                        "F3D {} metadata {} targets missing entity {}",
                        $label, item.$id $( $call )?, item.$field
                    )));
                }
                if !seen.insert(&item.$field) {
                    return Err(CodecError::InvalidInput(format!(
                        "multiple F3D {} records target {}",
                        $label, item.$field
                    )));
                }
            }
        }};
    }
    validate_unique_targets!(&native.body_native_keys, body, bodies, "body-native-key");
    validate_unique_targets!(
        &native.body_visibilities,
        body,
        bodies,
        "body-visibility",
        id
    );
    validate_unique_targets!(&native.transform_hints, body, bodies, "transform-hint");
    validate_unique_targets!(&native.edge_continuities, edge, edges, "edge-continuity");
    validate_unique_targets!(&native.edge_ownerships, edge, edges, "edge-ownership");
    validate_unique_targets!(
        &native.vertex_ownerships,
        vertex,
        vertices,
        "vertex-ownership"
    );
    validate_unique_targets!(&native.face_sidedness, face, faces, "face-sidedness");
    validate_unique_targets!(&native.tolerant_edge_tails, edge, edges, "tolerant-edge");
    validate_unique_targets!(
        &native.tolerant_vertex_tails,
        vertex,
        vertices,
        "tolerant-vertex"
    );
    let mut wire_record_indices = BTreeSet::new();
    for wire in &native.wire_topologies {
        if !shells.contains(&wire.shell) {
            return Err(CodecError::InvalidInput(format!(
                "F3D wire-topology metadata {} targets missing entity {}",
                wire.id(),
                wire.shell
            )));
        }
        if !wire_record_indices.insert(wire.record_index) {
            return Err(CodecError::InvalidInput(format!(
                "multiple F3D wire-topology records use native index {}",
                wire.record_index
            )));
        }
    }

    for visibility in &native.body_visibilities {
        let (ordinal, body) = body_by_id
            .get(visibility.body.as_str())
            .copied()
            .expect("validated body-visibility target");
        if body.visible != Some(visibility.visible) {
            return Err(CodecError::InvalidInput(format!(
                "F3D body visibility {} conflicts with body {} visibility",
                visibility.id, visibility.body
            )));
        }
        let emitted_key = source_less_body_key(attributes, body, ordinal)?;
        if u64::try_from(emitted_key).ok() != Some(visibility.asm_body_key) {
            return Err(CodecError::InvalidInput(format!(
                "F3D body visibility {} uses an ASM key different from body {}",
                visibility.id, visibility.body
            )));
        }
    }
    for hints in &native.transform_hints {
        if body_by_id
            .get(hints.body.as_str())
            .map(|(_, body)| *body)
            .is_none_or(|body| body.transform.is_none())
        {
            return Err(CodecError::InvalidInput(format!(
                "F3D transform hints {} target a body without a transform",
                hints.id()
            )));
        }
    }
    for tail in &native.tolerant_vertex_tails {
        if tail
            .leading_tolerances
            .iter()
            .any(|value| !value.is_finite())
            || vertex_by_id
                .get(tail.vertex.as_str())
                .copied()
                .is_none_or(|vertex| vertex.tolerance.is_none() && !tail.evaluated_unset)
        {
            return Err(CodecError::InvalidInput(format!(
                "F3D tolerant-vertex metadata {} requires finite fields and a tolerant vertex",
                tail.id()
            )));
        }
    }
    for tail in &native.tolerant_edge_tails {
        if edge_by_id
            .get(tail.edge.as_str())
            .copied()
            .is_none_or(|edge| edge.tolerance.is_none())
        {
            return Err(CodecError::InvalidInput(format!(
                "F3D tolerant-edge metadata {} requires a tolerant edge",
                tail.id()
            )));
        }
    }
    for wire in &native.wire_topologies {
        let shell = shell_by_id
            .get(wire.shell.as_str())
            .copied()
            .expect("validated wire-topology target");
        let member_form_is_valid = match (&wire.edges[..], &wire.free_vertex) {
            (edges, None) if !edges.is_empty() => {
                edges.iter().all(|edge| shell.wire_edges.contains(edge))
            }
            ([], Some(vertex)) => shell.free_vertices.contains(vertex),
            _ => false,
        };
        if !member_form_is_valid {
            return Err(CodecError::InvalidInput(format!(
                "F3D wire metadata {} has invalid edge-ring or isolated-vertex membership",
                wire.id()
            )));
        }
    }
    for sidedness in &native.face_sidedness {
        let face = face_by_id
            .get(sidedness.face.as_str())
            .copied()
            .expect("validated face-sidedness target");
        if sidedness.normalized_sense != face.sense {
            return Err(CodecError::InvalidInput(format!(
                "F3D face sidedness {} normalized sense conflicts with face {}",
                sidedness.id(),
                sidedness.face
            )));
        }
    }
    Ok(())
}

pub(crate) fn validate_source_less_body_kinds(
    model: &cadmpeg_ir::document::Model,
) -> Result<(), CodecError> {
    for body in &model.bodies {
        let shell_ids = model
            .regions
            .iter()
            .filter(|region| region.body == body.id)
            .flat_map(|region| &region.shells)
            .collect::<BTreeSet<_>>();
        let face_ids = model
            .shells
            .iter()
            .filter(|shell| shell_ids.contains(&shell.id))
            .flat_map(|shell| &shell.faces)
            .collect::<BTreeSet<_>>();
        let has_wires = model
            .shells
            .iter()
            .filter(|shell| shell_ids.contains(&shell.id))
            .any(|shell| !shell.wire_edges.is_empty() || !shell.free_vertices.is_empty());
        let loop_ids = model
            .faces
            .iter()
            .filter(|face| face_ids.contains(&face.id))
            .flat_map(|face| &face.loops)
            .collect::<BTreeSet<_>>();
        let coedge_ids = model
            .loops
            .iter()
            .filter(|loop_| loop_ids.contains(&loop_.id))
            .flat_map(|loop_| loop_.coedges())
            .collect::<BTreeSet<_>>();
        let mut uses = BTreeMap::<&cadmpeg_ir::ids::EdgeId, usize>::new();
        for coedge in model
            .coedges
            .iter()
            .filter(|coedge| coedge_ids.contains(&coedge.id))
        {
            *uses.entry(&coedge.edge).or_default() += 1;
        }
        let derived = if face_ids.is_empty() {
            cadmpeg_ir::topology::BodyKind::Wire
        } else if has_wires {
            cadmpeg_ir::topology::BodyKind::General
        } else if !uses.is_empty() && uses.values().all(|count| *count == 2) {
            cadmpeg_ir::topology::BodyKind::Solid
        } else {
            cadmpeg_ir::topology::BodyKind::Sheet
        };
        if body.kind != derived {
            return Err(CodecError::InvalidInput(format!(
                "body {} declares {:?} but its incidence graph is {:?}",
                body.id, body.kind, derived
            )));
        }
    }
    Ok(())
}

/// Proof that [`validate_source_less_wire_vertices`] ran against the borrowed
/// `CadIr`. The private field keeps construction inside this module, so the wire
/// encoder that maps free vertices to record ordinals cannot be reached without
/// the check having established that every free vertex exists in the model.
#[derive(Clone, Copy)]
pub(crate) struct WireVerticesValidated<'a> {
    target: &'a CadIr,
}

impl<'a> WireVerticesValidated<'a> {
    /// The `CadIr` whose wire vertices were validated.
    pub(super) fn target(self) -> &'a CadIr {
        self.target
    }
}

pub(crate) fn validate_source_less_wire_vertices(
    target: &CadIr,
) -> Result<WireVerticesValidated<'_>, CodecError> {
    let model = &target.model;
    let vertex_ids = model
        .vertices
        .iter()
        .map(|vertex| vertex.id.clone())
        .collect::<BTreeSet<_>>();
    let edge_vertex_ids = model
        .edges
        .iter()
        .flat_map(|edge| [&edge.start, &edge.end])
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut free_vertex_ids = BTreeSet::new();
    for vertex in model.shells.iter().flat_map(|shell| &shell.free_vertices) {
        if !vertex_ids.contains(vertex) {
            return Err(CodecError::InvalidInput(format!(
                "wire references missing free vertex {vertex}"
            )));
        }
        if edge_vertex_ids.contains(vertex) {
            return Err(CodecError::InvalidInput(format!(
                "wire vertex {vertex} is both free and an edge endpoint"
            )));
        }
        if !free_vertex_ids.insert(vertex.clone()) {
            return Err(CodecError::InvalidInput(format!(
                "free vertex {vertex} belongs to more than one wire"
            )));
        }
    }
    if vertex_ids != edge_vertex_ids.union(&free_vertex_ids).cloned().collect() {
        return Err(CodecError::InvalidInput(
            "source-less F3D vertices must be edge endpoints or free wire vertices".into(),
        ));
    }
    Ok(WireVerticesValidated { target })
}
