// SPDX-License-Identifier: Apache-2.0
//! Source-less pre-write validators for the neutral `CadIr` and its F3D native
//! extension.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::native::F3dNative;
use crate::records::{DesignObjectKind, PersistentDesignLink, PersistentSubentityTag};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};

use super::attributes::source_less_body_key;
use super::records::validate_dynamic_class_tag;
use crate::writer::primitives::{f3d_native, history_change_kind};

pub(crate) fn validate_source_less_procedural_carriers(target: &CadIr) -> Result<(), CodecError> {
    let mut surface_owners = BTreeSet::new();
    for procedural in &target.model.procedural_surfaces {
        if !surface_owners.insert(&procedural.surface) {
            return Err(CodecError::Malformed(format!(
                "surface {} has multiple procedural constructions",
                procedural.surface
            )));
        }
        let surface = target
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == procedural.surface)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "procedural surface {} references missing carrier {}",
                    procedural.id, procedural.surface
                ))
            })?;
        match &surface.geometry {
            SurfaceGeometry::Nurbs(_) | SurfaceGeometry::Unknown { .. } => {}
            SurfaceGeometry::Procedural { construction } if *construction == procedural.id => {}
            SurfaceGeometry::Procedural { construction } => {
                return Err(CodecError::Malformed(format!(
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
        if !curve_owners.insert(&procedural.curve) {
            return Err(CodecError::Malformed(format!(
                "curve {} has multiple procedural constructions",
                procedural.curve
            )));
        }
        let curve = target
            .model
            .curves
            .iter()
            .find(|curve| curve.id == procedural.curve)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "procedural curve {} references missing carrier {}",
                    procedural.id, procedural.curve
                ))
            })?;
        match &curve.geometry {
            CurveGeometry::Nurbs(_) => {}
            CurveGeometry::Procedural { construction }
                if *construction == procedural.id && procedural.cache_fit_tolerance.is_none() => {}
            CurveGeometry::Procedural { construction } => {
                return Err(CodecError::Malformed(format!(
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

pub(crate) fn validate_source_less_topology_tolerances(target: &CadIr) -> Result<(), CodecError> {
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
        return Err(CodecError::Malformed(format!(
            "F3D edge {} tolerance must be finite and nonnegative",
            edge.id
        )));
    }
    let tolerant = f3d_native(target)?
        .map(|native| native.tolerant_coedge_parameters)
        .unwrap_or_default();
    if let Some(coedge) = target.model.coedges.iter().find(|coedge| {
        coedge.use_curve.is_some()
            && !tolerant.iter().any(|parameters| {
                parameters.coedge == coedge.id
                    && matches!(
                        parameters.extension,
                        crate::records::TolerantCoedgeExtension::EmbeddedCurve { target: None, .. }
                    )
            })
    }) {
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
        return Err(CodecError::Malformed(
            "F3D construction recipes must be ordered by record index".into(),
        ));
    }
    let mut group_counts = HashMap::new();
    for recipe in &native.construction_recipes {
        let expected = group_counts
            .entry((recipe.kind, recipe.design_id.as_deref()))
            .or_insert(0u32);
        if recipe.recipe_index != *expected {
            return Err(CodecError::Malformed(format!(
                "F3D construction recipe {} has noncontiguous group index {}",
                recipe.id, recipe.recipe_index
            )));
        }
        *expected += 1;
    }
    Ok(())
}

pub(crate) fn validate_source_less_sketch_graph(native: &F3dNative) -> Result<(), CodecError> {
    let sketch_owners = native
        .design_entity_headers
        .iter()
        .filter(|header| header.object_kind == Some(DesignObjectKind::Sketch))
        .map(|header| header.entity_suffix)
        .collect::<BTreeSet<_>>();
    let root_indices = native
        .design_entity_headers
        .iter()
        .filter(|header| header.object_kind == Some(DesignObjectKind::Sketch))
        .flat_map(|header| header.reference_indices.iter().copied())
        .collect::<BTreeSet<_>>();
    let mut typed_indices = BTreeMap::<u32, &str>::new();
    for (record_index, id) in native
        .sketch_points
        .iter()
        .map(|record| (record.record_index, record.id.as_str()))
        .chain(
            native
                .sketch_curve_identities
                .iter()
                .map(|record| (record.record_index, record.id.as_str())),
        )
        .chain(
            native
                .sketch_relations
                .iter()
                .map(|record| (record.record_index, record.id.as_str())),
        )
        .chain(
            native
                .sketch_texts
                .iter()
                .map(|record| (record.record_index, record.id.as_str())),
        )
    {
        if let Some(before) = typed_indices.insert(record_index, id) {
            return Err(CodecError::Malformed(format!(
                "F3D sketch records {before} and {id} share record index {record_index}"
            )));
        }
    }
    for relation in &native.sketch_relations {
        if !root_indices.contains(&relation.record_index) {
            return Err(CodecError::Malformed(format!(
                "F3D sketch relation {} is not reachable from a sketch header",
                relation.id
            )));
        }
        if !sketch_owners.contains(&u64::from(relation.owner_reference)) {
            return Err(CodecError::Malformed(format!(
                "F3D sketch relation {} references missing sketch owner {}",
                relation.id, relation.owner_reference
            )));
        }
    }
    for text in &native.sketch_texts {
        if !root_indices.contains(&text.record_index) {
            return Err(CodecError::Malformed(format!(
                "F3D sketch text {} is not reachable from a sketch header",
                text.id
            )));
        }
        if !sketch_owners.contains(&u64::from(text.owner_reference)) {
            return Err(CodecError::Malformed(format!(
                "F3D sketch text {} references missing sketch owner {}",
                text.id, text.owner_reference
            )));
        }
    }
    let mut reachable_headers = root_indices;
    for relation in &native.sketch_relations {
        reachable_headers.extend(relation.members.iter().copied());
        reachable_headers.extend(relation.return_members.iter().copied());
    }
    let mut explicit_headers = BTreeSet::new();
    for header in &native.design_record_headers {
        if !explicit_headers.insert(header.record_index) {
            return Err(CodecError::Malformed(format!(
                "multiple F3D Design record headers use index {}",
                header.record_index
            )));
        }
        if typed_indices.contains_key(&header.record_index) {
            return Err(CodecError::Malformed(format!(
                "F3D Design record header {} shadows a typed sketch record",
                header.id
            )));
        }
        if !reachable_headers.contains(&header.record_index) {
            return Err(CodecError::Malformed(format!(
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
        let expected_prefix =
            crate::design::decode::parameters::design_parameter_prefix(&parameter.source_kind);
        if parameter.prefix_value != expected_prefix {
            return Err(CodecError::Malformed(format!(
                "F3D Design parameter {} has discriminator {}, expected {expected_prefix} for {}",
                parameter.id, parameter.prefix_value, parameter.source_kind
            )));
        }
        validate_dynamic_class_tag(&parameter.class_tag, "Design parameter")?;
        if parameter.kind != crate::records::DesignParameterKind::User
            || parameter.source_kind != "User Parameter"
            || parameter.owner_record_index.is_some()
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
            return Err(CodecError::Malformed(format!(
                "F3D Design parameter {} has an invalid document parameter value",
                parameter.id
            )));
        }
        if !parameter_indices.insert(parameter.record_index)
            || !parameter_ordinals.insert(parameter.source_ordinal)
        {
            return Err(CodecError::Malformed(format!(
                "F3D Design parameter {} duplicates a record index or source ordinal",
                parameter.id
            )));
        }
    }
    let mut objects_by_guid = BTreeMap::new();
    let mut entity_kinds = BTreeMap::new();
    for object in &native.design_objects {
        if objects_by_guid
            .insert(object.self_guid.as_str(), object)
            .is_some()
        {
            return Err(CodecError::Malformed(format!(
                "duplicate F3D Design object GUID: {}",
                object.self_guid
            )));
        }
        for entity_id in &object.entity_ids {
            if entity_kinds
                .insert(*entity_id, object.kind.clone())
                .is_some_and(|before| before != object.kind)
            {
                return Err(CodecError::Malformed(format!(
                    "F3D Design entity {entity_id} is owned by conflicting object kinds"
                )));
            }
        }
    }
    for object in &native.design_objects {
        if object.parent_guid.as_deref().is_some_and(|parent| {
            parent == object.self_guid || !objects_by_guid.contains_key(parent)
        }) {
            return Err(CodecError::Malformed(format!(
                "F3D Design object {} has a missing or self parent",
                object.id
            )));
        }
        let mut ancestors = BTreeSet::new();
        let mut cursor = object;
        while let Some(parent) = cursor.parent_guid.as_deref() {
            if !ancestors.insert(parent) {
                return Err(CodecError::Malformed(format!(
                    "F3D Design object hierarchy contains a cycle at {parent}"
                )));
            }
            cursor = objects_by_guid[parent];
        }
    }
    for header in &native.design_entity_headers {
        let suffix = header
            .entity_id
            .rsplit('_')
            .next()
            .and_then(|suffix| suffix.parse::<u64>().ok());
        if suffix != Some(header.entity_suffix) {
            return Err(CodecError::Malformed(format!(
                "F3D Design header {} entity id conflicts with suffix {}",
                header.id, header.entity_suffix
            )));
        }
        let owned_kind = entity_kinds.get(&header.entity_suffix).cloned();
        if header.object_kind != owned_kind {
            return Err(CodecError::Malformed(format!(
                "F3D Design header {} object kind conflicts with MetaStream ownership",
                header.id
            )));
        }
        if header.object_kind == Some(DesignObjectKind::Sketch) {
            // `record_reference` is absent on the sentinel (no-base-record)
            // reference-list form; the declared count must always match.
            if header.declared_reference_count != u32::try_from(header.reference_indices.len()).ok()
            {
                return Err(CodecError::Malformed(format!(
                    "F3D Design sketch header {} has an inconsistent reference list",
                    header.id
                )));
            }
        } else if header.record_reference.is_some()
            || header.declared_reference_count.is_some()
            || !header.reference_indices.is_empty()
        {
            return Err(CodecError::Malformed(format!(
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
            return Err(CodecError::Malformed(format!(
                "F3D Design body binding {id} conflicts with the body-map key/suffix bijection"
            )));
        }
        Ok(())
    };
    for assignment in &native.design_material_assignments {
        if assignment.physical_token.is_none() {
            return Err(CodecError::Malformed(format!(
                "F3D material assignment {} requires its physical-material token",
                assignment.id
            )));
        }
        let parsed_suffix = assignment
            .entity_id
            .rsplit('_')
            .next()
            .and_then(|suffix| suffix.parse::<u64>().ok());
        if parsed_suffix != Some(assignment.entity_suffix) {
            return Err(CodecError::Malformed(format!(
                "F3D material assignment {} entity id conflicts with suffix {}",
                assignment.id, assignment.entity_suffix
            )));
        }
        insert(
            assignment.asm_body_key,
            assignment.entity_suffix,
            &assignment.id,
        )?;
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

pub(crate) fn validate_source_less_act(native: &F3dNative) -> Result<(), CodecError> {
    let mut entity_keys = BTreeSet::new();
    for entity in &native.act_entities {
        if !entity_keys.insert((entity.record_index, entity.entity_id.as_str())) {
            return Err(CodecError::Malformed(format!(
                "duplicate F3D ACT entity identity: {}:{}",
                entity.record_index, entity.entity_id
            )));
        }
        if !entity.in_table && entity.channels.is_empty() {
            return Err(CodecError::Malformed(format!(
                "F3D ACT entity {} has neither a table row nor channels",
                entity.id
            )));
        }
        if entity.channels.is_empty() != entity.channel_class_tag.is_none() {
            return Err(CodecError::Malformed(format!(
                "F3D ACT entity {} requires a class tag exactly when channels are present",
                entity.id
            )));
        }
    }
    let mut channel_counts = BTreeMap::<&str, usize>::new();
    for guid in native
        .act_entities
        .iter()
        .flat_map(|entity| entity.channels.values())
    {
        *channel_counts.entry(guid).or_default() += 1;
    }
    let mut predicted = Vec::new();
    for (ordinal, guid) in native.act_guids.iter().enumerate() {
        if guid.ordinal != ordinal as u32 {
            return Err(CodecError::Malformed(
                "F3D ACT GUID ordinals must be contiguous in stream order".into(),
            ));
        }
        let remaining = channel_counts.entry(guid.guid.as_str()).or_default();
        if *remaining > 0 {
            *remaining -= 1;
        } else {
            predicted.push(guid.guid.as_str());
        }
    }
    predicted.extend(
        native
            .act_entities
            .iter()
            .flat_map(|entity| entity.channels.values().map(String::as_str)),
    );
    if predicted
        .into_iter()
        .ne(native.act_guids.iter().map(|guid| guid.guid.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "source-less F3D generation cannot preserve this ACT GUID pool ordering".into(),
        ));
    }
    Ok(())
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
                .map(|record| record.id.as_str())
                .collect::<BTreeSet<_>>();
            if unique.len() != records.len() {
                return Err(CodecError::Malformed(format!(
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
        return Err(CodecError::Malformed(
            "F3D ASM history graph contains orphaned or ambiguously parented records".into(),
        ));
    }
    for state in states {
        for board in &state.bulletin_boards {
            for change in &board.changes {
                if change.kind != history_change_kind(change.old_ref, change.new_ref)? {
                    return Err(CodecError::Malformed(format!(
                        "F3D entity change {} has a kind inconsistent with its references",
                        change.id
                    )));
                }
            }
        }
        for record in &state.records {
            if record.raw_bytes.is_empty() {
                return Err(CodecError::Malformed(format!(
                    "F3D ASM history record {} has an empty native payload",
                    record.id
                )));
            }
        }
    }
    for history in &native.asm_histories {
        match (history.stream_size, history.history_entry_count) {
            (Some(size), Some(entry_count))
                if history
                    .states
                    .first()
                    .is_some_and(|state| state.state_id == size)
                    && entry_count >= 0 => {}
            (Some(_), Some(_)) => {
                return Err(CodecError::Malformed(format!(
                    "F3D history {} requires head state_id == stream_size and nonnegative history_entry_count",
                    history.id
                )));
            }
            (None, None) => {}
            _ => {
                return Err(CodecError::Malformed(format!(
                    "F3D history {} has an incomplete history-stream preamble",
                    history.id
                )));
            }
        }
    }
    if native
        .asm_histories
        .iter()
        .any(|history| !crate::history::graph_is_coherent(history))
    {
        return Err(CodecError::Malformed(
            "F3D ASM history graph is not a coherent doubly linked state chain".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_source_less_design_links(
    target: &CadIr,
    native: &F3dNative,
) -> Result<(), CodecError> {
    if let Some(sentinel) = native.mesh_surface_sentinels.first() {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize mesh-surface sentinel {} without its retained ASM record",
            sentinel.id
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
        if !coedges.contains(&link.coedge) {
            return Err(CodecError::Malformed(format!(
                "F3D sketch-curve link {} targets a missing coedge {}",
                link.id, link.coedge.0
            )));
        }
        if !linked_coedges.insert(&link.coedge) {
            return Err(CodecError::Malformed(format!(
                "source-less F3D generation supports one sketch-curve link per coedge: {}",
                link.coedge.0
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
            return Err(CodecError::Malformed(format!(
                "F3D persistent design link {} has an unsupported or missing target",
                link.id
            )));
        };
        if link.entity_kind != 3
            || link.design_id.is_empty()
            || !link.design_id.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(CodecError::Malformed(format!(
                "F3D persistent body link {} has an invalid kind or design id",
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
            return Err(CodecError::Malformed(format!(
                "F3D persistent subentity tag {} has an unsupported or missing target",
                tag.id
            )));
        };
        if tag.token.is_empty() || tag.design_references.is_empty() {
            return Err(CodecError::Malformed(format!(
                "F3D persistent subentity tag {} requires a token and at least one reference",
                tag.id
            )));
        }
        subentity_groups.entry(target_key).or_default().push(tag);
    }
    for (target, mut tags) in subentity_groups {
        tags.sort_by_key(|tag| tag.ordinal);
        for (ordinal, tag) in tags.iter().enumerate() {
            if tag.ordinal != ordinal as u32 {
                return Err(CodecError::Malformed(format!(
                    "F3D persistent subentity tags for {target:?} require contiguous ordinals"
                )));
            }
        }
    }
    for (target, mut links) in groups {
        links.sort_by_key(|link| link.ordinal);
        for (ordinal, link) in links.iter().enumerate() {
            if link.ordinal != ordinal as u32 || link.is_current != (ordinal + 1 == links.len()) {
                return Err(CodecError::Malformed(format!(
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
    let mut tolerant_coedges = BTreeSet::new();
    for parameters in &native.tolerant_coedge_parameters {
        if !coedge_ids.contains(&parameters.coedge) {
            return Err(CodecError::Malformed(format!(
                "F3D tolerant-coedge metadata {} targets missing coedge {}",
                parameters.id, parameters.coedge
            )));
        }
        if !tolerant_coedges.insert(&parameters.coedge) {
            return Err(CodecError::Malformed(format!(
                "multiple F3D tolerant-coedge records target {}",
                parameters.coedge
            )));
        }
        if parameters
            .parameter_range
            .iter()
            .any(|value| !value.is_finite())
        {
            return Err(CodecError::Malformed(format!(
                "F3D tolerant-coedge metadata {} has non-finite parameters",
                parameters.id
            )));
        }
        match &parameters.extension {
            crate::records::TolerantCoedgeExtension::None
            | crate::records::TolerantCoedgeExtension::Empty { target: None } => {}
            crate::records::TolerantCoedgeExtension::EmbeddedCurve {
                target: None,
                parameter_range,
                ..
            } => {
                let coedge = target
                    .model
                    .coedges
                    .iter()
                    .find(|coedge| coedge.id == parameters.coedge)
                    .expect("validated tolerant-coedge target");
                let curve_id = coedge.use_curve.as_ref().ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "F3D tolerant-coedge extension {} has no use curve",
                        parameters.id
                    ))
                })?;
                let curve = target
                    .model
                    .curves
                    .iter()
                    .find(|curve| curve.id == *curve_id)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "F3D tolerant-coedge extension {} references missing use curve {curve_id}",
                            parameters.id
                        ))
                    })?;
                if !matches!(curve.geometry, CurveGeometry::Nurbs(_)) {
                    return Err(CodecError::NotImplemented(format!(
                        "source-less F3D tolerant-coedge extension {} requires a NURBS use curve",
                        parameters.id
                    )));
                }
                let effective_range = parameter_range.unwrap_or(parameters.parameter_range);
                if effective_range.iter().any(|value| !value.is_finite())
                    || coedge.use_curve_parameter_range != Some(effective_range)
                {
                    return Err(CodecError::Malformed(format!(
                        "F3D tolerant-coedge extension {} has an inconsistent use-curve parameter range",
                        parameters.id
                    )));
                }
            }
            crate::records::TolerantCoedgeExtension::Empty { target: Some(_) }
            | crate::records::TolerantCoedgeExtension::Reference { .. }
            | crate::records::TolerantCoedgeExtension::EmbeddedCurve {
                target: Some(_), ..
            } => {
                return Err(CodecError::NotImplemented(format!(
                    "source-less F3D cannot relocate tolerant-coedge extension {}",
                    parameters.id
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
    macro_rules! validate_unique_targets {
        ($items:expr, $field:ident, $valid:expr, $label:literal) => {{
            let mut seen = BTreeSet::new();
            for item in $items {
                if !$valid.contains(&item.$field) {
                    return Err(CodecError::Malformed(format!(
                        "F3D {} metadata {} targets missing entity {}",
                        $label, item.id, item.$field
                    )));
                }
                if !seen.insert(&item.$field) {
                    return Err(CodecError::Malformed(format!(
                        "multiple F3D {} records target {}",
                        $label, item.$field
                    )));
                }
            }
        }};
    }
    validate_unique_targets!(&native.body_native_keys, body, bodies, "body-native-key");
    validate_unique_targets!(&native.body_visibilities, body, bodies, "body-visibility");
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
            return Err(CodecError::Malformed(format!(
                "F3D wire-topology metadata {} targets missing entity {}",
                wire.id, wire.shell
            )));
        }
        if !wire_record_indices.insert(wire.record_index) {
            return Err(CodecError::Malformed(format!(
                "multiple F3D wire-topology records use native index {}",
                wire.record_index
            )));
        }
    }

    for visibility in &native.body_visibilities {
        let body = target
            .model
            .bodies
            .iter()
            .find(|body| body.id == visibility.body)
            .expect("validated body-visibility target");
        if body.visible != Some(visibility.visible) {
            return Err(CodecError::Malformed(format!(
                "F3D body visibility {} conflicts with body {} visibility",
                visibility.id, visibility.body
            )));
        }
        let ordinal = target
            .model
            .bodies
            .iter()
            .position(|body| body.id == visibility.body)
            .expect("validated body-visibility target");
        let emitted_key = source_less_body_key(target, body, ordinal)?;
        if u64::try_from(emitted_key).ok() != Some(visibility.asm_body_key) {
            return Err(CodecError::Malformed(format!(
                "F3D body visibility {} uses an ASM key different from body {}",
                visibility.id, visibility.body
            )));
        }
    }
    for hints in &native.transform_hints {
        if target
            .model
            .bodies
            .iter()
            .find(|body| body.id == hints.body)
            .is_none_or(|body| body.transform.is_none())
        {
            return Err(CodecError::Malformed(format!(
                "F3D transform hints {} target a body without a transform",
                hints.id
            )));
        }
    }
    for tail in &native.tolerant_vertex_tails {
        if tail
            .leading_tolerances
            .iter()
            .any(|value| !value.is_finite())
            || target
                .model
                .vertices
                .iter()
                .find(|vertex| vertex.id == tail.vertex)
                .is_none_or(|vertex| vertex.tolerance.is_none())
        {
            return Err(CodecError::Malformed(format!(
                "F3D tolerant-vertex metadata {} requires finite fields and a tolerant vertex",
                tail.id
            )));
        }
    }
    for tail in &native.tolerant_edge_tails {
        if tail.trailing_integers[1] != 0
            || target
                .model
                .edges
                .iter()
                .find(|edge| edge.id == tail.edge)
                .is_none_or(|edge| edge.tolerance.is_none())
        {
            return Err(CodecError::Malformed(format!(
                "F3D tolerant-edge metadata {} requires a tolerant edge and zero final LONG",
                tail.id
            )));
        }
    }
    for wire in &native.wire_topologies {
        let shell = target
            .model
            .shells
            .iter()
            .find(|shell| shell.id == wire.shell)
            .expect("validated wire-topology target");
        let member_form_is_valid = match (&wire.edges[..], &wire.free_vertex) {
            (edges, None) if !edges.is_empty() => {
                edges.iter().all(|edge| shell.wire_edges.contains(edge))
            }
            ([], Some(vertex)) => shell.free_vertices.contains(vertex),
            _ => false,
        };
        if !member_form_is_valid {
            return Err(CodecError::Malformed(format!(
                "F3D wire metadata {} has invalid edge-ring or isolated-vertex membership",
                wire.id
            )));
        }
    }
    for sidedness in &native.face_sidedness {
        let face = target
            .model
            .faces
            .iter()
            .find(|face| face.id == sidedness.face)
            .expect("validated face-sidedness target");
        if sidedness.normalized_sense != face.sense {
            return Err(CodecError::Malformed(format!(
                "F3D face sidedness {} normalized sense conflicts with face {}",
                sidedness.id, sidedness.face
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
            .flat_map(|loop_| &loop_.coedges)
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
            return Err(CodecError::Malformed(format!(
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
            return Err(CodecError::Malformed(format!(
                "wire references missing free vertex {vertex}"
            )));
        }
        if edge_vertex_ids.contains(vertex) {
            return Err(CodecError::Malformed(format!(
                "wire vertex {vertex} is both free and an edge endpoint"
            )));
        }
        if !free_vertex_ids.insert(vertex.clone()) {
            return Err(CodecError::Malformed(format!(
                "free vertex {vertex} belongs to more than one wire"
            )));
        }
    }
    if vertex_ids != edge_vertex_ids.union(&free_vertex_ids).cloned().collect() {
        return Err(CodecError::Malformed(
            "source-less F3D vertices must be edge endpoints or free wire vertices".into(),
        ));
    }
    Ok(WireVerticesValidated { target })
}
