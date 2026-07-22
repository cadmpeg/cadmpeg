// SPDX-License-Identifier: Apache-2.0
//! Encode source-less F3D archives and apply supported edits to retained source
//! archives.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Cursor, Read, Write};

use crate::history_records::{
    AsmBulletinBoard, AsmDeltaState, AsmEntityChange, AsmEntityChangeKind, AsmHistory,
};
use crate::native::F3dNative;
use crate::records::{
    ActEntity, ActGuid, ActRootComponent, ConstructionRecipeKind, CreationTimestamp,
    DesignMaterialAssignment, DesignObjectKind, LostEdgeReference, PersistentDesignLink,
    PersistentReferenceKind, SketchCurveGeometry, SketchCurveLink,
};
use cadmpeg_ir::codec::{Codec, CodecError, DecodeOptions};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    BlendRadiusLaw, Curve, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry,
    ProceduralCurve, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CoedgeId, ShellId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Body, Coedge, Color, Edge, Face, Sense};
use cadmpeg_ir::transform::Transform;
use zip::write::SimpleFileOptions;

use crate::{asm_header, decode, sab, F3dCodec};

fn f3d_native(ir: &CadIr) -> Result<Option<F3dNative>, CodecError> {
    if let Some(namespace) = ir.native.namespace("f3d") {
        if namespace.version != crate::native::F3D_NATIVE_VERSION {
            let version = namespace.version;
            return Err(CodecError::Malformed(format!(
                "unsupported F3D native namespace version {version}"
            )));
        }
    }
    ir.native
        .namespace("f3d")
        .map(F3dNative::load)
        .transpose()
        .map_err(Into::into)
}

fn validate_source_less_procedural_carriers(target: &CadIr) -> Result<(), CodecError> {
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
        if !matches!(
            surface.geometry,
            SurfaceGeometry::Nurbs(_) | SurfaceGeometry::Unknown { .. }
        ) {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D procedural surface {} cannot retain its construction on analytic carrier {}",
                procedural.id, surface.id
            )));
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
        if !matches!(curve.geometry, CurveGeometry::Nurbs(_)) {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D procedural curve {} cannot retain its construction on non-NURBS carrier {}",
                procedural.id, curve.id
            )));
        }
    }
    Ok(())
}

fn validate_source_less_topology_tolerances(target: &CadIr) -> Result<(), CodecError> {
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
    if let Some(edge) = target
        .model
        .edges
        .iter()
        .find(|edge| edge.tolerance.is_some())
    {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D cannot serialize edge {} tolerance until the tedge tolerance grammar is defined",
            edge.id
        )));
    }
    Ok(())
}

fn validate_source_less_auxiliary_geometry(target: &CadIr) -> Result<(), CodecError> {
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

/// Write a canonical source-less F3D archive for the currently supported
/// native construction profile.
pub(crate) fn write_new(target: &CadIr, writer: &mut dyn Write) -> Result<(), CodecError> {
    let native = f3d_native(target)?;
    if !target.model.subds.is_empty() {
        return Err(CodecError::NotImplemented(
            "source-less F3D generation does not support SubD surfaces".into(),
        ));
    }
    validate_source_less_procedural_carriers(target)?;
    validate_source_less_topology_tolerances(target)?;
    validate_source_less_auxiliary_geometry(target)?;
    if let Some(native) = &native {
        validate_source_less_history_graph(target, native)?;
        validate_source_less_act(native)?;
        validate_source_less_design_bindings(native)?;
        validate_source_less_design_ownership(native)?;
        validate_source_less_sketch_graph(native)?;
        validate_source_less_recipes(native)?;
        validate_source_less_design_links(target, native)?;
    }
    let smbh = encode_planar_triangle_smbh(target)?;
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    archive
        .start_file("Manifest.dat", options)
        .map_err(|error| CodecError::Malformed(format!("cannot create F3D manifest: {error}")))?;
    archive.write_all(b"cadmpeg-generated-f3d")?;
    archive
        .start_file("Properties.dat", options)
        .map_err(|error| CodecError::Malformed(format!("cannot create F3D properties: {error}")))?;
    archive.write_all(&0u32.to_le_bytes())?;
    archive
        .start_file(
            "FusionAssetName[Active]/Breps.BlobParts/BREP.generated.smbh",
            options,
        )
        .map_err(|error| CodecError::Malformed(format!("cannot create F3D BREP entry: {error}")))?;
    archive.write_all(&smbh)?;
    if let Some(native) = &native {
        let mut configuration_names = BTreeSet::new();
        let mut configuration_ids = BTreeSet::new();
        for configuration in &native.design_configurations {
            if !configuration_names.insert(configuration.entry_name.as_str())
                || !configuration_ids.insert(configuration.id.as_str())
            {
                return Err(CodecError::Malformed(format!(
                    "duplicate F3D configuration identity: {}",
                    configuration.entry_name
                )));
            }
            if !configuration.payload.is_object() {
                return Err(CodecError::Malformed(format!(
                    "F3D configuration JSON must be an object: {}",
                    configuration.entry_name
                )));
            }
            let valid_name = match configuration.kind {
                crate::records::DesignConfigurationKind::Table => {
                    configuration.entry_name.ends_with(".dsgcfg")
                }
                crate::records::DesignConfigurationKind::Rule => {
                    configuration.entry_name.ends_with(".dsgcfgrule")
                }
            };
            if !valid_name {
                return Err(CodecError::Malformed(format!(
                    "F3D configuration kind conflicts with entry name: {}",
                    configuration.entry_name
                )));
            }
            archive
                .start_file(&configuration.entry_name, options)
                .map_err(|error| {
                    CodecError::Malformed(format!("cannot create F3D configuration entry: {error}"))
                })?;
            let payload = serde_json::to_vec(&configuration.payload).map_err(|error| {
                CodecError::Malformed(format!("cannot encode F3D configuration JSON: {error}"))
            })?;
            archive.write_all(&payload)?;
        }
    }
    if let Some(bulk_stream) = encode_design_bulkstream(target)? {
        archive
            .start_file("FusionAssetName[Active]/Design1/BulkStream.dat", options)
            .map_err(|error| {
                CodecError::Malformed(format!("cannot create F3D Design BulkStream: {error}"))
            })?;
        archive.write_all(&bulk_stream)?;
    }
    if let Some(meta_stream) = encode_design_metastream(target)? {
        archive
            .start_file("FusionAssetName[Active]/Design1/MetaStream.dat", options)
            .map_err(|error| {
                CodecError::Malformed(format!("cannot create F3D Design MetaStream: {error}"))
            })?;
        archive.write_all(&meta_stream)?;
    }
    if let Some(act_stream) = encode_act_bulkstream(target)? {
        archive
            .start_file(
                "FusionAssetName[Active]/FusionACTSegmentType1/BulkStream.dat",
                options,
            )
            .map_err(|error| {
                CodecError::Malformed(format!("cannot create F3D ACT BulkStream: {error}"))
            })?;
        archive.write_all(&act_stream)?;
    }
    for (ordinal, appearance) in target.model.appearances.iter().enumerate() {
        let protein = crate::materials::encode_protein(appearance)?;
        archive
            .start_file(
                format!(
                    "FusionAssetName[Active]/ProteinAssets.BlobParts/ProteinAsset.{ordinal}.protein"
                ),
                options,
            )
            .map_err(|error| {
                CodecError::Malformed(format!("cannot create F3D Protein asset: {error}"))
            })?;
        archive.write_all(&protein)?;
    }
    let bytes = archive
        .finish()
        .map_err(|error| CodecError::Malformed(format!("cannot finish F3D archive: {error}")))?
        .into_inner();
    writer.write_all(&bytes)?;
    Ok(())
}

fn validate_source_less_recipes(native: &F3dNative) -> Result<(), CodecError> {
    if native
        .construction_recipes
        .windows(2)
        .any(|pair| pair[0].record_index > pair[1].record_index)
    {
        return Err(CodecError::Malformed(
            "F3D construction recipes must be ordered by record index".into(),
        ));
    }
    let mut record_indices = BTreeSet::new();
    let mut group_counts = HashMap::new();
    for recipe in &native.construction_recipes {
        if !record_indices.insert(recipe.record_index) {
            return Err(CodecError::Malformed(format!(
                "multiple F3D construction recipes use record index {}",
                recipe.record_index
            )));
        }
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
        if recipe.design_id_binary_u32 {
            let binary_id = recipe
                .design_id
                .as_deref()
                .and_then(|id| id.parse::<u32>().ok());
            if recipe.kind != ConstructionRecipeKind::BoundedFace
                || binary_id.is_none_or(|id| !(100..100_000).contains(&id))
                || binary_id.and_then(|id| i32::try_from(id).ok()) != Some(recipe.record_index)
            {
                return Err(CodecError::Malformed(format!(
                    "F3D binary bounded-face recipe {} requires design id == record index in [100, 100000)",
                    recipe.id
                )));
            }
        }
    }
    Ok(())
}

fn validate_source_less_sketch_graph(native: &F3dNative) -> Result<(), CodecError> {
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

fn validate_source_less_design_ownership(native: &F3dNative) -> Result<(), CodecError> {
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
                .insert(*entity_id, object.kind)
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
        let owned_kind = entity_kinds.get(&header.entity_suffix).copied();
        if header.object_kind != owned_kind {
            return Err(CodecError::Malformed(format!(
                "F3D Design header {} object kind conflicts with MetaStream ownership",
                header.id
            )));
        }
        if header.object_kind == Some(DesignObjectKind::Sketch) {
            if header.record_reference.is_none()
                || header.declared_reference_count
                    != u32::try_from(header.reference_indices.len()).ok()
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

fn validate_source_less_design_bindings(native: &F3dNative) -> Result<(), CodecError> {
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
    Ok(())
}

fn validate_source_less_act(native: &F3dNative) -> Result<(), CodecError> {
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

fn validate_source_less_history_graph(
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
        for record in &state.records {
            if record.raw_bytes.is_empty() {
                return Err(CodecError::Malformed(format!(
                    "F3D ASM history record {} has an empty native payload",
                    record.id
                )));
            }
        }
    }
    Ok(())
}

fn validate_source_less_design_links(target: &CadIr, native: &F3dNative) -> Result<(), CodecError> {
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
    let mut groups: BTreeMap<(u8, String), Vec<&PersistentDesignLink>> = BTreeMap::new();
    for link in &native.persistent_design_links {
        let target_key = match &link.target {
            cadmpeg_ir::attributes::AttributeTarget::Body(id) if bodies.contains(id) => {
                Some((3, id.0.clone()))
            }
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
                "F3D persistent design link {} has an unsupported or missing target",
                link.id
            )));
        };
        groups.entry(target_key).or_default().push(link);
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
    validate_unique_targets!(
        &native.tolerant_vertex_tails,
        vertex,
        vertices,
        "tolerant-vertex"
    );
    validate_unique_targets!(&native.wire_topologies, shell, shells, "wire-topology");

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
        if tail.trailing_floats.iter().any(|value| !value.is_finite())
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
    for wire in &native.wire_topologies {
        if target
            .model
            .shells
            .iter()
            .find(|shell| shell.id == wire.shell)
            .is_none_or(|shell| shell.wire_edges.is_empty())
        {
            return Err(CodecError::Malformed(format!(
                "F3D wire metadata {} targets a shell without wire edges",
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

fn tolerant_coedge_range(
    target: &CadIr,
    coedge: &CoedgeId,
) -> Result<Option<[f64; 2]>, CodecError> {
    Ok(f3d_native(target)?.and_then(|native| {
        native
            .tolerant_coedge_parameters
            .into_iter()
            .find(|parameters| parameters.coedge == *coedge)
            .map(|parameters| parameters.parameter_range)
    }))
}

fn encode_act_bulkstream(target: &CadIr) -> Result<Option<Vec<u8>>, CodecError> {
    let Some(native) = f3d_native(target)? else {
        return Ok(None);
    };
    if native.act_entities.is_empty()
        && native.act_guids.is_empty()
        && native.act_root_components.is_empty()
    {
        return Ok(None);
    }

    let mut out = Vec::new();
    native_lp_ascii(&mut out, "ACTTable")?;
    out.extend_from_slice(&0u16.to_le_bytes());
    let table_entities = native
        .act_entities
        .iter()
        .filter(|entity| entity.in_table)
        .collect::<Vec<_>>();
    let count = u32::try_from(table_entities.len())
        .map_err(|_| CodecError::Malformed("ACT table exceeds u32::MAX entities".into()))?;
    out.extend_from_slice(&count.to_le_bytes());
    for entity in table_entities {
        out.push(1);
        out.extend_from_slice(&entity.record_index.to_le_bytes());
        out.extend_from_slice(&[0; 6]);
        native_lp_utf16(&mut out, &entity.entity_id)?;
    }

    let channel_guids = native
        .act_entities
        .iter()
        .flat_map(|entity| entity.channels.values())
        .collect::<Vec<_>>();
    let mut emitted_channel_guids = BTreeMap::<&str, usize>::new();
    for guid in &channel_guids {
        *emitted_channel_guids.entry(guid.as_str()).or_default() += 1;
    }
    for guid in &native.act_guids {
        validate_guid(&guid.guid, "ACT GUID")?;
        let remaining = emitted_channel_guids.entry(guid.guid.as_str()).or_default();
        if *remaining > 0 {
            *remaining -= 1;
        } else {
            native_lp_utf16(&mut out, &guid.guid)?;
        }
    }
    for entity in native
        .act_entities
        .iter()
        .filter(|entity| !entity.channels.is_empty())
    {
        let class_tag = entity.channel_class_tag.as_deref().ok_or_else(|| {
            CodecError::Malformed("ACT channel entity lacks a dynamic class tag".into())
        })?;
        validate_dynamic_class_tag(class_tag, "ACT channel entity")?;
        native_lp_ascii(&mut out, class_tag)?;
        out.extend_from_slice(&entity.record_index.to_le_bytes());
        out.extend_from_slice(&[0; 10]);
        let channel_count = u32::try_from(entity.channels.len())
            .map_err(|_| CodecError::Malformed("ACT entity exceeds u32::MAX channels".into()))?;
        if !(1..=8).contains(&channel_count) {
            return Err(CodecError::NotImplemented(
                "source-less ACT channel groups require one to eight channels".into(),
            ));
        }
        out.extend_from_slice(&channel_count.to_le_bytes());
        for (name, guid) in &entity.channels {
            validate_guid(guid, "ACT channel GUID")?;
            native_lp_ascii(&mut out, name)?;
            native_lp_utf16(&mut out, guid)?;
        }
        native_lp_utf16(&mut out, &entity.entity_id)?;
    }
    for root in &native.act_root_components {
        validate_dynamic_class_tag(&root.class_tag, "ACT root component")?;
        native_lp_ascii(&mut out, &root.class_tag)?;
        out.extend_from_slice(&root.record_index.to_le_bytes());
        out.extend_from_slice(&[0; 10]);
        out.push(1);
        out.extend_from_slice(&root.instance_root_record.to_le_bytes());
        out.extend_from_slice(&[0; 6]);
        native_lp_utf16(&mut out, &root.entity_id)?;
        out.push(1);
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(&[0; 5]);
        out.push(1);
        out.extend_from_slice(&root.registry_flag.to_le_bytes());
        native_lp_utf16(&mut out, &root.display_name)?;
        out.push(0);
        out.push(1);
        out.extend_from_slice(&root.components_root_record.to_le_bytes());
    }
    Ok(Some(out))
}

fn encode_design_bulkstream(target: &CadIr) -> Result<Option<Vec<u8>>, CodecError> {
    let native = f3d_native(target)?.unwrap_or_default();
    let has_body_visibility = target
        .model
        .bodies
        .iter()
        .any(|body| body.visible.is_some());
    if native.construction_recipes.is_empty()
        && native.persistent_references.is_empty()
        && native.lost_edge_references.is_empty()
        && native.design_body_members.is_empty()
        && native.design_entity_headers.is_empty()
        && native.design_record_headers.is_empty()
        && native.design_material_assignments.is_empty()
        && native.sketch_points.is_empty()
        && native.sketch_curve_identities.is_empty()
        && native.sketch_relations.is_empty()
        && !has_body_visibility
    {
        return Ok(None);
    }

    let mut out = Vec::new();
    let mut body_map = native
        .design_material_assignments
        .iter()
        .map(|assignment| (assignment.asm_body_key, assignment.entity_suffix))
        .collect::<BTreeMap<_, _>>();
    let mut visibility_rows = Vec::new();
    for (ordinal, body) in target.model.bodies.iter().enumerate() {
        let Some(visible) = body.visible else {
            continue;
        };
        let metadata = native
            .body_visibilities
            .iter()
            .find(|metadata| metadata.body == body.id);
        let asm_body_key = match metadata {
            Some(metadata) => metadata.asm_body_key,
            None => u64::try_from(source_less_body_key(target, body, ordinal)?).map_err(|_| {
                CodecError::Malformed("source-less ASM body key is negative".into())
            })?,
        };
        let entity_suffix = metadata
            .map(|metadata| metadata.entity_suffix)
            .or_else(|| body_map.get(&asm_body_key).copied())
            .unwrap_or(asm_body_key);
        body_map.insert(asm_body_key, entity_suffix);
        visibility_rows.push((entity_suffix, visible));
    }
    if !body_map.is_empty() {
        let count = u32::try_from(body_map.len())
            .map_err(|_| CodecError::Malformed("Design body map exceeds u32::MAX".into()))?;
        out.extend_from_slice(&count.to_le_bytes());
        for (body_key, entity_suffix) in body_map {
            out.extend_from_slice(&body_key.to_le_bytes());
            out.extend_from_slice(&entity_suffix.to_le_bytes());
        }
        out.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        native_lp_utf16(&mut out, "BREP.generated.smbh")?;
    }
    for assignment in &native.design_material_assignments {
        native_lp_utf16(&mut out, &assignment.entity_id)?;
        native_lp_utf16(
            &mut out,
            assignment
                .physical_token
                .as_deref()
                .expect("validated source-less material token"),
        )?;
        native_lp_utf16(&mut out, "Body")?;
        native_lp_utf16(&mut out, "00000000-0000-0000-0000-000000000000")?;
        native_lp_utf16(&mut out, &assignment.visual_guid)?;
        native_lp_utf16(&mut out, "BA5EE55E-9982-449B-9D66-9F036540E140")?;
        if let Some(visual_preset) = &assignment.visual_preset {
            native_lp_utf16(&mut out, visual_preset)?;
        }
    }
    for (ordinal, (entity_suffix, visible)) in visibility_rows.into_iter().enumerate() {
        native_lp_utf16(
            &mut out,
            &format!("00000000-0000-0000-0000-{:012X}", ordinal + 1),
        )?;
        out.push(u8::from(!visible));
        out.extend_from_slice(&[0x01, 0x01]);
        out.extend_from_slice(&entity_suffix.to_le_bytes());
    }
    for recipe in &native.construction_recipes {
        let name = construction_recipe_name(recipe.kind);
        let mut prefix = [0u8; 27];
        if let Some(design_id) = &recipe.design_id {
            if recipe.design_id_binary_u32 {
                // The binary bounded-face layout aliases the record-index slot.
            } else if design_id.len() != 3 || !design_id.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(CodecError::Malformed(format!(
                    "source-less Design recipe id must be three ASCII digits: {design_id}"
                )));
            } else {
                prefix[0..4].copy_from_slice(&3u32.to_le_bytes());
                prefix[4..7].copy_from_slice(design_id.as_bytes());
            }
        }
        prefix[11..15].copy_from_slice(&recipe.record_index.to_le_bytes());
        prefix[23..27].copy_from_slice(
            &u32::try_from(name.len())
                .map_err(|_| CodecError::Malformed("Design recipe name exceeds u32::MAX".into()))?
                .to_le_bytes(),
        );
        out.extend_from_slice(&prefix);
        out.extend_from_slice(name);
        out.extend_from_slice(&(-1i64).to_le_bytes());
        for value in [2i32, 0, -1, 1, -1] {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    if !native.design_body_members.is_empty() {
        native_lp_ascii(&mut out, "BodiesRoot")?;
        out.extend_from_slice(&0u16.to_le_bytes());
        native_lp_ascii(&mut out, "BodiesRoot")?;
        let count = u32::try_from(native.design_body_members.len()).map_err(|_| {
            CodecError::Malformed("Design BodiesRoot exceeds u32::MAX members".into())
        })?;
        out.extend_from_slice(&count.to_le_bytes());
        for member in &native.design_body_members {
            out.push(1);
            out.extend_from_slice(&member.entity_suffix.to_le_bytes());
            out.extend_from_slice(&member.flags.to_le_bytes());
        }
        out.push(0);
    }
    for header in &native.design_entity_headers {
        validate_dynamic_class_tag(&header.class_tag, "Design entity header")?;
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(header.class_tag.as_bytes());
        out.extend_from_slice(&header.entity_suffix.to_le_bytes());
        out.extend_from_slice(&[0; 5]);
        out.push(u8::from(header.optional_slot_present));
        if header.optional_slot_present {
            out.extend_from_slice(&[0; 4]);
        }
        native_lp_utf16(&mut out, &header.entity_id)?;
        if header.object_kind == Some(DesignObjectKind::Sketch) {
            let record_reference = header.record_reference.ok_or_else(|| {
                CodecError::Malformed("Design sketch header lacks record_reference".into())
            })?;
            let count = u32::try_from(header.reference_indices.len()).map_err(|_| {
                CodecError::Malformed("Design sketch header exceeds u32::MAX references".into())
            })?;
            out.extend_from_slice(&record_reference.to_le_bytes());
            out.extend_from_slice(&[0; 4]);
            out.push(1);
            out.extend_from_slice(&count.to_le_bytes());
            for reference in &header.reference_indices {
                out.push(1);
                out.extend_from_slice(&reference.to_le_bytes());
                out.extend_from_slice(&[0; 6]);
            }
        }
    }
    for header in &native.design_record_headers {
        validate_dynamic_class_tag(&header.class_tag, "Design record header")?;
        native_lp_ascii(&mut out, &header.class_tag)?;
        out.extend_from_slice(&header.record_index.to_le_bytes());
    }
    for point in &native.sketch_points {
        encode_sketch_point(&mut out, point)?;
    }
    for curve in &native.sketch_curve_identities {
        encode_sketch_curve_identity(&mut out, curve)?;
    }
    for relation in &native.sketch_relations {
        encode_sketch_relation(&mut out, relation)?;
    }
    for reference in &native.persistent_references {
        out.extend_from_slice(persistent_reference_name(reference.kind));
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&14u32.to_le_bytes());
        out.extend_from_slice(&[0; 14]);
        out.extend_from_slice(&23u32.to_le_bytes());
        out.extend_from_slice(b"IntrinsicMetaTypeuint64");
        out.extend_from_slice(&reference.value.to_le_bytes());
    }
    for reference in &native.lost_edge_references {
        validate_dynamic_class_tag(&reference.class_tag, "lost-edge reference")?;
        out.extend_from_slice(b"EDGE_REFERENCE_LOST");
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(reference.class_tag.as_bytes());
        out.extend_from_slice(&reference.record_index.to_le_bytes());
    }
    Ok(Some(out))
}

fn encode_sketch_record_header(
    out: &mut [u8],
    class_tag: &str,
    record_index: u32,
) -> Result<(), CodecError> {
    validate_dynamic_class_tag(class_tag, "sketch record")?;
    out[0..4].copy_from_slice(&3u32.to_le_bytes());
    out[4..7].copy_from_slice(class_tag.as_bytes());
    out[7..11].copy_from_slice(&record_index.to_le_bytes());
    Ok(())
}

fn encode_sketch_point(
    out: &mut Vec<u8>,
    point: &crate::records::SketchPoint,
) -> Result<(), CodecError> {
    if !point.coordinates.u.is_finite() || !point.coordinates.v.is_finite() {
        return Err(CodecError::Malformed(
            "source-less sketch point coordinates must be finite".into(),
        ));
    }
    let shift = usize::from(point.entity_genesis.is_some()) * 52;
    let mut record = vec![0u8; 112 + shift];
    encode_sketch_record_header(&mut record, &point.class_tag, point.record_index)?;
    record[20] = 1;
    record[21..25].copy_from_slice(&(1 + u32::from(point.entity_genesis.is_some())).to_le_bytes());
    if let Some(entity_genesis) = point.entity_genesis {
        encode_entity_genesis(&mut record, entity_genesis);
    }
    record[25 + shift..29 + shift].copy_from_slice(&6u32.to_le_bytes());
    record[29 + shift..35 + shift].copy_from_slice(b"pt_tag");
    record[35 + shift..39 + shift].copy_from_slice(&23u32.to_le_bytes());
    record[39 + shift..62 + shift].copy_from_slice(b"IntrinsicMetaTypeuint64");
    record[62 + shift..70 + shift].copy_from_slice(&point.persistent_id.to_le_bytes());
    record[70 + shift] = 1;
    record[71 + shift..75 + shift].copy_from_slice(&point.paired_reference.to_le_bytes());
    record[89 + shift..97 + shift].copy_from_slice(&(point.coordinates.u / 10.0).to_le_bytes());
    record[97 + shift..105 + shift].copy_from_slice(&(point.coordinates.v / 10.0).to_le_bytes());
    out.extend_from_slice(&record);
    Ok(())
}

fn encode_sketch_curve_identity(
    out: &mut Vec<u8>,
    curve: &crate::records::SketchCurveIdentity,
) -> Result<(), CodecError> {
    let shift = usize::from(curve.entity_genesis.is_some()) * 52;
    let mut record = vec![0u8; 133 + shift];
    encode_sketch_record_header(&mut record, &curve.class_tag, curve.record_index)?;
    record[20] = 1;
    record[21..25].copy_from_slice(&(2 + u32::from(curve.entity_genesis.is_some())).to_le_bytes());
    if let Some(entity_genesis) = curve.entity_genesis {
        encode_entity_genesis(&mut record, entity_genesis);
    }
    record[25 + shift..29 + shift].copy_from_slice(&14u32.to_le_bytes());
    record[29 + shift..43 + shift].copy_from_slice(b"crv_primary_id");
    record[43 + shift..47 + shift].copy_from_slice(&23u32.to_le_bytes());
    record[47 + shift..70 + shift].copy_from_slice(b"IntrinsicMetaTypeuint64");
    record[70 + shift..78 + shift].copy_from_slice(&curve.primary_id.to_le_bytes());
    record[78 + shift..82 + shift].copy_from_slice(&16u32.to_le_bytes());
    record[82 + shift..98 + shift].copy_from_slice(b"crv_secondary_id");
    record[98 + shift..102 + shift].copy_from_slice(&23u32.to_le_bytes());
    record[102 + shift..125 + shift].copy_from_slice(b"IntrinsicMetaTypeuint64");
    record[125 + shift..133 + shift].copy_from_slice(&curve.secondary_id.to_le_bytes());
    match curve.geometry.as_ref() {
        Some(SketchCurveGeometry::Line {
            start,
            end,
            direction,
            normal,
        }) => {
            let values = [
                start.x / 10.0,
                start.y / 10.0,
                start.z / 10.0,
                (end.x - start.x) / 10.0,
                (end.y - start.y) / 10.0,
                (end.z - start.z) / 10.0,
                direction.x,
                direction.y,
                direction.z,
                normal.x,
                normal.y,
                normal.z,
            ];
            encode_f64_sequence(&mut record, &values)?;
        }
        Some(SketchCurveGeometry::Arc {
            center,
            normal,
            reference_direction,
            radius,
            start_angle,
            end_angle,
        }) => {
            let values = [
                center.x / 10.0,
                center.y / 10.0,
                center.z / 10.0,
                normal.x,
                normal.y,
                normal.z,
                reference_direction.x,
                reference_direction.y,
                reference_direction.z,
                radius / 10.0,
                *start_angle,
                *end_angle,
            ];
            encode_f64_sequence(&mut record, &values)?;
        }
        Some(SketchCurveGeometry::Nurbs {
            carrier_reference,
            subtype_class_tag,
            subtype_record_index,
            degree,
            fit_tolerance,
            scalar_width,
            knots,
            weights,
            control_points,
        }) => encode_sketch_nurbs(
            &mut record,
            *carrier_reference,
            subtype_class_tag,
            *subtype_record_index,
            *degree,
            *fit_tolerance,
            *scalar_width,
            knots,
            weights,
            control_points,
        )?,
        None => {}
    }
    out.extend_from_slice(&record);
    Ok(())
}

fn encode_entity_genesis(record: &mut [u8], entity_genesis: u64) {
    record[25..29].copy_from_slice(&13u32.to_le_bytes());
    record[29..42].copy_from_slice(b"EntityGenesis");
    record[42..46].copy_from_slice(&23u32.to_le_bytes());
    record[46..69].copy_from_slice(b"IntrinsicMetaTypeuint64");
    record[69..77].copy_from_slice(&entity_genesis.to_le_bytes());
}

fn encode_f64_sequence(out: &mut Vec<u8>, values: &[f64]) -> Result<(), CodecError> {
    if values.iter().any(|value| !value.is_finite()) {
        return Err(CodecError::Malformed(
            "source-less sketch geometry must contain finite scalars".into(),
        ));
    }
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn encode_sketch_nurbs(
    record: &mut Vec<u8>,
    carrier_reference: Option<u64>,
    subtype_class_tag: &str,
    subtype_record_index: u32,
    degree: u32,
    fit_tolerance: f64,
    scalar_width: u32,
    knots: &[f64],
    weights: &[f64],
    control_points: &[Point3],
) -> Result<(), CodecError> {
    validate_dynamic_class_tag(subtype_class_tag, "sketch NURBS subtype")?;
    if scalar_width != 8 || (!weights.is_empty() && weights.len() != control_points.len()) {
        return Err(CodecError::Malformed(
            "source-less sketch NURBS requires scalar width 8 and parallel weights".into(),
        ));
    }
    let expected_knots = control_points
        .len()
        .checked_add(usize::try_from(degree).unwrap_or(usize::MAX))
        .and_then(|count| count.checked_add(1));
    if expected_knots != Some(knots.len()) {
        return Err(CodecError::Malformed(
            "source-less sketch NURBS knot count must equal control points + degree + 1".into(),
        ));
    }
    record.extend_from_slice(&carrier_reference.unwrap_or(u64::MAX).to_le_bytes());
    record.extend_from_slice(&3u32.to_le_bytes());
    record.extend_from_slice(subtype_class_tag.as_bytes());
    record.extend_from_slice(&subtype_record_index.to_le_bytes());
    record.resize(133 + 88, 0);
    record.push(1);
    record.push(0);
    record.extend_from_slice(&degree.to_le_bytes());
    record.extend_from_slice(&(fit_tolerance / 10.0).to_le_bytes());
    let knot_count = u32::try_from(knots.len())
        .map_err(|_| CodecError::Malformed("sketch NURBS has too many knots".into()))?;
    record.extend_from_slice(&knot_count.to_le_bytes());
    record.extend_from_slice(&knot_count.to_le_bytes());
    record.extend_from_slice(&8u32.to_le_bytes());
    encode_f64_sequence(record, knots)?;
    let weight_count = u32::try_from(weights.len())
        .map_err(|_| CodecError::Malformed("sketch NURBS has too many weights".into()))?;
    record.extend_from_slice(&weight_count.to_le_bytes());
    record.extend_from_slice(&weight_count.to_le_bytes());
    record.extend_from_slice(&8u32.to_le_bytes());
    encode_f64_sequence(record, weights)?;
    let point_count = u32::try_from(control_points.len())
        .map_err(|_| CodecError::Malformed("sketch NURBS has too many control points".into()))?;
    record.extend_from_slice(&point_count.to_le_bytes());
    record.extend_from_slice(&point_count.to_le_bytes());
    record.extend_from_slice(&8u32.to_le_bytes());
    let coordinates = control_points
        .iter()
        .flat_map(|point| [point.x / 10.0, point.y / 10.0, point.z / 10.0])
        .collect::<Vec<_>>();
    encode_f64_sequence(record, &coordinates)
}

fn encode_sketch_relation(
    out: &mut Vec<u8>,
    relation: &crate::records::SketchRelation,
) -> Result<(), CodecError> {
    let (constraint_kinds, unknown_constraint_bits) =
        crate::design::decode_constraint_kinds(relation.state);
    if constraint_kinds != relation.constraint_kinds
        || unknown_constraint_bits != relation.unknown_constraint_bits
    {
        return Err(CodecError::Malformed(format!(
            "F3D sketch relation {} has a mask inconsistent with its typed constraint kinds",
            relation.id
        )));
    }
    let mut record = vec![0u8; 101];
    encode_sketch_record_header(&mut record, &relation.class_tag, relation.record_index)?;
    record[19] = 1;
    let member_count = u32::try_from(relation.members.len())
        .map_err(|_| CodecError::Malformed("sketch relation has too many members".into()))?;
    record[20..24].copy_from_slice(&member_count.to_le_bytes());
    let mut cursor = 24usize;
    for reference in relation
        .members
        .iter()
        .chain(&relation.auxiliary_references)
        .chain(std::iter::once(&relation.owner_reference))
    {
        write_marked_u32(&mut record, &mut cursor, *reference)?;
    }
    write_marked_u32(&mut record, &mut cursor, relation.state)?;
    let return_count = u32::try_from(relation.return_members.len())
        .map_err(|_| CodecError::Malformed("sketch relation has too many return members".into()))?;
    write_u32(&mut record, &mut cursor, return_count)?;
    for reference in &relation.return_members {
        write_marked_u32(&mut record, &mut cursor, *reference)?;
    }
    out.extend_from_slice(&record);
    Ok(())
}

fn write_marked_u32(out: &mut [u8], cursor: &mut usize, value: u32) -> Result<(), CodecError> {
    let end = cursor
        .checked_add(5)
        .ok_or_else(|| CodecError::Malformed("sketch relation record offset overflow".into()))?;
    let target = out.get_mut(*cursor..end).ok_or_else(|| {
        CodecError::NotImplemented("sketch relation does not fit canonical 101-byte record".into())
    })?;
    target[0] = 1;
    target[1..5].copy_from_slice(&value.to_le_bytes());
    *cursor = end;
    Ok(())
}

fn write_u32(out: &mut [u8], cursor: &mut usize, value: u32) -> Result<(), CodecError> {
    let end = cursor
        .checked_add(4)
        .ok_or_else(|| CodecError::Malformed("sketch relation record offset overflow".into()))?;
    out.get_mut(*cursor..end)
        .ok_or_else(|| {
            CodecError::NotImplemented("sketch relation does not fit canonical record".into())
        })?
        .copy_from_slice(&value.to_le_bytes());
    *cursor = end;
    Ok(())
}

fn construction_recipe_name(kind: ConstructionRecipeKind) -> &'static [u8] {
    match kind {
        ConstructionRecipeKind::Body => b"body_recipe_data",
        ConstructionRecipeKind::Face => b"face_recipe_data",
        ConstructionRecipeKind::BoundedFace => b"bounded_face_recipe_data",
        ConstructionRecipeKind::Edge => b"edge_recipe_data",
        ConstructionRecipeKind::Vertex => b"vertex_recipe_data",
    }
}

fn persistent_reference_name(kind: PersistentReferenceKind) -> &'static [u8] {
    match kind {
        PersistentReferenceKind::Point => b"pt_tag",
        PersistentReferenceKind::CurvePrimary => b"crv_primary_id",
        PersistentReferenceKind::CurveSecondary => b"crv_secondary_id",
    }
}

fn validate_dynamic_class_tag(value: &str, field: &str) -> Result<(), CodecError> {
    if value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_digit()) {
        Ok(())
    } else {
        Err(CodecError::Malformed(format!(
            "{field} class tag must be three ASCII digits: {value}"
        )))
    }
}

fn encode_design_metastream(target: &CadIr) -> Result<Option<Vec<u8>>, CodecError> {
    let Some(native) = f3d_native(target)? else {
        return Ok(None);
    };
    if native.design_objects.is_empty() {
        return Ok(None);
    }

    let mut out = Vec::new();
    for object in &native.design_objects {
        native_lp_ascii(&mut out, design_object_kind_name(object.kind))?;
        let count = u32::try_from(object.entity_ids.len()).map_err(|_| {
            CodecError::Malformed("Design object owns more than u32::MAX entities".into())
        })?;
        out.extend_from_slice(&count.to_le_bytes());
        for entity_id in &object.entity_ids {
            out.extend_from_slice(&entity_id.to_le_bytes());
        }
        validate_guid(&object.self_guid, "Design object self GUID")?;
        native_lp_ascii(&mut out, &object.self_guid)?;
        let zero_run_length = usize::try_from(object.zero_run_length).map_err(|_| {
            CodecError::Malformed("Design object zero-run length exceeds address space".into())
        })?;
        out.resize(
            out.len().checked_add(zero_run_length).ok_or_else(|| {
                CodecError::Malformed("Design MetaStream zero run exceeds address space".into())
            })?,
            0,
        );
        if let Some(parent_guid) = &object.parent_guid {
            validate_guid(parent_guid, "Design object parent GUID")?;
            native_lp_ascii(&mut out, parent_guid)?;
        }
        out.extend_from_slice(&object.revision.to_le_bytes());
    }
    Ok(Some(out))
}

fn design_object_kind_name(kind: DesignObjectKind) -> &'static str {
    match kind {
        DesignObjectKind::Fusion => "Fusion",
        DesignObjectKind::Body => "Body",
        DesignObjectKind::Component => "Component",
        DesignObjectKind::Geometry => "Geometry",
        DesignObjectKind::Sketch => "MSketch",
        DesignObjectKind::Dimension => "Dimension",
        DesignObjectKind::Scene => "Scene",
        DesignObjectKind::EntityTracking => "EntityTracking",
        DesignObjectKind::CommonData => "CommonData",
    }
}

fn native_lp_ascii(out: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    if !value.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(CodecError::Malformed(
            "Design MetaStream strings must contain printable ASCII".into(),
        ));
    }
    let length = u32::try_from(value.len())
        .map_err(|_| CodecError::Malformed("Design MetaStream string exceeds u32::MAX".into()))?;
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn native_lp_utf16(out: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    let units = value.encode_utf16().collect::<Vec<_>>();
    let length = u32::try_from(units.len())
        .map_err(|_| CodecError::Malformed("Design UTF-16 string exceeds u32::MAX".into()))?;
    out.extend_from_slice(&length.to_le_bytes());
    for unit in units {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    Ok(())
}

fn validate_guid(value: &str, field: &str) -> Result<(), CodecError> {
    let bytes = value.as_bytes();
    let valid = bytes.len() == 36
        && [8, 13, 18, 23]
            .into_iter()
            .all(|index| bytes.get(index) == Some(&b'-'))
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit());
    if valid {
        Ok(())
    } else {
        Err(CodecError::Malformed(format!(
            "{field} is not a canonical GUID: {value}"
        )))
    }
}

fn encode_planar_triangle_smbh(target: &CadIr) -> Result<Vec<u8>, CodecError> {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let model = &target.model;
    if model.faces.is_empty()
        && model
            .shells
            .iter()
            .any(|shell| !shell.wire_edges.is_empty())
    {
        return encode_wire_body_smbh(target);
    }
    validate_source_less_body_kinds(model)?;
    if model.faces.len() > 1
        || model.loops.len() > 1
        || model.surfaces.len() > 1
        || model
            .shells
            .iter()
            .any(|shell| !shell.wire_edges.is_empty())
        || model
            .bodies
            .iter()
            .any(|body| body.color.is_some() || body.transform.is_some())
        || model.faces.iter().any(|face| face.color.is_some())
    {
        return encode_multi_face_shell_smbh(target);
    }
    if model.bodies.is_empty()
        || model.regions.is_empty()
        || model.shells.is_empty()
        || model.faces.len() != 1
        || model.loops.len() != 1
        || model.coedges.len() < 3
        || model.edges.len() != model.coedges.len()
        || model.vertices.len() != model.coedges.len()
        || model.points.len() != model.coedges.len()
        || model.surfaces.len() != 1
    {
        return Err(CodecError::NotImplemented(
            "source-less F3D generation currently requires one polygonal planar face".into(),
        ));
    }
    let body = &model.bodies[0];
    let region = &model.regions[0];
    let shell = &model.shells[0];
    let face = &model.faces[0];
    let loop_ = &model.loops[0];
    let surface_geometry = &model.surfaces[0].geometry;
    if body.regions.as_slice() != [region.id.clone()]
        || region.body != body.id
        || region.shells.as_slice() != [shell.id.clone()]
        || shell.region != region.id
        || shell.faces.as_slice() != [face.id.clone()]
        || !shell.wire_edges.is_empty()
        || !shell.free_vertices.is_empty()
        || face.shell != shell.id
        || face.surface != model.surfaces[0].id
        || face.loops.as_slice() != [loop_.id.clone()]
        || loop_.face != face.id
        || loop_.coedges.len() != model.coedges.len()
        || body.transform.is_some()
    {
        return Err(CodecError::NotImplemented(
            "source-less F3D generation requires one directly owned polygonal face".into(),
        ));
    }

    let coedges = loop_
        .coedges
        .iter()
        .map(|id| {
            model
                .coedges
                .iter()
                .find(|coedge| coedge.id == *id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!("loop references missing coedge {id}"))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (index, coedge) in coedges.iter().enumerate() {
        if coedge.pcurves.len() > 1 {
            return Err(CodecError::NotImplemented(format!(
                "coedge {} has an ordered pcurve collection",
                coedge.id
            )));
        }
        let next = coedges[(index + 1) % coedges.len()];
        let previous = coedges[(index + coedges.len() - 1) % coedges.len()];
        if coedge.owner_loop != loop_.id
            || coedge.next != next.id
            || coedge.previous != previous.id
            || coedge.radial_next != coedge.id
        {
            return Err(CodecError::NotImplemented(
                "source-less F3D generation requires a laminar polygon coedge ring".into(),
            ));
        }
    }

    let curve_start = 7i64;
    let pcurve_start = native_record_index(curve_start, model.curves.len())?;
    let ref_pcurve_count = model
        .pcurves
        .iter()
        .map(pcurve_uses_ref_form)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|uses_ref_form| *uses_ref_form)
        .count();
    let pcurve_record_count = model
        .pcurves
        .len()
        .checked_add(ref_pcurve_count)
        .ok_or_else(|| CodecError::Malformed("pcurve record count overflows usize".into()))?;
    let coedge_start = native_record_index(pcurve_start, pcurve_record_count)?;
    let edge_start = native_record_index(coedge_start, coedges.len())?;
    let vertex_start = native_record_index(edge_start, model.edges.len())?;
    let point_start = native_record_index(vertex_start, model.vertices.len())?;

    let mut records = Vec::new();
    native_ident(&mut records, "asmheader")?;
    native_string(&mut records, "231.6.3.65535")?;
    records.push(0x11);

    native_ident(&mut records, "body")?;
    native_ref(&mut records, -1);
    native_i64(&mut records, source_less_body_key(target, body, 0)?);
    native_ref(&mut records, -1);
    native_ref(&mut records, 2);
    native_ref(&mut records, -1);
    native_ref(&mut records, -1);
    records.push(0x11);

    native_ident(&mut records, "region")?;
    native_ref(&mut records, -1);
    native_i64(&mut records, -1);
    native_ref(&mut records, -1);
    native_ref(&mut records, -1);
    native_ref(&mut records, 3);
    native_ref(&mut records, 1);
    records.push(0x11);

    native_ident(&mut records, "shell")?;
    native_ref(&mut records, -1);
    native_i64(&mut records, -1);
    native_ref(&mut records, -1);
    native_ref(&mut records, -1);
    native_ref(&mut records, -1);
    native_ref(&mut records, 4);
    native_ref(&mut records, -1);
    native_ref(&mut records, 2);
    records.push(0x11);

    native_ident(&mut records, "face")?;
    native_ref(&mut records, -1);
    native_i64(&mut records, -1);
    native_ref(&mut records, -1);
    native_ref(&mut records, -1);
    native_ref(&mut records, 5);
    native_ref(&mut records, 3);
    native_ref(&mut records, -1);
    native_ref(&mut records, 6);
    records.push(native_bool(
        native_face_sense(target, face)? == Sense::Reversed,
    ));
    native_face_sidedness(&mut records, target, face)?;
    records.push(0x11);

    native_ident(&mut records, "loop")?;
    native_ref(&mut records, -1);
    native_i64(&mut records, -1);
    native_ref(&mut records, -1);
    native_ref(&mut records, -1);
    native_ref(&mut records, coedge_start);
    native_ref(&mut records, 4);
    records.push(0x11);

    match *surface_geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            native_surface_base(&mut records, "plane")?;
            native_point(
                &mut records,
                [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0],
            );
            native_vector(&mut records, [normal.x, normal.y, normal.z]);
            native_vector(&mut records, [u_axis.x, u_axis.y, u_axis.z]);
            records.push(0x0b);
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => {
            native_surface_base(&mut records, "cone")?;
            native_point(
                &mut records,
                [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0],
            );
            native_vector(&mut records, [axis.x, axis.y, axis.z]);
            native_vector(
                &mut records,
                [
                    ref_direction.x * radius / 10.0,
                    ref_direction.y * radius / 10.0,
                    ref_direction.z * radius / 10.0,
                ],
            );
            native_f64(&mut records, 1.0);
            records.extend_from_slice(&[0x0b, 0x0b]);
            native_f64(&mut records, 0.0);
            native_f64(&mut records, 1.0);
            native_f64(&mut records, radius / 10.0);
            records.extend_from_slice(&[0x0b; 5]);
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } => {
            native_surface_base(&mut records, "cone")?;
            native_point(
                &mut records,
                [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0],
            );
            native_vector(&mut records, [axis.x, axis.y, axis.z]);
            native_vector(
                &mut records,
                [
                    ref_direction.x * radius / 10.0,
                    ref_direction.y * radius / 10.0,
                    ref_direction.z * radius / 10.0,
                ],
            );
            native_f64(&mut records, ratio);
            records.extend_from_slice(&[0x0b, 0x0b]);
            native_f64(&mut records, half_angle.sin());
            native_f64(&mut records, half_angle.cos());
            native_f64(&mut records, radius / 10.0);
            records.extend_from_slice(&[0x0b; 5]);
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            native_surface_base(&mut records, "sphere")?;
            native_point(
                &mut records,
                [center.x / 10.0, center.y / 10.0, center.z / 10.0],
            );
            native_f64(&mut records, radius / 10.0);
            native_vector(
                &mut records,
                [ref_direction.x, ref_direction.y, ref_direction.z],
            );
            native_vector(&mut records, [axis.x, axis.y, axis.z]);
            records.extend_from_slice(&[0x0b; 5]);
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            native_surface_base(&mut records, "torus")?;
            native_point(
                &mut records,
                [center.x / 10.0, center.y / 10.0, center.z / 10.0],
            );
            native_vector(&mut records, [axis.x, axis.y, axis.z]);
            native_f64(&mut records, major_radius / 10.0);
            native_f64(&mut records, minor_radius / 10.0);
            native_vector(
                &mut records,
                [ref_direction.x, ref_direction.y, ref_direction.z],
            );
            records.extend_from_slice(&[0x0b; 5]);
        }
        SurfaceGeometry::Nurbs(ref surface) => {
            if !native_procedural_surface(&mut records, target, &model.surfaces[0], surface)? {
                native_surface_base(&mut records, "spline")?;
                native_nurbs_surface(&mut records, surface)?;
            }
        }
        SurfaceGeometry::Procedural { .. } | SurfaceGeometry::Unknown { .. } => {
            if !native_cacheless_procedural_surface(&mut records, target, &model.surfaces[0])? {
                return Err(CodecError::NotImplemented(
                    "source-less F3D generation does not support this surface carrier".into(),
                ));
            }
        }
        SurfaceGeometry::Polygonal { .. } => {
            return Err(CodecError::NotImplemented(
                "source-less F3D generation does not support polygonal surface carriers".into(),
            ));
        }
        SurfaceGeometry::Transformed { .. } => {
            return Err(CodecError::NotImplemented(
                "source-less F3D generation does not support transformed surface carriers".into(),
            ));
        }
    }
    records.push(0x11);

    for carrier in &model.curves {
        match carrier.geometry {
            CurveGeometry::Line { origin, direction } => {
                native_curve_base(&mut records, "straight")?;
                native_point(
                    &mut records,
                    [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0],
                );
                native_vector(&mut records, [direction.x, direction.y, direction.z]);
            }
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } => {
                native_curve_base(&mut records, "ellipse")?;
                native_point(
                    &mut records,
                    [center.x / 10.0, center.y / 10.0, center.z / 10.0],
                );
                native_vector(&mut records, [axis.x, axis.y, axis.z]);
                native_vector(
                    &mut records,
                    [
                        ref_direction.x * radius / 10.0,
                        ref_direction.y * radius / 10.0,
                        ref_direction.z * radius / 10.0,
                    ],
                );
                native_f64(&mut records, 1.0);
            }
            CurveGeometry::Ellipse {
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius,
            } => {
                if major_radius == 0.0 {
                    return Err(CodecError::Malformed(
                        "source-less F3D ellipse has zero major radius".into(),
                    ));
                }
                native_curve_base(&mut records, "ellipse")?;
                native_point(
                    &mut records,
                    [center.x / 10.0, center.y / 10.0, center.z / 10.0],
                );
                native_vector(&mut records, [axis.x, axis.y, axis.z]);
                native_vector(
                    &mut records,
                    [
                        major_direction.x * major_radius / 10.0,
                        major_direction.y * major_radius / 10.0,
                        major_direction.z * major_radius / 10.0,
                    ],
                );
                native_f64(&mut records, minor_radius / major_radius);
            }
            CurveGeometry::Nurbs(ref curve) => {
                if !native_procedural_curve(&mut records, target, &carrier.id, curve)? {
                    native_curve_base(&mut records, "intcurve")?;
                    native_nurbs_curve(&mut records, curve)?;
                }
            }
            CurveGeometry::Degenerate { point } => {
                native_curve_base(&mut records, "degenerate_curve")?;
                native_point(
                    &mut records,
                    [point.x / 10.0, point.y / 10.0, point.z / 10.0],
                );
                records.extend_from_slice(&[0x0b, 0x0b]);
            }
            _ => {
                return Err(CodecError::NotImplemented(
                    "source-less F3D generation does not support this curve carrier".into(),
                ));
            }
        }
        records.push(0x11);
    }

    let ref_pcurve_start = native_record_index(pcurve_start, model.pcurves.len())?;
    let mut ref_pcurve_ordinal = 0usize;
    for pcurve in &model.pcurves {
        let companion_ref = pcurve_uses_ref_form(pcurve)?
            .then(|| native_record_index(ref_pcurve_start, ref_pcurve_ordinal))
            .transpose()?;
        native_pcurve(&mut records, pcurve, companion_ref)?;
        ref_pcurve_ordinal += usize::from(companion_ref.is_some());
        records.push(0x11);
    }
    for pcurve in model
        .pcurves
        .iter()
        .filter(|pcurve| pcurve_uses_ref_form(pcurve).is_ok_and(|value| value))
    {
        native_ref_pcurve_companion(&mut records, pcurve)?;
        records.push(0x11);
    }

    for (index, coedge) in coedges.iter().enumerate() {
        let edge_index = model
            .edges
            .iter()
            .position(|edge| edge.id == coedge.edge)
            .ok_or_else(|| {
                CodecError::Malformed(format!("coedge references missing edge {}", coedge.edge))
            })?;
        let tolerant_range = tolerant_coedge_range(target, &coedge.id)?;
        native_ident(
            &mut records,
            if tolerant_range.is_some() {
                "tcoedge"
            } else {
                "coedge"
            },
        )?;
        native_ref(&mut records, -1);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(
            &mut records,
            native_record_index(coedge_start, (index + 1) % coedges.len())?,
        );
        native_ref(
            &mut records,
            native_record_index(coedge_start, (index + coedges.len() - 1) % coedges.len())?,
        );
        native_ref(&mut records, -1);
        native_ref(&mut records, native_record_index(edge_start, edge_index)?);
        records.push(native_bool(coedge.sense == Sense::Reversed));
        native_ref(&mut records, 5);
        native_i64(&mut records, 0);
        let pcurve_ref = coedge
            .pcurves
            .first()
            .map(|use_| {
                let pcurve_id = &use_.pcurve;
                model
                    .pcurves
                    .iter()
                    .position(|pcurve| pcurve.id == *pcurve_id)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "coedge references missing pcurve {pcurve_id}"
                        ))
                    })
                    .and_then(|ordinal| native_record_index(pcurve_start, ordinal))
            })
            .transpose()?
            .unwrap_or(-1);
        native_ref(&mut records, pcurve_ref);
        if let Some(range) = tolerant_range {
            native_f64(&mut records, range[0]);
            native_f64(&mut records, range[1]);
        }
        records.push(0x11);
    }

    let mut edge_owners = BTreeMap::new();
    apply_native_edge_owners(target, coedge_start, &mut edge_owners)?;
    for edge in &model.edges {
        let start = model
            .vertices
            .iter()
            .position(|vertex| vertex.id == edge.start)
            .ok_or_else(|| {
                CodecError::Malformed(format!("edge references missing vertex {}", edge.start))
            })?;
        let end = model
            .vertices
            .iter()
            .position(|vertex| vertex.id == edge.end)
            .ok_or_else(|| {
                CodecError::Malformed(format!("edge references missing vertex {}", edge.end))
            })?;
        let curve_ref = edge
            .curve
            .as_ref()
            .map(|curve_id| {
                model
                    .curves
                    .iter()
                    .position(|curve| curve.id == *curve_id)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!("edge references missing curve {curve_id}"))
                    })
                    .and_then(|ordinal| native_record_index(curve_start, ordinal))
            })
            .transpose()?
            .unwrap_or(-1);
        let mut range = edge.param_range.unwrap_or([0.0, 1.0]);
        // Conic edge parameters are angles in both the IR and the native
        // stream; line parameters are arc lengths, millimeters in the IR
        // and centimeters natively.
        if edge.curve.as_ref().is_some_and(|curve_id| {
            model.curves.iter().any(|curve| {
                curve.id == *curve_id && matches!(curve.geometry, CurveGeometry::Line { .. })
            })
        }) {
            range[0] /= 10.0;
            range[1] /= 10.0;
        }
        native_ident(&mut records, "edge")?;
        native_ref(&mut records, -1);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(&mut records, native_record_index(vertex_start, start)?);
        native_f64(&mut records, range[0]);
        native_ref(&mut records, native_record_index(vertex_start, end)?);
        native_f64(&mut records, range[1]);
        native_ref(
            &mut records,
            edge_owners.get(&edge.id).copied().unwrap_or(-1),
        );
        native_ref(&mut records, curve_ref);
        let (sense, continuity) = edge_record_metadata(target, edge)?;
        records.push(native_bool(sense == Sense::Reversed));
        native_string(&mut records, &continuity)?;
        records.push(0x11);
    }

    for vertex in &model.vertices {
        let point = model
            .points
            .iter()
            .position(|point| point.id == vertex.point)
            .ok_or_else(|| {
                CodecError::Malformed(format!("vertex references missing point {}", vertex.point))
            })?;
        let (owning_edge, endpoint_index) = vertex_ownership(target, vertex)?;
        native_ident(
            &mut records,
            if vertex.tolerance.is_some() {
                "tvertex"
            } else {
                "vertex"
            },
        )?;
        native_ref(&mut records, -1);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(&mut records, native_record_index(edge_start, owning_edge)?);
        native_i64(&mut records, i64::from(endpoint_index));
        native_ref(&mut records, native_record_index(point_start, point)?);
        native_tolerant_vertex_tail(&mut records, target, vertex)?;
        records.push(0x11);
    }

    for point in &model.points {
        native_ident(&mut records, "point")?;
        native_ref(&mut records, -1);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_point(
            &mut records,
            [
                point.position.x / 10.0,
                point.position.y / 10.0,
                point.position.z / 10.0,
            ],
        );
        records.push(0x11);
    }
    native_history_tail(&mut records, target)?;

    let mut bytes = native_smbh_header(target)?;
    bytes.extend_from_slice(&records);
    Ok(bytes)
}

#[derive(Debug, Clone, Copy)]
struct NativeRecordPlan {
    body: i64,
    region: i64,
    shell: i64,
    wire: i64,
    face: i64,
    loop_: i64,
    surface: i64,
    curve: i64,
    pcurve: i64,
    coedge: i64,
    wire_coedge: i64,
    edge: i64,
    vertex: i64,
    point: i64,
    transform: i64,
    attribute: i64,
}

impl NativeRecordPlan {
    fn for_model(model: &cadmpeg_ir::document::Model) -> Result<Self, CodecError> {
        let body_start = 1;
        let region_start = native_record_index(body_start, model.bodies.len())?;
        let shell_start = native_record_index(region_start, model.regions.len())?;
        let wire_count = model
            .shells
            .iter()
            .filter(|shell| !shell.wire_edges.is_empty())
            .count();
        let wire_start = native_record_index(shell_start, model.shells.len())?;
        let face_start = native_record_index(wire_start, wire_count)?;
        let loop_start = native_record_index(face_start, model.faces.len())?;
        let surface_start = native_record_index(loop_start, model.loops.len())?;
        let curve_start = native_record_index(surface_start, model.surfaces.len())?;
        let pcurve_start = native_record_index(curve_start, model.curves.len())?;
        let ref_pcurve_count = model
            .pcurves
            .iter()
            .map(pcurve_uses_ref_form)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|uses_ref_form| *uses_ref_form)
            .count();
        let pcurve_record_count = model
            .pcurves
            .len()
            .checked_add(ref_pcurve_count)
            .ok_or_else(|| CodecError::Malformed("pcurve record count overflows usize".into()))?;
        let coedge_start = native_record_index(pcurve_start, pcurve_record_count)?;
        let wire_coedge_start = native_record_index(coedge_start, model.coedges.len())?;
        let wire_edge_count = model
            .shells
            .iter()
            .map(|shell| shell.wire_edges.len())
            .sum::<usize>();
        let edge_start = native_record_index(wire_coedge_start, wire_edge_count)?;
        let vertex_start = native_record_index(edge_start, model.edges.len())?;
        let point_start = native_record_index(vertex_start, model.vertices.len())?;
        let transform_start = native_record_index(point_start, model.points.len())?;
        let transform_count = model
            .bodies
            .iter()
            .filter(|body| body.transform.is_some())
            .count();
        let attribute_start = native_record_index(transform_start, transform_count)?;
        Ok(Self {
            body: body_start,
            region: region_start,
            shell: shell_start,
            wire: wire_start,
            face: face_start,
            loop_: loop_start,
            surface: surface_start,
            curve: curve_start,
            pcurve: pcurve_start,
            coedge: coedge_start,
            wire_coedge: wire_coedge_start,
            edge: edge_start,
            vertex: vertex_start,
            point: point_start,
            transform: transform_start,
            attribute: attribute_start,
        })
    }
}

fn wire_record_for_shell(
    model: &cadmpeg_ir::document::Model,
    wire_start: i64,
    shell_ordinal: usize,
) -> Result<i64, CodecError> {
    let wire_ordinal = model.shells[..shell_ordinal]
        .iter()
        .filter(|shell| !shell.wire_edges.is_empty())
        .count();
    native_record_index(wire_start, wire_ordinal)
}

fn encode_wire_body_smbh(target: &CadIr) -> Result<Vec<u8>, CodecError> {
    let model = &target.model;
    if model.bodies.is_empty()
        || model.regions.is_empty()
        || model.shells.is_empty()
        || !model.faces.is_empty()
        || !model.loops.is_empty()
        || !model.coedges.is_empty()
        || !model.surfaces.is_empty()
        || !model.pcurves.is_empty()
        || model
            .shells
            .iter()
            .any(|shell| !shell.free_vertices.is_empty() || shell.wire_edges.is_empty())
        || model
            .shells
            .iter()
            .flat_map(|shell| &shell.wire_edges)
            .zip(&model.edges)
            .any(|(id, edge)| *id != edge.id)
        || model
            .shells
            .iter()
            .map(|shell| shell.wire_edges.len())
            .sum::<usize>()
            != model.edges.len()
        || model
            .bodies
            .iter()
            .any(|body| body.kind != cadmpeg_ir::topology::BodyKind::Wire)
    {
        return Err(CodecError::NotImplemented(
            "source-less F3D wire generation requires one face-less shell per wire body".into(),
        ));
    }
    for body in &model.bodies {
        if body.regions.is_empty()
            || body.regions.iter().any(|id| {
                !model
                    .regions
                    .iter()
                    .any(|region| region.id == *id && region.body == body.id)
            })
        {
            return Err(CodecError::Malformed(
                "source-less F3D wire ownership is inconsistent".into(),
            ));
        }
    }
    for region in &model.regions {
        if region.shells.is_empty()
            || !model
                .bodies
                .iter()
                .any(|body| body.id == region.body && body.regions.contains(&region.id))
            || region.shells.iter().any(|id| {
                !model
                    .shells
                    .iter()
                    .any(|shell| shell.id == *id && shell.region == region.id)
            })
        {
            return Err(CodecError::Malformed(
                "source-less F3D wire ownership is inconsistent".into(),
            ));
        }
    }
    if model.shells.iter().any(|shell| {
        !model
            .regions
            .iter()
            .any(|region| region.id == shell.region && region.shells.contains(&shell.id))
    }) {
        return Err(CodecError::Malformed(
            "source-less F3D wire ownership is inconsistent".into(),
        ));
    }
    let body_start = 1i64;
    let region_start = native_record_index(body_start, model.bodies.len())?;
    let shell_start = native_record_index(region_start, model.regions.len())?;
    let wire_start = native_record_index(shell_start, model.shells.len())?;
    let wire_coedge_start = native_record_index(wire_start, model.shells.len())?;
    let curve_start = native_record_index(wire_coedge_start, model.edges.len())?;
    let edge_start = native_record_index(curve_start, model.curves.len())?;
    let vertex_start = native_record_index(edge_start, model.edges.len())?;
    let point_start = native_record_index(vertex_start, model.vertices.len())?;
    let transform_start = native_record_index(point_start, model.points.len())?;
    let transform_count = model
        .bodies
        .iter()
        .filter(|body| body.transform.is_some())
        .count();
    let attribute_start = native_record_index(transform_start, transform_count)?;

    let mut records = Vec::new();
    native_ident(&mut records, "asmheader")?;
    native_string(&mut records, "231.6.3.65535")?;
    records.push(0x11);
    for (ordinal, body) in model.bodies.iter().enumerate() {
        let first_region = body.regions.first().expect("wire ownership was validated");
        let region_ordinal = model
            .regions
            .iter()
            .position(|region| region.id == *first_region)
            .expect("wire ownership was validated");
        let first_shell = model.regions[region_ordinal]
            .shells
            .first()
            .expect("wire ownership was validated");
        let shell_ordinal = model
            .shells
            .iter()
            .position(|shell| shell.id == *first_shell)
            .expect("wire ownership was validated");
        let transform_ordinal = model.bodies[..ordinal]
            .iter()
            .filter(|candidate| candidate.transform.is_some())
            .count();
        native_ident(&mut records, "body")?;
        native_ref(
            &mut records,
            owner_color_or_body_tag_ref(target, body, ordinal, attribute_start)?,
        );
        native_i64(&mut records, source_less_body_key(target, body, ordinal)?);
        native_ref(&mut records, -1);
        native_ref(
            &mut records,
            native_record_index(region_start, region_ordinal)?,
        );
        native_ref(
            &mut records,
            native_record_index(wire_start, shell_ordinal)?,
        );
        native_ref(
            &mut records,
            if body.transform.is_some() {
                native_record_index(transform_start, transform_ordinal)?
            } else {
                -1
            },
        );
        records.push(0x11);
    }
    for region in &model.regions {
        let body_ordinal = model
            .bodies
            .iter()
            .position(|body| body.id == region.body)
            .expect("wire ownership was validated");
        let body = &model.bodies[body_ordinal];
        let position = body
            .regions
            .iter()
            .position(|id| *id == region.id)
            .expect("wire ownership was validated");
        let next = body
            .regions
            .get(position + 1)
            .map(|id| {
                model
                    .regions
                    .iter()
                    .position(|candidate| candidate.id == *id)
                    .expect("wire ownership was validated")
            })
            .map(|position| native_record_index(region_start, position))
            .transpose()?
            .unwrap_or(-1);
        let first_shell = region.shells.first().expect("wire ownership was validated");
        let shell_ordinal = model
            .shells
            .iter()
            .position(|shell| shell.id == *first_shell)
            .expect("wire ownership was validated");
        native_ident(&mut records, "region")?;
        native_ref(&mut records, next);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(
            &mut records,
            native_record_index(shell_start, shell_ordinal)?,
        );
        native_ref(&mut records, native_record_index(body_start, body_ordinal)?);
        records.push(0x11);
    }
    for (ordinal, shell) in model.shells.iter().enumerate() {
        let region_ordinal = model
            .regions
            .iter()
            .position(|region| region.id == shell.region)
            .expect("wire ownership was validated");
        let region = &model.regions[region_ordinal];
        let position = region
            .shells
            .iter()
            .position(|id| *id == shell.id)
            .expect("wire ownership was validated");
        let next = region
            .shells
            .get(position + 1)
            .map(|id| {
                model
                    .shells
                    .iter()
                    .position(|candidate| candidate.id == *id)
                    .expect("wire ownership was validated")
            })
            .map(|position| native_record_index(shell_start, position))
            .transpose()?
            .unwrap_or(-1);
        native_ident(&mut records, "shell")?;
        native_ref(&mut records, next);
        native_i64(&mut records, -1);
        for reference in [
            -1,
            -1,
            -1,
            -1,
            native_record_index(wire_start, ordinal)?,
            native_record_index(region_start, region_ordinal)?,
        ] {
            native_ref(&mut records, reference);
        }
        records.push(0x11);
    }
    let mut edge_base = 0usize;
    for (shell_ordinal, shell) in model.shells.iter().enumerate() {
        native_ident(&mut records, "wire")?;
        native_ref(&mut records, -1);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(
            &mut records,
            native_record_index(wire_coedge_start, edge_base)?,
        );
        native_ref(
            &mut records,
            native_record_index(shell_start, shell_ordinal)?,
        );
        native_ref(&mut records, -1);
        records.push(native_wire_side(target, &shell.id)?);
        records.push(0x11);
        edge_base += shell.wire_edges.len();
    }
    edge_base = 0;
    for (shell_ordinal, shell) in model.shells.iter().enumerate() {
        for ordinal in 0..shell.wire_edges.len() {
            let edge_ordinal = edge_base + ordinal;
            let next = edge_base + (ordinal + 1) % shell.wire_edges.len();
            let previous =
                edge_base + (ordinal + shell.wire_edges.len() - 1) % shell.wire_edges.len();
            native_ident(&mut records, "coedge")?;
            native_ref(&mut records, -1);
            native_i64(&mut records, -1);
            native_ref(&mut records, -1);
            native_ref(&mut records, native_record_index(wire_coedge_start, next)?);
            native_ref(
                &mut records,
                native_record_index(wire_coedge_start, previous)?,
            );
            native_ref(&mut records, -1);
            native_ref(&mut records, native_record_index(edge_start, edge_ordinal)?);
            records.push(0x0b);
            native_ref(
                &mut records,
                native_record_index(wire_start, shell_ordinal)?,
            );
            native_i64(&mut records, 0);
            native_ref(&mut records, -1);
            records.push(0x11);
        }
        edge_base += shell.wire_edges.len();
    }
    encode_source_less_curves(&mut records, target)?;
    let mut wire_edge_owners = model
        .edges
        .iter()
        .enumerate()
        .map(|(ordinal, edge)| {
            native_record_index(wire_coedge_start, ordinal).map(|owner| (edge.id.clone(), owner))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    apply_native_edge_owners(target, wire_coedge_start, &mut wire_edge_owners)?;
    encode_source_less_edges_vertices_points(
        &mut records,
        target,
        curve_start,
        edge_start,
        vertex_start,
        point_start,
        attribute_start,
        Some(&wire_edge_owners),
    )?;
    for body in &model.bodies {
        if let Some(transform) = body.transform {
            native_transform(&mut records, target, body, transform)?;
            records.push(0x11);
        }
    }
    encode_source_less_attributes(&mut records, target, attribute_start)?;
    native_history_tail(&mut records, target)?;
    let mut bytes = native_smbh_header(target)?;
    bytes.extend_from_slice(&records);
    Ok(bytes)
}

fn encode_source_less_curves(records: &mut Vec<u8>, target: &CadIr) -> Result<(), CodecError> {
    let model = &target.model;
    for carrier in &model.curves {
        match carrier.geometry {
            CurveGeometry::Line { origin, direction } => {
                native_curve_base(records, "straight")?;
                native_point(records, [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0]);
                native_vector(records, [direction.x, direction.y, direction.z]);
            }
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } => {
                native_curve_base(records, "ellipse")?;
                native_point(records, [center.x / 10.0, center.y / 10.0, center.z / 10.0]);
                native_vector(records, [axis.x, axis.y, axis.z]);
                native_vector(
                    records,
                    [
                        ref_direction.x * radius / 10.0,
                        ref_direction.y * radius / 10.0,
                        ref_direction.z * radius / 10.0,
                    ],
                );
                native_f64(records, 1.0);
            }
            CurveGeometry::Ellipse {
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius,
            } if major_radius != 0.0 => {
                native_curve_base(records, "ellipse")?;
                native_point(records, [center.x / 10.0, center.y / 10.0, center.z / 10.0]);
                native_vector(records, [axis.x, axis.y, axis.z]);
                native_vector(
                    records,
                    [
                        major_direction.x * major_radius / 10.0,
                        major_direction.y * major_radius / 10.0,
                        major_direction.z * major_radius / 10.0,
                    ],
                );
                native_f64(records, minor_radius / major_radius);
            }
            CurveGeometry::Nurbs(ref curve) => {
                if !native_procedural_curve(records, target, &carrier.id, curve)? {
                    native_curve_base(records, "intcurve")?;
                    native_nurbs_curve(records, curve)?;
                }
            }
            CurveGeometry::Degenerate { point } => {
                native_curve_base(records, "degenerate_curve")?;
                native_point(records, [point.x / 10.0, point.y / 10.0, point.z / 10.0]);
                records.extend_from_slice(&[0x0b, 0x0b]);
            }
            _ => {
                return Err(CodecError::NotImplemented(
                    "source-less F3D wire curve carrier is unsupported".into(),
                ))
            }
        }
        records.push(0x11);
    }
    Ok(())
}

fn encode_multi_face_shell_smbh(target: &CadIr) -> Result<Vec<u8>, CodecError> {
    use cadmpeg_ir::geometry::SurfaceGeometry;

    let model = &target.model;
    let region_ordinals: HashMap<_, _> = model
        .regions
        .iter()
        .enumerate()
        .map(|(ordinal, region)| (&region.id, ordinal))
        .collect();
    let shell_ordinals: HashMap<_, _> = model
        .shells
        .iter()
        .enumerate()
        .map(|(ordinal, shell)| (&shell.id, ordinal))
        .collect();
    let coedge_ordinals: HashMap<_, _> = model
        .coedges
        .iter()
        .enumerate()
        .map(|(ordinal, coedge)| (&coedge.id, ordinal))
        .collect();
    let edge_ordinals: HashMap<_, _> = model
        .edges
        .iter()
        .enumerate()
        .map(|(ordinal, edge)| (&edge.id, ordinal))
        .collect();
    let loop_ordinals: HashMap<_, _> = model
        .loops
        .iter()
        .enumerate()
        .map(|(ordinal, lp)| (&lp.id, ordinal))
        .collect();
    let pcurve_ordinals: HashMap<_, _> = model
        .pcurves
        .iter()
        .enumerate()
        .map(|(ordinal, pcurve)| (&pcurve.id, ordinal))
        .collect();
    if model.bodies.is_empty()
        || model.regions.is_empty()
        || model.shells.is_empty()
        || model.faces.is_empty()
        || model.loops.len() < model.faces.len()
    {
        return Err(CodecError::NotImplemented(
            "source-less F3D generation requires owned face topology".into(),
        ));
    }
    validate_source_less_body_kinds(model)?;

    let plan = NativeRecordPlan::for_model(model)?;
    let NativeRecordPlan {
        body: body_start,
        region: region_start,
        shell: shell_start,
        wire: wire_start,
        face: face_start,
        loop_: loop_start,
        surface: surface_start,
        curve: curve_start,
        pcurve: pcurve_start,
        coedge: coedge_start,
        wire_coedge: wire_coedge_start,
        edge: edge_start,
        vertex: vertex_start,
        point: point_start,
        transform: transform_start,
        attribute: attribute_start,
    } = plan;

    let mut records = Vec::new();
    native_ident(&mut records, "asmheader")?;
    native_string(&mut records, "231.6.3.65535")?;
    records.push(0x11);
    for (body_ordinal, body) in model.bodies.iter().enumerate() {
        let first_region = body
            .regions
            .first()
            .ok_or_else(|| CodecError::Malformed(format!("body {} has no region", body.id)))?;
        let region_ordinal = region_ordinals.get(first_region).copied().ok_or_else(|| {
            CodecError::Malformed(format!("body references missing region {first_region}"))
        })?;
        let transform_ordinal = model.bodies[..body_ordinal]
            .iter()
            .filter(|candidate| candidate.transform.is_some())
            .count();
        let first_wire = body
            .regions
            .iter()
            .filter_map(|region_id| {
                region_ordinals
                    .get(region_id)
                    .map(|ordinal| &model.regions[*ordinal])
            })
            .flat_map(|region| &region.shells)
            .find_map(|shell_id| {
                shell_ordinals
                    .get(shell_id)
                    .copied()
                    .filter(|ordinal| !model.shells[*ordinal].wire_edges.is_empty())
            })
            .map(|shell_ordinal| wire_record_for_shell(model, wire_start, shell_ordinal))
            .transpose()?
            .unwrap_or(-1);
        native_ident(&mut records, "body")?;
        native_ref(
            &mut records,
            owner_color_or_body_tag_ref(target, body, body_ordinal, attribute_start)?,
        );
        native_i64(
            &mut records,
            source_less_body_key(target, body, body_ordinal)?,
        );
        native_ref(&mut records, -1);
        native_ref(
            &mut records,
            native_record_index(region_start, region_ordinal)?,
        );
        native_ref(&mut records, first_wire);
        native_ref(
            &mut records,
            if body.transform.is_some() {
                native_record_index(transform_start, transform_ordinal)?
            } else {
                -1
            },
        );
        records.push(0x11);
    }
    for region in &model.regions {
        let body_ordinal = model
            .bodies
            .iter()
            .position(|body| body.id == region.body)
            .ok_or_else(|| CodecError::Malformed(format!("region {} has no body", region.id)))?;
        let body = &model.bodies[body_ordinal];
        let ordinal = body
            .regions
            .iter()
            .position(|id| *id == region.id)
            .ok_or_else(|| {
                CodecError::Malformed(format!("body does not own region {}", region.id))
            })?;
        let first_shell = region
            .shells
            .first()
            .ok_or_else(|| CodecError::Malformed(format!("region {} has no shell", region.id)))?;
        let shell_ordinal = model
            .shells
            .iter()
            .position(|shell| shell.id == *first_shell)
            .ok_or_else(|| {
                CodecError::Malformed(format!("region references missing shell {first_shell}"))
            })?;
        let next = if ordinal + 1 == body.regions.len() {
            -1
        } else {
            let id = &body.regions[ordinal + 1];
            let position = model
                .regions
                .iter()
                .position(|item| item.id == *id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!("body references missing region {id}"))
                })?;
            native_record_index(region_start, position)?
        };
        native_ident(&mut records, "region")?;
        native_ref(&mut records, next);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(
            &mut records,
            native_record_index(shell_start, shell_ordinal)?,
        );
        native_ref(&mut records, native_record_index(body_start, body_ordinal)?);
        records.push(0x11);
    }
    for shell in &model.shells {
        let region_ordinal = model
            .regions
            .iter()
            .position(|region| region.id == shell.region)
            .ok_or_else(|| CodecError::Malformed(format!("shell {} has no region", shell.id)))?;
        let region = &model.regions[region_ordinal];
        let ordinal = region
            .shells
            .iter()
            .position(|id| *id == shell.id)
            .ok_or_else(|| {
                CodecError::Malformed(format!("region does not own shell {}", shell.id))
            })?;
        let first_face = shell
            .faces
            .first()
            .map(|first_face| {
                model
                    .faces
                    .iter()
                    .position(|face| face.id == *first_face)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!("shell references missing face {first_face}"))
                    })
                    .and_then(|ordinal| native_record_index(face_start, ordinal))
            })
            .transpose()?
            .unwrap_or(-1);
        let next = if ordinal + 1 == region.shells.len() {
            -1
        } else {
            let id = &region.shells[ordinal + 1];
            let position = model
                .shells
                .iter()
                .position(|item| item.id == *id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!("region references missing shell {id}"))
                })?;
            native_record_index(shell_start, position)?
        };
        native_ident(&mut records, "shell")?;
        native_ref(&mut records, next);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(&mut records, first_face);
        native_ref(
            &mut records,
            if shell.wire_edges.is_empty() {
                -1
            } else {
                wire_record_for_shell(
                    model,
                    wire_start,
                    model
                        .shells
                        .iter()
                        .position(|item| item.id == shell.id)
                        .expect("current shell is present"),
                )?
            },
        );
        native_ref(
            &mut records,
            native_record_index(region_start, region_ordinal)?,
        );
        records.push(0x11);
    }

    let mut wire_edge_base = 0usize;
    for (shell_ordinal, shell) in model.shells.iter().enumerate() {
        if shell.wire_edges.is_empty() {
            continue;
        }
        native_ident(&mut records, "wire")?;
        native_ref(&mut records, -1);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(
            &mut records,
            native_record_index(wire_coedge_start, wire_edge_base)?,
        );
        native_ref(
            &mut records,
            native_record_index(shell_start, shell_ordinal)?,
        );
        native_ref(&mut records, -1);
        records.push(native_wire_side(target, &shell.id)?);
        records.push(0x11);
        wire_edge_base += shell.wire_edges.len();
    }

    for (face_ordinal_global, face) in model.faces.iter().enumerate() {
        let shell_ordinal = model
            .shells
            .iter()
            .position(|shell| shell.id == face.shell)
            .ok_or_else(|| CodecError::Malformed(format!("face {} has no shell", face.id)))?;
        let shell = &model.shells[shell_ordinal];
        let ordinal = shell
            .faces
            .iter()
            .position(|id| *id == face.id)
            .ok_or_else(|| CodecError::Malformed(format!("shell does not own face {}", face.id)))?;
        if face.loops.is_empty() {
            return Err(CodecError::NotImplemented(
                "source-less multi-loop F3D requires every face to own a loop".into(),
            ));
        }
        let loop_position = model
            .loops
            .iter()
            .position(|loop_| loop_.id == face.loops[0])
            .ok_or_else(|| {
                CodecError::Malformed(format!("face references missing loop {}", face.loops[0]))
            })?;
        let surface_position = model
            .surfaces
            .iter()
            .position(|surface| surface.id == face.surface)
            .ok_or_else(|| {
                CodecError::Malformed(format!("face references missing surface {}", face.surface))
            })?;
        native_ident(&mut records, "face")?;
        native_ref(
            &mut records,
            owner_color_or_face_tag_ref(target, face, face_ordinal_global, attribute_start)?,
        );
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(
            &mut records,
            if ordinal + 1 == shell.faces.len() {
                -1
            } else {
                let id = &shell.faces[ordinal + 1];
                let position = model
                    .faces
                    .iter()
                    .position(|item| item.id == *id)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!("shell references missing face {id}"))
                    })?;
                native_record_index(face_start, position)?
            },
        );
        native_ref(
            &mut records,
            native_record_index(loop_start, loop_position)?,
        );
        native_ref(
            &mut records,
            native_record_index(shell_start, shell_ordinal)?,
        );
        native_ref(&mut records, -1);
        native_ref(
            &mut records,
            native_record_index(surface_start, surface_position)?,
        );
        records.push(native_bool(
            native_face_sense(target, face)? == Sense::Reversed,
        ));
        native_face_sidedness(&mut records, target, face)?;
        records.push(0x11);
    }

    for loop_ in &model.loops {
        let face_position = model
            .faces
            .iter()
            .position(|face| face.id == loop_.face)
            .ok_or_else(|| {
                CodecError::Malformed(format!("loop references missing face {}", loop_.face))
            })?;
        let first = loop_
            .coedges
            .first()
            .ok_or_else(|| CodecError::Malformed(format!("loop {} has no coedges", loop_.id)))?;
        let coedge_position = model
            .coedges
            .iter()
            .position(|coedge| coedge.id == *first)
            .ok_or_else(|| {
                CodecError::Malformed(format!("loop references missing coedge {first}"))
            })?;
        native_ident(&mut records, "loop")?;
        native_ref(&mut records, -1);
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        let face = &model.faces[face_position];
        let ordinal = face
            .loops
            .iter()
            .position(|id| *id == loop_.id)
            .ok_or_else(|| {
                CodecError::Malformed(format!("face {} does not own loop {}", face.id, loop_.id))
            })?;
        let next_loop = if ordinal + 1 == face.loops.len() {
            -1
        } else {
            let next_id = &face.loops[ordinal + 1];
            let position = model
                .loops
                .iter()
                .position(|candidate| candidate.id == *next_id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!("face references missing loop {next_id}"))
                })?;
            native_record_index(loop_start, position)?
        };
        native_ref(&mut records, next_loop);
        native_ref(
            &mut records,
            native_record_index(coedge_start, coedge_position)?,
        );
        native_ref(
            &mut records,
            native_record_index(face_start, face_position)?,
        );
        records.push(0x11);
    }

    for surface in &model.surfaces {
        match surface.geometry {
            SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            } => {
                native_surface_base(&mut records, "plane")?;
                native_point(
                    &mut records,
                    [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0],
                );
                native_vector(&mut records, [normal.x, normal.y, normal.z]);
                native_vector(&mut records, [u_axis.x, u_axis.y, u_axis.z]);
                records.push(0x0b);
            }
            SurfaceGeometry::Cylinder {
                origin,
                axis,
                ref_direction,
                radius,
            } => {
                native_surface_base(&mut records, "cone")?;
                native_point(
                    &mut records,
                    [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0],
                );
                native_vector(&mut records, [axis.x, axis.y, axis.z]);
                native_vector(
                    &mut records,
                    [
                        ref_direction.x * radius / 10.0,
                        ref_direction.y * radius / 10.0,
                        ref_direction.z * radius / 10.0,
                    ],
                );
                native_f64(&mut records, 1.0);
                records.extend_from_slice(&[0x0b, 0x0b]);
                native_f64(&mut records, 0.0);
                native_f64(&mut records, 1.0);
                native_f64(&mut records, radius / 10.0);
                records.extend_from_slice(&[0x0b; 5]);
            }
            SurfaceGeometry::Cone {
                origin,
                axis,
                ref_direction,
                radius,
                ratio,
                half_angle,
            } => {
                native_surface_base(&mut records, "cone")?;
                native_point(
                    &mut records,
                    [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0],
                );
                native_vector(&mut records, [axis.x, axis.y, axis.z]);
                native_vector(
                    &mut records,
                    [
                        ref_direction.x * radius / 10.0,
                        ref_direction.y * radius / 10.0,
                        ref_direction.z * radius / 10.0,
                    ],
                );
                native_f64(&mut records, ratio);
                records.extend_from_slice(&[0x0b, 0x0b]);
                native_f64(&mut records, half_angle.sin());
                native_f64(&mut records, half_angle.cos());
                native_f64(&mut records, radius / 10.0);
                records.extend_from_slice(&[0x0b; 5]);
            }
            SurfaceGeometry::Sphere {
                center,
                axis,
                ref_direction,
                radius,
            } => {
                native_surface_base(&mut records, "sphere")?;
                native_point(
                    &mut records,
                    [center.x / 10.0, center.y / 10.0, center.z / 10.0],
                );
                native_f64(&mut records, radius / 10.0);
                native_vector(
                    &mut records,
                    [ref_direction.x, ref_direction.y, ref_direction.z],
                );
                native_vector(&mut records, [axis.x, axis.y, axis.z]);
                records.extend_from_slice(&[0x0b; 5]);
            }
            SurfaceGeometry::Nurbs(ref nurbs) => {
                if !native_procedural_surface(&mut records, target, surface, nurbs)? {
                    native_surface_base(&mut records, "spline")?;
                    native_nurbs_surface(&mut records, nurbs)?;
                }
            }
            SurfaceGeometry::Torus {
                center,
                axis,
                ref_direction,
                major_radius,
                minor_radius,
            } => {
                native_surface_base(&mut records, "torus")?;
                native_point(
                    &mut records,
                    [center.x / 10.0, center.y / 10.0, center.z / 10.0],
                );
                native_vector(&mut records, [axis.x, axis.y, axis.z]);
                native_f64(&mut records, major_radius / 10.0);
                native_f64(&mut records, minor_radius / 10.0);
                native_vector(
                    &mut records,
                    [ref_direction.x, ref_direction.y, ref_direction.z],
                );
                records.extend_from_slice(&[0x0b; 5]);
            }
            SurfaceGeometry::Procedural { .. } | SurfaceGeometry::Unknown { .. } => {
                if !native_cacheless_procedural_surface(&mut records, target, surface)? {
                    return Err(CodecError::NotImplemented(format!(
                        "source-less multi-face F3D does not support surface carrier {}",
                        surface.id
                    )));
                }
            }
            SurfaceGeometry::Polygonal { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "source-less multi-face F3D does not support polygonal surface carrier {}",
                    surface.id
                )));
            }
            SurfaceGeometry::Transformed { .. } => {
                return Err(CodecError::NotImplemented(format!(
                    "source-less multi-face F3D does not support transformed surface carrier {}",
                    surface.id
                )));
            }
        }
        records.push(0x11);
    }

    for carrier in &model.curves {
        match carrier.geometry {
            CurveGeometry::Line { origin, direction } => {
                native_curve_base(&mut records, "straight")?;
                native_point(
                    &mut records,
                    [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0],
                );
                native_vector(&mut records, [direction.x, direction.y, direction.z]);
            }
            CurveGeometry::Nurbs(ref curve) => {
                if !native_procedural_curve(&mut records, target, &carrier.id, curve)? {
                    native_curve_base(&mut records, "intcurve")?;
                    native_nurbs_curve(&mut records, curve)?;
                }
            }
            CurveGeometry::Degenerate { point } => {
                native_curve_base(&mut records, "degenerate_curve")?;
                native_point(
                    &mut records,
                    [point.x / 10.0, point.y / 10.0, point.z / 10.0],
                );
                records.extend_from_slice(&[0x0b, 0x0b]);
            }
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } => {
                native_curve_base(&mut records, "ellipse")?;
                native_point(
                    &mut records,
                    [center.x / 10.0, center.y / 10.0, center.z / 10.0],
                );
                native_vector(&mut records, [axis.x, axis.y, axis.z]);
                native_vector(
                    &mut records,
                    [
                        ref_direction.x * radius / 10.0,
                        ref_direction.y * radius / 10.0,
                        ref_direction.z * radius / 10.0,
                    ],
                );
                native_f64(&mut records, 1.0);
            }
            CurveGeometry::Ellipse {
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius,
            } => {
                if major_radius == 0.0 {
                    return Err(CodecError::Malformed(
                        "source-less F3D ellipse has zero major radius".into(),
                    ));
                }
                native_curve_base(&mut records, "ellipse")?;
                native_point(
                    &mut records,
                    [center.x / 10.0, center.y / 10.0, center.z / 10.0],
                );
                native_vector(&mut records, [axis.x, axis.y, axis.z]);
                native_vector(
                    &mut records,
                    [
                        major_direction.x * major_radius / 10.0,
                        major_direction.y * major_radius / 10.0,
                        major_direction.z * major_radius / 10.0,
                    ],
                );
                native_f64(&mut records, minor_radius / major_radius);
            }
            _ => {
                return Err(CodecError::NotImplemented(
                    "source-less multi-face F3D does not support this curve carrier".into(),
                ));
            }
        }
        records.push(0x11);
    }

    let ref_pcurve_start = native_record_index(pcurve_start, model.pcurves.len())?;
    let mut ref_pcurve_ordinal = 0usize;
    for pcurve in &model.pcurves {
        let companion_ref = pcurve_uses_ref_form(pcurve)?
            .then(|| native_record_index(ref_pcurve_start, ref_pcurve_ordinal))
            .transpose()?;
        native_pcurve(&mut records, pcurve, companion_ref)?;
        ref_pcurve_ordinal += usize::from(companion_ref.is_some());
        records.push(0x11);
    }
    for pcurve in model
        .pcurves
        .iter()
        .filter(|pcurve| pcurve_uses_ref_form(pcurve).is_ok_and(|value| value))
    {
        native_ref_pcurve_companion(&mut records, pcurve)?;
        records.push(0x11);
    }

    for (coedge_ordinal, coedge) in model.coedges.iter().enumerate() {
        if coedge.pcurves.len() > 1 {
            return Err(CodecError::NotImplemented(format!(
                "coedge {} has an ordered pcurve collection",
                coedge.id
            )));
        }
        let next = coedge_ordinals.get(&coedge.next).copied();
        let previous = coedge_ordinals.get(&coedge.previous).copied();
        let radial = coedge_ordinals.get(&coedge.radial_next).copied();
        let edge = edge_ordinals.get(&coedge.edge).copied();
        let owner = loop_ordinals.get(&coedge.owner_loop).copied();
        let (Some(next), Some(previous), Some(radial), Some(edge), Some(owner)) =
            (next, previous, radial, edge, owner)
        else {
            return Err(CodecError::Malformed(format!(
                "coedge {} has an unresolved topology reference",
                coedge.id
            )));
        };
        let tolerant_range = tolerant_coedge_range(target, &coedge.id)?;
        native_ident(
            &mut records,
            if tolerant_range.is_some() {
                "tcoedge"
            } else {
                "coedge"
            },
        )?;
        native_ref(
            &mut records,
            sketch_link_attribute_ref(target, coedge, coedge_ordinal, attribute_start)?,
        );
        native_i64(&mut records, -1);
        native_ref(&mut records, -1);
        native_ref(&mut records, native_record_index(coedge_start, next)?);
        native_ref(&mut records, native_record_index(coedge_start, previous)?);
        native_ref(
            &mut records,
            if radial == coedge_ordinals.get(&coedge.id).copied().unwrap_or(radial) {
                -1
            } else {
                native_record_index(coedge_start, radial)?
            },
        );
        native_ref(&mut records, native_record_index(edge_start, edge)?);
        records.push(native_bool(coedge.sense == Sense::Reversed));
        native_ref(&mut records, native_record_index(loop_start, owner)?);
        native_i64(&mut records, 0);
        let pcurve_ref = coedge
            .pcurves
            .first()
            .map(|use_| {
                let pcurve_id = &use_.pcurve;
                pcurve_ordinals
                    .get(pcurve_id)
                    .copied()
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "coedge references missing pcurve {pcurve_id}"
                        ))
                    })
                    .and_then(|ordinal| native_record_index(pcurve_start, ordinal))
            })
            .transpose()?
            .unwrap_or(-1);
        native_ref(&mut records, pcurve_ref);
        if let Some(range) = tolerant_range {
            native_f64(&mut records, range[0]);
            native_f64(&mut records, range[1]);
        }
        records.push(0x11);
    }

    let mut wire_edge_owners = BTreeMap::new();
    let mut wire_edge_base = 0usize;
    for (shell_ordinal, shell) in model.shells.iter().enumerate() {
        if shell.wire_edges.is_empty() {
            continue;
        }
        let wire_ref = wire_record_for_shell(model, wire_start, shell_ordinal)?;
        for (ordinal, edge_id) in shell.wire_edges.iter().enumerate() {
            let edge_ordinal = edge_ordinals.get(edge_id).copied().ok_or_else(|| {
                CodecError::Malformed(format!("wire references missing edge {edge_id}"))
            })?;
            let coedge_ordinal = wire_edge_base + ordinal;
            let owner = native_record_index(wire_coedge_start, coedge_ordinal)?;
            if wire_edge_owners.insert(edge_id.clone(), owner).is_some() {
                return Err(CodecError::Malformed(format!(
                    "wire edge {edge_id} belongs to more than one shell"
                )));
            }
            let next = wire_edge_base + (ordinal + 1) % shell.wire_edges.len();
            let previous =
                wire_edge_base + (ordinal + shell.wire_edges.len() - 1) % shell.wire_edges.len();
            native_ident(&mut records, "coedge")?;
            native_ref(&mut records, -1);
            native_i64(&mut records, -1);
            native_ref(&mut records, -1);
            native_ref(&mut records, native_record_index(wire_coedge_start, next)?);
            native_ref(
                &mut records,
                native_record_index(wire_coedge_start, previous)?,
            );
            native_ref(&mut records, -1);
            native_ref(&mut records, native_record_index(edge_start, edge_ordinal)?);
            records.push(0x0b);
            native_ref(&mut records, wire_ref);
            native_i64(&mut records, 0);
            native_ref(&mut records, -1);
            records.push(0x11);
        }
        wire_edge_base += shell.wire_edges.len();
    }
    apply_native_edge_owners(target, coedge_start, &mut wire_edge_owners)?;

    encode_source_less_edges_vertices_points(
        &mut records,
        target,
        curve_start,
        edge_start,
        vertex_start,
        point_start,
        attribute_start,
        Some(&wire_edge_owners),
    )?;
    for body in &model.bodies {
        if let Some(transform) = body.transform {
            native_transform(&mut records, target, body, transform)?;
            records.push(0x11);
        }
    }
    encode_source_less_attributes(&mut records, target, attribute_start)?;
    native_history_tail(&mut records, target)?;
    let mut bytes = native_smbh_header(target)?;
    bytes.extend_from_slice(&records);
    Ok(bytes)
}

fn validate_source_less_body_kinds(model: &cadmpeg_ir::document::Model) -> Result<(), CodecError> {
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
        let has_wire_edges = model
            .shells
            .iter()
            .filter(|shell| shell_ids.contains(&shell.id))
            .any(|shell| !shell.wire_edges.is_empty());
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
        } else if has_wire_edges {
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

fn source_less_body_key(
    target: &CadIr,
    body: &Body,
    body_ordinal: usize,
) -> Result<i64, CodecError> {
    let native = f3d_native(target)?;
    if let Some(key) = native.as_ref().and_then(|native| {
        native
            .body_native_keys
            .iter()
            .find(|key| key.body == body.id)
    }) {
        return key.asm_body_key.map_or(Ok(-1), |key| {
            i64::try_from(key)
                .map_err(|_| CodecError::NotImplemented("F3D ASM body key exceeds i64::MAX".into()))
        });
    }
    let assigned = target
        .model
        .appearance_bindings
        .iter()
        .find_map(|binding| match &binding.target {
            cadmpeg_ir::appearance::AppearanceTarget::Body(id) if id == &body.id => target
                .model
                .appearances
                .iter()
                .find(|appearance| appearance.id == binding.appearance)
                .and_then(|appearance| appearance.visual_guid.as_deref()),
            _ => None,
        })
        .and_then(|visual_guid| {
            native
                .as_ref()?
                .design_material_assignments
                .iter()
                .find(|assignment| assignment.visual_guid == visual_guid)
                .map(|assignment| assignment.asm_body_key)
        });
    let key = assigned.unwrap_or(
        u64::try_from(body_ordinal)
            .ok()
            .and_then(|ordinal| ordinal.checked_add(1))
            .ok_or_else(|| CodecError::NotImplemented("F3D body ordinal exceeds u64".into()))?,
    );
    i64::try_from(key)
        .map_err(|_| CodecError::NotImplemented("F3D ASM body key exceeds i64::MAX".into()))
}

fn color_attribute_ref(
    model: &cadmpeg_ir::document::Model,
    color: Option<Color>,
    ordinal: usize,
    body: bool,
    attribute_start: i64,
) -> Result<i64, CodecError> {
    if color.is_none() {
        return Ok(-1);
    }
    let preceding = if body {
        model.bodies[..ordinal]
            .iter()
            .filter(|body| body.color.is_some())
            .count()
    } else {
        model
            .bodies
            .iter()
            .filter(|body| body.color.is_some())
            .count()
            + model.faces[..ordinal]
                .iter()
                .filter(|face| face.color.is_some())
                .count()
    };
    native_record_index(attribute_start, preceding)
}

fn persistent_links(
    target: &CadIr,
    entity: &cadmpeg_ir::attributes::AttributeTarget,
) -> Vec<PersistentDesignLink> {
    let mut links = f3d_native(target)
        .ok()
        .flatten()
        .into_iter()
        .flat_map(|native| native.persistent_design_links)
        .filter(|link| &link.target == entity)
        .collect::<Vec<_>>();
    links.sort_by_key(|link| link.ordinal);
    links
}

fn creation_timestamp(
    target: &CadIr,
    entity: &cadmpeg_ir::attributes::AttributeTarget,
) -> Option<CreationTimestamp> {
    f3d_native(target)
        .ok()
        .flatten()?
        .creation_timestamps
        .into_iter()
        .find(|timestamp| &timestamp.target == entity)
}

fn timestamp_attribute_ordinal(
    target: &CadIr,
    entity: &cadmpeg_ir::attributes::AttributeTarget,
) -> Option<usize> {
    let model = &target.model;
    let targets = model
        .bodies
        .iter()
        .map(|item| cadmpeg_ir::attributes::AttributeTarget::Body(item.id.clone()))
        .chain(
            model
                .faces
                .iter()
                .map(|item| cadmpeg_ir::attributes::AttributeTarget::Face(item.id.clone())),
        )
        .chain(
            model
                .edges
                .iter()
                .map(|item| cadmpeg_ir::attributes::AttributeTarget::Edge(item.id.clone())),
        )
        .chain(
            model
                .coedges
                .iter()
                .map(|item| cadmpeg_ir::attributes::AttributeTarget::Coedge(item.id.clone())),
        )
        .chain(
            model
                .vertices
                .iter()
                .map(|item| cadmpeg_ir::attributes::AttributeTarget::Vertex(item.id.clone())),
        );
    let mut ordinal = 0;
    for candidate in targets {
        if creation_timestamp(target, &candidate).is_some() {
            if &candidate == entity {
                return Some(ordinal);
            }
            ordinal += 1;
        }
    }
    None
}

fn existing_source_less_attribute_count(target: &CadIr) -> usize {
    source_less_color_count(target)
        + source_less_name_count(target)
        + persistent_body_group_count(target)
        + persistent_face_group_count(target)
        + persistent_edge_group_count(target)
        + target
            .model
            .coedges
            .iter()
            .filter(|coedge| sketch_link(target, coedge).is_some())
            .count()
}

fn timestamp_attribute_ref(
    target: &CadIr,
    entity: &cadmpeg_ir::attributes::AttributeTarget,
    attribute_start: i64,
) -> Result<Option<i64>, CodecError> {
    timestamp_attribute_ordinal(target, entity)
        .map(|ordinal| {
            native_record_index(
                attribute_start,
                existing_source_less_attribute_count(target) + ordinal,
            )
        })
        .transpose()
}

fn body_persistent_links(target: &CadIr, body: &Body) -> Vec<PersistentDesignLink> {
    persistent_links(
        target,
        &cadmpeg_ir::attributes::AttributeTarget::Body(body.id.clone()),
    )
}

fn persistent_group_count(
    target: &CadIr,
    entities: impl Iterator<Item = cadmpeg_ir::attributes::AttributeTarget>,
) -> usize {
    entities
        .filter(|entity| !persistent_links(target, entity).is_empty())
        .count()
}

fn persistent_body_group_count(target: &CadIr) -> usize {
    persistent_group_count(
        target,
        target
            .model
            .bodies
            .iter()
            .map(|body| cadmpeg_ir::attributes::AttributeTarget::Body(body.id.clone())),
    )
}

fn face_persistent_links(target: &CadIr, face: &Face) -> Vec<PersistentDesignLink> {
    persistent_links(
        target,
        &cadmpeg_ir::attributes::AttributeTarget::Face(face.id.clone()),
    )
}

fn edge_persistent_links(target: &CadIr, edge: &Edge) -> Vec<PersistentDesignLink> {
    persistent_links(
        target,
        &cadmpeg_ir::attributes::AttributeTarget::Edge(edge.id.clone()),
    )
}

fn persistent_face_group_count(target: &CadIr) -> usize {
    persistent_group_count(
        target,
        target
            .model
            .faces
            .iter()
            .map(|face| cadmpeg_ir::attributes::AttributeTarget::Face(face.id.clone())),
    )
}

fn persistent_edge_group_count(target: &CadIr) -> usize {
    persistent_group_count(
        target,
        target
            .model
            .edges
            .iter()
            .map(|edge| cadmpeg_ir::attributes::AttributeTarget::Edge(edge.id.clone())),
    )
}

fn body_persistent_attribute_ref(
    target: &CadIr,
    body: &Body,
    attribute_start: i64,
) -> Result<Option<i64>, CodecError> {
    if body_persistent_links(target, body).is_empty() {
        return Ok(None);
    }
    let ordinal = target
        .model
        .bodies
        .iter()
        .take_while(|candidate| candidate.id != body.id)
        .filter(|candidate| !body_persistent_links(target, candidate).is_empty())
        .count();
    let color_count = target
        .model
        .bodies
        .iter()
        .filter(|body| body.color.is_some())
        .count()
        + target
            .model
            .faces
            .iter()
            .filter(|face| face.color.is_some())
            .count();
    native_record_index(
        attribute_start,
        color_count + source_less_name_count(target) + ordinal,
    )
    .map(Some)
}

fn body_name_attribute_ref(
    target: &CadIr,
    body: &Body,
    attribute_start: i64,
) -> Result<Option<i64>, CodecError> {
    if body.name.is_none() {
        return Ok(None);
    }
    let ordinal = target
        .model
        .bodies
        .iter()
        .take_while(|candidate| candidate.id != body.id)
        .filter(|candidate| candidate.name.is_some())
        .count();
    native_record_index(attribute_start, source_less_color_count(target) + ordinal).map(Some)
}

fn owner_color_or_body_tag_ref(
    target: &CadIr,
    body: &Body,
    body_ordinal: usize,
    attribute_start: i64,
) -> Result<i64, CodecError> {
    if body.color.is_some() {
        return color_attribute_ref(
            &target.model,
            body.color,
            body_ordinal,
            true,
            attribute_start,
        );
    }
    if let Some(reference) = body_name_attribute_ref(target, body, attribute_start)? {
        return Ok(reference);
    }
    if let Some(reference) = body_persistent_attribute_ref(target, body, attribute_start)? {
        return Ok(reference);
    }
    Ok(timestamp_attribute_ref(
        target,
        &cadmpeg_ir::attributes::AttributeTarget::Body(body.id.clone()),
        attribute_start,
    )?
    .unwrap_or(-1))
}

fn face_persistent_attribute_ref(
    target: &CadIr,
    face: &Face,
    attribute_start: i64,
) -> Result<Option<i64>, CodecError> {
    if face_persistent_links(target, face).is_empty() {
        return Ok(None);
    }
    let ordinal = target
        .model
        .faces
        .iter()
        .take_while(|candidate| candidate.id != face.id)
        .filter(|candidate| !face_persistent_links(target, candidate).is_empty())
        .count();
    native_record_index(
        attribute_start,
        source_less_color_count(target)
            + source_less_name_count(target)
            + persistent_body_group_count(target)
            + ordinal,
    )
    .map(Some)
}

fn face_name_attribute_ref(
    target: &CadIr,
    face: &Face,
    attribute_start: i64,
) -> Result<Option<i64>, CodecError> {
    if face.name.is_none() {
        return Ok(None);
    }
    let ordinal = target
        .model
        .faces
        .iter()
        .take_while(|candidate| candidate.id != face.id)
        .filter(|candidate| candidate.name.is_some())
        .count();
    let body_name_count = target
        .model
        .bodies
        .iter()
        .filter(|body| body.name.is_some())
        .count();
    native_record_index(
        attribute_start,
        source_less_color_count(target) + body_name_count + ordinal,
    )
    .map(Some)
}

fn owner_color_or_face_tag_ref(
    target: &CadIr,
    face: &Face,
    face_ordinal: usize,
    attribute_start: i64,
) -> Result<i64, CodecError> {
    if face.color.is_some() {
        return color_attribute_ref(
            &target.model,
            face.color,
            face_ordinal,
            false,
            attribute_start,
        );
    }
    if let Some(reference) = face_name_attribute_ref(target, face, attribute_start)? {
        return Ok(reference);
    }
    if let Some(reference) = face_persistent_attribute_ref(target, face, attribute_start)? {
        return Ok(reference);
    }
    Ok(timestamp_attribute_ref(
        target,
        &cadmpeg_ir::attributes::AttributeTarget::Face(face.id.clone()),
        attribute_start,
    )?
    .unwrap_or(-1))
}

fn edge_persistent_attribute_ref(
    target: &CadIr,
    edge: &Edge,
    edge_ordinal: usize,
    attribute_start: i64,
) -> Result<Option<i64>, CodecError> {
    if edge_persistent_links(target, edge).is_empty() {
        return Ok(None);
    }
    let ordinal = target.model.edges[..edge_ordinal]
        .iter()
        .filter(|candidate| !edge_persistent_links(target, candidate).is_empty())
        .count();
    native_record_index(
        attribute_start,
        source_less_color_count(target)
            + source_less_name_count(target)
            + persistent_body_group_count(target)
            + persistent_face_group_count(target)
            + ordinal,
    )
    .map(Some)
}

fn source_less_color_count(target: &CadIr) -> usize {
    target
        .model
        .bodies
        .iter()
        .filter(|body| body.color.is_some())
        .count()
        + target
            .model
            .faces
            .iter()
            .filter(|face| face.color.is_some())
            .count()
}

fn source_less_name_count(target: &CadIr) -> usize {
    target
        .model
        .bodies
        .iter()
        .filter(|body| body.name.is_some())
        .count()
        + target
            .model
            .faces
            .iter()
            .filter(|face| face.name.is_some())
            .count()
}

fn sketch_link(target: &CadIr, coedge: &Coedge) -> Option<SketchCurveLink> {
    f3d_native(target)
        .ok()
        .flatten()?
        .sketch_curve_links
        .into_iter()
        .find(|link| link.coedge == coedge.id)
}

fn sketch_link_attribute_ref(
    target: &CadIr,
    coedge: &Coedge,
    coedge_ordinal: usize,
    attribute_start: i64,
) -> Result<i64, CodecError> {
    if sketch_link(target, coedge).is_none() {
        return Ok(timestamp_attribute_ref(
            target,
            &cadmpeg_ir::attributes::AttributeTarget::Coedge(coedge.id.clone()),
            attribute_start,
        )?
        .unwrap_or(-1));
    }
    let preceding = target.model.coedges[..coedge_ordinal]
        .iter()
        .filter(|candidate| sketch_link(target, candidate).is_some())
        .count();
    let color_count = source_less_color_count(target);
    native_record_index(
        attribute_start,
        color_count
            + source_less_name_count(target)
            + persistent_body_group_count(target)
            + persistent_face_group_count(target)
            + persistent_edge_group_count(target)
            + preceding,
    )
}

fn native_persistent_design_attribute(
    records: &mut Vec<u8>,
    links: &[PersistentDesignLink],
    kind: i64,
    next: i64,
) -> Result<(), CodecError> {
    native_subident(records, "ATTRIB_CUSTOM")?;
    native_ident(records, "attrib")?;
    native_ref(records, next);
    native_string(records, "generic_tag_attrib_def")?;
    for value in [kind, kind, -1] {
        native_i64(records, value);
    }
    native_string(records, "generic_tag_attrib_def ")?;
    native_i64(
        records,
        i64::try_from(links.len())
            .map_err(|_| CodecError::NotImplemented("too many persistent body IDs".into()))?,
    );
    for link in links {
        if link.entity_kind != kind {
            return Err(CodecError::Malformed(format!(
                "persistent design link {} has entity kind {}, expected {kind}",
                link.id, link.entity_kind
            )));
        }
        native_i64(records, link.entity_kind);
        native_string(records, &link.design_id)?;
        for value in [link.design_reference, 0, 0] {
            native_i64(records, value);
        }
    }
    Ok(())
}

fn native_sketch_link_attribute(
    records: &mut Vec<u8>,
    link: &SketchCurveLink,
    next: i64,
) -> Result<(), CodecError> {
    native_subident(records, "ATTRIB_CUSTOM")?;
    native_ident(records, "attrib")?;
    native_ref(records, next);
    native_string(records, "sketch_attrib_def")?;
    for value in [1, 1, 3] {
        native_i64(records, value);
    }
    native_string(
        records,
        &format!(
            "{} 0 {} 0 {} {}",
            link.sketch_curve_id,
            link.signed_reference.unwrap_or(-1),
            link.role,
            link.closure
        ),
    )
}

fn encode_source_less_attributes(
    records: &mut Vec<u8>,
    target: &CadIr,
    attribute_start: i64,
) -> Result<(), CodecError> {
    let model = &target.model;
    if let Some(native) = f3d_native(target)? {
        for (ordinal, timestamp) in native.creation_timestamps.iter().enumerate() {
            if !timestamp.unix_microseconds.is_finite() {
                return Err(CodecError::Malformed(format!(
                    "F3D creation timestamp {} is non-finite",
                    timestamp.id
                )));
            }
            if native.creation_timestamps[..ordinal]
                .iter()
                .any(|before| before.target == timestamp.target)
            {
                return Err(CodecError::Malformed(format!(
                    "multiple F3D creation timestamps target the same entity: {}",
                    timestamp.id
                )));
            }
            if timestamp_attribute_ordinal(target, &timestamp.target).is_none() {
                return Err(CodecError::NotImplemented(format!(
                    "F3D creation timestamp has an unsupported or missing target: {}",
                    timestamp.id
                )));
            }
        }
    }
    for body in model.bodies.iter().filter(|body| body.color.is_some()) {
        let color = body.color.expect("filtered colored body");
        let next = if let Some(reference) = body_name_attribute_ref(target, body, attribute_start)?
        {
            reference
        } else if let Some(reference) =
            body_persistent_attribute_ref(target, body, attribute_start)?
        {
            reference
        } else {
            timestamp_attribute_ref(
                target,
                &cadmpeg_ir::attributes::AttributeTarget::Body(body.id.clone()),
                attribute_start,
            )?
            .unwrap_or(-1)
        };
        native_color_attribute(records, color, next)?;
        records.push(0x11);
    }
    for face in model.faces.iter().filter(|face| face.color.is_some()) {
        let next = if let Some(reference) = face_name_attribute_ref(target, face, attribute_start)?
        {
            reference
        } else if let Some(reference) =
            face_persistent_attribute_ref(target, face, attribute_start)?
        {
            reference
        } else {
            timestamp_attribute_ref(
                target,
                &cadmpeg_ir::attributes::AttributeTarget::Face(face.id.clone()),
                attribute_start,
            )?
            .unwrap_or(-1)
        };
        native_color_attribute(records, face.color.expect("filtered colored face"), next)?;
        records.push(0x11);
    }
    for body in model.bodies.iter().filter(|body| body.name.is_some()) {
        let next = if let Some(reference) =
            body_persistent_attribute_ref(target, body, attribute_start)?
        {
            reference
        } else {
            timestamp_attribute_ref(
                target,
                &cadmpeg_ir::attributes::AttributeTarget::Body(body.id.clone()),
                attribute_start,
            )?
            .unwrap_or(-1)
        };
        native_name_attribute(
            records,
            body.name.as_deref().expect("filtered named body"),
            next,
        )?;
        records.push(0x11);
    }
    for face in model.faces.iter().filter(|face| face.name.is_some()) {
        let next = if let Some(reference) =
            face_persistent_attribute_ref(target, face, attribute_start)?
        {
            reference
        } else {
            timestamp_attribute_ref(
                target,
                &cadmpeg_ir::attributes::AttributeTarget::Face(face.id.clone()),
                attribute_start,
            )?
            .unwrap_or(-1)
        };
        native_name_attribute(
            records,
            face.name.as_deref().expect("filtered named face"),
            next,
        )?;
        records.push(0x11);
    }
    for body in &model.bodies {
        let links = body_persistent_links(target, body);
        if links.is_empty() {
            continue;
        }
        let next = timestamp_attribute_ref(
            target,
            &cadmpeg_ir::attributes::AttributeTarget::Body(body.id.clone()),
            attribute_start,
        )?
        .unwrap_or(-1);
        native_persistent_design_attribute(records, &links, 3, next)?;
        records.push(0x11);
    }
    for face in &model.faces {
        let links = face_persistent_links(target, face);
        if links.is_empty() {
            continue;
        }
        let next = timestamp_attribute_ref(
            target,
            &cadmpeg_ir::attributes::AttributeTarget::Face(face.id.clone()),
            attribute_start,
        )?
        .unwrap_or(-1);
        native_persistent_design_attribute(records, &links, 2, next)?;
        records.push(0x11);
    }
    for edge in &model.edges {
        let links = edge_persistent_links(target, edge);
        if links.is_empty() {
            continue;
        }
        let next = timestamp_attribute_ref(
            target,
            &cadmpeg_ir::attributes::AttributeTarget::Edge(edge.id.clone()),
            attribute_start,
        )?
        .unwrap_or(-1);
        native_persistent_design_attribute(records, &links, 1, next)?;
        records.push(0x11);
    }
    for coedge in &model.coedges {
        let Some(link) = sketch_link(target, coedge) else {
            continue;
        };
        let next = timestamp_attribute_ref(
            target,
            &cadmpeg_ir::attributes::AttributeTarget::Coedge(coedge.id.clone()),
            attribute_start,
        )?
        .unwrap_or(-1);
        native_sketch_link_attribute(records, &link, next)?;
        records.push(0x11);
    }
    for entity in model
        .bodies
        .iter()
        .map(|item| cadmpeg_ir::attributes::AttributeTarget::Body(item.id.clone()))
        .chain(
            model
                .faces
                .iter()
                .map(|item| cadmpeg_ir::attributes::AttributeTarget::Face(item.id.clone())),
        )
        .chain(
            model
                .edges
                .iter()
                .map(|item| cadmpeg_ir::attributes::AttributeTarget::Edge(item.id.clone())),
        )
        .chain(
            model
                .coedges
                .iter()
                .map(|item| cadmpeg_ir::attributes::AttributeTarget::Coedge(item.id.clone())),
        )
        .chain(
            model
                .vertices
                .iter()
                .map(|item| cadmpeg_ir::attributes::AttributeTarget::Vertex(item.id.clone())),
        )
    {
        let Some(timestamp) = creation_timestamp(target, &entity) else {
            continue;
        };
        native_subident(records, "ATTRIB_CUSTOM")?;
        native_ident(records, "attrib")?;
        native_ref(records, -1);
        native_string(records, "Timestamp_attrib_def")?;
        native_i64(records, 1);
        native_f64(records, timestamp.unix_microseconds);
        records.push(0x11);
    }
    Ok(())
}

fn native_name_attribute(records: &mut Vec<u8>, name: &str, next: i64) -> Result<(), CodecError> {
    if name.is_empty() {
        return Err(CodecError::Malformed(
            "source-less F3D display name must not be empty".into(),
        ));
    }
    native_subident(records, "string_attrib")?;
    native_subident(records, "name_attrib")?;
    native_subident(records, "gen")?;
    native_ident(records, "attrib")?;
    native_ref(records, next);
    for flag in [1, 1, 1, 1] {
        native_i64(records, flag);
    }
    native_string(records, "name")?;
    native_string(records, name)
}

fn native_color_attribute(
    records: &mut Vec<u8>,
    color: Color,
    next: i64,
) -> Result<(), CodecError> {
    let channels = [color.r, color.g, color.b, color.a];
    if channels
        .iter()
        .any(|channel| !channel.is_finite() || !(0.0..=1.0).contains(channel))
    {
        return Err(CodecError::Malformed(
            "source-less F3D color channels must be finite and in [0, 1]".into(),
        ));
    }
    if color.a == 1.0 {
        native_subident(records, "rgb_color")?;
        native_subident(records, "st")?;
        native_ident(records, "attrib")?;
        native_ref(records, next);
        native_f64(records, f64::from(color.r));
        native_f64(records, f64::from(color.g));
        native_f64(records, f64::from(color.b));
        return Ok(());
    }
    let quantized = channels.map(|channel| (channel * 255.0).round() as u8);
    let decoded = quantized.map(|channel| f32::from(channel) / 255.0);
    if decoded != channels {
        return Err(CodecError::NotImplemented(
            "source-less F3D translucent direct color requires exact 8-bit channels".into(),
        ));
    }
    native_subident(records, "truecolor")?;
    native_subident(records, "st")?;
    native_ident(records, "attrib")?;
    native_ref(records, next);
    let packed = u32::from_be_bytes([quantized[3], quantized[0], quantized[1], quantized[2]]);
    native_i64(records, i64::from(packed));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn encode_source_less_edges_vertices_points(
    records: &mut Vec<u8>,
    target: &CadIr,
    curve_start: i64,
    edge_start: i64,
    vertex_start: i64,
    point_start: i64,
    attribute_start: i64,
    edge_owners: Option<&BTreeMap<cadmpeg_ir::ids::EdgeId, i64>>,
) -> Result<(), CodecError> {
    let model = &target.model;
    let vertex_ordinals: HashMap<_, _> = model
        .vertices
        .iter()
        .enumerate()
        .map(|(ordinal, vertex)| (&vertex.id, ordinal))
        .collect();
    let curve_ordinals: HashMap<_, _> = model
        .curves
        .iter()
        .enumerate()
        .map(|(ordinal, curve)| (&curve.id, ordinal))
        .collect();
    let point_ordinals: HashMap<_, _> = model
        .points
        .iter()
        .enumerate()
        .map(|(ordinal, point)| (&point.id, ordinal))
        .collect();
    for (edge_ordinal, edge) in model.edges.iter().enumerate() {
        let start = vertex_ordinals.get(&edge.start).copied();
        let end = vertex_ordinals.get(&edge.end).copied();
        let (Some(start), Some(end)) = (start, end) else {
            return Err(CodecError::Malformed(format!(
                "edge {} has an unresolved vertex",
                edge.id
            )));
        };
        let curve_ref = edge
            .curve
            .as_ref()
            .map(|curve_id| {
                curve_ordinals
                    .get(curve_id)
                    .copied()
                    .ok_or_else(|| {
                        CodecError::Malformed(format!("edge references missing curve {curve_id}"))
                    })
                    .and_then(|ordinal| native_record_index(curve_start, ordinal))
            })
            .transpose()?
            .unwrap_or(-1);
        let mut range = edge.param_range.unwrap_or([0.0, 1.0]);
        // Conic edge parameters are angles in both the IR and the native
        // stream; line parameters are arc lengths, millimeters in the IR
        // and centimeters natively.
        if edge.curve.as_ref().is_some_and(|curve_id| {
            curve_ordinals.get(curve_id).is_some_and(|ordinal| {
                matches!(model.curves[*ordinal].geometry, CurveGeometry::Line { .. })
            })
        }) {
            range[0] /= 10.0;
            range[1] /= 10.0;
        }
        native_ident(records, "edge")?;
        let persistent =
            edge_persistent_attribute_ref(target, edge, edge_ordinal, attribute_start)?;
        native_ref(
            records,
            if let Some(reference) = persistent {
                reference
            } else {
                timestamp_attribute_ref(
                    target,
                    &cadmpeg_ir::attributes::AttributeTarget::Edge(edge.id.clone()),
                    attribute_start,
                )?
                .unwrap_or(-1)
            },
        );
        native_i64(records, -1);
        native_ref(records, -1);
        native_ref(records, native_record_index(vertex_start, start)?);
        native_f64(records, range[0]);
        native_ref(records, native_record_index(vertex_start, end)?);
        native_f64(records, range[1]);
        native_ref(
            records,
            edge_owners
                .and_then(|owners| owners.get(&edge.id))
                .copied()
                .unwrap_or(-1),
        );
        native_ref(records, curve_ref);
        let (sense, continuity) = edge_record_metadata(target, edge)?;
        records.push(native_bool(sense == Sense::Reversed));
        native_string(records, &continuity)?;
        records.push(0x11);
    }
    for vertex in &model.vertices {
        let point = point_ordinals.get(&vertex.point).copied();
        let Some(point) = point else {
            return Err(CodecError::Malformed(format!(
                "vertex {} has an unresolved carrier",
                vertex.id
            )));
        };
        let (edge, endpoint_index) = vertex_ownership(target, vertex)?;
        native_ident(
            records,
            if vertex.tolerance.is_some() {
                "tvertex"
            } else {
                "vertex"
            },
        )?;
        native_ref(
            records,
            timestamp_attribute_ref(
                target,
                &cadmpeg_ir::attributes::AttributeTarget::Vertex(vertex.id.clone()),
                attribute_start,
            )?
            .unwrap_or(-1),
        );
        native_i64(records, -1);
        native_ref(records, -1);
        native_ref(records, native_record_index(edge_start, edge)?);
        native_i64(records, i64::from(endpoint_index));
        native_ref(records, native_record_index(point_start, point)?);
        native_tolerant_vertex_tail(records, target, vertex)?;
        records.push(0x11);
    }
    for point in &model.points {
        native_ident(records, "point")?;
        native_ref(records, -1);
        native_i64(records, -1);
        native_ref(records, -1);
        native_point(
            records,
            [
                point.position.x / 10.0,
                point.position.y / 10.0,
                point.position.z / 10.0,
            ],
        );
        records.push(0x11);
    }
    Ok(())
}

fn vertex_ownership(
    target: &CadIr,
    vertex: &cadmpeg_ir::topology::Vertex,
) -> Result<(usize, u8), CodecError> {
    let model = &target.model;
    if let Some(metadata) = f3d_native(target)?.and_then(|native| {
        native
            .vertex_ownerships
            .into_iter()
            .find(|metadata| metadata.vertex == vertex.id)
    }) {
        let ordinal = model
            .edges
            .iter()
            .position(|edge| edge.id == metadata.owning_edge)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "vertex {} references missing owning edge {}",
                    vertex.id, metadata.owning_edge
                ))
            })?;
        let edge = &model.edges[ordinal];
        let valid = match metadata.endpoint_index {
            0 => edge.start == vertex.id,
            1 => edge.end == vertex.id,
            _ => false,
        };
        if !valid {
            return Err(CodecError::Malformed(format!(
                "vertex {} endpoint slot {} conflicts with owning edge {}",
                vertex.id, metadata.endpoint_index, metadata.owning_edge
            )));
        }
        return Ok((ordinal, metadata.endpoint_index));
    }
    model
        .edges
        .iter()
        .enumerate()
        .find_map(|(ordinal, edge)| {
            if edge.start == vertex.id {
                Some((ordinal, 0))
            } else if edge.end == vertex.id {
                Some((ordinal, 1))
            } else {
                None
            }
        })
        .ok_or_else(|| CodecError::Malformed(format!("vertex {} has no edge", vertex.id)))
}

fn native_face_sidedness(
    records: &mut Vec<u8>,
    target: &CadIr,
    face: &cadmpeg_ir::topology::Face,
) -> Result<(), CodecError> {
    let containment = f3d_native(target)?.and_then(|native| {
        native
            .face_sidedness
            .into_iter()
            .find(|metadata| metadata.face == face.id)
            .and_then(|metadata| metadata.containment)
    });
    records.push(native_bool(containment.is_some()));
    if let Some(containment) = containment {
        records.push(match containment {
            crate::records::FaceContainment::In => 0x0a,
            crate::records::FaceContainment::Out => 0x0b,
        });
    }
    Ok(())
}

fn native_face_sense(
    target: &CadIr,
    face: &cadmpeg_ir::topology::Face,
) -> Result<Sense, CodecError> {
    Ok(f3d_native(target)?
        .and_then(|native| {
            native
                .face_sidedness
                .into_iter()
                .find(|metadata| metadata.face == face.id)
                .map(|metadata| {
                    normalized_face_sense_to_native(
                        face.sense,
                        metadata.native_sense,
                        metadata.normalized_sense,
                    )
                })
        })
        .unwrap_or(face.sense))
}

fn native_wire_side(target: &CadIr, shell: &ShellId) -> Result<u8, CodecError> {
    let matches = f3d_native(target)?
        .map(|native| {
            native
                .wire_topologies
                .into_iter()
                .filter(|wire| wire.shell == *shell)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let side = match matches.as_slice() {
        [] => crate::records::WireSide::Out,
        [wire] => wire.side,
        _ => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D generation cannot collapse multiple native wires on shell {shell}"
            )))
        }
    };
    Ok(match side {
        crate::records::WireSide::In => 0x0a,
        crate::records::WireSide::Out => 0x0b,
    })
}

fn normalized_face_sense_to_native(
    desired: Sense,
    native_at_decode: Sense,
    normalized_at_decode: Sense,
) -> Sense {
    if native_at_decode == normalized_at_decode {
        desired
    } else {
        match desired {
            Sense::Forward => Sense::Reversed,
            Sense::Reversed => Sense::Forward,
        }
    }
}

fn native_tolerant_vertex_tail(
    records: &mut Vec<u8>,
    target: &CadIr,
    vertex: &cadmpeg_ir::topology::Vertex,
) -> Result<(), CodecError> {
    let Some(tolerance) = vertex.tolerance else {
        return Ok(());
    };
    if !tolerance.is_finite() || tolerance < 0.0 {
        return Err(CodecError::Malformed(format!(
            "F3D vertex {} tolerance must be finite and non-negative",
            vertex.id
        )));
    }
    native_f64(records, tolerance / 10.0);
    let trailing = f3d_native(target)?
        .and_then(|native| {
            native
                .tolerant_vertex_tails
                .into_iter()
                .find(|tail| tail.vertex == vertex.id)
        })
        .map_or([0.0; 2], |tail| tail.trailing_floats);
    for value in trailing {
        native_f32(records, value);
    }
    Ok(())
}

fn edge_record_metadata(
    target: &CadIr,
    edge: &cadmpeg_ir::topology::Edge,
) -> Result<(Sense, String), CodecError> {
    let metadata = f3d_native(target)?.and_then(|native| {
        native
            .edge_continuities
            .into_iter()
            .find(|metadata| metadata.edge == edge.id)
    });
    let sense = metadata
        .as_ref()
        .map_or(Sense::Forward, |metadata| metadata.sense);
    let continuity = metadata.map_or_else(|| "unknown".to_owned(), |metadata| metadata.continuity);
    if continuity != "tangent" && continuity != "unknown" {
        return Err(CodecError::Malformed(format!(
            "F3D edge {} has unsupported continuity token {continuity}",
            edge.id
        )));
    }
    Ok((sense, continuity))
}

fn apply_native_edge_owners(
    target: &CadIr,
    coedge_start: i64,
    owners: &mut BTreeMap<cadmpeg_ir::ids::EdgeId, i64>,
) -> Result<(), CodecError> {
    let metadata = f3d_native(target)?
        .map(|native| native.edge_ownerships)
        .unwrap_or_default();
    for ownership in metadata {
        if !target
            .model
            .edges
            .iter()
            .any(|edge| edge.id == ownership.edge)
        {
            return Err(CodecError::Malformed(format!(
                "F3D edge ownership {} references missing edge {}",
                ownership.id, ownership.edge
            )));
        }
        let owner = match ownership.owner_coedge {
            None => -1,
            Some(owner) => {
                if let Some((ordinal, coedge)) = target
                    .model
                    .coedges
                    .iter()
                    .enumerate()
                    .find(|(_, coedge)| coedge.id == owner)
                {
                    if coedge.edge != ownership.edge {
                        return Err(CodecError::Malformed(format!(
                            "F3D edge ownership {} selects a coedge of another edge",
                            ownership.id
                        )));
                    }
                    native_record_index(coedge_start, ordinal)?
                } else if owners.contains_key(&ownership.edge) {
                    // Wire coedges are native-only and are reconstructed from
                    // the shell's wire-edge list before this override runs.
                    continue;
                } else {
                    return Err(CodecError::Malformed(format!(
                        "F3D edge ownership {} references missing coedge {owner}",
                        ownership.id
                    )));
                }
            }
        };
        owners.insert(ownership.edge, owner);
    }
    Ok(())
}

fn native_smbh_header(target: &CadIr) -> Result<Vec<u8>, CodecError> {
    if !target.tolerances.linear.is_finite()
        || target.tolerances.linear <= 0.0
        || !target.tolerances.angular.is_finite()
        || target.tolerances.angular <= 0.0
    {
        return Err(CodecError::Malformed(
            "source-less F3D tolerances must be finite and positive".into(),
        ));
    }
    let mut bytes = b"ASM BinaryFile8".to_vec();
    // Release word matching the product string, the zero region, then the
    // entity-count and flags words (bit 0: history partition present).
    bytes.extend_from_slice(&23100u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 12]);
    bytes.extend_from_slice(&7u64.to_le_bytes());
    bytes.extend_from_slice(&3u64.to_le_bytes());
    native_string(&mut bytes, "Autodesk Neutron")?;
    native_string(&mut bytes, "ASM 231.6.3.65535 OSX")?;
    native_string(&mut bytes, "Thu Jan  1 00:00:00 1970")?;
    native_f64(&mut bytes, 60.0);
    native_f64(&mut bytes, target.tolerances.linear);
    native_f64(&mut bytes, target.tolerances.angular);
    Ok(bytes)
}

fn native_ident(bytes: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    native_text(bytes, 0x0d, value)
}

fn native_subident(bytes: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    native_text(bytes, 0x0e, value)
}

fn native_curve_base(bytes: &mut Vec<u8>, kind: &str) -> Result<(), CodecError> {
    native_subident(bytes, kind)?;
    native_ident(bytes, "curve")?;
    native_ref(bytes, -1);
    native_i64(bytes, -1);
    native_ref(bytes, -1);
    if kind == "intcurve" {
        bytes.push(native_bool(false));
    }
    Ok(())
}

fn native_surface_base(bytes: &mut Vec<u8>, kind: &str) -> Result<(), CodecError> {
    native_subident(bytes, kind)?;
    native_ident(bytes, "surface")?;
    native_ref(bytes, -1);
    native_i64(bytes, -1);
    native_ref(bytes, -1);
    if kind == "spline" {
        bytes.push(native_bool(false));
    }
    Ok(())
}

fn native_nurbs_surface(bytes: &mut Vec<u8>, surface: &NurbsSurface) -> Result<(), CodecError> {
    let u_count = usize::try_from(surface.u_count)
        .map_err(|_| CodecError::NotImplemented("F3D NURBS u count exceeds usize".into()))?;
    let v_count = usize::try_from(surface.v_count)
        .map_err(|_| CodecError::NotImplemented("F3D NURBS v count exceeds usize".into()))?;
    if surface.control_points.len() != u_count.saturating_mul(v_count)
        || surface
            .weights
            .as_ref()
            .is_some_and(|weights| weights.len() != surface.control_points.len())
    {
        return Err(CodecError::Malformed(
            "source-less F3D NURBS surface has inconsistent control-grid cardinality".into(),
        ));
    }
    native_ident(
        bytes,
        if surface.weights.is_some() {
            "nurbs"
        } else {
            "nubs"
        },
    )?;
    native_i64(bytes, i64::from(surface.u_degree));
    native_i64(bytes, i64::from(surface.v_degree));
    native_enum(bytes, if surface.u_periodic { 2 } else { 0 });
    native_enum(bytes, if surface.v_periodic { 2 } else { 0 });
    native_enum(bytes, 0);
    native_enum(bytes, 0);
    native_nurbs_knot_counts(bytes, [&surface.u_knots, &surface.v_knots])?;
    native_nurbs_knots(bytes, &surface.u_knots)?;
    native_nurbs_knots(bytes, &surface.v_knots)?;
    for v in 0..v_count {
        for u in 0..u_count {
            let index = u * v_count + v;
            let point = surface.control_points[index];
            native_f64(bytes, point.x / 10.0);
            native_f64(bytes, point.y / 10.0);
            native_f64(bytes, point.z / 10.0);
            if let Some(weights) = surface.weights.as_ref() {
                native_f64(bytes, weights[index]);
            }
        }
    }
    Ok(())
}

fn native_procedural_surface(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    solved_surface: &Surface,
    solved_cache: &NurbsSurface,
) -> Result<bool, CodecError> {
    let mut definitions = target
        .model
        .procedural_surfaces
        .iter()
        .filter(|procedural| procedural.surface == solved_surface.id);
    let Some(procedural) = definitions.next() else {
        return Ok(false);
    };
    if definitions.next().is_some() {
        return Err(CodecError::Malformed(format!(
            "surface {} has multiple procedural constructions",
            solved_surface.id
        )));
    }
    match &procedural.definition {
        ProceduralSurfaceDefinition::Deformable { construction } => {
            use cadmpeg_ir::geometry::DeformableSurfaceData;
            let cache_fit_tolerance = procedural.cache_fit_tolerance.ok_or_else(|| {
                CodecError::Malformed(
                    "deformable surface requires a native cache-fit tolerance".into(),
                )
            })?;
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "defm_spl_sur")?;
            let support = target
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == construction.support)
                .ok_or_else(|| CodecError::Malformed("deformable support is missing".into()))?;
            native_embedded_surface(bytes, &support.geometry)?;
            let write_frame =
                |bytes: &mut Vec<u8>, frame: &cadmpeg_ir::geometry::DeformableSurfaceFrame| {
                    for vector in frame.leading_vectors {
                        native_vector(bytes, [vector.x, vector.y, vector.z]);
                    }
                    native_f64(bytes, frame.leading_parameter);
                    for flag in frame.leading_flags {
                        bytes.push(native_bool(flag));
                    }
                    for vector in frame.secondary_vectors {
                        native_vector(bytes, [vector.x, vector.y, vector.z]);
                    }
                    native_f64(bytes, frame.secondary_parameter);
                    for flag in frame.secondary_flags {
                        bytes.push(native_bool(flag));
                    }
                    native_point(
                        bytes,
                        [
                            frame.point.x / 10.0,
                            frame.point.y / 10.0,
                            frame.point.z / 10.0,
                        ],
                    );
                    for flag in frame.trailing_flags {
                        bytes.push(native_bool(flag));
                    }
                };
            match &construction.data {
                DeformableSurfaceData::Full {
                    leading_vectors,
                    leading_parameter,
                    leading_flags,
                    selector,
                    surface,
                    native_id,
                    flag,
                    first_parameter,
                    version_value,
                    second_parameter,
                    curve,
                    frames,
                    trailing_value,
                } => {
                    native_i64(bytes, 6);
                    for vector in leading_vectors {
                        native_vector(bytes, [vector.x, vector.y, vector.z]);
                    }
                    native_f64(bytes, *leading_parameter);
                    for flag in leading_flags {
                        bytes.push(native_bool(*flag));
                    }
                    native_i64(bytes, *selector);
                    let secondary = target
                        .model
                        .surfaces
                        .iter()
                        .find(|candidate| candidate.id == *surface)
                        .ok_or_else(|| {
                            CodecError::Malformed("deformable secondary surface is missing".into())
                        })?;
                    native_embedded_surface(bytes, &secondary.geometry)?;
                    native_i64(bytes, *native_id);
                    bytes.push(native_bool(*flag));
                    native_f64(bytes, *first_parameter);
                    if let Some(value) = version_value {
                        native_i64(bytes, *value);
                    }
                    native_f64(bytes, *second_parameter);
                    let curve = native_loft_curve_in_range(
                        target,
                        curve,
                        Some([*first_parameter, *second_parameter]),
                    )?;
                    native_nurbs_curve(bytes, &curve)?;
                    for frame in frames.iter() {
                        for vector in frame.vectors {
                            native_vector(bytes, [vector.x, vector.y, vector.z]);
                        }
                        native_f64(bytes, frame.parameter);
                        for flag in frame.flags {
                            bytes.push(native_bool(flag));
                        }
                    }
                    native_i64(bytes, *trailing_value);
                }
                DeformableSurfaceData::SurfaceCurve {
                    surface,
                    native_id,
                    flag,
                    first_parameter,
                    selector,
                    second_parameter,
                    curve,
                    vectors,
                    frame_parameter,
                    flags,
                    parameter_triples,
                } => {
                    native_i64(bytes, 5);
                    let secondary = target
                        .model
                        .surfaces
                        .iter()
                        .find(|candidate| candidate.id == *surface)
                        .ok_or_else(|| {
                            CodecError::Malformed("deformable secondary surface is missing".into())
                        })?;
                    native_embedded_surface(bytes, &secondary.geometry)?;
                    native_i64(bytes, *native_id);
                    bytes.push(native_bool(*flag));
                    native_f64(bytes, *first_parameter);
                    native_i64(bytes, *selector);
                    native_f64(bytes, *second_parameter);
                    let curve = native_loft_curve_in_range(
                        target,
                        curve,
                        Some([*first_parameter, *second_parameter]),
                    )?;
                    native_nurbs_curve(bytes, &curve)?;
                    for vector in vectors {
                        native_vector(bytes, [vector.x, vector.y, vector.z]);
                    }
                    native_f64(bytes, *frame_parameter);
                    for flag in flags {
                        bytes.push(native_bool(*flag));
                    }
                    native_i64(
                        bytes,
                        i64::try_from(parameter_triples.len()).map_err(|_| {
                            CodecError::NotImplemented("deformable triple count exceeds i64".into())
                        })?,
                    );
                    for triple in parameter_triples {
                        for value in triple {
                            native_f64(bytes, *value);
                        }
                    }
                }
                DeformableSurfaceData::Plain {
                    frame,
                    parameter_triples,
                } => {
                    native_i64(bytes, 1);
                    write_frame(bytes, frame);
                    native_i64(
                        bytes,
                        i64::try_from(parameter_triples.len()).map_err(|_| {
                            CodecError::NotImplemented("deformable triple count exceeds i64".into())
                        })?,
                    );
                    for triple in parameter_triples {
                        for value in triple {
                            native_f64(bytes, *value);
                        }
                    }
                }
                DeformableSurfaceData::Guided {
                    frame,
                    selector,
                    guide_parameter,
                } => {
                    native_i64(bytes, 3);
                    write_frame(bytes, frame);
                    native_i64(bytes, *selector);
                    native_f64(bytes, *guide_parameter);
                }
                DeformableSurfaceData::Minimal { vectors, selector } => {
                    native_i64(bytes, 8);
                    for vector in vectors {
                        native_vector(bytes, [vector.x, vector.y, vector.z]);
                    }
                    native_i64(bytes, *selector);
                }
            }
            native_nurbs_surface(bytes, solved_cache)?;
            native_f64(bytes, cache_fit_tolerance / 10.0);
            for values in &construction.discontinuities {
                native_compound_loft_float_array(bytes, values)?;
            }
            bytes.push(native_bool(construction.discontinuity_flag));
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::TSpline { construction } => {
            use cadmpeg_ir::geometry::TSplineSubtransform;
            let cache_fit_tolerance = procedural.cache_fit_tolerance.ok_or_else(|| {
                CodecError::Malformed(
                    "T-spline surface requires a native cache-fit tolerance".into(),
                )
            })?;
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "t_spl_sur")?;
            native_nurbs_surface(bytes, solved_cache)?;
            native_f64(bytes, cache_fit_tolerance / 10.0);
            for values in &construction.discontinuities {
                native_compound_loft_float_array(bytes, values)?;
            }
            bytes.push(native_bool(construction.discontinuity_flag));
            for range in &construction.parameter_ranges {
                for value in range {
                    native_f64(bytes, *value / 10.0);
                }
            }
            native_i64(bytes, construction.type_code);
            bytes.push(0x0f);
            match &construction.subtransform {
                TSplineSubtransform::Inline {
                    program,
                    separator,
                    values,
                } => {
                    let parsed = cadmpeg_ir::geometry::TSplineProgram::parse(program);
                    if construction.program_graph.as_ref() != Some(&parsed) {
                        return Err(CodecError::Malformed(
                            "T-spline parsed program graph diverges from its native program".into(),
                        ));
                    }
                    if construction.values_graph.as_ref()
                        != Some(&cadmpeg_ir::geometry::TSplineProgram::parse(values))
                    {
                        return Err(CodecError::Malformed(
                            "T-spline parsed values graph diverges from its native program".into(),
                        ));
                    }
                    native_ident(bytes, "t_spl_subtrans_object")?;
                    native_u16_string(bytes, program)?;
                    if let Some(separator) = separator {
                        bytes.push(native_bool(*separator));
                    }
                    native_u16_string(bytes, values)?;
                }
                TSplineSubtransform::Reference {
                    resolved: Some(resolved),
                    ..
                } => {
                    let TSplineSubtransform::Inline {
                        program,
                        separator,
                        values,
                    } = resolved.as_ref()
                    else {
                        return Err(CodecError::Malformed(
                            "resolved T-spline subtransform must be inline".into(),
                        ));
                    };
                    let parsed = cadmpeg_ir::geometry::TSplineProgram::parse(program);
                    if construction.program_graph.as_ref() != Some(&parsed) {
                        return Err(CodecError::Malformed(
                            "T-spline parsed program graph diverges from its resolved program"
                                .into(),
                        ));
                    }
                    if construction.values_graph.as_ref()
                        != Some(&cadmpeg_ir::geometry::TSplineProgram::parse(values))
                    {
                        return Err(CodecError::Malformed(
                            "T-spline parsed values graph diverges from its resolved program"
                                .into(),
                        ));
                    }
                    native_ident(bytes, "t_spl_subtrans_object")?;
                    native_u16_string(bytes, program)?;
                    if let Some(separator) = separator {
                        bytes.push(native_bool(*separator));
                    }
                    native_u16_string(bytes, values)?;
                }
                TSplineSubtransform::Reference { resolved: None, .. } => {
                    return Err(CodecError::NotImplemented(
                        "source-less referenced t_spl_subtrans_object has no resolved target"
                            .into(),
                    ));
                }
            }
            bytes.push(0x10);
            native_i64(bytes, construction.trailing_value);
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::Exact {
            parameter_ranges,
            extension,
        } => {
            let cache_fit_tolerance = procedural.cache_fit_tolerance.ok_or_else(|| {
                CodecError::Malformed(
                    "exact spline surface requires a native cache-fit tolerance".into(),
                )
            })?;
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "exact_spl_sur")?;
            native_nurbs_surface(bytes, solved_cache)?;
            native_f64(bytes, cache_fit_tolerance / 10.0);
            for range in parameter_ranges {
                for value in range {
                    native_f64(bytes, *value);
                }
            }
            native_i64(bytes, *extension);
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::Compound {
            parameters,
            components,
        } => {
            if parameters.len() != components.len() {
                return Err(CodecError::Malformed(
                    "comp_spl_sur requires one parameter per component surface".into(),
                ));
            }
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "comp_spl_sur")?;
            native_nurbs_surface(bytes, solved_cache)?;
            if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
                native_f64(bytes, cache_fit_tolerance / 10.0);
            }
            native_i64(
                bytes,
                i64::try_from(parameters.len()).map_err(|_| {
                    CodecError::NotImplemented("compound surface count exceeds i64".into())
                })?,
            );
            for parameter in parameters {
                native_f64(bytes, *parameter);
            }
            for component in components {
                let component = target
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == *component)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "compound surface {} references missing component {component}",
                            procedural.id
                        ))
                    })?;
                native_embedded_surface(bytes, &component.geometry)?;
            }
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::Taper {
            support,
            reference,
            pcurve,
            parameter,
            taper,
        } => {
            let support = target
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == *support)
                .ok_or_else(|| CodecError::Malformed("taper support surface is missing".into()))?;
            let reference = target
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *reference)
                .ok_or_else(|| CodecError::Malformed("taper reference curve is missing".into()))?;
            let reference = native_spline_field_curve(
                &reference.geometry,
                native_pcurve_knot_domain(pcurve.as_ref())?,
            )?;
            let subtype = match taper {
                cadmpeg_ir::geometry::TaperSurfaceKind::Standard => "taper_spl_sur",
                cadmpeg_ir::geometry::TaperSurfaceKind::Orthogonal { .. } => "ortho_spl_sur",
                cadmpeg_ir::geometry::TaperSurfaceKind::Edge { .. } => "edge_tpr_spl_sur",
                cadmpeg_ir::geometry::TaperSurfaceKind::Shadow { .. } => "shadow_tpr_spl_sur",
                cadmpeg_ir::geometry::TaperSurfaceKind::Ruled { .. } => "ruled_tpr_spl_sur",
                cadmpeg_ir::geometry::TaperSurfaceKind::Swept { .. } => "swept_tpr_spl_sur",
            };
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, subtype)?;
            native_embedded_surface(bytes, &support.geometry)?;
            native_nurbs_curve(bytes, &reference)?;
            if let Some(pcurve) = pcurve {
                native_nurbs_pcurve_block(bytes, pcurve)?;
            } else {
                native_ident(bytes, "nullbs")?;
            }
            native_f64(bytes, *parameter);
            native_nurbs_surface(bytes, solved_cache)?;
            if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
                native_f64(bytes, cache_fit_tolerance / 10.0);
            }
            let write_draft = |bytes: &mut Vec<u8>, draft: Vector3| {
                native_vector(bytes, [draft.x, draft.y, draft.z]);
            };
            match taper {
                cadmpeg_ir::geometry::TaperSurfaceKind::Standard => {}
                cadmpeg_ir::geometry::TaperSurfaceKind::Orthogonal { sense } => {
                    bytes.push(native_bool(*sense));
                }
                cadmpeg_ir::geometry::TaperSurfaceKind::Edge { draft } => {
                    write_draft(bytes, *draft);
                }
                cadmpeg_ir::geometry::TaperSurfaceKind::Shadow {
                    draft,
                    sine,
                    cosine,
                }
                | cadmpeg_ir::geometry::TaperSurfaceKind::Swept {
                    draft,
                    sine,
                    cosine,
                } => {
                    write_draft(bytes, *draft);
                    native_f64(bytes, *sine);
                    native_f64(bytes, *cosine);
                }
                cadmpeg_ir::geometry::TaperSurfaceKind::Ruled {
                    draft,
                    sine,
                    cosine,
                    factor,
                } => {
                    write_draft(bytes, *draft);
                    native_f64(bytes, *sine);
                    native_f64(bytes, *cosine);
                    native_f64(bytes, *factor);
                }
            }
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::Loft {
            sections,
            parameter_ranges,
            closures,
            singularities,
            mode,
            bridge,
        } => encode_native_loft(
            bytes,
            target,
            procedural,
            sections,
            parameter_ranges,
            closures,
            singularities,
            *mode,
            bridge,
            solved_cache,
        )?,
        ProceduralSurfaceDefinition::CompoundLoft { construction } => {
            encode_native_compound_loft(bytes, target, procedural, construction, solved_cache)?;
        }
        ProceduralSurfaceDefinition::ScaledCompoundLoft { construction } => {
            encode_native_scaled_compound_loft(
                bytes,
                target,
                procedural,
                construction,
                Some(solved_cache),
            )?;
        }
        ProceduralSurfaceDefinition::Skin { construction } => {
            encode_native_skin_surface(bytes, target, procedural, construction, solved_cache)?;
        }
        ProceduralSurfaceDefinition::Net { construction } => {
            encode_native_net_surface(bytes, target, procedural, construction, solved_cache)?;
        }
        ProceduralSurfaceDefinition::Sweep {
            profile,
            spine,
            native: Some(construction),
        } => encode_native_sweep_surface(
            bytes,
            target,
            procedural,
            profile,
            spine,
            construction,
            solved_cache,
        )?,
        ProceduralSurfaceDefinition::Sweep { native: None, .. } => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D sweep surface {} lacks its native construction graph",
                procedural.id
            )))
        }
        ProceduralSurfaceDefinition::G2Blend { construction } => {
            encode_native_g2_blend(bytes, target, procedural, construction, solved_cache)?;
        }
        ProceduralSurfaceDefinition::VariableBlend { construction } => {
            encode_native_variable_blend(bytes, target, procedural, construction, solved_cache)?;
        }
        ProceduralSurfaceDefinition::VertexBlend { construction } => {
            encode_native_vertex_blend(bytes, target, construction, solved_cache)?;
        }
        ProceduralSurfaceDefinition::Ruled { first, second } => {
            let profiles = [first, second]
                .map(|id| {
                    target
                        .model
                        .curves
                        .iter()
                        .find(|curve| curve.id == *id)
                        .ok_or_else(|| {
                            CodecError::Malformed(format!(
                                "ruled surface {} references missing profile {id}",
                                procedural.id
                            ))
                        })
                })
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?;
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "rule_sur")?;
            let profile_range = [
                solved_cache.u_knots.first().copied().ok_or_else(|| {
                    CodecError::Malformed("ruled solved surface has no U knot domain".into())
                })?,
                solved_cache.u_knots.last().copied().ok_or_else(|| {
                    CodecError::Malformed("ruled solved surface has no U knot domain".into())
                })?,
            ];
            for profile in profiles {
                let profile = native_interval_curve(&profile.geometry, profile_range)?;
                native_nurbs_curve(bytes, &profile)?;
            }
            native_nurbs_surface(bytes, solved_cache)?;
            if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
                native_f64(bytes, cache_fit_tolerance / 10.0);
            }
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::Sum {
            first,
            second,
            basepoint,
        } => {
            let curves = [first, second]
                .map(|id| {
                    target
                        .model
                        .curves
                        .iter()
                        .find(|curve| curve.id == *id)
                        .ok_or_else(|| {
                            CodecError::Malformed(format!(
                                "sum surface {} references missing curve {id}",
                                procedural.id
                            ))
                        })
                })
                .into_iter()
                .collect::<Result<Vec<_>, _>>()?;
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "sum_spl_sur")?;
            let ranges = [&solved_cache.u_knots, &solved_cache.v_knots]
                .into_iter()
                .map(|knots| {
                    Ok::<_, CodecError>([
                        knots.first().copied().ok_or_else(|| {
                            CodecError::Malformed(
                                "sum solved surface has an empty knot domain".into(),
                            )
                        })?,
                        knots.last().copied().ok_or_else(|| {
                            CodecError::Malformed(
                                "sum solved surface has an empty knot domain".into(),
                            )
                        })?,
                    ])
                })
                .collect::<Result<Vec<_>, _>>()?;
            for (curve, range) in curves.into_iter().zip(ranges) {
                let curve = native_interval_curve(&curve.geometry, range)?;
                native_nurbs_curve(bytes, &curve)?;
            }
            native_point(
                bytes,
                [basepoint.x / 10.0, basepoint.y / 10.0, basepoint.z / 10.0],
            );
            native_nurbs_surface(bytes, solved_cache)?;
            if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
                native_f64(bytes, cache_fit_tolerance / 10.0);
            }
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::Revolution {
            directrix,
            axis_origin,
            axis_direction,
            angular_interval,
            parameter_interval,
            transposed,
        } => {
            let parameter_interval = (*parameter_interval).ok_or_else(|| {
                CodecError::NotImplemented(
                    "source-less F3D rot_spl_sur requires a directrix parameter interval".into(),
                )
            })?;
            let directrix = target
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *directrix)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "revolution surface {} references a missing directrix",
                        procedural.id
                    ))
                })?;
            let directrix = native_interval_curve(&directrix.geometry, parameter_interval)?;
            let native_parameter_interval = [
                directrix.knots.first().copied().unwrap_or(0.0),
                directrix.knots.last().copied().unwrap_or(0.0),
            ];
            let native_angular_interval = [
                solved_cache.v_knots.first().copied().unwrap_or(0.0),
                solved_cache.v_knots.last().copied().unwrap_or(0.0),
            ];
            if *transposed
                || parameter_interval != native_parameter_interval
                || *angular_interval != native_angular_interval
            {
                return Err(CodecError::NotImplemented(
                    "source-less F3D rot_spl_sur intervals must match its profile and solved cache and cannot be transposed".into(),
                ));
            }
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(bytes, "rot_spl_sur")?;
            native_nurbs_curve(bytes, &directrix)?;
            native_point(
                bytes,
                [
                    axis_origin.x / 10.0,
                    axis_origin.y / 10.0,
                    axis_origin.z / 10.0,
                ],
            );
            native_vector(
                bytes,
                [axis_direction.x, axis_direction.y, axis_direction.z],
            );
            native_nurbs_surface(bytes, solved_cache)?;
            if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
                native_f64(bytes, cache_fit_tolerance / 10.0);
            }
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::Offset {
            support,
            distance,
            u_sense,
            v_sense,
            extension_flags,
        } => {
            let u_sense = (*u_sense).ok_or_else(|| {
                CodecError::NotImplemented(
                    "source-less F3D offset surface requires a U sense".into(),
                )
            })?;
            let v_sense = (*v_sense).ok_or_else(|| {
                CodecError::NotImplemented(
                    "source-less F3D offset surface requires a V sense".into(),
                )
            })?;
            let support = target
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == *support)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "offset surface {} references a missing support",
                        procedural.id
                    ))
                })?;
            let valid_flags = matches!(
                extension_flags.as_slice(),
                [] | [false] | [true, _] | [true, _, _]
            );
            if !valid_flags {
                return Err(CodecError::Malformed(
                    "off_spl_sur ASM extension flags have an invalid conditional shape".into(),
                ));
            }
            native_surface_base(bytes, "spline")?;
            bytes.push(0x0f);
            native_ident(
                bytes,
                if extension_flags.is_empty() {
                    "offsur"
                } else {
                    "off_spl_sur"
                },
            )?;
            native_embedded_surface(bytes, &support.geometry)?;
            native_f64(bytes, *distance / 10.0);
            native_enum(bytes, u_sense);
            native_enum(bytes, v_sense);
            for flag in extension_flags {
                bytes.push(native_bool(*flag));
            }
            native_nurbs_surface(bytes, solved_cache)?;
            if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
                native_f64(bytes, cache_fit_tolerance / 10.0);
            }
            bytes.push(0x10);
        }
        ProceduralSurfaceDefinition::Extrusion {
            directrix,
            parameter_interval,
            direction,
            native_position,
        } => encode_native_extrusion(
            bytes,
            target,
            procedural,
            directrix,
            parameter_interval.ok_or_else(|| {
                CodecError::Malformed("source-less F3D extrusion lacks its native interval".into())
            })?,
            *direction,
            native_position.ok_or_else(|| {
                CodecError::Malformed("source-less F3D extrusion lacks its native position".into())
            })?,
            solved_cache,
        )?,
        ProceduralSurfaceDefinition::Blend {
            supports,
            spine,
            radius,
            cross_section,
            native,
        } => {
            if let Some(native) = native {
                encode_complete_native_rolling_ball(
                    bytes,
                    target,
                    procedural,
                    native,
                    solved_cache,
                )?;
            } else {
                encode_native_rolling_ball(
                    bytes,
                    target,
                    procedural,
                    supports,
                    spine.as_ref(),
                    radius,
                    cross_section,
                    solved_cache,
                )?;
            }
        }
        ProceduralSurfaceDefinition::Helix { .. } => {
            return Err(CodecError::Malformed(format!(
                "source-less F3D helix surface {} must use its cacheless native carrier",
                procedural.id
            )))
        }
        ProceduralSurfaceDefinition::RollingBallJet { .. }
        | ProceduralSurfaceDefinition::LinearSweep { .. }
        | ProceduralSurfaceDefinition::AxisRevolution { .. }
        | ProceduralSurfaceDefinition::ParallelOffset { .. }
        | ProceduralSurfaceDefinition::DegenerateTorus { .. }
        | ProceduralSurfaceDefinition::CurveBounded { .. }
        | ProceduralSurfaceDefinition::Subset { .. }
        | ProceduralSurfaceDefinition::Unknown { .. } => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D procedural surface {} has no lossless native encoding",
                procedural.id
            )))
        }
    }
    Ok(true)
}

fn native_bridge_token(
    bytes: &mut Vec<u8>,
    token: &cadmpeg_ir::geometry::LoftBridgeToken,
) -> Result<(), CodecError> {
    match token {
        cadmpeg_ir::geometry::LoftBridgeToken::Boolean(value) => {
            bytes.push(native_bool(*value));
        }
        cadmpeg_ir::geometry::LoftBridgeToken::Integer(value) => native_i64(bytes, *value),
        cadmpeg_ir::geometry::LoftBridgeToken::Double(value) => native_f64(bytes, *value),
        cadmpeg_ir::geometry::LoftBridgeToken::Text(value) => native_string(bytes, value)?,
        cadmpeg_ir::geometry::LoftBridgeToken::Enum(value) => native_enum(bytes, *value),
    }
    Ok(())
}

fn native_g2_pcurve(
    bytes: &mut Vec<u8>,
    pcurve: Option<&PcurveGeometry>,
) -> Result<(), CodecError> {
    if let Some(pcurve) = pcurve {
        native_nurbs_pcurve_block(bytes, pcurve)
    } else {
        native_ident(bytes, "nullbs")
    }
}

fn native_g2_side(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    side: &cadmpeg_ir::geometry::G2BlendSide,
) -> Result<(), CodecError> {
    native_string(bytes, &side.label)?;
    let surface = target
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == side.surface)
        .ok_or_else(|| CodecError::Malformed(format!("G2 support {} is missing", side.surface)))?;
    native_embedded_surface(bytes, &surface.geometry)?;
    let curve = target
        .model
        .curves
        .iter()
        .find(|curve| curve.id == side.curve)
        .ok_or_else(|| CodecError::Malformed(format!("G2 side curve {} is missing", side.curve)))?;
    let pcurve = side
        .pcurves
        .iter()
        .flatten()
        .find(|pcurve| matches!(pcurve, PcurveGeometry::Nurbs { .. }));
    let curve = native_spline_field_curve(&curve.geometry, native_pcurve_knot_domain(pcurve)?)?;
    native_nurbs_curve(bytes, &curve)?;
    native_g2_pcurve(bytes, side.pcurves[0].as_ref())?;
    native_vector(
        bytes,
        [side.direction.x, side.direction.y, side.direction.z],
    );
    native_g2_pcurve(bytes, side.pcurves[1].as_ref())?;
    Ok(())
}

fn encode_native_g2_blend(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::G2BlendConstruction,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "g2_blend_spl_sur")?;
    native_g2_side(bytes, target, &construction.first)?;
    native_enum(bytes, construction.singularity);
    match &construction.first_shape {
        cadmpeg_ir::geometry::G2BlendFirstShape::Full { surface, tolerance } => {
            match (surface, tolerance) {
                (None, None) => native_ident(bytes, "nullbs")?,
                (Some(surface), Some(tolerance)) => {
                    let surface = target
                        .model
                        .surfaces
                        .iter()
                        .find(|candidate| candidate.id == *surface)
                        .ok_or_else(|| {
                            CodecError::Malformed("G2 first exact surface is missing".into())
                        })?;
                    let SurfaceGeometry::Nurbs(surface) = &surface.geometry else {
                        return Err(CodecError::NotImplemented(
                            "source-less G2 full branch requires a NURBS exact surface".into(),
                        ));
                    };
                    native_nurbs_surface(bytes, surface)?;
                    native_f64(bytes, *tolerance / 10.0);
                }
                _ => {
                    return Err(CodecError::Malformed(
                        "G2 full surface and tolerance must be paired".into(),
                    ));
                }
            }
        }
        cadmpeg_ir::geometry::G2BlendFirstShape::None {
            coefficients,
            tolerance,
            extension,
            pcurve,
        } => {
            for coefficient in coefficients {
                native_f64(bytes, *coefficient);
            }
            native_f64(bytes, *tolerance / 10.0);
            if let Some(extension) = extension {
                native_bridge_token(bytes, extension)?;
            }
            native_g2_pcurve(bytes, pcurve.as_ref())?;
        }
    }
    native_g2_side(bytes, target, &construction.second)?;
    let second_exact = target
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == construction.second_exact_surface)
        .ok_or_else(|| CodecError::Malformed("G2 second exact surface is missing".into()))?;
    let SurfaceGeometry::Nurbs(second_exact) = &second_exact.geometry else {
        return Err(CodecError::NotImplemented(
            "source-less G2 second exact surface must be NURBS".into(),
        ));
    };
    native_nurbs_surface(bytes, second_exact)?;
    let center_curve = native_loft_curve_in_range(
        target,
        &construction.center_curve,
        Some(construction.center_parameters),
    )?;
    native_nurbs_curve(bytes, &center_curve)?;
    for value in construction.center_parameters {
        native_f64(bytes, value);
    }
    native_i64(bytes, construction.center_flag);
    for range in construction.parameter_ranges {
        native_f64(bytes, range[0]);
        native_f64(bytes, range[1]);
    }
    for value in construction.trailing_parameters {
        native_f64(bytes, value);
    }
    native_nurbs_surface(bytes, solved_cache)?;
    if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
        native_f64(bytes, cache_fit_tolerance / 10.0);
    }
    for discontinuities in &construction.discontinuities {
        native_i64(
            bytes,
            i64::try_from(discontinuities.len()).map_err(|_| {
                CodecError::NotImplemented("G2 discontinuity count exceeds i64".into())
            })?,
        );
        for value in discontinuities {
            native_f64(bytes, *value);
        }
    }
    bytes.push(0x10);
    Ok(())
}

fn native_loft_curve(
    target: &CadIr,
    id: &cadmpeg_ir::ids::CurveId,
) -> Result<NurbsCurve, CodecError> {
    let curve = target
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *id)
        .ok_or_else(|| CodecError::Malformed(format!("loft references missing curve {id}")))?;
    native_spline_field_curve(&curve.geometry, None).map_err(|_| {
        CodecError::NotImplemented(format!(
            "source-less F3D loft requires a NURBS, circle, or ellipse curve {id}"
        ))
    })
}

fn native_loft_subdata(
    bytes: &mut Vec<u8>,
    subdata: &cadmpeg_ir::geometry::LoftSubdata,
) -> Result<(), CodecError> {
    let expected_rows = if subdata.type_code == 211 {
        1
    } else {
        usize::try_from(subdata.row_count)
            .map_err(|_| CodecError::Malformed("negative loft row count".into()))?
    };
    let expected_columns = usize::try_from(subdata.column_count)
        .map_err(|_| CodecError::Malformed("negative loft column count".into()))?;
    if subdata.rows.len() != expected_rows
        || (subdata.type_code != 211
            && subdata
                .rows
                .iter()
                .any(|row| row.columns.len() != expected_columns))
    {
        return Err(CodecError::Malformed(
            "loft subdata counts do not match their rows".into(),
        ));
    }
    native_i64(bytes, subdata.type_code);
    native_i64(bytes, subdata.row_count);
    native_i64(bytes, subdata.column_count);
    for row in &subdata.rows {
        for value in row.parameters {
            native_f64(bytes, value);
        }
        for column in &row.columns {
            native_f64(bytes, column[0]);
            native_f64(bytes, column[1]);
        }
    }
    Ok(())
}

fn native_loft_section(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    section: &cadmpeg_ir::geometry::LoftSection,
    parameter_range: Option<[f64; 2]>,
) -> Result<(), CodecError> {
    native_i64(
        bytes,
        i64::try_from(section.entries.len())
            .map_err(|_| CodecError::NotImplemented("loft section count exceeds i64".into()))?,
    );
    for entry in &section.entries {
        native_f64(bytes, entry.parameter);
        native_i64(
            bytes,
            i64::try_from(entry.profile.len())
                .map_err(|_| CodecError::NotImplemented("loft profile count exceeds i64".into()))?,
        );
        for member in &entry.profile {
            native_i64(bytes, member.type_code);
            let curve = native_loft_curve_in_range(target, &member.curve, parameter_range)?;
            native_nurbs_curve(bytes, &curve)?;
            let surface = target
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == member.data.surface)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "loft references missing surface {}",
                        member.data.surface
                    ))
                })?;
            native_embedded_surface(bytes, &surface.geometry)?;
            if let Some(pcurve) = &member.data.pcurve {
                native_nurbs_pcurve_block(bytes, pcurve)?;
            } else {
                native_ident(bytes, "nullbs")?;
            }
            bytes.push(native_bool(member.data.first_flag));
            native_i64(bytes, member.data.asm_extension);
            native_loft_subdata(bytes, &member.data.subdata)?;
            bytes.push(native_bool(member.data.direction.is_some()));
            if let Some(direction) = member.data.direction {
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
        }
        let path = native_loft_curve_in_range(target, &entry.path.curve, parameter_range)?;
        native_nurbs_curve(bytes, &path)?;
        native_i64(
            bytes,
            i64::try_from(entry.path.auxiliaries.len()).map_err(|_| {
                CodecError::NotImplemented("loft auxiliary count exceeds i64".into())
            })?,
        );
        for auxiliary in &entry.path.auxiliaries {
            let auxiliary = native_loft_curve_in_range(target, auxiliary, parameter_range)?;
            native_nurbs_curve(bytes, &auxiliary)?;
        }
        native_i64(bytes, entry.path.flag);
    }
    Ok(())
}

fn native_loft_curve_in_range(
    target: &CadIr,
    id: &cadmpeg_ir::ids::CurveId,
    parameter_range: Option<[f64; 2]>,
) -> Result<NurbsCurve, CodecError> {
    let curve = target
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *id)
        .ok_or_else(|| CodecError::Malformed(format!("loft references missing curve {id}")))?;
    native_spline_field_curve(&curve.geometry, parameter_range).map_err(|_| {
        CodecError::NotImplemented(format!(
            "source-less F3D loft requires NURBS curve {id} without a section domain"
        ))
    })
}

fn native_compound_loft_scale(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    scale: &cadmpeg_ir::geometry::CompoundLoftScale,
) -> Result<(), CodecError> {
    native_i64(
        bytes,
        i64::try_from(scale.members.len()).map_err(|_| {
            CodecError::NotImplemented("compound-loft member count exceeds i64".into())
        })?,
    );
    for member in &scale.members {
        native_i64(bytes, member.type_code);
        let curve = target
            .model
            .curves
            .iter()
            .find(|curve| curve.id == member.curve)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "compound loft references missing member curve {}",
                    member.curve
                ))
            })?;
        let curve = native_spline_field_curve(
            &curve.geometry,
            native_pcurve_knot_domain(member.data.pcurve.as_ref())?,
        )?;
        native_nurbs_curve(bytes, &curve)?;
        let surface = target
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == member.data.surface)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "compound loft references missing surface {}",
                    member.data.surface
                ))
            })?;
        native_embedded_surface(bytes, &surface.geometry)?;
        native_optional_pcurve(bytes, member.data.pcurve.as_ref())?;
        bytes.push(native_bool(member.data.first_flag));
        native_i64(bytes, member.data.asm_extension);
        native_loft_subdata(bytes, &member.data.subdata)?;
        bytes.push(native_bool(member.data.direction.is_some()));
        if let Some(direction) = member.data.direction {
            native_vector(bytes, [direction.x, direction.y, direction.z]);
        }
    }
    native_nurbs_curve(bytes, &native_loft_curve(target, &scale.path)?)?;
    native_i64(
        bytes,
        i64::try_from(scale.auxiliaries.len()).map_err(|_| {
            CodecError::NotImplemented("compound-loft auxiliary count exceeds i64".into())
        })?,
    );
    for auxiliary in &scale.auxiliaries {
        native_nurbs_curve(bytes, &native_loft_curve(target, auxiliary)?)?;
    }
    for value in scale.tail {
        native_i64(bytes, value);
    }
    Ok(())
}

fn encode_native_compound_loft(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::CompoundLoftConstruction,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::{CompoundLoftDirection, CompoundLoftTail};

    let cache_fit_tolerance = procedural.cache_fit_tolerance.ok_or_else(|| {
        CodecError::Malformed("compound-loft surface requires a native cache-fit tolerance".into())
    })?;

    let first_absent = construction.scales.iter().position(Option::is_none);
    if first_absent
        .is_some_and(|index| construction.scales[index + 1..].iter().any(Option::is_some))
    {
        return Err(CodecError::Malformed(
            "compound-loft leading scales must form a contiguous prefix".into(),
        ));
    }
    if construction.fifth_scale.is_some() && first_absent.is_some() {
        return Err(CodecError::Malformed(
            "compound-loft fifth scale requires all four leading scales".into(),
        ));
    }

    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "cl_loft_spl_sur")?;
    native_nurbs_surface(bytes, solved_cache)?;
    native_f64(bytes, cache_fit_tolerance / 10.0);
    for scale in construction.scales.iter().flatten() {
        native_compound_loft_scale(bytes, target, scale)?;
    }
    if let Some(scale) = construction.fifth_scale.as_deref() {
        native_compound_loft_scale(bytes, target, scale)?;
    }
    for flag in construction.flags {
        bytes.push(native_bool(flag));
    }
    match &construction.tail {
        CompoundLoftTail::Six {
            flags,
            scale,
            selector,
            direction,
            parameter_range,
            curve,
        } => {
            native_i64(bytes, 6);
            for flag in flags {
                bytes.push(native_bool(*flag));
            }
            native_compound_loft_scale(bytes, target, scale)?;
            native_i64(bytes, *selector);
            native_vector(bytes, [direction.x, direction.y, direction.z]);
            for value in parameter_range {
                native_f64(bytes, *value);
            }
            let curve = native_loft_curve_in_range(target, curve, Some(*parameter_range))?;
            native_nurbs_curve(bytes, &curve)?;
        }
        CompoundLoftTail::Seven {
            first_flag,
            first_scale,
            second_flag,
            second_scale,
            selector,
            direction,
            trailing_flags,
        } => {
            native_i64(bytes, 7);
            bytes.push(native_bool(*first_flag));
            if let Some(scale) = first_scale.as_deref() {
                native_compound_loft_scale(bytes, target, scale)?;
            }
            bytes.push(native_bool(*second_flag));
            native_compound_loft_scale(bytes, target, second_scale)?;
            native_i64(bytes, *selector);
            native_vector(bytes, [direction.x, direction.y, direction.z]);
            for flag in trailing_flags {
                bytes.push(native_bool(*flag));
            }
        }
        CompoundLoftTail::Zero {
            flags,
            selector,
            direction,
            trailing_flags,
        } => {
            native_i64(bytes, 0);
            for flag in flags {
                bytes.push(native_bool(*flag));
            }
            native_i64(bytes, *selector);
            match direction {
                CompoundLoftDirection::Vector { value } if *selector == 0 => {
                    native_vector(bytes, [value.x, value.y, value.z]);
                }
                CompoundLoftDirection::Curve { curve } if *selector != 0 => {
                    native_nurbs_curve(bytes, &native_loft_curve(target, curve)?)?;
                }
                _ => {
                    return Err(CodecError::Malformed(
                        "compound-loft direction conflicts with its selector".into(),
                    ));
                }
            }
            for flag in trailing_flags {
                bytes.push(native_bool(*flag));
            }
        }
    }
    bytes.push(0x10);
    Ok(())
}

fn native_compound_loft_float_array(bytes: &mut Vec<u8>, values: &[f64]) -> Result<(), CodecError> {
    native_i64(
        bytes,
        i64::try_from(values.len()).map_err(|_| {
            CodecError::NotImplemented("compound-loft float-array count exceeds i64".into())
        })?,
    );
    for value in values {
        native_f64(bytes, *value);
    }
    Ok(())
}

fn encode_native_scaled_compound_loft(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::ScaledCompoundLoftConstruction,
    solved_cache: Option<&NurbsSurface>,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::{
        CompoundLoftDirection, ScaledCompoundLoftBranch, ScaledCompoundLoftShape,
    };

    let first_absent = construction.scales.iter().position(Option::is_none);
    if first_absent
        .is_some_and(|index| construction.scales[index + 1..].iter().any(Option::is_some))
    {
        return Err(CodecError::Malformed(
            "scaled compound-loft scales must form a contiguous prefix".into(),
        ));
    }
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "scaled_cloft_spl_sur")?;
    native_enum(bytes, construction.singularity);
    match &construction.shape {
        ScaledCompoundLoftShape::Full => {
            let solved_cache = solved_cache.ok_or_else(|| {
                CodecError::Malformed(
                    "scaled compound-loft full shape requires a solved NURBS cache".into(),
                )
            })?;
            let cache_fit_tolerance = procedural.cache_fit_tolerance.ok_or_else(|| {
                CodecError::Malformed(
                    "scaled compound-loft full shape requires a native cache-fit tolerance".into(),
                )
            })?;
            native_nurbs_surface(bytes, solved_cache)?;
            native_f64(bytes, cache_fit_tolerance / 10.0);
        }
        ScaledCompoundLoftShape::None {
            parameter_ranges,
            parameters,
        } => {
            if procedural.cache_fit_tolerance.is_some() {
                return Err(CodecError::Malformed(
                    "scaled compound-loft none shape cannot carry a cache-fit tolerance".into(),
                ));
            }
            for range in parameter_ranges {
                for value in range {
                    native_f64(bytes, *value);
                }
            }
            for values in parameters {
                native_compound_loft_float_array(bytes, values)?;
            }
        }
    }
    for values in &construction.discontinuities {
        native_compound_loft_float_array(bytes, values)?;
    }
    bytes.push(native_bool(construction.discontinuity_flag));
    for scale in construction.scales.iter().flatten() {
        native_compound_loft_scale(bytes, target, scale)?;
    }
    for flag in construction.flags {
        bytes.push(native_bool(flag));
    }
    native_i64(bytes, construction.selector);
    match &construction.branch {
        ScaledCompoundLoftBranch::ExtendedVector {
            first_scale,
            second_scale,
            selector,
            direction,
        } => {
            bytes.push(native_bool(true));
            if let Some(scale) = first_scale.as_deref() {
                native_compound_loft_scale(bytes, target, scale)?;
            }
            bytes.push(native_bool(true));
            native_compound_loft_scale(bytes, target, second_scale)?;
            native_i64(bytes, *selector);
            native_vector(bytes, [direction.x, direction.y, direction.z]);
        }
        ScaledCompoundLoftBranch::ExtendedCurve {
            scale,
            flag,
            singularity,
            curve,
        } => {
            bytes.push(native_bool(true));
            if let Some(scale) = scale.as_deref() {
                native_compound_loft_scale(bytes, target, scale)?;
            }
            bytes.push(native_bool(false));
            bytes.push(native_bool(*flag));
            native_enum(bytes, *singularity);
            native_nurbs_curve(bytes, &native_loft_curve(target, curve)?)?;
        }
        ScaledCompoundLoftBranch::Direct {
            flag,
            selector,
            direction,
        } => {
            bytes.push(native_bool(false));
            bytes.push(native_bool(*flag));
            native_i64(bytes, *selector);
            match direction {
                CompoundLoftDirection::Vector { value } if *selector == 0 => {
                    native_vector(bytes, [value.x, value.y, value.z]);
                }
                CompoundLoftDirection::Curve { curve } if *selector != 0 => {
                    native_nurbs_curve(bytes, &native_loft_curve(target, curve)?)?;
                }
                _ => {
                    return Err(CodecError::Malformed(
                        "scaled compound-loft direction conflicts with its selector".into(),
                    ));
                }
            }
        }
    }
    for flag in construction.trailing_flags {
        bytes.push(native_bool(flag));
    }
    native_i64(bytes, construction.tail_kind);
    for direction in construction.tail_directions {
        native_vector(bytes, [direction.x, direction.y, direction.z]);
    }
    native_enum(bytes, construction.tail_singularity);
    native_nurbs_curve(bytes, &native_loft_curve(target, &construction.tail_curve)?)?;
    bytes.push(0x10);
    Ok(())
}

fn native_cacheless_procedural_surface(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    surface: &Surface,
) -> Result<bool, CodecError> {
    let mut definitions = target
        .model
        .procedural_surfaces
        .iter()
        .filter(|procedural| procedural.surface == surface.id);
    let Some(procedural) = definitions.next() else {
        return Ok(false);
    };
    if definitions.next().is_some() {
        return Err(CodecError::Malformed(format!(
            "surface {} has multiple procedural constructions",
            surface.id
        )));
    }
    if let ProceduralSurfaceDefinition::Helix { construction } = &procedural.definition {
        use cadmpeg_ir::geometry::HelixSurfaceProfile;
        native_surface_base(bytes, "spline")?;
        bytes.push(0x0f);
        let circular = matches!(construction.profile, HelixSurfaceProfile::Circle { .. });
        native_ident(
            bytes,
            if circular {
                "helix_spl_circ"
            } else {
                "helix_spl_line"
            },
        )?;
        for value in construction.angle_range {
            native_f64(bytes, value);
        }
        for value in construction.dimension_range {
            native_f64(bytes, if circular { value / 10.0 } else { value });
        }
        if let HelixSurfaceProfile::Circle { length, .. } = construction.profile {
            native_f64(bytes, length / 10.0);
        }
        for value in construction.path.angle_range {
            native_f64(bytes, value);
        }
        native_point(
            bytes,
            [
                construction.path.center.x / 10.0,
                construction.path.center.y / 10.0,
                construction.path.center.z / 10.0,
            ],
        );
        for vector in [
            construction.path.major,
            construction.path.minor,
            construction.path.pitch,
        ] {
            native_point(bytes, [vector.x / 10.0, vector.y / 10.0, vector.z / 10.0]);
        }
        native_f64(bytes, construction.path.apex_factor);
        native_vector(
            bytes,
            [
                construction.path.axis.x,
                construction.path.axis.y,
                construction.path.axis.z,
            ],
        );
        for sentinel in ["null_surface", "null_surface", "nullbs", "nullbs"] {
            native_ident(bytes, sentinel)?;
        }
        match construction.profile {
            HelixSurfaceProfile::Circle { radius, .. } => native_f64(bytes, radius / 10.0),
            HelixSurfaceProfile::Line { origin } => {
                native_point(bytes, [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0]);
            }
        }
        bytes.push(0x10);
        return Ok(true);
    }
    if let ProceduralSurfaceDefinition::ScaledCompoundLoft { construction } = &procedural.definition
    {
        if matches!(
            construction.shape,
            cadmpeg_ir::geometry::ScaledCompoundLoftShape::None { .. }
        ) {
            encode_native_scaled_compound_loft(bytes, target, procedural, construction, None)?;
            return Ok(true);
        }
    }
    Ok(false)
}

fn native_law_expression(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    expression: &cadmpeg_ir::geometry::LawExpression,
    depth: usize,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::LawExpression;
    if depth > 64 {
        return Err(CodecError::Malformed(
            "native law expression exceeds 64 recursive levels".into(),
        ));
    }
    match expression {
        LawExpression::Null => native_string(bytes, "null_law")?,
        LawExpression::Integer { value } => native_i64(bytes, *value),
        LawExpression::Double { value } => native_f64(bytes, *value),
        LawExpression::Point { value } => {
            native_point(bytes, [value.x / 10.0, value.y / 10.0, value.z / 10.0]);
        }
        LawExpression::Vector { value } => {
            native_vector(bytes, [value.x, value.y, value.z]);
        }
        LawExpression::Transform { scalars, enums } => {
            native_string(bytes, "TRANS")?;
            for scalar in scalars {
                native_f64(bytes, *scalar);
            }
            for value in enums {
                native_enum(bytes, *value);
            }
        }
        LawExpression::Edge { curve, parameters } => {
            native_string(bytes, "EDGE")?;
            let curve = target
                .model
                .curves
                .iter()
                .find(|candidate| candidate.id == *curve)
                .ok_or_else(|| {
                    CodecError::Malformed(format!("law edge curve {curve} is missing"))
                })?;
            let curve = native_interval_curve(&curve.geometry, *parameters)?;
            native_nurbs_curve(bytes, &curve)?;
            for parameter in parameters {
                native_f64(bytes, *parameter);
            }
        }
        LawExpression::Spline {
            native_id,
            knots,
            controls,
            point,
        } => {
            native_string(bytes, "SPLINE_LAW")?;
            native_i64(bytes, *native_id);
            native_compound_loft_float_array(bytes, knots)?;
            native_compound_loft_float_array(bytes, controls)?;
            native_point(bytes, [point.x / 10.0, point.y / 10.0, point.z / 10.0]);
        }
        LawExpression::Algebraic { operator, operands } => {
            let arity = match operator.as_str() {
                "COS" | "SIN" | "TAN" | "COT" | "SEC" | "CSC" | "COSH" | "SINH" | "TANH"
                | "COTH" | "SECH" | "CSCH" | "ARCCOS" | "ARCSIN" | "ARCTAN" | "ARCOT"
                | "ARCSEC" | "ARCCSC" | "ARCCOSH" | "ARCSINH" | "ARCTANH" | "ARCOTH"
                | "ARCSECH" | "ARCCSCH" | "ABS" | "EXP" | "LN" | "LOG" | "SIGN" | "SIZE"
                | "TERM" | "SQRT" | "NORM" | "NOT" => 1,
                "CROSS" | "DOT" | "DCUR" => 2,
                "VEC" | "DSURF" => 3,
                "MIN" | "MAX" | "SET" | "ROTATE" | "STEP" => {
                    return Err(CodecError::NotImplemented(format!(
                        "source-less F3D law operator {operator} has unresolved variable arity"
                    )));
                }
                _ => {
                    return Err(CodecError::NotImplemented(format!(
                        "source-less F3D law operator {operator} has no defined byte grammar"
                    )));
                }
            };
            if operands.len() != arity {
                return Err(CodecError::Malformed(format!(
                    "F3D law operator {operator} requires {arity} operands, got {}",
                    operands.len()
                )));
            }
            native_string(bytes, operator)?;
            for operand in operands {
                native_law_expression(bytes, target, operand, depth + 1)?;
            }
        }
    }
    Ok(())
}

fn native_law_formula(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    formula: &cadmpeg_ir::geometry::LawFormula,
) -> Result<(), CodecError> {
    native_string(bytes, &formula.name)?;
    if formula.name == "null_law" {
        if !formula.variables.is_empty() {
            return Err(CodecError::Malformed(
                "null_law formula cannot carry variables".into(),
            ));
        }
        return Ok(());
    }
    native_i64(
        bytes,
        i64::try_from(formula.variables.len())
            .map_err(|_| CodecError::NotImplemented("law variable count exceeds i64".into()))?,
    );
    for variable in &formula.variables {
        native_law_expression(bytes, target, variable, 0)?;
    }
    Ok(())
}

fn native_skin_profile_data(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    data: &cadmpeg_ir::geometry::LoftProfileData,
) -> Result<(), CodecError> {
    let surface = target
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == data.surface)
        .ok_or_else(|| {
            CodecError::Malformed(format!("skin references missing surface {}", data.surface))
        })?;
    native_embedded_surface(bytes, &surface.geometry)?;
    native_optional_pcurve(bytes, data.pcurve.as_ref())?;
    bytes.push(native_bool(data.first_flag));
    native_i64(bytes, data.asm_extension);
    native_loft_subdata(bytes, &data.subdata)?;
    bytes.push(native_bool(data.direction.is_some()));
    if let Some(direction) = data.direction {
        native_vector(bytes, [direction.x, direction.y, direction.z]);
    }
    Ok(())
}

fn encode_native_skin_surface(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::SkinSurfaceConstruction,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::SkinSurfaceLayout;
    let cache_fit_tolerance = procedural.cache_fit_tolerance.ok_or_else(|| {
        CodecError::Malformed("skin surface requires a native cache-fit tolerance".into())
    })?;
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "skin_spl_sur")?;
    native_enum(bytes, construction.surface_boolean);
    native_enum(bytes, construction.surface_normal);
    native_enum(bytes, construction.surface_direction);
    native_i64(bytes, construction.count);
    native_f64(bytes, construction.parameter);
    native_i64(bytes, construction.inner_count);
    match &construction.layout {
        SkinSurfaceLayout::Profiles {
            profiles,
            path,
            tail,
        } => {
            if usize::try_from(construction.inner_count).ok() != Some(profiles.len()) {
                return Err(CodecError::Malformed(
                    "skin profile count conflicts with its inner count".into(),
                ));
            }
            for profile in profiles {
                native_i64(bytes, profile.type_code);
                let curve = target
                    .model
                    .curves
                    .iter()
                    .find(|curve| curve.id == profile.curve)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "skin references missing profile curve {}",
                            profile.curve
                        ))
                    })?;
                let curve = native_spline_field_curve(
                    &curve.geometry,
                    native_pcurve_knot_domain(profile.data.pcurve.as_ref())?,
                )?;
                native_nurbs_curve(bytes, &curve)?;
                native_skin_profile_data(bytes, target, &profile.data)?;
            }
            native_nurbs_curve(bytes, &native_loft_curve(target, path)?)?;
            for value in tail {
                native_i64(bytes, *value);
            }
        }
        SkinSurfaceLayout::Compact {
            curve,
            subdata,
            first_tail,
            secondary_curve,
            second_tail,
        } => {
            native_nurbs_curve(bytes, &native_loft_curve(target, curve)?)?;
            native_loft_subdata(bytes, subdata)?;
            native_i64(bytes, *first_tail);
            native_nurbs_curve(bytes, &native_loft_curve(target, secondary_curve)?)?;
            native_i64(bytes, *second_tail);
        }
    }
    native_vector(
        bytes,
        [
            construction.direction.x,
            construction.direction.y,
            construction.direction.z,
        ],
    );
    native_f64(bytes, construction.trailing_parameter);
    native_law_formula(bytes, target, &construction.formula)?;
    native_nurbs_curve(
        bytes,
        &native_loft_curve(target, &construction.parameter_curve)?,
    )?;
    native_nurbs_surface(bytes, solved_cache)?;
    native_f64(bytes, cache_fit_tolerance / 10.0);
    for values in &construction.discontinuities {
        native_compound_loft_float_array(bytes, values)?;
    }
    bytes.push(native_bool(construction.discontinuity_flag));
    bytes.push(0x10);
    Ok(())
}

fn encode_native_net_surface(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::NetSurfaceConstruction,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    let cache_fit_tolerance = procedural.cache_fit_tolerance.ok_or_else(|| {
        CodecError::Malformed("net surface requires a native cache-fit tolerance".into())
    })?;
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "net_spl_sur")?;
    for section in construction.sections.iter() {
        native_loft_section(bytes, target, section, None)?;
    }
    for parameter in construction.frame_parameters {
        native_f64(bytes, parameter);
    }
    native_i64(bytes, construction.flag);
    for direction in construction.directions {
        native_vector(bytes, [direction.x, direction.y, direction.z]);
    }
    for formula in construction.formulas.iter() {
        native_law_formula(bytes, target, formula)?;
    }
    native_nurbs_surface(bytes, solved_cache)?;
    native_f64(bytes, cache_fit_tolerance / 10.0);
    for values in &construction.discontinuities {
        native_compound_loft_float_array(bytes, values)?;
    }
    bytes.push(native_bool(construction.discontinuity_flag));
    bytes.push(0x10);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn encode_native_sweep_surface(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    profile: &cadmpeg_ir::ids::CurveId,
    spine: &cadmpeg_ir::ids::CurveId,
    construction: &cadmpeg_ir::geometry::SweepSurfaceConstruction,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::SweepSurfaceLayout;
    let cache_fit_tolerance = procedural.cache_fit_tolerance.ok_or_else(|| {
        CodecError::Malformed("sweep surface requires a native cache-fit tolerance".into())
    })?;
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "sweep_spl_sur")?;
    native_enum(bytes, construction.primary_kind);
    match &construction.layout {
        SweepSurfaceLayout::ProfileFirst {
            secondary_kind,
            directions,
            origin,
            parameters,
            formulas,
        } => {
            native_nurbs_curve(bytes, &native_loft_curve(target, profile)?)?;
            native_nurbs_curve(bytes, &native_loft_curve(target, spine)?)?;
            native_enum(bytes, *secondary_kind);
            for direction in directions {
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_point(bytes, [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0]);
            for parameter in parameters {
                native_f64(bytes, *parameter);
            }
            for formula in formulas.iter() {
                native_law_formula(bytes, target, formula)?;
            }
        }
        SweepSurfaceLayout::ExplicitFormula {
            mode,
            profile_range,
            profile_frame,
            origin,
            directions,
            trajectory_flag,
            path_range,
            path_parameter,
            formula_flag,
            formula,
            trailing_flag,
        } => {
            native_i64(bytes, *mode);
            let profile = native_loft_curve_in_range(target, profile, Some(*profile_range))?;
            native_nurbs_curve(bytes, &profile)?;
            for value in profile_range {
                native_f64(bytes, *value);
            }
            bytes.push(native_bool(profile_frame.is_some()));
            if let Some((point, direction)) = profile_frame {
                native_point(bytes, [point.x / 10.0, point.y / 10.0, point.z / 10.0]);
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_point(bytes, [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0]);
            for direction in directions {
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_i64(bytes, 1);
            bytes.push(native_bool(*trajectory_flag));
            let native_path_range = [path_range[0] / 10.0, path_range[1] / 10.0];
            let spine = native_loft_curve_in_range(target, spine, Some(native_path_range))?;
            native_nurbs_curve(bytes, &spine)?;
            for value in path_range {
                native_f64(bytes, *value / 10.0);
            }
            native_f64(bytes, *path_parameter);
            bytes.push(native_bool(*formula_flag));
            native_law_formula(bytes, target, formula)?;
            bytes.push(native_bool(*trailing_flag));
        }
        SweepSurfaceLayout::ExplicitGuide {
            mode,
            profile_range,
            profile_frame,
            origin,
            directions,
            trajectory_flag,
            path_range,
            path_parameter,
            guide_flags,
            guide_curve,
            guide_range,
            guide_modes,
            guide_parameters,
            trailing_flags,
        } => {
            native_i64(bytes, *mode);
            let profile = native_loft_curve_in_range(target, profile, Some(*profile_range))?;
            native_nurbs_curve(bytes, &profile)?;
            for value in profile_range {
                native_f64(bytes, *value);
            }
            bytes.push(native_bool(profile_frame.is_some()));
            if let Some((point, direction)) = profile_frame {
                native_point(bytes, [point.x / 10.0, point.y / 10.0, point.z / 10.0]);
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_point(bytes, [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0]);
            for direction in directions {
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_i64(bytes, 2);
            bytes.push(native_bool(*trajectory_flag));
            let native_path_range = [path_range[0] / 10.0, path_range[1] / 10.0];
            let spine = native_loft_curve_in_range(target, spine, Some(native_path_range))?;
            native_nurbs_curve(bytes, &spine)?;
            for value in path_range {
                native_f64(bytes, *value / 10.0);
            }
            native_f64(bytes, *path_parameter);
            for flag in guide_flags {
                bytes.push(native_bool(*flag));
            }
            let guide_curve = native_loft_curve_in_range(target, guide_curve, Some(*guide_range))?;
            native_nurbs_curve(bytes, &guide_curve)?;
            for value in guide_range {
                native_f64(bytes, *value);
            }
            for mode in guide_modes {
                native_i64(bytes, *mode);
            }
            for parameter in guide_parameters {
                native_f64(bytes, *parameter);
            }
            for flag in trailing_flags {
                bytes.push(native_bool(*flag));
            }
        }
        SweepSurfaceLayout::ExplicitSurface {
            mode,
            profile_range,
            profile_frame,
            origin,
            directions,
            trajectory_flag,
            path_range,
            path_parameter,
            singularity,
            support_surface,
            auxiliary_curve,
            support_flag,
            legacy_flag,
        } => {
            native_i64(bytes, *mode);
            let profile = native_loft_curve_in_range(target, profile, Some(*profile_range))?;
            native_nurbs_curve(bytes, &profile)?;
            for value in profile_range {
                native_f64(bytes, *value);
            }
            bytes.push(native_bool(profile_frame.is_some()));
            if let Some((point, direction)) = profile_frame {
                native_point(bytes, [point.x / 10.0, point.y / 10.0, point.z / 10.0]);
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_point(bytes, [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0]);
            for direction in directions {
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_i64(bytes, 3);
            bytes.push(native_bool(*trajectory_flag));
            let native_path_range = [path_range[0] / 10.0, path_range[1] / 10.0];
            let spine = native_loft_curve_in_range(target, spine, Some(native_path_range))?;
            native_nurbs_curve(bytes, &spine)?;
            for value in path_range {
                native_f64(bytes, *value / 10.0);
            }
            native_f64(bytes, *path_parameter);
            native_enum(bytes, *singularity);
            let support = target
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == *support_surface)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "sweep references missing support surface {support_surface}"
                    ))
                })?;
            native_embedded_surface(bytes, &support.geometry)?;
            bytes.push(native_bool(auxiliary_curve.is_some()));
            if let Some(curve) = auxiliary_curve {
                native_nurbs_curve(bytes, &native_loft_curve(target, curve)?)?;
            }
            bytes.push(native_bool(*support_flag));
            if let Some(flag) = legacy_flag {
                bytes.push(native_bool(*flag));
            }
        }
        SweepSurfaceLayout::LawDriven {
            mode,
            profile_range,
            profile_frame,
            origin,
            directions,
            first_law,
            first_mode,
            first_range,
            law_direction,
            path_mode,
            path_flag,
            path_range,
            path_parameter,
            second_law_flag,
            second_law,
            formula_mode,
            formula,
            trailing_flag,
        } => {
            native_i64(bytes, *mode);
            let profile = native_loft_curve_in_range(target, profile, Some(*profile_range))?;
            native_nurbs_curve(bytes, &profile)?;
            for value in profile_range {
                native_f64(bytes, *value);
            }
            bytes.push(native_bool(profile_frame.is_some()));
            if let Some((point, direction)) = profile_frame {
                native_point(bytes, [point.x / 10.0, point.y / 10.0, point.z / 10.0]);
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_point(bytes, [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0]);
            for direction in directions {
                native_vector(bytes, [direction.x, direction.y, direction.z]);
            }
            native_law_expression(bytes, target, first_law, 0)?;
            native_i64(bytes, *first_mode);
            for value in first_range {
                native_f64(bytes, *value);
            }
            native_vector(bytes, [law_direction.x, law_direction.y, law_direction.z]);
            native_i64(bytes, *path_mode);
            bytes.push(native_bool(*path_flag));
            let spine = native_loft_curve_in_range(target, spine, Some(*path_range))?;
            native_nurbs_curve(bytes, &spine)?;
            for value in path_range {
                native_f64(bytes, *value);
            }
            native_f64(bytes, *path_parameter);
            bytes.push(native_bool(*second_law_flag));
            native_law_expression(bytes, target, second_law, 0)?;
            native_i64(bytes, *formula_mode);
            native_law_formula(bytes, target, formula)?;
            bytes.push(native_bool(*trailing_flag));
        }
    }
    native_nurbs_surface(bytes, solved_cache)?;
    native_f64(bytes, cache_fit_tolerance / 10.0);
    for values in &construction.discontinuities {
        native_compound_loft_float_array(bytes, values)?;
    }
    bytes.push(native_bool(construction.discontinuity_flag));
    bytes.push(0x10);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn encode_native_loft(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    sections: &[cadmpeg_ir::geometry::LoftSection; 2],
    parameter_ranges: &[[f64; 2]; 2],
    closures: &[i64; 2],
    singularities: &[i64; 2],
    mode: i64,
    bridge: &[cadmpeg_ir::geometry::LoftBridgeToken],
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "loft_spl_sur")?;
    for (section, range) in sections.iter().zip(parameter_ranges) {
        native_loft_section(bytes, target, section, Some(*range))?;
    }
    for range in parameter_ranges {
        native_f64(bytes, range[0]);
        native_f64(bytes, range[1]);
    }
    for closure in closures {
        native_enum(bytes, *closure);
    }
    for singularity in singularities {
        native_enum(bytes, *singularity);
    }
    native_i64(bytes, mode);
    for token in bridge {
        match token {
            cadmpeg_ir::geometry::LoftBridgeToken::Boolean(value) => {
                bytes.push(native_bool(*value));
            }
            cadmpeg_ir::geometry::LoftBridgeToken::Integer(value) => native_i64(bytes, *value),
            cadmpeg_ir::geometry::LoftBridgeToken::Double(value) => native_f64(bytes, *value),
            cadmpeg_ir::geometry::LoftBridgeToken::Text(value) => native_string(bytes, value)?,
            cadmpeg_ir::geometry::LoftBridgeToken::Enum(value) => native_enum(bytes, *value),
        }
    }
    native_nurbs_surface(bytes, solved_cache)?;
    if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
        native_f64(bytes, cache_fit_tolerance / 10.0);
    }
    bytes.push(0x10);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn encode_native_extrusion(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    directrix: &cadmpeg_ir::ids::CurveId,
    parameter_interval: [f64; 2],
    direction: Vector3,
    native_position: cadmpeg_ir::math::Point3,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    let directrix = target
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *directrix)
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "procedural surface {} references missing directrix {directrix}",
                procedural.id
            ))
        })?;
    let directrix_cache = native_interval_curve(&directrix.geometry, parameter_interval)?;
    if [
        parameter_interval[0],
        parameter_interval[1],
        direction.x,
        direction.y,
        direction.z,
        native_position.x,
        native_position.y,
        native_position.z,
    ]
    .into_iter()
    .any(|component| !component.is_finite())
    {
        return Err(CodecError::Malformed(
            "source-less extrusion fields must be finite".into(),
        ));
    }
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "cyl_spl_sur")?;
    native_f64(bytes, parameter_interval[0]);
    native_f64(bytes, parameter_interval[1]);
    native_vector(
        bytes,
        [direction.x / 10.0, direction.y / 10.0, direction.z / 10.0],
    );
    native_point(
        bytes,
        [
            native_position.x / 10.0,
            native_position.y / 10.0,
            native_position.z / 10.0,
        ],
    );
    native_nurbs_curve(bytes, &directrix_cache)?;
    native_nurbs_surface(bytes, solved_cache)?;
    if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
        native_f64(bytes, cache_fit_tolerance / 10.0);
    }
    bytes.push(0x10);
    Ok(())
}

fn native_optional_pcurve(
    bytes: &mut Vec<u8>,
    pcurve: Option<&PcurveGeometry>,
) -> Result<(), CodecError> {
    if let Some(pcurve) = pcurve {
        native_nurbs_pcurve_block(bytes, pcurve)
    } else {
        native_ident(bytes, "nullbs")
    }
}

fn native_variable_blend_value(
    bytes: &mut Vec<u8>,
    value: &cadmpeg_ir::geometry::VariableBlendValue,
    depth: usize,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::{LoftBridgeToken, VariableBlendValuePayload};
    if depth > 32 {
        return Err(CodecError::Malformed(
            "variable blend-value recursion exceeds 32 levels".into(),
        ));
    }
    native_string(bytes, &value.name)?;
    bytes.push(native_bool(value.modern_flag));
    if value.discriminator != 1 {
        native_i64(bytes, value.discriminator);
    }
    native_enum(bytes, value.calibrated);
    match &value.payload {
        VariableBlendValuePayload::TwoEnds { parameters, radii } => {
            for parameter in parameters {
                native_f64(bytes, *parameter);
            }
            for radius in radii {
                native_f64(bytes, *radius / 10.0);
            }
        }
        VariableBlendValuePayload::EdgeOffset { scalars, lengths } => {
            let expected = if value.discriminator == 0 {
                (2, 1)
            } else {
                (1, 2)
            };
            if (scalars.len(), lengths.len()) != expected {
                return Err(CodecError::Malformed(
                    "variable edge-offset payload has inconsistent arity".into(),
                ));
            }
            for scalar in scalars {
                native_f64(bytes, *scalar);
            }
            for length in lengths {
                native_f64(bytes, *length / 10.0);
            }
        }
        VariableBlendValuePayload::Functional {
            parameter,
            radius,
            function,
            terminal,
        } => {
            native_f64(bytes, *parameter);
            native_f64(bytes, *radius / 10.0);
            native_nurbs_pcurve_block(bytes, function)?;
            match terminal {
                LoftBridgeToken::Double(value) => native_f64(bytes, *value),
                LoftBridgeToken::Text(value) => native_string(bytes, value)?,
                _ => {
                    return Err(CodecError::NotImplemented(
                        "functional variable-blend terminal must be double or text".into(),
                    ));
                }
            }
        }
        VariableBlendValuePayload::Constant {
            parameters,
            radius,
            variable_chamfer,
            chamfer_type,
            nested,
        } => {
            for parameter in parameters {
                native_f64(bytes, *parameter);
            }
            native_f64(bytes, *radius / 10.0);
            native_enum(bytes, *variable_chamfer);
            native_enum(bytes, *chamfer_type);
            native_variable_blend_value(bytes, nested, depth + 1)?;
        }
        VariableBlendValuePayload::Interpolated {
            parameter,
            radius,
            function,
            enum_count,
            points,
            tail,
        } => {
            native_f64(bytes, *parameter);
            native_f64(bytes, *radius / 10.0);
            native_nurbs_pcurve_block(bytes, function)?;
            native_i64(bytes, *enum_count);
            native_i64(
                bytes,
                i64::try_from(points.len()).map_err(|_| {
                    CodecError::NotImplemented("variable blend point count exceeds i64".into())
                })?,
            );
            for point in points {
                native_f64(bytes, point.parameter);
                native_f64(bytes, point.radius / 10.0);
                for tangent in point.tangents {
                    native_f64(bytes, tangent);
                }
                native_point(
                    bytes,
                    [
                        point.location.x / 10.0,
                        point.location.y / 10.0,
                        point.location.z / 10.0,
                    ],
                );
                native_vector(bytes, [point.normal.x, point.normal.y, point.normal.z]);
            }
            native_i64(bytes, i64::from(tail.is_some()));
            if let Some(tail) = tail {
                native_f64(bytes, tail[0]);
                native_f64(bytes, tail[1]);
            }
        }
    }
    Ok(())
}

fn native_vertex_blend_bool(bytes: &mut Vec<u8>, value: i64) -> Result<(), CodecError> {
    match value {
        0 => bytes.push(native_bool(false)),
        1 => bytes.push(native_bool(true)),
        _ => {
            return Err(CodecError::Malformed(
                "vertex-blend boolean enum must be 0 or 1".into(),
            ));
        }
    }
    Ok(())
}

fn native_vertex_blend_boundary(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    boundary: &cadmpeg_ir::geometry::VertexBlendBoundary,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::VertexBlendBoundaryGeometry;
    let kind = match &boundary.geometry {
        VertexBlendBoundaryGeometry::Circle { .. } => "circle",
        VertexBlendBoundaryGeometry::Degenerate { .. } => "deg",
        VertexBlendBoundaryGeometry::Pcurve { .. } => "pcurve",
        VertexBlendBoundaryGeometry::Plane { .. } => "plane",
    };
    native_string(bytes, kind)?;
    native_vertex_blend_bool(bytes, boundary.boundary_type)?;
    native_point(
        bytes,
        [
            boundary.magic.x / 10.0,
            boundary.magic.y / 10.0,
            boundary.magic.z / 10.0,
        ],
    );
    native_vertex_blend_bool(bytes, boundary.u_smoothing)?;
    native_vertex_blend_bool(bytes, boundary.v_smoothing)?;
    native_f64(bytes, boundary.fullness);
    match &boundary.geometry {
        VertexBlendBoundaryGeometry::Circle {
            curve,
            form,
            twists,
            parameters,
            sense,
        } => {
            let expected_twists = match form {
                0 => 0,
                1 => 1,
                3 => 2,
                _ => {
                    return Err(CodecError::Malformed(
                        "vertex-blend circle form must be 0, 1, or 3".into(),
                    ));
                }
            };
            if twists.len() != expected_twists {
                return Err(CodecError::Malformed(
                    "vertex-blend circle twist count conflicts with its form".into(),
                ));
            }
            let curve = native_loft_curve_in_range(target, curve, Some(*parameters))?;
            native_nurbs_curve(bytes, &curve)?;
            native_enum(bytes, *form);
            for twist in twists {
                native_point(bytes, [twist.x / 10.0, twist.y / 10.0, twist.z / 10.0]);
            }
            native_f64(bytes, parameters[0]);
            native_f64(bytes, parameters[1]);
            native_vertex_blend_bool(bytes, *sense)?;
        }
        VertexBlendBoundaryGeometry::Degenerate { location, normals } => {
            native_point(
                bytes,
                [location.x / 10.0, location.y / 10.0, location.z / 10.0],
            );
            for normal in normals {
                native_vector(bytes, [normal.x, normal.y, normal.z]);
            }
        }
        VertexBlendBoundaryGeometry::Pcurve {
            surface,
            pcurve,
            sense,
            fit_tolerance,
        } => {
            let surface = target
                .model
                .surfaces
                .iter()
                .find(|candidate| candidate.id == *surface)
                .ok_or_else(|| {
                    CodecError::Malformed(format!("vertex-blend support {surface} is missing"))
                })?;
            native_embedded_surface(bytes, &surface.geometry)?;
            native_optional_pcurve(bytes, pcurve.as_ref())?;
            native_vertex_blend_bool(bytes, *sense)?;
            native_f64(bytes, *fit_tolerance);
        }
        VertexBlendBoundaryGeometry::Plane {
            normal,
            parameters,
            curve,
        } => {
            native_vector(bytes, [normal.x, normal.y, normal.z]);
            native_f64(bytes, parameters[0]);
            native_f64(bytes, parameters[1]);
            let curve = native_loft_curve_in_range(target, curve, Some(*parameters))?;
            native_nurbs_curve(bytes, &curve)?;
        }
    }
    Ok(())
}

fn encode_native_vertex_blend(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    construction: &cadmpeg_ir::geometry::VertexBlendConstruction,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "VBL_SURF")?;
    native_i64(
        bytes,
        i64::try_from(construction.boundaries.len()).map_err(|_| {
            CodecError::NotImplemented("vertex-blend boundary count exceeds i64".into())
        })?,
    );
    for boundary in &construction.boundaries {
        native_vertex_blend_boundary(bytes, target, boundary)?;
    }
    native_i64(bytes, construction.grid_size);
    native_f64(bytes, construction.fit_tolerance / 10.0);
    native_nurbs_surface(bytes, solved_cache)?;
    native_f64(bytes, 0.0);
    bytes.push(0x10);
    Ok(())
}

fn native_variable_blend_side(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    side: &cadmpeg_ir::geometry::VariableBlendSide,
) -> Result<(), CodecError> {
    native_string(bytes, &side.label)?;
    let surface = target
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == side.surface)
        .ok_or_else(|| {
            CodecError::Malformed(format!("variable support {} is missing", side.surface))
        })?;
    native_embedded_surface(bytes, &surface.geometry)?;
    let curve = target
        .model
        .curves
        .iter()
        .find(|curve| curve.id == side.curve)
        .ok_or_else(|| {
            CodecError::Malformed(format!("variable side curve {} is missing", side.curve))
        })?;
    let curve = native_spline_field_curve(
        &curve.geometry,
        native_pcurve_knot_domain(side.pcurve.as_ref())?,
    )?;
    native_nurbs_curve(bytes, &curve)?;
    native_optional_pcurve(bytes, side.pcurve.as_ref())?;
    native_point(
        bytes,
        [
            side.location.x / 10.0,
            side.location.y / 10.0,
            side.location.z / 10.0,
        ],
    );
    native_optional_pcurve(bytes, side.secondary_pcurve.as_ref())?;
    native_f64(bytes, side.scalar);
    native_optional_pcurve(bytes, side.tertiary_pcurve.as_ref())?;
    Ok(())
}

fn encode_native_variable_blend(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::VariableBlendConstruction,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    use cadmpeg_ir::geometry::LoftBridgeToken;
    let cache_fit_tolerance = procedural.cache_fit_tolerance.ok_or_else(|| {
        CodecError::Malformed("variable blend requires a native cache-fit tolerance".into())
    })?;
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "var_blend_spl_sur")?;
    for side in construction.sides.iter() {
        native_variable_blend_side(bytes, target, side)?;
    }
    let primary_curve = native_loft_curve_in_range(
        target,
        &construction.primary_curve,
        Some(construction.u_range),
    )?;
    native_nurbs_curve(bytes, &primary_curve)?;
    for offset in construction.offsets {
        native_f64(bytes, offset / 10.0);
    }
    native_enum(bytes, construction.radius_kind);
    native_variable_blend_value(bytes, &construction.first_value, 0)?;
    if construction.radius_kind == 1 {
        let second = construction.second_value.as_ref().ok_or_else(|| {
            CodecError::Malformed("two-radii variable blend lacks its second value".into())
        })?;
        native_variable_blend_value(bytes, second, 0)?;
        if let Some(chamfer) = &construction.chamfer {
            native_enum(bytes, chamfer.variable_chamfer);
            native_enum(bytes, chamfer.chamfer_type);
            native_variable_blend_value(bytes, &chamfer.value, 0)?;
        }
    } else if construction.radius_kind == 0 {
        if construction.second_value.is_some() || construction.chamfer.is_some() {
            return Err(CodecError::Malformed(
                "single-radius variable blend carries two-radii payloads".into(),
            ));
        }
        if let Some(tail) = &construction.single_radius_tail {
            match &tail.selector {
                LoftBridgeToken::Integer(value) => native_i64(bytes, *value),
                _ => {
                    return Err(CodecError::NotImplemented(
                        "variable single-radius selector must be an integer".into(),
                    ));
                }
            }
            for parameter in tail.parameters {
                native_f64(bytes, parameter);
            }
        }
    } else {
        return Err(CodecError::Malformed(
            "variable blend radius kind must be 0 or 1".into(),
        ));
    }
    for range in [construction.u_range, construction.v_range] {
        native_f64(bytes, range[0]);
        native_f64(bytes, range[1]);
    }
    native_i64(bytes, construction.shape_prefix);
    native_f64(bytes, construction.shape_parameter);
    native_f64(bytes, construction.shape_length / 10.0);
    native_i64(bytes, construction.shape_tail);
    native_nurbs_surface(bytes, solved_cache)?;
    native_f64(bytes, cache_fit_tolerance / 10.0);
    for extension in construction.shape_extensions {
        native_i64(bytes, extension);
    }
    native_nurbs_curve(
        bytes,
        &native_loft_curve(target, &construction.secondary_curve)?,
    )?;
    bytes.push(native_bool(construction.convexity != 0));
    bytes.push(native_bool(construction.render_blend != 0));
    for value in construction.post_range {
        native_f64(bytes, value);
    }
    let post_curve = native_loft_curve_in_range(
        target,
        &construction.post_curve,
        Some(construction.post_range),
    )?;
    native_nurbs_curve(bytes, &post_curve)?;
    native_optional_pcurve(bytes, construction.post_pcurve.as_ref())?;
    bytes.push(0x10);
    Ok(())
}

fn native_rolling_ball_side(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    side: &cadmpeg_ir::geometry::RollingBallSide,
) -> Result<(), CodecError> {
    native_string(bytes, &side.label)?;
    if let Some(id) = &side.surface {
        let surface = target
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *id)
            .ok_or_else(|| {
                CodecError::Malformed(format!("rolling-ball support {id} is missing"))
            })?;
        native_embedded_surface(bytes, &surface.geometry)?;
    } else {
        native_ident(bytes, "null_surface")?;
    }
    let curve = target
        .model
        .curves
        .iter()
        .find(|curve| curve.id == side.curve)
        .ok_or_else(|| {
            CodecError::Malformed(format!("rolling-ball side curve {} is missing", side.curve))
        })?;
    let curve = native_spline_field_curve(
        &curve.geometry,
        native_pcurve_knot_domain(side.pcurve.as_ref())?,
    )?;
    native_nurbs_curve(bytes, &curve)?;
    native_optional_pcurve(bytes, side.pcurve.as_ref())?;
    native_point(
        bytes,
        [
            side.location.x / 10.0,
            side.location.y / 10.0,
            side.location.z / 10.0,
        ],
    );
    native_optional_pcurve(bytes, side.secondary_pcurve.as_ref())?;
    if let Some(id) = &side.exact_support {
        let surface = target
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *id)
            .ok_or_else(|| {
                CodecError::Malformed(format!("rolling-ball exact support {id} is missing"))
            })?;
        let SurfaceGeometry::Nurbs(surface) = &surface.geometry else {
            return Err(CodecError::NotImplemented(
                "rolling-ball exact support must be NURBS".into(),
            ));
        };
        native_ident(bytes, "spline")?;
        native_nurbs_surface(bytes, surface)?;
    } else {
        native_ident(bytes, "nullbs")?;
    }
    Ok(())
}

fn native_rolling_ball_third_side(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    side: &cadmpeg_ir::geometry::RollingBallThirdSide,
) -> Result<(), CodecError> {
    native_string(bytes, &side.label)?;
    let surface = target
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == side.surface)
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "rolling-ball third support {} is missing",
                side.surface
            ))
        })?;
    native_embedded_surface(bytes, &surface.geometry)?;
    let curve = target
        .model
        .curves
        .iter()
        .find(|curve| curve.id == side.curve)
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "rolling-ball third-side curve {} is missing",
                side.curve
            ))
        })?;
    let pcurve = [
        side.pcurve.as_ref(),
        side.secondary_pcurve.as_ref(),
        side.tertiary_pcurve.as_ref(),
    ]
    .into_iter()
    .flatten()
    .find(|pcurve| matches!(pcurve, PcurveGeometry::Nurbs { .. }));
    let curve = native_spline_field_curve(&curve.geometry, native_pcurve_knot_domain(pcurve)?)?;
    native_nurbs_curve(bytes, &curve)?;
    native_optional_pcurve(bytes, side.pcurve.as_ref())?;
    native_vector(
        bytes,
        [side.direction.x, side.direction.y, side.direction.z],
    );
    native_optional_pcurve(bytes, side.secondary_pcurve.as_ref())?;
    native_i64(bytes, side.extension);
    native_optional_pcurve(bytes, side.tertiary_pcurve.as_ref())?;
    bytes.push(native_bool(side.flag));
    Ok(())
}

fn encode_complete_native_rolling_ball(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    construction: &cadmpeg_ir::geometry::RollingBallConstruction,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    let cache_fit_tolerance = procedural.cache_fit_tolerance.ok_or_else(|| {
        CodecError::Malformed("rolling-ball blend requires a native cache-fit tolerance".into())
    })?;
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(
        bytes,
        if construction.third.is_some() {
            "sss_blend_spl_sur"
        } else {
            "rb_blend_spl_sur"
        },
    )?;
    for side in construction.sides.iter() {
        native_rolling_ball_side(bytes, target, side)?;
    }
    let slice =
        native_loft_curve_in_range(target, &construction.slice, Some(construction.u_range))?;
    native_nurbs_curve(bytes, &slice)?;
    for offset in construction.offsets {
        native_f64(bytes, offset / 10.0);
    }
    match construction.radius_selector {
        cadmpeg_ir::geometry::RollingBallRadiusSelector::None => native_enum(bytes, -1),
        cadmpeg_ir::geometry::RollingBallRadiusSelector::Value { value } => {
            native_f64(bytes, value);
        }
    }
    for range in [construction.u_range, construction.v_range] {
        native_f64(bytes, range[0]);
        native_f64(bytes, range[1]);
    }
    for parameter in construction.parameters {
        native_f64(bytes, parameter);
    }
    native_i64(bytes, construction.tail);
    native_nurbs_surface(bytes, solved_cache)?;
    native_f64(bytes, cache_fit_tolerance / 10.0);
    for values in &construction.discontinuities {
        native_i64(
            bytes,
            i64::try_from(values.len()).map_err(|_| {
                CodecError::NotImplemented("rolling-ball discontinuity count exceeds i64".into())
            })?,
        );
        for value in values {
            native_f64(bytes, *value);
        }
    }
    if let Some(third) = &construction.third {
        native_rolling_ball_third_side(bytes, target, third)?;
    }
    bytes.push(0x10);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn encode_native_rolling_ball(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    procedural: &cadmpeg_ir::geometry::ProceduralSurface,
    supports: &[Option<cadmpeg_ir::geometry::BlendSupport>; 2],
    spine: Option<&cadmpeg_ir::ids::CurveId>,
    radius: &BlendRadiusLaw,
    cross_section: &cadmpeg_ir::geometry::BlendCrossSection,
    solved_cache: &NurbsSurface,
) -> Result<(), CodecError> {
    if *cross_section != cadmpeg_ir::geometry::BlendCrossSection::Circular {
        return Err(CodecError::NotImplemented(
            "source-less rb_blend_spl_sur requires a circular cross-section".into(),
        ));
    }
    native_surface_base(bytes, "spline")?;
    bytes.push(0x0f);
    native_ident(bytes, "rb_blend_spl_sur")?;
    for (side, support) in supports.iter().enumerate() {
        let Some(support) = support else { continue };
        if support.reversed {
            return Err(CodecError::NotImplemented(
                "source-less rb_blend_spl_sur reversed support is not defined".into(),
            ));
        }
        let carrier = target
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == support.surface)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "procedural surface {} references missing support {}",
                    procedural.id, support.surface
                ))
            })?;
        native_string(bytes, "blend_support_surface")?;
        native_subident(bytes, if side == 0 { "plane" } else { "sphere" })?;
        native_embedded_surface(bytes, &carrier.geometry)?;
    }
    let spine = spine.ok_or_else(|| {
        CodecError::Malformed("source-less rb_blend_spl_sur lacks a spine".into())
    })?;
    let spine = target
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *spine)
        .ok_or_else(|| CodecError::Malformed(format!("blend references missing spine {spine}")))?;
    let spine_range = [
        solved_cache.u_knots.first().copied().ok_or_else(|| {
            CodecError::Malformed("rolling-ball solved surface has no U knot domain".into())
        })?,
        solved_cache.u_knots.last().copied().ok_or_else(|| {
            CodecError::Malformed("rolling-ball solved surface has no U knot domain".into())
        })?,
    ];
    let spine = native_interval_curve(&spine.geometry, spine_range)?;
    native_nurbs_curve(bytes, &spine)?;
    let (start, end) = match radius {
        BlendRadiusLaw::Constant { signed_radius } => (*signed_radius, *signed_radius),
        BlendRadiusLaw::Linear { start, end } => (*start, *end),
        BlendRadiusLaw::Law { .. } => {
            return Err(CodecError::NotImplemented(
                "source-less rb_blend_spl_sur explicit radius law is not defined".into(),
            ))
        }
    };
    native_f64(bytes, start / 10.0);
    native_f64(bytes, end / 10.0);
    native_enum(bytes, -1);
    native_nurbs_surface(bytes, solved_cache)?;
    if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
        native_f64(bytes, cache_fit_tolerance / 10.0);
    }
    bytes.push(0x10);
    Ok(())
}

fn native_nurbs_curve(bytes: &mut Vec<u8>, curve: &NurbsCurve) -> Result<(), CodecError> {
    let degree = usize::try_from(curve.degree)
        .map_err(|_| CodecError::NotImplemented("F3D NURBS curve degree exceeds usize".into()))?;
    if curve.knots.len() != curve.control_points.len() + degree + 1
        || curve
            .weights
            .as_ref()
            .is_some_and(|weights| weights.len() != curve.control_points.len())
    {
        return Err(CodecError::Malformed(
            "source-less F3D NURBS curve has inconsistent cardinality".into(),
        ));
    }
    native_ident(
        bytes,
        if curve.weights.is_some() {
            "nurbs"
        } else {
            "nubs"
        },
    )?;
    native_i64(bytes, i64::from(curve.degree));
    native_enum(bytes, if curve.periodic { 2 } else { 0 });
    native_i64(
        bytes,
        i64::try_from(unique_knot_count(&curve.knots))
            .map_err(|_| CodecError::NotImplemented("F3D unique-knot count exceeds i64".into()))?,
    );
    native_nurbs_knots(bytes, &curve.knots)?;
    for (index, point) in curve.control_points.iter().enumerate() {
        native_f64(bytes, point.x / 10.0);
        native_f64(bytes, point.y / 10.0);
        native_f64(bytes, point.z / 10.0);
        if let Some(weights) = curve.weights.as_ref() {
            native_f64(bytes, weights[index]);
        }
    }
    Ok(())
}

fn native_spline_field_curve(
    geometry: &CurveGeometry,
    parameter_range: Option<[f64; 2]>,
) -> Result<NurbsCurve, CodecError> {
    match (geometry, parameter_range) {
        (CurveGeometry::Nurbs(curve), _) => Ok(curve.clone()),
        (_, Some(range)) => native_interval_curve(geometry, range),
        (CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. }, None) => {
            native_interval_curve(geometry, [0.0, std::f64::consts::TAU])
        }
        _ => Err(CodecError::NotImplemented(
            "source-less F3D spline field lacks a finite curve domain".into(),
        )),
    }
}

fn native_pcurve_knot_domain(
    pcurve: Option<&PcurveGeometry>,
) -> Result<Option<[f64; 2]>, CodecError> {
    let Some(PcurveGeometry::Nurbs { knots, .. }) = pcurve else {
        return Ok(None);
    };
    Ok(Some([
        *knots
            .first()
            .ok_or_else(|| CodecError::Malformed("pcurve has no knot domain".into()))?,
        *knots
            .last()
            .ok_or_else(|| CodecError::Malformed("pcurve has no knot domain".into()))?,
    ]))
}

fn native_interval_curve(
    geometry: &CurveGeometry,
    parameter_range: [f64; 2],
) -> Result<NurbsCurve, CodecError> {
    if !parameter_range.into_iter().all(f64::is_finite) || parameter_range[0] >= parameter_range[1]
    {
        return Err(CodecError::Malformed(
            "source-less F3D interval curve requires a finite ordered range".into(),
        ));
    }
    match geometry {
        CurveGeometry::Nurbs(curve) => Ok(curve.clone()),
        CurveGeometry::Line { origin, direction } => {
            if !finite_point(*origin) || !finite_vector(*direction) || direction.norm() == 0.0 {
                return Err(CodecError::Malformed(
                    "source-less F3D interval line requires finite nonzero geometry".into(),
                ));
            }
            let point = |parameter: f64| {
                Point3::new(
                    origin.x + parameter * direction.x,
                    origin.y + parameter * direction.y,
                    origin.z + parameter * direction.z,
                )
            };
            Ok(NurbsCurve {
                degree: 1,
                knots: vec![
                    parameter_range[0],
                    parameter_range[0],
                    parameter_range[1],
                    parameter_range[1],
                ],
                control_points: vec![point(parameter_range[0]), point(parameter_range[1])],
                weights: None,
                periodic: false,
            })
        }
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => native_conic_interval_curve(
            *center,
            *axis,
            *ref_direction,
            *radius,
            *radius,
            parameter_range,
        ),
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => native_conic_interval_curve(
            *center,
            *axis,
            *major_direction,
            *major_radius,
            *minor_radius,
            parameter_range,
        ),
        _ => Err(CodecError::NotImplemented(
            "source-less F3D interval construction requires a NURBS, line, circle, or ellipse source curve".into(),
        )),
    }
}

fn native_conic_interval_curve(
    center: Point3,
    axis: Vector3,
    major_direction: Vector3,
    major_radius: f64,
    minor_radius: f64,
    parameter_range: [f64; 2],
) -> Result<NurbsCurve, CodecError> {
    if !finite_point(center)
        || !finite_vector(axis)
        || !finite_vector(major_direction)
        || !major_radius.is_finite()
        || !minor_radius.is_finite()
        || axis.norm() == 0.0
        || major_direction.norm() == 0.0
        || major_radius <= 0.0
        || minor_radius <= 0.0
    {
        return Err(CodecError::Malformed(
            "source-less F3D conic interval requires finite nondegenerate geometry".into(),
        ));
    }
    let axis_norm = axis.norm();
    let axis = Vector3::new(axis.x / axis_norm, axis.y / axis_norm, axis.z / axis_norm);
    let major_norm = major_direction.norm();
    let major_direction = Vector3::new(
        major_direction.x / major_norm,
        major_direction.y / major_norm,
        major_direction.z / major_norm,
    );
    let minor_direction = Vector3::new(
        axis.y * major_direction.z - axis.z * major_direction.y,
        axis.z * major_direction.x - axis.x * major_direction.z,
        axis.x * major_direction.y - axis.y * major_direction.x,
    );
    let minor_norm = minor_direction.norm();
    if !minor_norm.is_finite() || minor_norm == 0.0 {
        return Err(CodecError::Malformed(
            "source-less F3D conic axis and major direction must not be parallel".into(),
        ));
    }
    let minor_direction = Vector3::new(
        minor_direction.x / minor_norm,
        minor_direction.y / minor_norm,
        minor_direction.z / minor_norm,
    );
    let delta = parameter_range[1] - parameter_range[0];
    let spans = (delta / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let step = delta / spans as f64;
    let mut control_points = Vec::with_capacity(spans * 2 + 1);
    let mut weights = Vec::with_capacity(spans * 2 + 1);
    let mut knots = Vec::with_capacity(spans * 2 + 4);
    let point = |angle: f64, scale: f64| {
        let major_scale = major_radius * angle.cos() * scale;
        let minor_scale = minor_radius * angle.sin() * scale;
        Point3::new(
            center.x + major_direction.x * major_scale + minor_direction.x * minor_scale,
            center.y + major_direction.y * major_scale + minor_direction.y * minor_scale,
            center.z + major_direction.z * major_scale + minor_direction.z * minor_scale,
        )
    };
    for span in 0..spans {
        let start = parameter_range[0] + step * span as f64;
        let end = start + step;
        let middle = (start + end) * 0.5;
        let weight = (step * 0.5).cos();
        if !weight.is_finite() || weight <= 0.0 {
            return Err(CodecError::Malformed(
                "source-less F3D conic interval has an invalid rational span".into(),
            ));
        }
        if span == 0 {
            control_points.push(point(start, 1.0));
            weights.push(1.0);
            knots.extend([start, start, start]);
        } else {
            knots.extend([start, start]);
        }
        control_points.push(point(middle, 1.0 / weight));
        weights.push(weight);
        control_points.push(point(end, 1.0));
        weights.push(1.0);
        if span + 1 == spans {
            knots.extend([end, end, end]);
        }
    }
    Ok(NurbsCurve {
        degree: 2,
        knots,
        control_points,
        weights: Some(weights),
        periodic: false,
    })
}

#[cfg(test)]
mod native_interval_curve_tests {
    use super::*;

    #[test]
    fn generated_circle_interval_lowers_to_exact_rational_nurbs() {
        let curve = native_interval_curve(
            &CurveGeometry::Circle {
                center: Point3::new(2.0, 3.0, 4.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 5.0,
            },
            [0.0, std::f64::consts::PI],
        )
        .expect("generated circle interval");
        let midpoint = cadmpeg_ir::eval::nurbs_curve_point(
            curve.degree,
            &curve.knots,
            &curve.control_points,
            curve.weights.as_deref(),
            std::f64::consts::FRAC_PI_2,
        )
        .expect("evaluate generated circle interval");
        assert!((midpoint.x - 2.0).abs() < 1.0e-12);
        assert!((midpoint.y - 8.0).abs() < 1.0e-12);
        assert!((midpoint.z - 4.0).abs() < 1.0e-12);
    }

    #[test]
    fn generated_ellipse_interval_preserves_both_radii() {
        let curve = native_interval_curve(
            &CurveGeometry::Ellipse {
                center: Point3::new(-1.0, 2.0, 0.5),
                axis: Vector3::new(0.0, 0.0, 1.0),
                major_direction: Vector3::new(1.0, 0.0, 0.0),
                major_radius: 6.0,
                minor_radius: 2.0,
            },
            [0.0, std::f64::consts::FRAC_PI_2],
        )
        .expect("generated ellipse interval");
        assert_eq!(curve.control_points[0], Point3::new(5.0, 2.0, 0.5));
        assert!((curve.control_points[2].x + 1.0).abs() < 1.0e-12);
        assert_eq!(curve.control_points[2].y, 4.0);
        assert_eq!(curve.control_points[2].z, 0.5);
        assert_eq!(
            curve.knots,
            vec![
                0.0,
                0.0,
                0.0,
                std::f64::consts::FRAC_PI_2,
                std::f64::consts::FRAC_PI_2,
                std::f64::consts::FRAC_PI_2,
            ]
        );
    }

    #[test]
    fn generated_domainless_circle_uses_its_full_natural_domain() {
        let geometry = CurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 3.0,
        };
        let curve = native_spline_field_curve(&geometry, None)
            .expect("generated domainless circle spline field");
        assert_eq!(curve.knots.first().copied(), Some(0.0));
        assert_eq!(curve.knots.last().copied(), Some(std::f64::consts::TAU));
        assert_eq!(curve.control_points.len(), 9);
        assert_eq!(curve.weights.as_ref().map(Vec::len), Some(9));
    }

    #[test]
    fn generated_domainless_line_remains_rejected() {
        let geometry = CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        };
        assert!(native_spline_field_curve(&geometry, None).is_err());
    }
}

fn native_procedural_curve(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    curve_id: &cadmpeg_ir::ids::CurveId,
    solved_cache: &NurbsCurve,
) -> Result<bool, CodecError> {
    let mut definitions = target
        .model
        .procedural_curves
        .iter()
        .filter(|procedural| procedural.curve == *curve_id);
    let Some(procedural) = definitions.next() else {
        return Ok(false);
    };
    if definitions.next().is_some() {
        return Err(CodecError::Malformed(format!(
            "curve {curve_id} has multiple procedural constructions"
        )));
    }
    if matches!(
        procedural.definition,
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Unknown { .. }
    ) {
        return Err(CodecError::NotImplemented(format!(
            "source-less F3D unknown procedural curve {} cannot be regenerated losslessly",
            procedural.id
        )));
    }
    let write_cache_fit_tolerance = |bytes: &mut Vec<u8>| {
        if let Some(cache_fit_tolerance) = procedural.cache_fit_tolerance {
            native_f64(bytes, cache_fit_tolerance / 10.0);
        }
    };
    if matches!(
        procedural.definition,
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Exact
    ) {
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "exact_int_cur")?;
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Law {
        context,
        extension,
        primary,
        additional,
    } = &procedural.definition
    {
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "law_int_cur")?;
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        native_intcurve_support_context(bytes, target, context)?;
        native_i64(bytes, *extension);
        native_law_formula(bytes, target, primary)?;
        native_i64(
            bytes,
            i64::try_from(additional.len())
                .map_err(|_| CodecError::NotImplemented("law formula count exceeds i64".into()))?,
        );
        for formula in additional {
            native_law_formula(bytes, target, formula)?;
        }
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Deformable {
        extension,
        bend,
        data,
    } = &procedural.definition
    {
        let bend = target
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *bend)
            .ok_or_else(|| CodecError::Malformed("deformable bend curve is missing".into()))?;
        let bend_range = [
            solved_cache.knots.first().copied().ok_or_else(|| {
                CodecError::Malformed("deformable solved curve has no knot domain".into())
            })?,
            solved_cache.knots.last().copied().ok_or_else(|| {
                CodecError::Malformed("deformable solved curve has no knot domain".into())
            })?,
        ];
        let bend = native_interval_curve(&bend.geometry, bend_range)?;
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "defm_int_cur")?;
        native_i64(bytes, *extension);
        native_nurbs_curve(bytes, &bend)?;
        match data {
            cadmpeg_ir::geometry::DeformableCurveData::VectorField {
                vectors,
                parameter_pairs,
            } => {
                native_i64(bytes, 8);
                for vector in vectors {
                    native_vector(bytes, [vector.x, vector.y, vector.z]);
                }
                native_i64(
                    bytes,
                    i64::try_from(parameter_pairs.len()).map_err(|_| {
                        CodecError::NotImplemented(
                            "deformable parameter-pair count exceeds i64".into(),
                        )
                    })?,
                );
                for pair in parameter_pairs {
                    native_f64(bytes, pair[0]);
                    native_f64(bytes, pair[1]);
                }
            }
            cadmpeg_ir::geometry::DeformableCurveData::Surface { surface } => {
                native_i64(bytes, 5);
                let surface = target
                    .model
                    .surfaces
                    .iter()
                    .find(|candidate| candidate.id == *surface)
                    .ok_or_else(|| {
                        CodecError::Malformed("deformable support surface is missing".into())
                    })?;
                native_embedded_surface(bytes, &surface.geometry)?;
            }
        }
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Projection {
        context,
        discontinuity_flag,
        source,
        tail,
    } = &procedural.definition
    {
        let source = target
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *source)
            .ok_or_else(|| CodecError::Malformed("projection source curve is missing".into()))?;
        let source = native_interval_curve(&source.geometry, context.parameter_range)?;
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "proj_int_cur")?;
        native_intcurve_support_context(bytes, target, context)?;
        bytes.push(native_bool(*discontinuity_flag));
        native_nurbs_curve(bytes, &source)?;
        match tail {
            cadmpeg_ir::geometry::ProjectionTail::EarlyClose { flag } => {
                bytes.push(native_bool(*flag));
                bytes.push(0x10);
                native_nurbs_curve(bytes, solved_cache)?;
                write_cache_fit_tolerance(bytes);
            }
            cadmpeg_ir::geometry::ProjectionTail::Ranged {
                flag,
                parameter_range,
                role,
            } => {
                bytes.push(native_bool(*flag));
                for value in parameter_range {
                    native_f64(bytes, *value);
                }
                native_string(bytes, role)?;
                native_nurbs_curve(bytes, solved_cache)?;
                write_cache_fit_tolerance(bytes);
                bytes.push(0x10);
            }
        }
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound {
        parameters,
        component_parameters,
        components,
    } = &procedural.definition
    {
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "comp_int_cur")?;
        native_i64(
            bytes,
            i64::try_from(parameters.len()).map_err(|_| {
                CodecError::NotImplemented("compound parameter count exceeds i64".into())
            })?,
        );
        for value in parameters {
            native_f64(bytes, *value);
        }
        native_i64(
            bytes,
            i64::try_from(components.len()).map_err(|_| {
                CodecError::NotImplemented("compound component count exceeds i64".into())
            })?,
        );
        for value in component_parameters {
            native_f64(bytes, *value);
        }
        bytes.push(0x0b);
        for (ordinal, component) in components.iter().enumerate() {
            let component = target
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *component)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "compound curve references missing component {component}"
                    ))
                })?;
            let parameter_range = if matches!(component.geometry, CurveGeometry::Nurbs(_)) {
                None
            } else {
                let range = parameters.get(ordinal..ordinal + 2).ok_or_else(|| {
                    CodecError::Malformed("compound component has no construction interval".into())
                })?;
                Some([range[0], range[1]])
            };
            let component = native_spline_field_curve(&component.geometry, parameter_range)?;
            native_nurbs_curve(bytes, &component)?;
        }
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection {
        context,
        discontinuity_flag,
    } = &procedural.definition
    {
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "int_int_cur")?;
        native_intcurve_support_context(bytes, target, context)?;
        bytes.push(native_bool(*discontinuity_flag));
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve { family, context } =
        &procedural.definition
    {
        let name = match family {
            cadmpeg_ir::geometry::SurfaceCurveFamily::Blend => "blend_int_cur",
            cadmpeg_ir::geometry::SurfaceCurveFamily::SurfaceConstrained => "surf_int_cur",
            cadmpeg_ir::geometry::SurfaceCurveFamily::Parametric => "par_int_cur",
            cadmpeg_ir::geometry::SurfaceCurveFamily::Skin => "skin_int_cur",
        };
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, name)?;
        native_intcurve_support_context(bytes, target, context)?;
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Silhouette {
        context,
        silhouette,
        cast_surface,
        light_direction,
    } = &procedural.definition
    {
        let (name, draft_factor) = match silhouette {
            cadmpeg_ir::geometry::SilhouetteKind::Standard => ("silh_int_cur", None),
            cadmpeg_ir::geometry::SilhouetteKind::Parametric => ("para_silh_int_cur", None),
            cadmpeg_ir::geometry::SilhouetteKind::Taper { draft_factor } => {
                ("taper_silh_int_cur", Some(*draft_factor))
            }
        };
        let cast_surface = target
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *cast_surface)
            .ok_or_else(|| CodecError::Malformed("silhouette cast surface is missing".into()))?;
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, name)?;
        native_intcurve_support_context(bytes, target, context)?;
        native_embedded_surface(bytes, &cast_surface.geometry)?;
        native_vector(
            bytes,
            [light_direction.x, light_direction.y, light_direction.z],
        );
        if let Some(draft_factor) = draft_factor {
            native_f64(bytes, draft_factor);
        }
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceOffset {
        context,
        discontinuity_flag,
        base_u_range,
        base_v_range,
        base,
        base_range,
        distance,
        shift,
        scale,
    } = &procedural.definition
    {
        let base = target
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *base)
            .ok_or_else(|| CodecError::Malformed("surface-offset base curve is missing".into()))?;
        let base = native_interval_curve(&base.geometry, *base_range)?;
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "off_surf_int_cur")?;
        native_intcurve_support_context(bytes, target, context)?;
        bytes.push(native_bool(*discontinuity_flag));
        for range in [base_u_range, base_v_range] {
            for value in *range {
                native_f64(bytes, value);
            }
        }
        native_nurbs_curve(bytes, &base)?;
        for value in base_range {
            native_f64(bytes, *value);
        }
        native_f64(bytes, *distance / 10.0);
        native_f64(bytes, *shift);
        native_f64(bytes, *scale);
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Spring {
        context,
        surface_parameter_ranges,
        first_pcurve_parameter_range,
        discontinuity_flag,
        direction,
    } = &procedural.definition
    {
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "spring_int_cur")?;
        for (side_index, side) in context.sides.iter().enumerate() {
            if let Some(surface_id) = &side.surface {
                if surface_parameter_ranges[side_index].is_some() {
                    return Err(CodecError::Malformed(
                        "spring surface ranges require a null_surface support".into(),
                    ));
                }
                let surface = target
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == *surface_id)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "spring references missing support {surface_id}"
                        ))
                    })?;
                native_embedded_surface(bytes, &surface.geometry)?;
            } else {
                native_ident(bytes, "null_surface")?;
                let ranges = surface_parameter_ranges[side_index].ok_or_else(|| {
                    CodecError::Malformed(
                        "spring null_surface support requires U/V parameter ranges".into(),
                    )
                })?;
                for range in ranges {
                    for value in range {
                        native_f64(bytes, value);
                    }
                }
            }
        }
        for (side_index, side) in context.sides.iter().enumerate() {
            if let Some(pcurve) = &side.pcurve {
                if side_index == 0 && first_pcurve_parameter_range.is_some() {
                    return Err(CodecError::Malformed(
                        "spring first-pcurve range requires a nullbs support".into(),
                    ));
                }
                native_nurbs_pcurve_block(bytes, pcurve)?;
            } else {
                native_ident(bytes, "nullbs")?;
                if side_index == 0 {
                    let range = first_pcurve_parameter_range.ok_or_else(|| {
                        CodecError::Malformed(
                            "spring first nullbs support requires a parameter range".into(),
                        )
                    })?;
                    for value in range {
                        native_f64(bytes, value);
                    }
                }
            }
        }
        for value in context.parameter_range {
            native_f64(bytes, value);
        }
        for discontinuities in &context.discontinuities {
            native_i64(
                bytes,
                i64::try_from(discontinuities.len()).map_err(|_| {
                    CodecError::NotImplemented("discontinuity count exceeds i64".into())
                })?,
            );
            for value in discontinuities {
                native_f64(bytes, *value);
            }
        }
        bytes.push(native_bool(*discontinuity_flag));
        native_enum(bytes, *direction);
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::ThreeSurfaceIntersection {
        context,
        selector,
        third,
    } = &procedural.definition
    {
        let surface_id = third.surface.as_ref().ok_or_else(|| {
            CodecError::NotImplemented(
                "source-less F3D sss_int_cur requires a third support surface".into(),
            )
        })?;
        let surface = target
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *surface_id)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "three-surface intersection references missing support {surface_id}"
                ))
            })?;
        let pcurve = third.pcurve.as_ref().ok_or_else(|| {
            CodecError::NotImplemented(
                "source-less F3D sss_int_cur requires a third support pcurve".into(),
            )
        })?;
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "sss_int_cur")?;
        native_intcurve_support_context(bytes, target, context)?;
        native_i64(bytes, *selector);
        native_embedded_surface(bytes, &surface.geometry)?;
        native_nurbs_pcurve_block(bytes, pcurve)?;
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::TwoSidedOffset {
        context,
        discontinuity_flag,
        offsets,
    } = &procedural.definition
    {
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "off_int_cur")?;
        native_intcurve_support_context(bytes, target, context)?;
        bytes.push(native_bool(*discontinuity_flag));
        for offset in offsets {
            native_f64(bytes, *offset / 10.0);
        }
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::VectorOffset {
        source,
        parameter_range,
        offset,
        labels: [labels_0, labels_1, ..],
        codes,
    } = &procedural.definition
    {
        let source = target
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *source)
            .ok_or_else(|| CodecError::Malformed("vector offset source curve is missing".into()))?;
        let source = native_interval_curve(&source.geometry, *parameter_range)?;
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "offset_int_cur")?;
        bytes.push(0x0b);
        native_nurbs_curve(bytes, &source)?;
        native_f64(bytes, parameter_range[0]);
        native_f64(bytes, parameter_range[1]);
        native_vector(bytes, [offset.x / 10.0, offset.y / 10.0, offset.z / 10.0]);
        native_string(bytes, labels_0)?;
        native_i64(bytes, codes[0]);
        native_string(bytes, labels_1)?;
        native_i64(bytes, codes[1]);
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    if let cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
        source,
        parameter_range,
    } = &procedural.definition
    {
        let source = target
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *source)
            .ok_or_else(|| CodecError::Malformed("subset source curve is missing".into()))?;
        let source = native_interval_curve(&source.geometry, *parameter_range)?;
        native_curve_base(bytes, "intcurve")?;
        bytes.push(0x0f);
        native_ident(bytes, "subset_int_cur")?;
        native_nurbs_curve(bytes, &source)?;
        native_f64(bytes, parameter_range[0]);
        native_f64(bytes, parameter_range[1]);
        native_nurbs_curve(bytes, solved_cache)?;
        write_cache_fit_tolerance(bytes);
        bytes.push(0x10);
        return Ok(true);
    }
    let (angle_range, center, major, minor, pitch, apex_factor, axis) = match &procedural.definition
    {
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
            angle_range,
            center,
            major,
            minor,
            pitch,
            apex_factor,
            axis,
        } => (angle_range, center, major, minor, pitch, apex_factor, axis),
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Offset { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::SpatialOffset { .. } => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D offset curve {} lacks a defined native offset-law grammar",
                procedural.id
            )))
        }
        cadmpeg_ir::geometry::ProceduralCurveDefinition::BlendSpine { .. } => {
            return Err(CodecError::NotImplemented(format!(
                "source-less F3D blend-spine curve {} lacks its native blend construction",
                procedural.id
            )))
        }
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Exact
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::Law { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::ThreeSurfaceIntersection { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::Silhouette { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceOffset { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::Spring { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::Deformable { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::Projection { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::TwoSidedOffset { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::VectorOffset { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset { .. }
        | cadmpeg_ir::geometry::ProceduralCurveDefinition::Unknown { .. } => {
            unreachable!("procedural curve variant returned from its native writer")
        }
    };
    native_curve_base(bytes, "intcurve")?;
    bytes.push(0x0f);
    native_ident(bytes, "helix_int_cur")?;
    for value in *angle_range {
        bytes.push(0x0a);
        native_f64(bytes, value);
    }
    native_point(bytes, [center.x / 10.0, center.y / 10.0, center.z / 10.0]);
    for vector in [major, minor, pitch] {
        native_point(bytes, [vector.x / 10.0, vector.y / 10.0, vector.z / 10.0]);
    }
    native_f64(bytes, *apex_factor);
    native_vector(bytes, [axis.x, axis.y, axis.z]);
    native_nurbs_curve(bytes, solved_cache)?;
    write_cache_fit_tolerance(bytes);
    bytes.push(0x10);
    Ok(true)
}

fn native_embedded_surface(
    bytes: &mut Vec<u8>,
    geometry: &SurfaceGeometry,
) -> Result<(), CodecError> {
    match geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            native_ident(bytes, "plane")?;
            native_point(bytes, [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0]);
            native_vector(bytes, [normal.x, normal.y, normal.z]);
            native_vector(bytes, [u_axis.x, u_axis.y, u_axis.z]);
            bytes.push(0x0b);
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => native_embedded_cone(bytes, *origin, *axis, *ref_direction, *radius, 1.0, 0.0)?,
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } => native_embedded_cone(
            bytes,
            *origin,
            *axis,
            *ref_direction,
            *radius,
            *ratio,
            *half_angle,
        )?,
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            native_ident(bytes, "sphere")?;
            native_point(bytes, [center.x / 10.0, center.y / 10.0, center.z / 10.0]);
            native_f64(bytes, *radius / 10.0);
            native_vector(bytes, [ref_direction.x, ref_direction.y, ref_direction.z]);
            native_vector(bytes, [axis.x, axis.y, axis.z]);
            bytes.extend_from_slice(&[0x0b; 5]);
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            native_ident(bytes, "torus")?;
            native_point(bytes, [center.x / 10.0, center.y / 10.0, center.z / 10.0]);
            native_vector(bytes, [axis.x, axis.y, axis.z]);
            native_f64(bytes, *major_radius / 10.0);
            native_f64(bytes, *minor_radius / 10.0);
            native_vector(bytes, [ref_direction.x, ref_direction.y, ref_direction.z]);
            bytes.extend_from_slice(&[0x0b; 5]);
        }
        SurfaceGeometry::Nurbs(surface) => {
            native_ident(bytes, "spline")?;
            native_nurbs_surface(bytes, surface)?;
        }
        SurfaceGeometry::Procedural { .. } | SurfaceGeometry::Unknown { .. } => {
            return Err(CodecError::NotImplemented(
                "source-less F3D embedded unknown support surfaces are unsupported".into(),
            ));
        }
        SurfaceGeometry::Polygonal { .. } => {
            return Err(CodecError::NotImplemented(
                "source-less F3D embedded polygonal support surfaces are unsupported".into(),
            ));
        }
        SurfaceGeometry::Transformed { .. } => {
            return Err(CodecError::NotImplemented(
                "source-less F3D embedded transformed support surfaces are unsupported".into(),
            ));
        }
    }
    Ok(())
}

fn native_intcurve_support_context(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    context: &cadmpeg_ir::geometry::IntcurveSupportContext,
) -> Result<(), CodecError> {
    if context
        .sides
        .iter()
        .any(|side| side.pcurve_parameter_range.is_some())
    {
        return Err(CodecError::NotImplemented(
            "F3D intcurve writing does not encode independent support-pcurve parameter intervals"
                .into(),
        ));
    }
    for side in &context.sides {
        if let Some(surface_id) = &side.surface {
            let surface = target
                .model
                .surfaces
                .iter()
                .find(|surface| surface.id == *surface_id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "intcurve references missing support {surface_id}"
                    ))
                })?;
            native_embedded_surface(bytes, &surface.geometry)?;
        } else {
            native_ident(bytes, "null_surface")?;
        }
    }
    for side in &context.sides {
        if let Some(pcurve) = &side.pcurve {
            native_nurbs_pcurve_block(bytes, pcurve)?;
        } else {
            native_ident(bytes, "nullbs")?;
        }
    }
    for value in context.parameter_range {
        native_f64(bytes, value);
    }
    for discontinuities in &context.discontinuities {
        native_i64(
            bytes,
            i64::try_from(discontinuities.len()).map_err(|_| {
                CodecError::NotImplemented("discontinuity count exceeds i64".into())
            })?,
        );
        for value in discontinuities {
            native_f64(bytes, *value);
        }
    }
    Ok(())
}

fn native_embedded_cone(
    bytes: &mut Vec<u8>,
    origin: cadmpeg_ir::math::Point3,
    axis: Vector3,
    ref_direction: Vector3,
    radius: f64,
    ratio: f64,
    half_angle: f64,
) -> Result<(), CodecError> {
    native_ident(bytes, "cone")?;
    native_point(bytes, [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0]);
    native_vector(bytes, [axis.x, axis.y, axis.z]);
    native_vector(
        bytes,
        [
            ref_direction.x * radius / 10.0,
            ref_direction.y * radius / 10.0,
            ref_direction.z * radius / 10.0,
        ],
    );
    native_f64(bytes, ratio);
    bytes.extend_from_slice(&[0x0b, 0x0b]);
    native_f64(bytes, half_angle.sin());
    native_f64(bytes, half_angle.cos());
    native_f64(bytes, radius / 10.0);
    bytes.extend_from_slice(&[0x0b; 5]);
    Ok(())
}

fn pcurve_uses_ref_form(pcurve: &Pcurve) -> Result<bool, CodecError> {
    match (
        pcurve.wrapper_reversed,
        pcurve.native_tail_flags,
        pcurve.fit_tolerance,
    ) {
        (None, None, None) => Ok(true),
        (Some(_), Some(_), Some(_)) => Ok(false),
        _ => Err(CodecError::Malformed(format!(
            "pcurve {} mixes inline and ref-form native fields",
            pcurve.id
        ))),
    }
}

fn native_pcurve(
    bytes: &mut Vec<u8>,
    pcurve: &Pcurve,
    companion_ref: Option<i64>,
) -> Result<(), CodecError> {
    if pcurve_uses_ref_form(pcurve)? {
        let companion_ref = companion_ref.ok_or_else(|| {
            CodecError::Malformed(format!(
                "ref-form pcurve {} has no companion record",
                pcurve.id
            ))
        })?;
        let range = pcurve.parameter_range.ok_or_else(|| {
            CodecError::Malformed(format!(
                "ref-form pcurve {} has no parameter range",
                pcurve.id
            ))
        })?;
        native_ident(bytes, "pcurve")?;
        native_ref(bytes, -1);
        native_i64(bytes, -1);
        native_ref(bytes, -1);
        native_i64(bytes, 2);
        native_ref(bytes, companion_ref);
        native_f64(bytes, range[0]);
        native_f64(bytes, range[1]);
        return Ok(());
    }
    if companion_ref.is_some() {
        return Err(CodecError::Malformed(format!(
            "inline pcurve {} unexpectedly has a companion record",
            pcurve.id
        )));
    }
    let range = pcurve.parameter_range.unwrap_or([0.0, 1.0]);
    let NativePcurveGeometry {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    } = native_pcurve_geometry(&pcurve.geometry, range)?;
    let degree_usize = usize::try_from(degree)
        .map_err(|_| CodecError::NotImplemented("F3D pcurve degree exceeds usize".into()))?;
    if knots.len() != control_points.len() + degree_usize + 1
        || weights
            .as_ref()
            .is_some_and(|weights| weights.len() != control_points.len())
    {
        return Err(CodecError::Malformed(
            "source-less F3D pcurve has inconsistent cardinality".into(),
        ));
    }
    native_ident(bytes, "pcurve")?;
    native_ref(bytes, -1);
    native_i64(bytes, -1);
    native_ref(bytes, -1);
    native_i64(bytes, 0);
    bytes.push(native_bool(pcurve.wrapper_reversed.unwrap_or(false)));
    bytes.push(0x0f);
    native_ident(bytes, "exp_par_cur")?;
    native_ident(bytes, if weights.is_some() { "nurbs" } else { "nubs" })?;
    native_i64(bytes, i64::from(degree));
    native_enum(bytes, if periodic { 2 } else { 0 });
    native_i64(
        bytes,
        i64::try_from(unique_knot_count(&knots)).map_err(|_| {
            CodecError::NotImplemented("F3D pcurve unique-knot count exceeds i64".into())
        })?,
    );
    native_nurbs_knots(bytes, &knots)?;
    for (index, point) in control_points.iter().enumerate() {
        native_f64(bytes, point.u);
        native_f64(bytes, point.v);
        if let Some(weights) = weights.as_ref() {
            native_f64(bytes, weights[index]);
        }
    }
    native_f64(bytes, pcurve.fit_tolerance.unwrap_or(0.0));
    bytes.push(0x10);
    for flag in pcurve.native_tail_flags.unwrap_or([true; 4]) {
        bytes.push(native_bool(flag));
    }
    let range = pcurve.parameter_range.unwrap_or_else(|| {
        [
            knots.first().copied().unwrap_or(0.0),
            knots.last().copied().unwrap_or(0.0),
        ]
    });
    native_f64(bytes, range[0]);
    native_f64(bytes, range[1]);
    Ok(())
}

fn native_ref_pcurve_companion(bytes: &mut Vec<u8>, pcurve: &Pcurve) -> Result<(), CodecError> {
    if !pcurve_uses_ref_form(pcurve)? {
        return Err(CodecError::Malformed(format!(
            "inline pcurve {} cannot emit a ref-form companion",
            pcurve.id
        )));
    }
    let range = pcurve.parameter_range.ok_or_else(|| {
        CodecError::Malformed(format!(
            "ref-form pcurve {} has no parameter range",
            pcurve.id
        ))
    })?;
    let native = native_pcurve_geometry(&pcurve.geometry, range)?;
    let lifted = NurbsCurve {
        degree: native.degree,
        knots: native.knots,
        control_points: native
            .control_points
            .into_iter()
            .map(|point| Point3::new(point.u * 10.0, point.v * 10.0, 0.0))
            .collect(),
        weights: native.weights,
        periodic: native.periodic,
    };
    native_curve_base(bytes, "intcurve")?;
    native_nurbs_curve(bytes, &lifted)?;
    native_nurbs_pcurve_block(bytes, &pcurve.geometry)?;
    Ok(())
}

struct NativePcurveGeometry {
    degree: u32,
    knots: Vec<f64>,
    control_points: Vec<cadmpeg_ir::math::Point2>,
    weights: Option<Vec<f64>>,
    periodic: bool,
}

fn native_pcurve_geometry(
    geometry: &PcurveGeometry,
    range: [f64; 2],
) -> Result<NativePcurveGeometry, CodecError> {
    match geometry {
        PcurveGeometry::Line { origin, direction } => {
            if !range.iter().all(|value| value.is_finite()) || range[0] >= range[1] {
                return Err(CodecError::Malformed(
                    "source-less F3D line pcurve requires an ordered finite range".into(),
                ));
            }
            Ok(NativePcurveGeometry {
                degree: 1,
                knots: vec![range[0], range[0], range[1], range[1]],
                control_points: vec![
                    cadmpeg_ir::math::Point2::new(
                        origin.u + range[0] * direction.u,
                        origin.v + range[0] * direction.v,
                    ),
                    cadmpeg_ir::math::Point2::new(
                        origin.u + range[1] * direction.u,
                        origin.v + range[1] * direction.v,
                    ),
                ],
                weights: None,
                periodic: false,
            })
        }
        PcurveGeometry::Circle { .. }
        | PcurveGeometry::Ellipse { .. }
        | PcurveGeometry::PolarHarmonic { .. }
        | PcurveGeometry::PolarNurbs { .. } => Err(CodecError::NotImplemented(
            "F3D analytic pcurve writing is not supported".into(),
        )),
        PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic,
        } => Ok(NativePcurveGeometry {
            degree: *degree,
            knots: knots.clone(),
            control_points: control_points.clone(),
            weights: weights.clone(),
            periodic: *periodic,
        }),
        PcurveGeometry::Trimmed {
            parameter_range,
            basis,
        } => native_pcurve_geometry(basis, *parameter_range),
        PcurveGeometry::Parabola { .. }
        | PcurveGeometry::Hyperbola { .. }
        | PcurveGeometry::Offset { .. } => Err(CodecError::NotImplemented(
            "F3D writing of this exact pcurve family is not implemented".into(),
        )),
    }
}

fn native_nurbs_pcurve_block(
    bytes: &mut Vec<u8>,
    geometry: &PcurveGeometry,
) -> Result<(), CodecError> {
    let NativePcurveGeometry {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    } = native_pcurve_geometry(geometry, [0.0, 1.0])?;
    let degree_usize = usize::try_from(degree)
        .map_err(|_| CodecError::NotImplemented("F3D pcurve degree exceeds usize".into()))?;
    if knots.len() != control_points.len() + degree_usize + 1
        || weights
            .as_ref()
            .is_some_and(|weights| weights.len() != control_points.len())
    {
        return Err(CodecError::Malformed(
            "embedded F3D support pcurve has inconsistent cardinality".into(),
        ));
    }
    native_ident(bytes, if weights.is_some() { "nurbs" } else { "nubs" })?;
    native_i64(bytes, i64::from(degree));
    native_enum(bytes, if periodic { 2 } else { 0 });
    native_i64(
        bytes,
        i64::try_from(unique_knot_count(&knots)).map_err(|_| {
            CodecError::NotImplemented("F3D pcurve unique-knot count exceeds i64".into())
        })?,
    );
    native_nurbs_knots(bytes, &knots)?;
    for (index, point) in control_points.iter().enumerate() {
        native_f64(bytes, point.u);
        native_f64(bytes, point.v);
        if let Some(weights) = weights.as_ref() {
            native_f64(bytes, weights[index]);
        }
    }
    Ok(())
}

fn native_nurbs_knot_counts(bytes: &mut Vec<u8>, knots: [&[f64]; 2]) -> Result<(), CodecError> {
    for knots in knots {
        native_i64(
            bytes,
            i64::try_from(unique_knot_count(knots)).map_err(|_| {
                CodecError::NotImplemented("F3D unique-knot count exceeds i64".into())
            })?,
        );
    }
    Ok(())
}

fn native_nurbs_knots(bytes: &mut Vec<u8>, knots: &[f64]) -> Result<(), CodecError> {
    let mut runs = Vec::<(f64, usize)>::new();
    for knot in knots {
        if let Some((value, count)) = runs.last_mut() {
            if *value == *knot {
                *count += 1;
                continue;
            }
        }
        runs.push((*knot, 1));
    }
    let run_count = runs.len();
    for (index, (value, expanded)) in runs.into_iter().enumerate() {
        let endpoint_extra = usize::from(index == 0 || index + 1 == run_count);
        let stored = expanded
            .checked_sub(endpoint_extra)
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                CodecError::Malformed("F3D NURBS endpoint multiplicity is invalid".into())
            })?;
        native_f64(bytes, value);
        native_i64(
            bytes,
            i64::try_from(stored).map_err(|_| {
                CodecError::NotImplemented("F3D knot multiplicity exceeds i64".into())
            })?,
        );
    }
    Ok(())
}

fn native_string(bytes: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    native_text(bytes, 0x07, value)
}

fn native_u16_string(bytes: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    let length = u16::try_from(value.len())
        .map_err(|_| CodecError::NotImplemented("F3D native text exceeds u16".into()))?;
    bytes.push(0x08);
    bytes.extend_from_slice(&length.to_le_bytes());
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

fn native_text(bytes: &mut Vec<u8>, tag: u8, value: &str) -> Result<(), CodecError> {
    let length = u8::try_from(value.len())
        .map_err(|_| CodecError::NotImplemented("F3D native text exceeds 255 bytes".into()))?;
    bytes.extend_from_slice(&[tag, length]);
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

fn native_ref(bytes: &mut Vec<u8>, value: i64) {
    bytes.push(0x0c);
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn native_record_index(base: i64, ordinal: usize) -> Result<i64, CodecError> {
    let ordinal = i64::try_from(ordinal)
        .map_err(|_| CodecError::NotImplemented("F3D record ordinal exceeds i64".into()))?;
    base.checked_add(ordinal)
        .ok_or_else(|| CodecError::NotImplemented("F3D record index exceeds i64".into()))
}

fn native_i64(bytes: &mut Vec<u8>, value: i64) {
    bytes.push(0x04);
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn native_enum(bytes: &mut Vec<u8>, value: i64) {
    bytes.push(0x15);
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn native_f64(bytes: &mut Vec<u8>, value: f64) {
    bytes.push(0x06);
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn native_f32(bytes: &mut Vec<u8>, value: f32) {
    bytes.push(0x05);
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn native_point(bytes: &mut Vec<u8>, point: [f64; 3]) {
    bytes.push(0x13);
    for value in point {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

fn native_vector(bytes: &mut Vec<u8>, vector: [f64; 3]) {
    bytes.push(0x14);
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
}

fn native_transform(
    bytes: &mut Vec<u8>,
    target: &CadIr,
    body: &Body,
    transform: Transform,
) -> Result<(), CodecError> {
    native_ident(bytes, "transform")?;
    for vector in [
        [
            transform.rows[0][0],
            transform.rows[1][0],
            transform.rows[2][0],
        ],
        [
            transform.rows[0][1],
            transform.rows[1][1],
            transform.rows[2][1],
        ],
        [
            transform.rows[0][2],
            transform.rows[1][2],
            transform.rows[2][2],
        ],
        [
            transform.rows[0][3] / 600.0,
            transform.rows[1][3] / 600.0,
            transform.rows[2][3] / 600.0,
        ],
    ] {
        native_vector(bytes, vector);
    }
    native_f64(bytes, transform.rows[3][3]);
    let hints = f3d_native(target)?
        .and_then(|native| {
            native
                .transform_hints
                .into_iter()
                .find(|hints| hints.body == body.id)
        })
        .map_or_else(
            || derived_transform_hints(transform),
            |hints| [hints.rotation, hints.reflection, hints.shear],
        );
    for hint in hints {
        bytes.push(native_bool(hint));
    }
    Ok(())
}

fn derived_transform_hints(transform: Transform) -> [bool; 3] {
    let linear = [
        [
            transform.rows[0][0],
            transform.rows[0][1],
            transform.rows[0][2],
        ],
        [
            transform.rows[1][0],
            transform.rows[1][1],
            transform.rows[1][2],
        ],
        [
            transform.rows[2][0],
            transform.rows[2][1],
            transform.rows[2][2],
        ],
    ];
    let determinant = linear[0][0] * (linear[1][1] * linear[2][2] - linear[1][2] * linear[2][1])
        - linear[0][1] * (linear[1][0] * linear[2][2] - linear[1][2] * linear[2][0])
        + linear[0][2] * (linear[1][0] * linear[2][1] - linear[1][1] * linear[2][0]);
    let reflection = determinant.is_sign_negative();
    let columns = [
        [linear[0][0], linear[1][0], linear[2][0]],
        [linear[0][1], linear[1][1], linear[2][1]],
        [linear[0][2], linear[1][2], linear[2][2]],
    ];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let scale = columns
        .iter()
        .map(|column| dot(*column, *column))
        .fold(1.0f64, f64::max);
    let shear = dot(columns[0], columns[1]).abs() > f64::EPSILON * scale
        || dot(columns[0], columns[2]).abs() > f64::EPSILON * scale
        || dot(columns[1], columns[2]).abs() > f64::EPSILON * scale;
    let rotation = linear != [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    [rotation, reflection, shear]
}

fn native_history_tail(bytes: &mut Vec<u8>, target: &CadIr) -> Result<(), CodecError> {
    let native = f3d_native(target)?;
    let histories = native
        .as_ref()
        .map_or(&[][..], |native| native.asm_histories.as_slice());
    if histories.is_empty() {
        native_ident(bytes, "delta_state")?;
        return Ok(());
    }
    if histories.len() != 1 {
        return Err(CodecError::NotImplemented(
            "source-less F3D generation supports one ASM history stream".into(),
        ));
    }
    let history = &histories[0];
    match (history.stream_size, history.high_water_mark) {
        (Some(stream_size), Some(high_water_mark)) => {
            if history
                .states
                .first()
                .is_none_or(|state| state.state_id != stream_size)
                || high_water_mark < stream_size
            {
                return Err(CodecError::Malformed(format!(
                    "F3D history {} requires head state_id == stream_size <= high_water_mark",
                    history.id
                )));
            }
            for name in ["Begin", "of", "ASM", "History"] {
                native_subident(bytes, name)?;
            }
            native_ident(bytes, "Data")?;
            native_ident(bytes, "history_stream")?;
            native_i64(bytes, stream_size);
            native_i64(bytes, stream_size);
            native_i64(bytes, 0);
            native_i64(bytes, high_water_mark);
            for reference in [-1, 0, 1, -1] {
                native_ref(bytes, reference);
            }
            bytes.push(0x11);
        }
        (None, None) => {}
        _ => {
            return Err(CodecError::Malformed(format!(
                "F3D history {} has an incomplete history-stream preamble",
                history.id
            )));
        }
    }
    for state in &history.states {
        native_ident(bytes, "delta_state")?;
        native_i64(bytes, state.state_id);
        native_i64(bytes, state.version_flag);
        native_i64(bytes, state.state_flag);
        native_ref(bytes, state.previous_ref.unwrap_or(-1));
        native_ref(bytes, state.next_ref.unwrap_or(-1));
        native_ref(bytes, state.node_index);
        native_ref(bytes, state.partner_ref.unwrap_or(-1));
        native_ref(bytes, state.owner_ref);
        bytes.push(0x0b);
        for board in &state.bulletin_boards {
            native_i64(bytes, 1);
            native_ref(bytes, board.owner_ref);
            native_i64(bytes, board.number);
            for change in &board.changes {
                if change.kind != history_change_kind(change.old_ref, change.new_ref)? {
                    return Err(CodecError::Malformed(format!(
                        "F3D entity change {} has a kind inconsistent with its references",
                        change.id
                    )));
                }
                native_i64(bytes, 1);
                native_ref(bytes, change.old_ref.unwrap_or(-1));
                native_ref(bytes, change.new_ref.unwrap_or(-1));
            }
            native_i64(bytes, 0);
        }
        native_i64(bytes, 0);
        bytes.push(0x11);
        for record in &state.records {
            bytes.extend_from_slice(&record.raw_bytes);
        }
    }
    Ok(())
}

fn native_bool(value: bool) -> u8 {
    if value {
        0x0a
    } else {
        0x0b
    }
}

/// Apply supported semantic edits to a retained F3D archive.
///
/// `source_image` must match the F3D source represented by `target`. The
/// function validates changed topology, geometry, design, sketch, history, and
/// appearance fields before patching records. Unsupported edits return
/// [`CodecError::NotImplemented`].
pub fn write_semantic(
    target: &CadIr,
    source_image: &[u8],
    writer: &mut dyn Write,
) -> Result<(), CodecError> {
    let _ = f3d_native(target)?;
    let baseline = F3dCodec.decode(&mut Cursor::new(source_image), &DecodeOptions::default())?;
    let baseline_point_ids = baseline
        .ir
        .model
        .points
        .iter()
        .map(|point| point.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let target_point_ids = target
        .model
        .points
        .iter()
        .map(|point| point.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if baseline_point_ids != target_point_ids
        || target.model.points.iter().any(|point| {
            !point.position.x.is_finite()
                || !point.position.y.is_finite()
                || !point.position.z.is_finite()
        })
    {
        return Err(CodecError::NotImplemented(
            "F3D point regeneration requires the unchanged point-id set and finite coordinates"
                .into(),
        ));
    }
    let edited_curves = validate_curve_edits(&baseline.ir.model.curves, &target.model.curves)?;
    let nurbs_curve_edits = target
        .model
        .curves
        .iter()
        .filter_map(|curve| match &curve.geometry {
            CurveGeometry::Nurbs(nurbs) if edited_curves.contains(curve.id.as_str()) => {
                let before = baseline
                    .ir
                    .model
                    .curves
                    .iter()
                    .find(|before| before.id == curve.id)?;
                let CurveGeometry::Nurbs(before) = &before.geometry else {
                    return None;
                };
                Some((
                    curve.id.0.clone(),
                    NurbsCurveEdit {
                        curve: nurbs.clone(),
                        periodic: (before.periodic != nurbs.periodic).then_some(nurbs.periodic),
                    },
                ))
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let pcurve_edits = validate_pcurve_edits(&baseline.ir.model.pcurves, &target.model.pcurves)?;
    let edited_surfaces =
        validate_surface_edits(&baseline.ir.model.surfaces, &target.model.surfaces)?;
    let nurbs_surface_edits = target
        .model
        .surfaces
        .iter()
        .filter_map(|surface| match &surface.geometry {
            SurfaceGeometry::Nurbs(nurbs) if edited_surfaces.contains(surface.id.as_str()) => {
                let before = baseline
                    .ir
                    .model
                    .surfaces
                    .iter()
                    .find(|before| before.id == surface.id)?;
                let SurfaceGeometry::Nurbs(before) = &before.geometry else {
                    return None;
                };
                Some((
                    surface.id.0.clone(),
                    NurbsSurfaceEdit {
                        surface: nurbs.clone(),
                        periodic: (before.u_periodic != nurbs.u_periodic
                            || before.v_periodic != nurbs.v_periodic)
                            .then_some([nurbs.u_periodic, nurbs.v_periodic]),
                    },
                ))
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let extrusion_direction_edits = validate_procedural_surface_edits(&baseline.ir, target)?;
    let procedural_surface_fit_edits = validate_procedural_surface_fit_edits(&baseline.ir, target)?;
    let procedural_curve_edits = validate_procedural_curve_edits(
        &baseline.ir.model.procedural_curves,
        &target.model.procedural_curves,
    )?;
    let sketch_point_edits = validate_sketch_point_edits(&baseline.ir, target)?;
    let sketch_curve_edits = validate_sketch_curve_edits(&baseline.ir, target)?;
    let sketch_relation_edits = validate_sketch_relation_edits(&baseline.ir, target)?;
    let persistent_reference_edits = validate_persistent_reference_edits(&baseline.ir, target)?;
    let construction_recipe_edits = validate_construction_recipe_edits(&baseline.ir, target)?;
    let body_member_edits = validate_body_member_edits(&baseline.ir, target)?;
    let entity_header_edits = validate_entity_header_edits(&baseline.ir, target)?;
    let design_object_edits = validate_design_object_edits(&baseline.ir, target)?;
    let lost_edge_edits = validate_lost_edge_edits(&baseline.ir, target)?;
    let material_assignment_edits = validate_material_assignment_edits(&baseline.ir, target)?;
    let protein_appearance_edits = validate_material_assignment_appearances(&baseline.ir, target)?;
    let act_guid_edits = validate_act_guid_edits(&baseline.ir, target)?;
    let act_root_edits = validate_act_root_edits(&baseline.ir, target)?;
    let act_entity_edits = validate_act_entity_edits(&baseline.ir, target)?;
    let configuration_edits = validate_configuration_edits(&baseline.ir, target)?;
    validate_act_appearance_bindings(&baseline.ir, target)?;
    let body_transform_edits =
        validate_body_transform_edits(&baseline.ir.model.bodies, &target.model.bodies)?;
    let body_visibility_edits = validate_body_visibility_edits(&baseline.ir, target)?;
    let body_native_key_edits = validate_body_native_key_edits(&baseline.ir, target)?;
    let transform_hint_edits = validate_transform_hint_edits(&baseline.ir, target)?;
    let mut entity_color_edits =
        validate_body_color_edits(&baseline.ir.model.bodies, &target.model.bodies)?;
    entity_color_edits.extend(validate_face_color_edits(
        &baseline.ir.model.faces,
        &target.model.faces,
    )?);
    let mut edge_range_edits =
        validate_edge_range_edits(&baseline.ir.model.edges, &target.model.edges)?;
    // IR line-edge parameters are millimeter arc lengths; the native stream
    // stores centimeters. Conic parameters are angles in both.
    for (edge_id, range) in &mut edge_range_edits {
        let is_line = target
            .model
            .edges
            .iter()
            .find(|edge| edge.id.as_str() == edge_id)
            .and_then(|edge| edge.curve.as_ref())
            .is_some_and(|curve_id| {
                target.model.curves.iter().any(|curve| {
                    curve.id == *curve_id && matches!(curve.geometry, CurveGeometry::Line { .. })
                })
            });
        if is_line {
            range[0] /= 10.0;
            range[1] /= 10.0;
        }
    }
    let face_sense_edits = validate_face_sense_edits(&baseline.ir, target)?;
    let coedge_sense_edits =
        validate_coedge_sense_edits(&baseline.ir.model.coedges, &target.model.coedges)?;
    let history_state_edits = validate_history_state_edits(&baseline.ir, target)?;
    let creation_timestamp_edits = validate_creation_timestamp_edits(&baseline.ir, target)?;
    let edge_continuity_edits = validate_edge_continuity_edits(&baseline.ir, target)?;
    let edge_ownership_edits = validate_edge_ownership_edits(&baseline.ir, target)?;
    let vertex_ownership_edits = validate_vertex_ownership_edits(&baseline.ir, target)?;
    let face_sidedness_edits = validate_face_sidedness_edits(&baseline.ir, target)?;
    let tolerant_vertex_edits = validate_tolerant_vertex_edits(&baseline.ir, target)?;
    let tolerant_coedge_edits = validate_tolerant_coedge_edits(&baseline.ir, target)?;
    let wire_topology_edits = validate_wire_topology_edits(&baseline.ir, target)?;
    let mut supported_target = baseline.ir.clone();
    supported_target
        .model
        .points
        .clone_from(&target.model.points);
    supported_target
        .model
        .curves
        .clone_from(&target.model.curves);
    supported_target
        .model
        .surfaces
        .clone_from(&target.model.surfaces);
    supported_target
        .model
        .pcurves
        .clone_from(&target.model.pcurves);
    for body in &mut supported_target.model.bodies {
        if let Some(candidate) = target
            .model
            .bodies
            .iter()
            .find(|candidate| candidate.id == body.id)
        {
            body.transform = candidate.transform;
            body.color = candidate.color;
            body.visible = candidate.visible;
        }
    }
    supported_target.model.edges.clone_from(&target.model.edges);
    supported_target
        .model
        .vertices
        .clone_from(&target.model.vertices);
    supported_target.model.faces.clone_from(&target.model.faces);
    supported_target
        .model
        .coedges
        .clone_from(&target.model.coedges);
    supported_target
        .model
        .appearance_bindings
        .clone_from(&target.model.appearance_bindings);
    supported_target
        .model
        .appearances
        .clone_from(&target.model.appearances);
    supported_target
        .model
        .procedural_surfaces
        .clone_from(&target.model.procedural_surfaces);
    supported_target
        .model
        .procedural_curves
        .clone_from(&target.model.procedural_curves);
    if let (Some(mut supported), Some(target_native)) =
        (f3d_native(&supported_target)?, f3d_native(target)?)
    {
        supported
            .body_native_keys
            .clone_from(&target_native.body_native_keys);
        supported
            .sketch_points
            .clone_from(&target_native.sketch_points);
        supported
            .sketch_curve_identities
            .clone_from(&target_native.sketch_curve_identities);
        supported
            .sketch_relations
            .clone_from(&target_native.sketch_relations);
        supported
            .persistent_references
            .clone_from(&target_native.persistent_references);
        supported
            .construction_recipes
            .clone_from(&target_native.construction_recipes);
        supported
            .design_body_members
            .clone_from(&target_native.design_body_members);
        supported
            .design_configurations
            .clone_from(&target_native.design_configurations);
        supported
            .design_entity_headers
            .clone_from(&target_native.design_entity_headers);
        supported
            .design_objects
            .clone_from(&target_native.design_objects);
        supported
            .lost_edge_references
            .clone_from(&target_native.lost_edge_references);
        supported
            .design_material_assignments
            .clone_from(&target_native.design_material_assignments);
        supported.act_guids.clone_from(&target_native.act_guids);
        supported
            .act_root_components
            .clone_from(&target_native.act_root_components);
        supported
            .act_entities
            .clone_from(&target_native.act_entities);
        supported
            .asm_histories
            .clone_from(&target_native.asm_histories);
        supported
            .creation_timestamps
            .clone_from(&target_native.creation_timestamps);
        supported
            .edge_continuities
            .clone_from(&target_native.edge_continuities);
        supported
            .edge_ownerships
            .clone_from(&target_native.edge_ownerships);
        supported
            .vertex_ownerships
            .clone_from(&target_native.vertex_ownerships);
        supported
            .face_sidedness
            .clone_from(&target_native.face_sidedness);
        supported
            .tolerant_coedge_parameters
            .clone_from(&target_native.tolerant_coedge_parameters);
        supported
            .tolerant_vertex_tails
            .clone_from(&target_native.tolerant_vertex_tails);
        supported
            .transform_hints
            .clone_from(&target_native.transform_hints);
        supported
            .wire_topologies
            .clone_from(&target_native.wire_topologies);
        supported.store(supported_target.native.namespace_mut("f3d"))?;
    }
    if decode::semantic_hash(&supported_target) != decode::semantic_hash(target) {
        return Err(CodecError::NotImplemented(
            "modified F3D IR contains edits beyond supported point, line, and plane carriers"
                .into(),
        ));
    }

    let active_brep = baseline
        .ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("active_brep"))
        .ok_or_else(|| CodecError::Malformed("F3D baseline has no active BREP".into()))?;
    let positions = target
        .model
        .points
        .iter()
        .map(|point| (point.id.0.clone(), point.position))
        .collect::<BTreeMap<_, _>>();
    let lines = target
        .model
        .curves
        .iter()
        .filter_map(|curve| match curve.geometry {
            CurveGeometry::Line { origin, direction } => edited_curves
                .contains(curve.id.as_str())
                .then(|| (curve.id.0.clone(), (origin, direction))),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let conics = target
        .model
        .curves
        .iter()
        .filter_map(|curve| match curve.geometry {
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } => edited_curves.contains(curve.id.as_str()).then(|| {
                (
                    curve.id.0.clone(),
                    (center, axis, ref_direction, radius, radius),
                )
            }),
            CurveGeometry::Ellipse {
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius,
            } => edited_curves.contains(curve.id.as_str()).then(|| {
                (
                    curve.id.0.clone(),
                    (center, axis, major_direction, major_radius, minor_radius),
                )
            }),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let degenerate_curves = target
        .model
        .curves
        .iter()
        .filter_map(|curve| match curve.geometry {
            CurveGeometry::Degenerate { point } => edited_curves
                .contains(curve.id.as_str())
                .then(|| (curve.id.0.clone(), point)),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let planes = target
        .model
        .surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            } => edited_surfaces
                .contains(surface.id.as_str())
                .then(|| (surface.id.0.clone(), (origin, normal, u_axis))),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let spheres = target
        .model
        .surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Sphere {
                center,
                axis,
                ref_direction,
                radius,
            } => edited_surfaces
                .contains(surface.id.as_str())
                .then(|| (surface.id.0.clone(), (center, axis, ref_direction, radius))),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let tori = target
        .model
        .surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Torus {
                center,
                axis,
                ref_direction,
                major_radius,
                minor_radius,
            } => edited_surfaces.contains(surface.id.as_str()).then(|| {
                (
                    surface.id.0.clone(),
                    (center, axis, ref_direction, major_radius, minor_radius),
                )
            }),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let cones = target
        .model
        .surfaces
        .iter()
        .filter_map(|surface| match surface.geometry {
            SurfaceGeometry::Cylinder {
                origin,
                axis,
                ref_direction,
                radius,
            } => edited_surfaces.contains(surface.id.as_str()).then(|| {
                (
                    surface.id.0.clone(),
                    (origin, axis, ref_direction, radius, 1.0, 0.0),
                )
            }),
            SurfaceGeometry::Cone {
                origin,
                axis,
                ref_direction,
                radius,
                ratio,
                half_angle,
            } => edited_surfaces.contains(surface.id.as_str()).then(|| {
                (
                    surface.id.0.clone(),
                    (origin, axis, ref_direction, radius, ratio, half_angle),
                )
            }),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();

    let mut archive = zip::ZipArchive::new(Cursor::new(source_image))
        .map_err(|error| CodecError::Malformed(format!("retained F3D ZIP is invalid: {error}")))?;
    let output = Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(output);
    let mut patched_protein_appearances = BTreeSet::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| CodecError::Malformed(format!("invalid F3D ZIP entry: {error}")))?;
        let name = entry.name().to_owned();
        let options = SimpleFileOptions::default().compression_method(entry.compression());
        if entry.is_dir() {
            zip.add_directory(name, options).map_err(|error| {
                CodecError::Malformed(format!("cannot write F3D directory: {error}"))
            })?;
            continue;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        if let Some(configuration) = configuration_edits.get(&name) {
            bytes.clone_from(configuration);
        }
        if name == *active_brep {
            patch_geometry(
                &mut bytes,
                &positions,
                &lines,
                &conics,
                &degenerate_curves,
                &planes,
                &spheres,
                &tori,
                &cones,
                &body_transform_edits,
                &entity_color_edits,
                &edge_range_edits,
                &face_sense_edits,
                &coedge_sense_edits,
                &extrusion_direction_edits,
                &nurbs_surface_edits,
                &nurbs_curve_edits,
                &pcurve_edits,
                &procedural_curve_edits,
                &procedural_surface_fit_edits,
                &creation_timestamp_edits,
                &edge_continuity_edits,
                &vertex_ownership_edits,
                &face_sidedness_edits,
                &tolerant_vertex_edits,
            )?;
            patch_transform_hints(&mut bytes, &transform_hint_edits)?;
            patch_tolerant_coedge_parameters(&mut bytes, &tolerant_coedge_edits)?;
            patch_wire_topologies(&mut bytes, &wire_topology_edits)?;
            patch_edge_ownerships(&mut bytes, &edge_ownership_edits)?;
            patch_body_native_keys(&mut bytes, &body_native_key_edits.asm)?;
            if let Some(edits) = history_state_edits.get(&name) {
                patch_history_states(&mut bytes, edits)?;
            }
        } else {
            if name.ends_with(".protein") && !protein_appearance_edits.is_empty() {
                let (patched_bytes, patched_guids) =
                    crate::materials::patch_protein_appearances(&bytes, &protein_appearance_edits)?;
                bytes = patched_bytes;
                patched_protein_appearances.extend(patched_guids);
            }
            if let Some(edits) = sketch_point_edits.get(&name) {
                patch_sketch_points(&mut bytes, edits)?;
            }
            if let Some(edits) = sketch_curve_edits.get(&name) {
                patch_sketch_curves(&mut bytes, edits)?;
            }
            if let Some(edits) = sketch_relation_edits.get(&name) {
                patch_sketch_relations(&mut bytes, edits)?;
            }
            if let Some(edits) = persistent_reference_edits.get(&name) {
                patch_persistent_references(&mut bytes, edits)?;
            }
            if let Some(edits) = construction_recipe_edits.get(&name) {
                patch_construction_recipes(&mut bytes, edits)?;
            }
            if let Some(edits) = body_member_edits.get(&name) {
                patch_body_members(&mut bytes, edits)?;
            }
            if let Some(edits) = body_visibility_edits.get(&name) {
                patch_body_visibilities(&mut bytes, edits)?;
            }
            if let Some(edits) = body_native_key_edits.design.get(&name) {
                patch_design_body_keys(&mut bytes, edits)?;
            }
            if let Some(edits) = entity_header_edits.get(&name) {
                patch_entity_headers(&mut bytes, edits)?;
            }
            if let Some(edits) = design_object_edits.get(&name) {
                patch_design_objects(&mut bytes, edits)?;
            }
            if let Some(edits) = lost_edge_edits.get(&name) {
                patch_lost_edge_references(&mut bytes, edits)?;
            }
            if let Some(edits) = material_assignment_edits.get(&name) {
                patch_material_assignments(&mut bytes, edits)?;
            }
            if let Some(edits) = act_guid_edits.get(&name) {
                patch_act_guids(&mut bytes, edits)?;
            }
            if let Some(edits) = act_root_edits.get(&name) {
                patch_act_roots(&mut bytes, edits)?;
            }
            if let Some(edits) = act_entity_edits.get(&name) {
                patch_act_entities(&mut bytes, edits)?;
            }
        }
        zip.start_file(name, options)
            .map_err(|error| CodecError::Malformed(format!("cannot write F3D entry: {error}")))?;
        zip.write_all(&bytes)?;
    }
    if patched_protein_appearances.len() != protein_appearance_edits.len() {
        return Err(CodecError::NotImplemented(
            "one or more edited F3D appearances have no writable Protein carrier".into(),
        ));
    }
    let output = zip
        .finish()
        .map_err(|error| CodecError::Malformed(format!("cannot finish F3D ZIP: {error}")))?
        .into_inner();
    writer.write_all(&output)?;
    Ok(())
}

type SketchPointEdit = (u64, u32, cadmpeg_ir::math::Point2);
type PersistentReferenceEdit = (u64, u32, u64);
type BodyMemberEdit = (u64, u64, u16);

fn validate_creation_timestamp_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, f64>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map_or(&[][..], |native| native.creation_timestamps.as_slice());
    let target = target_native
        .as_ref()
        .map_or(&[][..], |native| native.creation_timestamps.as_slice());
    let by_id = baseline
        .iter()
        .map(|timestamp| (timestamp.id.as_str(), timestamp))
        .collect::<BTreeMap<_, _>>();
    if by_id
        .keys()
        .copied()
        .ne(target.iter().map(|timestamp| timestamp.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D timestamp regeneration requires the unchanged timestamp-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for timestamp in target {
        let before = by_id[timestamp.id.as_str()];
        let mut normalized = timestamp.clone();
        normalized.unix_microseconds = before.unix_microseconds;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D timestamp edit changes structural fields: {}",
                timestamp.id
            )));
        }
        if timestamp.unix_microseconds == before.unix_microseconds {
            continue;
        }
        if !timestamp.unix_microseconds.is_finite() {
            return Err(CodecError::Malformed(format!(
                "F3D creation timestamp {} is non-finite",
                timestamp.id
            )));
        }
        let record_index = usize::try_from(timestamp.record_index).map_err(|_| {
            CodecError::Malformed(format!(
                "F3D timestamp record index exceeds usize: {}",
                timestamp.id
            ))
        })?;
        edits.insert(record_index, timestamp.unix_microseconds);
    }
    Ok(edits)
}

fn validate_edge_continuity_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, (Sense, String)>, CodecError> {
    let baseline = f3d_native(baseline)?
        .map(|native| native.edge_continuities)
        .unwrap_or_default();
    let target = f3d_native(target)?
        .map(|native| native.edge_continuities)
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|metadata| (metadata.id.as_str(), metadata))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|metadata| (metadata.id.as_str(), metadata))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D edge-continuity regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.sense = before.sense;
        normalized.continuity.clone_from(&before.continuity);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D edge-continuity edit changes structural fields: {id}"
            )));
        }
        if after.continuity == before.continuity && after.sense == before.sense {
            continue;
        }
        if !matches!(after.continuity.as_str(), "tangent" | "unknown") {
            return Err(CodecError::Malformed(format!(
                "F3D edge continuity {id} has unsupported token {}",
                after.continuity
            )));
        }
        edits.insert(
            after.record_index as usize,
            (after.sense, after.continuity.clone()),
        );
    }
    Ok(edits)
}

fn validate_edge_ownership_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, i64>, CodecError> {
    let baseline = f3d_native(baseline)?
        .map(|native| native.edge_ownerships)
        .unwrap_or_default();
    let target_ownerships = f3d_native(target)?
        .map(|native| native.edge_ownerships)
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|ownership| (ownership.id.as_str(), ownership))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target_ownerships
        .iter()
        .map(|ownership| (ownership.id.as_str(), ownership))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D edge-ownership regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.owner_coedge.clone_from(&before.owner_coedge);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D edge-ownership edit changes structural fields: {id}"
            )));
        }
        if after.owner_coedge == before.owner_coedge {
            continue;
        }
        let owner = if let Some(owner) = &after.owner_coedge {
            let coedge = target
                .model
                .coedges
                .iter()
                .find(|coedge| coedge.id == *owner)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "F3D edge ownership {id} references missing coedge {owner}"
                    ))
                })?;
            if coedge.edge != after.edge {
                return Err(CodecError::Malformed(format!(
                    "F3D edge ownership {id} selects a coedge of another edge"
                )));
            }
            owner
                .as_str()
                .rsplit_once('#')
                .and_then(|(_, index)| index.parse::<i64>().ok())
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "F3D owning coedge {owner} has no native record index"
                    ))
                })?
        } else {
            -1
        };
        edits.insert(after.record_index as usize, owner);
    }
    Ok(edits)
}

fn validate_vertex_ownership_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, (i64, u8)>, CodecError> {
    let baseline = f3d_native(baseline)?
        .map(|native| native.vertex_ownerships)
        .unwrap_or_default();
    let target_native = f3d_native(target)?
        .map(|native| native.vertex_ownerships)
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|metadata| (metadata.id.as_str(), metadata))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target_native
        .iter()
        .map(|metadata| (metadata.id.as_str(), metadata))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D vertex-ownership regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.owning_edge.clone_from(&before.owning_edge);
        normalized.endpoint_index = before.endpoint_index;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D vertex-ownership edit changes structural fields: {id}"
            )));
        }
        if after.owning_edge == before.owning_edge && after.endpoint_index == before.endpoint_index
        {
            continue;
        }
        let edge = target
            .model
            .edges
            .iter()
            .find(|edge| edge.id == after.owning_edge)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "F3D vertex ownership {id} references missing edge {}",
                    after.owning_edge
                ))
            })?;
        let valid = match after.endpoint_index {
            0 => edge.start == after.vertex,
            1 => edge.end == after.vertex,
            _ => false,
        };
        if !valid {
            return Err(CodecError::Malformed(format!(
                "F3D vertex ownership {id} has an inconsistent endpoint slot"
            )));
        }
        let edge_record = after
            .owning_edge
            .as_str()
            .rsplit_once('#')
            .and_then(|(_, index)| index.parse::<i64>().ok())
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "F3D owning edge {} has no native record index",
                    after.owning_edge
                ))
            })?;
        edits.insert(
            after.record_index as usize,
            (edge_record, after.endpoint_index),
        );
    }
    Ok(edits)
}

fn validate_face_sidedness_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, crate::records::FaceContainment>, CodecError> {
    let baseline = f3d_native(baseline)?
        .map(|native| native.face_sidedness)
        .unwrap_or_default();
    let target = f3d_native(target)?
        .map(|native| native.face_sidedness)
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|metadata| (metadata.id.as_str(), metadata))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|metadata| (metadata.id.as_str(), metadata))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D face-sidedness regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.containment = before.containment;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D face-sidedness edit changes structural fields: {id}"
            )));
        }
        if after.containment == before.containment {
            continue;
        }
        match (before.containment, after.containment) {
            (Some(_), Some(containment)) => {
                edits.insert(after.record_index as usize, containment);
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "F3D face sidedness {id} cannot change record width"
                )));
            }
        }
    }
    Ok(edits)
}

fn validate_tolerant_vertex_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, (f64, [f32; 2])>, CodecError> {
    let baseline_vertices = baseline
        .model
        .vertices
        .iter()
        .map(|vertex| (vertex.id.as_str(), vertex))
        .collect::<BTreeMap<_, _>>();
    let target_vertices = target
        .model
        .vertices
        .iter()
        .map(|vertex| (vertex.id.as_str(), vertex))
        .collect::<BTreeMap<_, _>>();
    if baseline_vertices.keys().ne(target_vertices.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D tolerant-vertex regeneration requires the unchanged vertex-id set".into(),
        ));
    }
    for (id, before) in &baseline_vertices {
        let after = target_vertices[id];
        let mut normalized = after.clone();
        normalized.tolerance = before.tolerance;
        if &normalized != *before {
            return Err(CodecError::NotImplemented(format!(
                "F3D vertex edit changes fields other than tolerance: {id}"
            )));
        }
        if after.tolerance != before.tolerance
            && (before.tolerance.is_none() || after.tolerance.is_none())
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D vertex tolerance {id} cannot change record width"
            )));
        }
    }
    let baseline_tails = f3d_native(baseline)?
        .map(|native| native.tolerant_vertex_tails)
        .unwrap_or_default();
    let target_tails = f3d_native(target)?
        .map(|native| native.tolerant_vertex_tails)
        .unwrap_or_default();
    let baseline_by_id = baseline_tails
        .iter()
        .map(|tail| (tail.id.as_str(), tail))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target_tails
        .iter()
        .map(|tail| (tail.id.as_str(), tail))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D tolerant-vertex regeneration requires the unchanged tail-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.trailing_floats = before.trailing_floats;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D tolerant-vertex tail edit changes structural fields: {id}"
            )));
        }
        let tolerance = target_vertices[after.vertex.as_str()]
            .tolerance
            .ok_or_else(|| {
                CodecError::Malformed(format!("tolerant vertex {id} has no tolerance"))
            })?;
        if !tolerance.is_finite()
            || tolerance < 0.0
            || after.trailing_floats.iter().any(|value| !value.is_finite())
        {
            return Err(CodecError::Malformed(format!(
                "F3D tolerant vertex {id} has non-finite fields"
            )));
        }
        if tolerance
            != baseline_vertices[after.vertex.as_str()]
                .tolerance
                .unwrap_or(tolerance)
            || after.trailing_floats != before.trailing_floats
        {
            edits.insert(
                after.record_index as usize,
                (tolerance / 10.0, after.trailing_floats),
            );
        }
    }
    Ok(edits)
}

fn validate_tolerant_coedge_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, [f64; 2]>, CodecError> {
    let baseline_parameters = f3d_native(baseline)?
        .map(|native| native.tolerant_coedge_parameters)
        .unwrap_or_default();
    let target_parameters = f3d_native(target)?
        .map(|native| native.tolerant_coedge_parameters)
        .unwrap_or_default();
    let baseline_by_id = baseline_parameters
        .iter()
        .map(|parameters| (parameters.id.as_str(), parameters))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target_parameters
        .iter()
        .map(|parameters| (parameters.id.as_str(), parameters))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D tolerant-coedge regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.parameter_range = before.parameter_range;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D tolerant-coedge edit changes structural fields: {id}"
            )));
        }
        if after.parameter_range.iter().any(|value| !value.is_finite()) {
            return Err(CodecError::Malformed(format!(
                "F3D tolerant coedge {id} has non-finite parameters"
            )));
        }
        if after.parameter_range != before.parameter_range {
            edits.insert(after.record_index as usize, after.parameter_range);
        }
    }
    Ok(edits)
}

fn validate_wire_topology_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, crate::records::WireSide>, CodecError> {
    let baseline_wires = f3d_native(baseline)?
        .map(|native| native.wire_topologies)
        .unwrap_or_default();
    let target_wires = f3d_native(target)?
        .map(|native| native.wire_topologies)
        .unwrap_or_default();
    let baseline_by_id = baseline_wires
        .iter()
        .map(|wire| (wire.id.as_str(), wire))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target_wires
        .iter()
        .map(|wire| (wire.id.as_str(), wire))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D wire regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.side = before.side;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D wire edit changes structural fields: {id}"
            )));
        }
        if after.side != before.side {
            edits.insert(after.record_index as usize, after.side);
        }
    }
    Ok(edits)
}

enum ProceduralSurfaceEdit {
    Extrusion {
        parameter_interval: [f64; 2],
        direction: Vector3,
        native_position: Point3,
    },
    BlendRadii([f64; 2]),
}

struct NurbsSurfaceEdit {
    surface: NurbsSurface,
    periodic: Option<[bool; 2]>,
}

struct NurbsCurveEdit {
    curve: NurbsCurve,
    periodic: Option<bool>,
}

#[derive(Clone)]
struct NurbsPcurveEdit {
    geometry: PcurveGeometry,
    periodic: Option<bool>,
    wrapper_reversed: Option<bool>,
    native_tail_flags: Option<[bool; 4]>,
    parameter_range: Option<[f64; 2]>,
    fit_tolerance: Option<f64>,
}

#[derive(Clone)]
struct ProceduralCurveEdit {
    definition: Option<cadmpeg_ir::geometry::ProceduralCurveDefinition>,
    fit_tolerance: Option<f64>,
}

fn validate_material_assignment_appearances(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, crate::materials::ProteinAppearanceEdit>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline_appearances = baseline
        .model
        .appearances
        .iter()
        .map(|appearance| (appearance.id.as_str(), appearance))
        .collect::<BTreeMap<_, _>>();
    let target_appearances = target
        .model
        .appearances
        .iter()
        .map(|appearance| (appearance.id.as_str(), appearance))
        .collect::<BTreeMap<_, _>>();
    if baseline_appearances.keys().ne(target_appearances.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D material regeneration requires the unchanged appearance-id set".into(),
        ));
    }
    let target_assignments = target_native
        .as_ref()
        .map(|native| &native.design_material_assignments[..])
        .unwrap_or_default();
    let baseline_assignments = baseline_native
        .as_ref()
        .map(|native| &native.design_material_assignments[..])
        .unwrap_or_default();
    let mut appearance_edits = BTreeMap::new();
    for (id, before) in baseline_appearances {
        let after = target_appearances[id];
        let mut normalized = after.clone();
        normalized.physical_token.clone_from(&before.physical_token);
        normalized.base_color = before.base_color;
        normalized.properties.clone_from(&before.properties);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D appearance edit changes fields outside physical token or Protein values: {id}"
            )));
        }
        if before.properties.keys().ne(after.properties.keys()) {
            return Err(CodecError::NotImplemented(format!(
                "F3D Protein property regeneration requires the unchanged property set: {id}"
            )));
        }
        let mut edit = crate::materials::ProteinAppearanceEdit::default();
        if after.base_color != before.base_color {
            let color = after.base_color.ok_or_else(|| {
                CodecError::NotImplemented(format!("cannot remove F3D appearance color: {id}"))
            })?;
            if before.base_color.is_none()
                || ![color.r, color.g, color.b, color.a]
                    .into_iter()
                    .all(|component| component.is_finite() && (0.0..=1.0).contains(&component))
                || color.a != 1.0
            {
                return Err(CodecError::Malformed(format!(
                    "F3D Protein color {id} must replace an existing opaque finite RGBA color"
                )));
            }
            edit.color = Some(color);
        }
        for (name, before_value) in &before.properties {
            let after_value = after.properties[name];
            if after_value == *before_value {
                continue;
            }
            let valid = after_value.is_finite()
                && match name.as_str() {
                    "reflectivity_at_0deg" | "surface_roughness" => {
                        (0.0..=1.0).contains(&after_value)
                    }
                    "refraction_index" => (1.0..=4.0).contains(&after_value),
                    _ => false,
                };
            if !valid {
                return Err(CodecError::Malformed(format!(
                    "F3D Protein property {id}.{name} is outside its writable range"
                )));
            }
            edit.properties.insert(name.clone(), after_value);
        }
        if edit.color.is_some() || !edit.properties.is_empty() {
            let guid = after.visual_guid.clone().ok_or_else(|| {
                CodecError::NotImplemented(format!("F3D appearance {id} has no visual GUID"))
            })?;
            appearance_edits.insert(guid, edit);
        }
        if after.physical_token == before.physical_token {
            continue;
        }
        let synchronized = target_assignments.iter().any(|assignment| {
            after.visual_guid.as_deref() == Some(assignment.visual_guid.as_str())
                && after.physical_token == assignment.physical_token
        });
        if !synchronized {
            return Err(CodecError::NotImplemented(format!(
                "F3D appearance {id} changed without a synchronized material assignment"
            )));
        }
    }
    for before in baseline_assignments {
        let Some(after) = target_assignments
            .iter()
            .find(|assignment| assignment.id == before.id)
        else {
            continue;
        };
        if after.physical_token != before.physical_token
            && !target.model.appearances.iter().any(|appearance| {
                appearance.visual_guid.as_deref() == Some(after.visual_guid.as_str())
                    && appearance.physical_token == after.physical_token
            })
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D material assignment {} changed without its appearance physical token",
                before.id
            )));
        }
    }
    Ok(appearance_edits)
}

fn validate_material_assignment_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<DesignMaterialAssignment>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.design_material_assignments[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.design_material_assignments[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|assignment| (assignment.id.as_str(), assignment))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|assignment| (assignment.id.as_str(), assignment))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D material-assignment regeneration requires the unchanged assignment-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<DesignMaterialAssignment>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.entity_id.clone_from(&before.entity_id);
        normalized.entity_suffix = before.entity_suffix;
        normalized.visual_guid.clone_from(&before.visual_guid);
        normalized.physical_token.clone_from(&before.physical_token);
        normalized.visual_preset.clone_from(&before.visual_preset);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D material-assignment edit changes fields outside writable strings: {id}"
            )));
        }
        if after == before {
            continue;
        }
        let suffix = after
            .entity_id
            .rsplit_once('_')
            .and_then(|(_, suffix)| suffix.parse::<u64>().ok())
            .ok_or_else(|| CodecError::Malformed(format!("invalid assignment entity id: {id}")))?;
        if suffix != after.entity_suffix {
            return Err(CodecError::Malformed(format!(
                "F3D assignment entity id and suffix disagree: {id}"
            )));
        }
        validate_utf16_replacement(id, &before.entity_id, &after.entity_id)?;
        validate_utf16_replacement(id, &before.visual_guid, &after.visual_guid)?;
        validate_optional_utf16_replacement(
            id,
            before.physical_token.as_deref(),
            after.physical_token.as_deref(),
        )?;
        validate_optional_utf16_replacement(
            id,
            before.visual_preset.as_deref(),
            after.visual_preset.as_deref(),
        )?;
        edits
            .entry(native_stream(id, ":material-assignment#")?)
            .or_default()
            .push(after.clone());
    }
    Ok(edits)
}

fn validate_utf16_replacement(id: &str, before: &str, after: &str) -> Result<(), CodecError> {
    if before.encode_utf16().count() != after.encode_utf16().count() {
        return Err(CodecError::NotImplemented(format!(
            "F3D native string {id} must retain its UTF-16 length"
        )));
    }
    Ok(())
}

fn validate_optional_utf16_replacement(
    id: &str,
    before: Option<&str>,
    after: Option<&str>,
) -> Result<(), CodecError> {
    match (before, after) {
        (Some(before), Some(after)) => validate_utf16_replacement(id, before, after),
        (None, None) => Ok(()),
        _ => Err(CodecError::NotImplemented(format!(
            "cannot add or remove F3D native string carrier: {id}"
        ))),
    }
}

fn patch_material_assignments(
    bytes: &mut [u8],
    edits: &[DesignMaterialAssignment],
) -> Result<(), CodecError> {
    for assignment in edits {
        let suffix_start = usize::try_from(assignment.entity_suffix_offset).map_err(|_| {
            CodecError::Malformed("material-assignment suffix offset exceeds address space".into())
        })?;
        bytes
            .get_mut(suffix_start..suffix_start + 8)
            .ok_or_else(|| CodecError::Malformed("material-assignment suffix is truncated".into()))?
            .copy_from_slice(&assignment.entity_suffix.to_le_bytes());
        patch_utf16_if_changed(
            bytes,
            assignment.entity_id_offset,
            &assignment.entity_id,
            "material-assignment entity id",
        )?;
        patch_utf16_if_changed(
            bytes,
            assignment.visual_guid_offset,
            &assignment.visual_guid,
            "material-assignment visual GUID",
        )?;
        if let (Some(offset), Some(value)) = (
            assignment.physical_token_offset,
            assignment.physical_token.as_deref(),
        ) {
            patch_utf16_if_changed(bytes, offset, value, "material-assignment physical token")?;
        }
        if let (Some(offset), Some(value)) = (
            assignment.visual_preset_offset,
            assignment.visual_preset.as_deref(),
        ) {
            patch_utf16_if_changed(bytes, offset, value, "material-assignment visual preset")?;
        }
    }
    Ok(())
}

fn validate_lost_edge_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<LostEdgeReference>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.lost_edge_references[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.lost_edge_references[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|reference| (reference.id.as_str(), reference))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|reference| (reference.id.as_str(), reference))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D lost-edge regeneration requires the unchanged reference-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<LostEdgeReference>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.class_tag.clone_from(&before.class_tag);
        normalized.record_index = before.record_index;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D lost-edge edit changes fields outside its fixed payload: {id}"
            )));
        }
        if after == before {
            continue;
        }
        if after.class_tag.len() != 3 || !after.class_tag.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(CodecError::Malformed(format!(
                "F3D lost-edge class tag must contain three digits: {id}"
            )));
        }
        edits
            .entry(native_stream(id, ":lost-edge-reference#")?)
            .or_default()
            .push(after.clone());
    }
    Ok(edits)
}

fn patch_lost_edge_references(
    bytes: &mut [u8],
    edits: &[LostEdgeReference],
) -> Result<(), CodecError> {
    for reference in edits {
        patch_bytes_at(
            bytes,
            reference.class_tag_offset,
            reference.class_tag.as_bytes(),
            "lost-edge class tag",
        )?;
        patch_u32_at(
            bytes,
            reference.record_index_offset,
            reference.record_index,
            "lost-edge record index",
        )?;
    }
    Ok(())
}
type ActGuidEdit = (u64, Vec<u8>);

fn validate_act_appearance_bindings(baseline: &CadIr, target: &CadIr) -> Result<(), CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline_entities = baseline_native
        .as_ref()
        .map(|native| &native.act_entities[..])
        .unwrap_or_default();
    let target_entities = target_native
        .as_ref()
        .map(|native| &native.act_entities[..])
        .unwrap_or_default();
    if baseline.model.appearance_bindings.len() != target.model.appearance_bindings.len() {
        return Err(CodecError::NotImplemented(
            "F3D ACT regeneration requires the unchanged appearance-binding count".into(),
        ));
    }
    let target_bindings = target
        .model
        .appearance_bindings
        .iter()
        .map(|binding| {
            (
                (binding.target.clone(), binding.appearance.clone()),
                binding,
            )
        })
        .collect::<HashMap<_, _>>();
    let mut baseline_entities_by_source = HashMap::<_, Vec<_>>::new();
    for entity in baseline_entities {
        baseline_entities_by_source
            .entry(entity.entity_id.as_str())
            .or_default()
            .push(entity);
    }
    let target_entities_by_id = target_entities
        .iter()
        .map(|entity| (entity.id.as_str(), entity))
        .collect::<HashMap<_, _>>();
    for before in &baseline.model.appearance_bindings {
        let after = target_bindings
            .get(&(before.target.clone(), before.appearance.clone()))
            .copied()
            .ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "F3D appearance binding target or appearance changed: {}",
                    before.id
                ))
            })?;
        let mut normalized = after.clone();
        normalized.id.clone_from(&before.id);
        normalized
            .source_entity_id
            .clone_from(&before.source_entity_id);
        normalized.channels.clone_from(&before.channels);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D appearance binding edit changes fields outside its derived identity, source, or channels: {}",
                before.id
            )));
        }
        if after == before {
            continue;
        }
        let before_entity = before
            .source_entity_id
            .as_deref()
            .and_then(|source| baseline_entities_by_source.get(source))
            .and_then(|entities| {
                entities
                    .iter()
                    .copied()
                    .find(|entity| before.channels == entity.channels)
            });
        let after_entity = before_entity.and_then(|before_entity| {
            target_entities_by_id
                .get(before_entity.id.as_str())
                .copied()
                .filter(|entity| after.channels == entity.channels)
        });
        if before_entity.is_none() || after_entity.is_none() {
            return Err(CodecError::NotImplemented(format!(
                "F3D appearance binding {} must remain synchronized with one ACT entity",
                before.id
            )));
        }
        if after.source_entity_id != before.source_entity_id
            && after
                .source_entity_id
                .as_deref()
                .is_none_or(|source| !after.id.contains(source))
        {
            return Err(CodecError::Malformed(format!(
                "F3D appearance binding id does not contain its changed source entity: {}",
                after.id
            )));
        }
    }
    let derived_bindings = baseline
        .model
        .appearance_bindings
        .iter()
        .filter_map(|binding| {
            Some((
                binding.source_entity_id.clone()?,
                binding.channels.clone(),
                binding,
            ))
        })
        .fold(HashMap::<_, Vec<_>>::new(), |mut grouped, entry| {
            grouped.entry(entry.0).or_default().push((entry.1, entry.2));
            grouped
        });
    let assignment_entities = target_native
        .as_ref()
        .into_iter()
        .flat_map(|native| &native.design_material_assignments)
        .map(|assignment| assignment.entity_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    for (before, after) in baseline_entities.iter().zip(target_entities) {
        let matching_bindings = derived_bindings.get(&before.entity_id);
        let derived_binding = matching_bindings.is_some_and(|bindings| {
            bindings
                .iter()
                .any(|(channels, _)| channels == &before.channels)
        });
        let assignment_synchronized = assignment_entities.contains(after.entity_id.as_str());
        if before.entity_id != after.entity_id && derived_binding && !assignment_synchronized {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT entity {} changed without its material-assignment carrier",
                before.id
            )));
        }
        let synchronized = matching_bindings.is_some_and(|bindings| {
            bindings.iter().any(|(_, before_binding)| {
                target_bindings
                    .get(&(
                        before_binding.target.clone(),
                        before_binding.appearance.clone(),
                    ))
                    .is_some_and(|binding| {
                        binding.source_entity_id.as_deref() == Some(after.entity_id.as_str())
                            && binding.channels == after.channels
                    })
            })
        });
        if (before.entity_id != after.entity_id || before.channels != after.channels)
            && derived_binding
            && !synchronized
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT entity {} changed without a synchronized appearance binding",
                before.id
            )));
        }
    }
    Ok(())
}

fn validate_act_entity_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<ActEntity>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.act_entities[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.act_entities[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|entity| (entity.id.as_str(), entity))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|entity| (entity.id.as_str(), entity))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D ACT entity regeneration requires the unchanged entity-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<ActEntity>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.entity_id.clone_from(&before.entity_id);
        normalized.channels.clone_from(&before.channels);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT entity edit changes fields other than entity_id or channel GUIDs: {id}"
            )));
        }
        if after == before {
            continue;
        }
        if after.entity_id.encode_utf16().count() != before.entity_id.encode_utf16().count() {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT entity id {id} must retain its UTF-16 length"
            )));
        }
        if after.channels.keys().ne(before.channels.keys())
            || after.channels.keys().ne(after.channel_guid_offsets.keys())
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT entity {id} must retain its channel set and offsets"
            )));
        }
        for (name, guid) in &after.channels {
            let before_guid = &before.channels[name];
            if guid.encode_utf16().count() != before_guid.encode_utf16().count()
                || !canonical_guid(guid)
            {
                return Err(CodecError::Malformed(format!(
                    "F3D ACT channel {name} on {id} must be a same-length canonical GUID"
                )));
            }
        }
        edits
            .entry(native_stream(id, ":act-entity#")?)
            .or_default()
            .push(after.clone());
    }
    Ok(edits)
}

fn patch_act_entities(bytes: &mut [u8], edits: &[ActEntity]) -> Result<(), CodecError> {
    for entity in edits {
        let encoded_id = entity
            .entity_id
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        for offset in [
            entity.table_entity_id_offset,
            entity.channel_entity_id_offset,
        ]
        .into_iter()
        .flatten()
        {
            patch_bytes_at(bytes, offset, &encoded_id, "ACT entity id")?;
        }
        for (name, guid) in &entity.channels {
            let encoded = guid
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>();
            patch_bytes_at(
                bytes,
                entity.channel_guid_offsets[name],
                &encoded,
                "ACT channel GUID",
            )?;
        }
    }
    Ok(())
}

fn validate_act_guid_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<ActGuidEdit>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.act_guids[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.act_guids[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|guid| (guid.id.as_str(), guid))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|guid| (guid.id.as_str(), guid))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D ACT GUID regeneration requires the unchanged GUID-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<ActGuidEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized: ActGuid = after.clone();
        normalized.guid.clone_from(&before.guid);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT GUID edit changes fields other than guid: {id}"
            )));
        }
        if after.guid == before.guid {
            continue;
        }
        if after.guid.encode_utf16().count() != before.guid.encode_utf16().count() {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT GUID {id} must retain its UTF-16 length"
            )));
        }
        if !canonical_guid(&after.guid) {
            return Err(CodecError::Malformed(format!(
                "F3D ACT GUID {id} is not canonical"
            )));
        }
        let encoded = after
            .guid
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>();
        let stream = native_stream(id, ":act-guid#")?;
        edits
            .entry(stream)
            .or_default()
            .push((after.guid_offset, encoded));
    }
    Ok(edits)
}

fn patch_act_guids(bytes: &mut [u8], edits: &[ActGuidEdit]) -> Result<(), CodecError> {
    for (offset, encoded) in edits {
        patch_bytes_at(bytes, *offset, encoded, "ACT GUID")?;
    }
    Ok(())
}

fn validate_act_root_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<ActRootComponent>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.act_root_components[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.act_root_components[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|root| (root.id.as_str(), root))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|root| (root.id.as_str(), root))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D ACT root regeneration requires the unchanged root-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<ActRootComponent>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.record_index = before.record_index;
        normalized.instance_root_record = before.instance_root_record;
        normalized.components_root_record = before.components_root_record;
        normalized.registry_flag = before.registry_flag;
        normalized.entity_id.clone_from(&before.entity_id);
        normalized.display_name.clone_from(&before.display_name);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT root edit changes fields outside fixed numeric graph links: {id}"
            )));
        }
        if after == before {
            continue;
        }
        if after.registry_flag > 1 {
            return Err(CodecError::Malformed(format!(
                "F3D ACT root registry flag must be zero or one: {id}"
            )));
        }
        if after.entity_id.encode_utf16().count() != before.entity_id.encode_utf16().count()
            || after.display_name.encode_utf16().count()
                != before.display_name.encode_utf16().count()
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D ACT root strings must retain their UTF-16 lengths: {id}"
            )));
        }
        edits
            .entry(native_stream(id, ":act-root-component#")?)
            .or_default()
            .push(after.clone());
    }
    Ok(edits)
}

fn patch_act_roots(bytes: &mut [u8], edits: &[ActRootComponent]) -> Result<(), CodecError> {
    for root in edits {
        for (offset, value, field) in [
            (
                root.record_index_offset,
                root.record_index,
                "ACT root record index",
            ),
            (
                root.instance_root_record_offset,
                root.instance_root_record,
                "ACT instance-root reference",
            ),
            (
                root.components_root_record_offset,
                root.components_root_record,
                "ACT components-root reference",
            ),
            (
                root.registry_flag_offset,
                root.registry_flag,
                "ACT registry flag",
            ),
        ] {
            patch_u32_at(bytes, offset, value, field)?;
        }
        patch_utf16_if_changed(
            bytes,
            root.entity_id_offset,
            &root.entity_id,
            "ACT root entity id",
        )?;
        patch_utf16_if_changed(
            bytes,
            root.display_name_offset,
            &root.display_name,
            "ACT root display name",
        )?;
    }
    Ok(())
}

fn patch_utf16_if_changed(
    bytes: &mut [u8],
    offset: u64,
    value: &str,
    field: &str,
) -> Result<(), CodecError> {
    let encoded = value
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    patch_bytes_at(bytes, offset, &encoded, field)
}

fn canonical_guid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn native_stream(id: &str, delimiter: &str) -> Result<String, CodecError> {
    id.strip_prefix("f3d:")
        .and_then(|id| id.rsplit_once(delimiter))
        .map(|(stream, _)| stream.to_owned())
        .ok_or_else(|| CodecError::Malformed(format!("invalid native record id {id}")))
}

fn patch_bytes_at(
    bytes: &mut [u8],
    offset: u64,
    encoded: &[u8],
    field: &str,
) -> Result<(), CodecError> {
    let start = usize::try_from(offset)
        .map_err(|_| CodecError::Malformed(format!("{field} offset exceeds address space")))?;
    bytes
        .get_mut(start..start + encoded.len())
        .ok_or_else(|| CodecError::Malformed(format!("{field} is truncated")))?
        .copy_from_slice(encoded);
    Ok(())
}

struct DesignObjectEdit {
    integers: Vec<(u64, Vec<u8>)>,
    strings: Vec<(u64, Vec<u8>)>,
}

fn validate_design_object_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<DesignObjectEdit>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.design_objects[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.design_objects[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|object| (object.id.as_str(), object))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|object| (object.id.as_str(), object))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D design-object regeneration requires the unchanged object-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<DesignObjectEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.entity_ids.clone_from(&before.entity_ids);
        normalized.self_guid.clone_from(&before.self_guid);
        normalized.parent_guid.clone_from(&before.parent_guid);
        normalized.revision = before.revision;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D design-object edit changes fields outside its fixed object payload: {id}"
            )));
        }
        if after.entity_ids.len() != before.entity_ids.len()
            || after.entity_ids.len() != after.entity_id_offsets.len()
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D design object {id} must retain its entity-id cardinality"
            )));
        }
        let mut integers = after
            .entity_ids
            .iter()
            .zip(&before.entity_ids)
            .zip(&after.entity_id_offsets)
            .filter(|((value, before), _)| value != before)
            .map(|((&value, _), &offset)| (offset, value.to_le_bytes().to_vec()))
            .collect::<Vec<_>>();
        if after.revision != before.revision {
            integers.push((after.revision_offset, after.revision.to_le_bytes().to_vec()));
        }
        let mut strings = Vec::new();
        if after.self_guid != before.self_guid {
            validate_fixed_design_string(id, &before.self_guid, &after.self_guid)?;
            strings.push((after.self_guid_offset, after.self_guid.as_bytes().to_vec()));
        }
        if after.parent_guid != before.parent_guid {
            let before_parent = before.parent_guid.as_deref().ok_or_else(|| {
                CodecError::NotImplemented(format!("cannot add F3D object parent GUID: {id}"))
            })?;
            let after_parent = after.parent_guid.as_deref().ok_or_else(|| {
                CodecError::NotImplemented(format!("cannot remove F3D object parent GUID: {id}"))
            })?;
            validate_fixed_design_string(id, before_parent, after_parent)?;
            strings.push((
                after.parent_guid_offset.ok_or_else(|| {
                    CodecError::Malformed(format!("F3D object {id} has no parent-GUID offset"))
                })?,
                after_parent.as_bytes().to_vec(),
            ));
        }
        if integers.is_empty() && strings.is_empty() {
            continue;
        }
        let stream = id
            .strip_prefix("f3d:")
            .and_then(|id| id.rsplit_once(":design-object#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| CodecError::Malformed(format!("invalid design-object id {id}")))?;
        edits
            .entry(stream)
            .or_default()
            .push(DesignObjectEdit { integers, strings });
    }
    Ok(edits)
}

fn validate_fixed_design_string(id: &str, before: &str, after: &str) -> Result<(), CodecError> {
    if before.len() != after.len()
        || !after
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(CodecError::NotImplemented(format!(
            "F3D object string {id} must retain its encoded length and alphabet"
        )));
    }
    Ok(())
}

fn validate_configuration_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<u8>>, CodecError> {
    let baseline = f3d_native(baseline)?
        .map(|native| native.design_configurations)
        .unwrap_or_default();
    let target = f3d_native(target)?
        .map(|native| native.design_configurations)
        .unwrap_or_default();
    let baseline = baseline
        .iter()
        .map(|configuration| (configuration.entry_name.as_str(), configuration))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|configuration| (configuration.entry_name.as_str(), configuration))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "retained F3D configuration editing requires the unchanged entry-name set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (name, before) in baseline {
        let after = target[name];
        if before.id != after.id || before.kind != after.kind {
            return Err(CodecError::NotImplemented(format!(
                "retained F3D configuration edit changes entry identity: {name}"
            )));
        }
        if before.payload != after.payload {
            edits.insert(
                name.to_owned(),
                serde_json::to_vec(&after.payload).map_err(|error| {
                    CodecError::Malformed(format!(
                        "cannot encode retained F3D configuration {name}: {error}"
                    ))
                })?,
            );
        }
    }
    Ok(edits)
}

fn patch_design_objects(bytes: &mut [u8], edits: &[DesignObjectEdit]) -> Result<(), CodecError> {
    for edit in edits {
        for (offset, encoded) in edit.integers.iter().chain(&edit.strings) {
            let start = usize::try_from(*offset).map_err(|_| {
                CodecError::Malformed("design-object offset exceeds address space".into())
            })?;
            bytes
                .get_mut(start..start + encoded.len())
                .ok_or_else(|| CodecError::Malformed("design-object field is truncated".into()))?
                .copy_from_slice(encoded);
        }
    }
    Ok(())
}

struct EntityHeaderEdit {
    record_reference: Option<(u64, u32)>,
    references: Vec<(u64, u32)>,
}

fn validate_entity_header_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<EntityHeaderEdit>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.design_entity_headers[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.design_entity_headers[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|header| (header.id.as_str(), header))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|header| (header.id.as_str(), header))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D entity-header regeneration requires the unchanged header-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<EntityHeaderEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.record_reference = before.record_reference;
        normalized
            .reference_indices
            .clone_from(&before.reference_indices);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D entity-header edit changes fields outside fixed record references: {id}"
            )));
        }
        if after.record_reference == before.record_reference
            && after.reference_indices == before.reference_indices
        {
            continue;
        }
        if after.reference_indices.len() != after.reference_offsets.len() {
            return Err(CodecError::Malformed(format!(
                "F3D entity header {id} has mismatched reference values and offsets"
            )));
        }
        let record_reference = if after.record_reference == before.record_reference {
            None
        } else {
            Some((
                after.record_reference_offset.ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "F3D entity header {id} has no writable owning-record reference"
                    ))
                })?,
                after.record_reference.ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "cannot remove F3D entity-header record reference: {id}"
                    ))
                })?,
            ))
        };
        let references = after
            .reference_offsets
            .iter()
            .copied()
            .zip(after.reference_indices.iter().copied())
            .zip(&before.reference_indices)
            .filter_map(|((offset, value), before)| (value != *before).then_some((offset, value)))
            .collect();
        let stream = id
            .strip_prefix("f3d:")
            .and_then(|id| id.rsplit_once(":design-entity-header#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::Malformed(format!("invalid design-entity-header id {id}"))
            })?;
        edits.entry(stream).or_default().push(EntityHeaderEdit {
            record_reference,
            references,
        });
    }
    Ok(edits)
}

fn patch_entity_headers(bytes: &mut [u8], edits: &[EntityHeaderEdit]) -> Result<(), CodecError> {
    for edit in edits {
        if let Some((offset, value)) = edit.record_reference {
            patch_u32_at(bytes, offset, value, "entity-header record reference")?;
        }
        for &(offset, value) in &edit.references {
            patch_u32_at(bytes, offset, value, "entity-header child reference")?;
        }
    }
    Ok(())
}

fn patch_u32_at(bytes: &mut [u8], offset: u64, value: u32, field: &str) -> Result<(), CodecError> {
    let start = usize::try_from(offset)
        .map_err(|_| CodecError::Malformed(format!("{field} offset exceeds address space")))?;
    bytes
        .get_mut(start..start + 4)
        .ok_or_else(|| CodecError::Malformed(format!("{field} is truncated")))?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn validate_body_member_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<BodyMemberEdit>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.design_body_members[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.design_body_members[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|member| (member.id.as_str(), member))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|member| (member.id.as_str(), member))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D body-membership regeneration requires the unchanged member-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<BodyMemberEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.entity_suffix = before.entity_suffix;
        normalized.flags = before.flags;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D body-member edit changes fields outside its fixed payload: {id}"
            )));
        }
        if after.entity_suffix == before.entity_suffix && after.flags == before.flags {
            continue;
        }
        let stream = id
            .strip_prefix("f3d:")
            .and_then(|id| id.rsplit_once(":design-body-member#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| CodecError::Malformed(format!("invalid design-body-member id {id}")))?;
        edits.entry(stream).or_default().push((
            after.byte_offset,
            after.entity_suffix,
            after.flags,
        ));
    }
    Ok(edits)
}

fn patch_body_members(bytes: &mut [u8], edits: &[BodyMemberEdit]) -> Result<(), CodecError> {
    for &(offset, entity_suffix, flags) in edits {
        let start = usize::try_from(offset).map_err(|_| {
            CodecError::Malformed("design-body-member offset exceeds address space".into())
        })?;
        if bytes.get(start) != Some(&1) {
            return Err(CodecError::Malformed(format!(
                "design-body-member at byte {start} has no presence marker"
            )));
        }
        bytes
            .get_mut(start + 1..start + 9)
            .ok_or_else(|| CodecError::Malformed("design-body-member id is truncated".into()))?
            .copy_from_slice(&entity_suffix.to_le_bytes());
        bytes
            .get_mut(start + 9..start + 11)
            .ok_or_else(|| CodecError::Malformed("design-body-member flags are truncated".into()))?
            .copy_from_slice(&flags.to_le_bytes());
    }
    Ok(())
}

fn validate_body_visibility_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<(u64, bool)>>, CodecError> {
    let baseline_bodies = baseline
        .model
        .bodies
        .iter()
        .map(|body| (&body.id, body))
        .collect::<BTreeMap<_, _>>();
    let target_bodies = target
        .model
        .bodies
        .iter()
        .map(|body| (&body.id, body))
        .collect::<BTreeMap<_, _>>();
    if baseline_bodies.keys().ne(target_bodies.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D body-visibility regeneration requires the unchanged body-id set".into(),
        ));
    }
    let metadata = f3d_native(baseline)?
        .map(|native| {
            native
                .body_visibilities
                .into_iter()
                .map(|item| (item.body.clone(), item))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let mut edits = BTreeMap::<String, Vec<(u64, bool)>>::new();
    for (id, before) in baseline_bodies {
        let after = target_bodies[id];
        if after.visible == before.visible {
            continue;
        }
        let visible = after.visible.ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove F3D body visibility: {id}"))
        })?;
        let record = metadata.get(id).ok_or_else(|| {
            CodecError::NotImplemented(format!(
                "F3D body visibility {id} has no joined Design browser node"
            ))
        })?;
        edits
            .entry(record.stream.clone())
            .or_default()
            .push((record.byte_offset, visible));
    }
    Ok(edits)
}

fn patch_body_visibilities(bytes: &mut [u8], edits: &[(u64, bool)]) -> Result<(), CodecError> {
    for &(offset, visible) in edits {
        let at = usize::try_from(offset).map_err(|_| {
            CodecError::Malformed("body-visibility offset exceeds address space".into())
        })?;
        let flag = bytes
            .get_mut(at)
            .filter(|flag| **flag <= 1)
            .ok_or_else(|| {
                CodecError::Malformed("body-visibility flag is missing or invalid".into())
            })?;
        *flag = u8::from(!visible);
    }
    Ok(())
}

fn validate_transform_hint_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<usize, [bool; 3]>, CodecError> {
    let baseline = f3d_native(baseline)?
        .map(|native| native.transform_hints)
        .unwrap_or_default();
    let target = f3d_native(target)?
        .map(|native| native.transform_hints)
        .unwrap_or_default();
    let baseline = baseline
        .iter()
        .map(|hints| (hints.id.as_str(), hints))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|hints| (hints.id.as_str(), hints))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D transform-hint regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.rotation = before.rotation;
        normalized.reflection = before.reflection;
        normalized.shear = before.shear;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D transform-hint edit changes structural fields: {id}"
            )));
        }
        let flags = [after.rotation, after.reflection, after.shear];
        if flags != [before.rotation, before.reflection, before.shear] {
            edits.insert(after.record_index as usize, flags);
        }
    }
    Ok(edits)
}

fn validate_body_native_key_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BodyNativeKeyEdits, CodecError> {
    let baseline_native = f3d_native(baseline)?.unwrap_or_default();
    let target_native = f3d_native(target)?.unwrap_or_default();
    let baseline = baseline_native
        .body_native_keys
        .iter()
        .map(|key| (key.id.as_str(), key))
        .collect::<BTreeMap<_, _>>();
    let target = target_native
        .body_native_keys
        .iter()
        .map(|key| (key.id.as_str(), key))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D body-key regeneration requires the unchanged metadata-id set".into(),
        ));
    }
    let mut edits = BodyNativeKeyEdits::default();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.asm_body_key = before.asm_body_key;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D body-key edit changes structural fields: {id}"
            )));
        }
        if after.asm_body_key != before.asm_body_key {
            let key = after.asm_body_key.map_or(Ok(-1), |key| {
                i64::try_from(key).map_err(|_| {
                    CodecError::Malformed(format!("F3D ASM body key exceeds i64::MAX: {key}"))
                })
            })?;
            edits.asm.insert(after.record_index as usize, key);
            let mut joined = Vec::new();
            joined.extend(
                baseline_native
                    .body_visibilities
                    .iter()
                    .filter(|visibility| visibility.body == before.body)
                    .map(|visibility| (visibility.stream.clone(), visibility.asm_body_key_offset)),
            );
            if let Some(old_key) = before.asm_body_key {
                joined.extend(
                    baseline_native
                        .design_material_assignments
                        .iter()
                        .filter(|assignment| assignment.asm_body_key == old_key)
                        .map(|assignment| {
                            Ok((
                                native_stream(&assignment.id, ":material-assignment#")?,
                                assignment.asm_body_key_offset,
                            ))
                        })
                        .collect::<Result<Vec<_>, CodecError>>()?,
                );
            }
            if !joined.is_empty() {
                let design_key = after.asm_body_key.ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "cannot remove joined F3D ASM body key: {id}"
                    ))
                })?;
                for (stream, offset) in joined {
                    edits
                        .design
                        .entry(stream)
                        .or_default()
                        .insert((offset, design_key));
                }
            }
        }
    }
    Ok(edits)
}

#[derive(Default)]
struct BodyNativeKeyEdits {
    asm: BTreeMap<usize, i64>,
    design: BTreeMap<String, BTreeSet<(u64, u64)>>,
}

fn patch_design_body_keys(
    bytes: &mut [u8],
    edits: &BTreeSet<(u64, u64)>,
) -> Result<(), CodecError> {
    for &(offset, key) in edits {
        let at = usize::try_from(offset).map_err(|_| {
            CodecError::Malformed("Design body-key offset exceeds address space".into())
        })?;
        bytes
            .get_mut(at..at + 8)
            .ok_or_else(|| CodecError::Malformed("Design body-map key is truncated".into()))?
            .copy_from_slice(&key.to_le_bytes());
    }
    Ok(())
}

fn patch_body_native_keys(
    bytes: &mut [u8],
    edits: &BTreeMap<usize, i64>,
) -> Result<(), CodecError> {
    if edits.is_empty() {
        return Ok(());
    }
    let start = asm_header::record_stream_start(bytes)
        .ok_or_else(|| CodecError::Malformed("active BREP has no SAB record stream".into()))?;
    let limit = asm_header::first_delta_state_offset(bytes).unwrap_or(bytes.len());
    let ref_width = asm_header::parse(bytes).map_or(8, |header| usize::from(header.width));
    let records = sab::frame(bytes, start, limit, ref_width)
        .map_err(|error| CodecError::Malformed(format!("cannot frame active BREP: {error}")))?;
    for (record_index, key) in edits {
        let record = records
            .iter()
            .find(|record| record.index == *record_index)
            .ok_or_else(|| {
                CodecError::Malformed(format!("F3D body-key record {record_index} is missing"))
            })?;
        if record.head != "body" {
            return Err(CodecError::Malformed(format!(
                "F3D body-key record {record_index} is not a body"
            )));
        }
        patch_integer_field(bytes, record, ref_width, 1, 0x04, *key)?;
    }
    Ok(())
}

fn patch_transform_hints(
    bytes: &mut [u8],
    edits: &BTreeMap<usize, [bool; 3]>,
) -> Result<(), CodecError> {
    if edits.is_empty() {
        return Ok(());
    }
    let start = asm_header::record_stream_start(bytes)
        .ok_or_else(|| CodecError::Malformed("active BREP has no SAB record stream".into()))?;
    let limit = asm_header::first_delta_state_offset(bytes).unwrap_or(bytes.len());
    let ref_width = active_ref_width(bytes);
    let records = sab::frame(bytes, start, limit, ref_width)
        .map_err(|error| CodecError::Malformed(format!("cannot frame active BREP: {error}")))?;
    for (record_index, flags) in edits {
        let record = records
            .iter()
            .find(|record| record.index == *record_index)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "F3D transform-hint record {record_index} is missing"
                ))
            })?;
        if !record.name.ends_with("transform") {
            return Err(CodecError::Malformed(format!(
                "F3D transform-hint record {record_index} is {}, not a transform",
                record.head
            )));
        }
        for (index, flag) in (5usize..=7).zip(flags) {
            let offset =
                sab::payload_token_offset(bytes, record, ref_width, index).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "F3D transform record {record_index} lacks hint field {index}"
                    ))
                })?;
            if !matches!(bytes.get(offset), Some(0x0a | 0x0b)) {
                return Err(CodecError::Malformed(format!(
                    "F3D transform record {record_index} field {index} is not a hint flag"
                )));
            }
            bytes[offset] = native_bool(*flag);
        }
    }
    Ok(())
}

fn patch_tolerant_coedge_parameters(
    bytes: &mut [u8],
    edits: &BTreeMap<usize, [f64; 2]>,
) -> Result<(), CodecError> {
    if edits.is_empty() {
        return Ok(());
    }
    let start = asm_header::record_stream_start(bytes)
        .ok_or_else(|| CodecError::Malformed("active BREP has no SAB record stream".into()))?;
    let limit = asm_header::first_delta_state_offset(bytes).unwrap_or(bytes.len());
    let ref_width = active_ref_width(bytes);
    let records = sab::frame(bytes, start, limit, ref_width)
        .map_err(|error| CodecError::Malformed(format!("cannot frame active BREP: {error}")))?;
    for (record_index, range) in edits {
        let record = records
            .iter()
            .find(|record| record.index == *record_index)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "F3D tolerant-coedge record {record_index} is missing"
                ))
            })?;
        if record.head != "tcoedge" {
            return Err(CodecError::Malformed(format!(
                "F3D tolerant-coedge record {record_index} is {}",
                record.head
            )));
        }
        for (index, value) in [(11usize, range[0]), (12, range[1])] {
            let offset = required_payload_field(bytes, record, ref_width, index, 0x06)?;
            bytes[offset + 1..offset + 9].copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(())
}

fn patch_wire_topologies(
    bytes: &mut [u8],
    edits: &BTreeMap<usize, crate::records::WireSide>,
) -> Result<(), CodecError> {
    if edits.is_empty() {
        return Ok(());
    }
    let start = asm_header::record_stream_start(bytes)
        .ok_or_else(|| CodecError::Malformed("active BREP has no SAB record stream".into()))?;
    let limit = asm_header::first_delta_state_offset(bytes).unwrap_or(bytes.len());
    let ref_width = active_ref_width(bytes);
    let records = sab::frame(bytes, start, limit, ref_width)
        .map_err(|error| CodecError::Malformed(format!("cannot frame active BREP: {error}")))?;
    for (record_index, side) in edits {
        let record = records
            .iter()
            .find(|record| record.index == *record_index)
            .ok_or_else(|| {
                CodecError::Malformed(format!("F3D wire record {record_index} is missing"))
            })?;
        if record.head != "wire" {
            return Err(CodecError::Malformed(format!(
                "F3D wire record {record_index} is {}",
                record.head
            )));
        }
        let offset = sab::payload_token_offset(bytes, record, ref_width, 7).ok_or_else(|| {
            CodecError::Malformed(format!("F3D wire record {record_index} lacks side field 7"))
        })?;
        if !matches!(bytes.get(offset), Some(0x0a | 0x0b)) {
            return Err(CodecError::Malformed(format!(
                "F3D wire record {record_index} field 7 is not a side token"
            )));
        }
        bytes[offset] = match side {
            crate::records::WireSide::In => 0x0a,
            crate::records::WireSide::Out => 0x0b,
        };
    }
    Ok(())
}

fn patch_edge_ownerships(bytes: &mut [u8], edits: &BTreeMap<usize, i64>) -> Result<(), CodecError> {
    if edits.is_empty() {
        return Ok(());
    }
    let start = asm_header::record_stream_start(bytes)
        .ok_or_else(|| CodecError::Malformed("active BREP has no SAB record stream".into()))?;
    let limit = asm_header::first_delta_state_offset(bytes).unwrap_or(bytes.len());
    let ref_width = active_ref_width(bytes);
    let records = sab::frame(bytes, start, limit, ref_width)
        .map_err(|error| CodecError::Malformed(format!("cannot frame active BREP: {error}")))?;
    for (record_index, owner) in edits {
        let record = records
            .iter()
            .find(|record| record.index == *record_index)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "F3D edge-ownership record {record_index} is missing"
                ))
            })?;
        if !matches!(record.head.as_str(), "edge" | "tedge") {
            return Err(CodecError::Malformed(format!(
                "F3D edge-ownership record {record_index} is {}",
                record.head
            )));
        }
        patch_integer_field(bytes, record, ref_width, 7, 0x0c, *owner)?;
    }
    Ok(())
}

struct ConstructionRecipeEdit {
    record_index: Option<(u64, i32)>,
    design_id: Option<(u64, Vec<u8>)>,
}

fn validate_construction_recipe_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<ConstructionRecipeEdit>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.construction_recipes[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.construction_recipes[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|recipe| (recipe.id.as_str(), recipe))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|recipe| (recipe.id.as_str(), recipe))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D construction-recipe regeneration requires the unchanged recipe-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<ConstructionRecipeEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.record_index = before.record_index;
        normalized.design_id.clone_from(&before.design_id);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D construction-recipe edit changes fields other than record_index or design_id: {id}"
            )));
        }
        if after.record_index == before.record_index && after.design_id == before.design_id {
            continue;
        }
        let record_index = (after.record_index != before.record_index)
            .then(|| {
                after
                    .record_index_offset
                    .map(|offset| (offset, after.record_index))
                    .ok_or_else(|| {
                        CodecError::NotImplemented(format!(
                            "F3D construction recipe {id} has no writable record-index carrier"
                        ))
                    })
            })
            .transpose()?;
        let design_id = if after.design_id == before.design_id {
            None
        } else {
            let before_value = before.design_id.as_deref().ok_or_else(|| {
                CodecError::NotImplemented(format!("cannot add F3D recipe design id: {id}"))
            })?;
            let after_value = after.design_id.as_deref().ok_or_else(|| {
                CodecError::NotImplemented(format!("cannot remove F3D recipe design id: {id}"))
            })?;
            let offset = after.design_id_offset.ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "F3D construction recipe {id} has no writable design-id carrier"
                ))
            })?;
            let encoded = if after.design_id_binary_u32 {
                after_value
                    .parse::<u32>()
                    .map_err(|_| {
                        CodecError::Malformed(format!(
                            "binary F3D recipe design id is not a u32: {after_value}"
                        ))
                    })?
                    .to_le_bytes()
                    .to_vec()
            } else {
                if after_value.len() != before_value.len()
                    || !after_value.bytes().all(|byte| byte.is_ascii_alphanumeric())
                {
                    return Err(CodecError::NotImplemented(format!(
                        "ASCII F3D recipe design id {id} must retain its encoded length"
                    )));
                }
                after_value.as_bytes().to_vec()
            };
            Some((offset, encoded))
        };
        let stream = id
            .strip_prefix("f3d:")
            .and_then(|id| id.rsplit_once(":construction-recipe#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| CodecError::Malformed(format!("invalid construction-recipe id {id}")))?;
        edits
            .entry(stream)
            .or_default()
            .push(ConstructionRecipeEdit {
                record_index,
                design_id,
            });
    }
    Ok(edits)
}

fn patch_construction_recipes(
    bytes: &mut [u8],
    edits: &[ConstructionRecipeEdit],
) -> Result<(), CodecError> {
    for edit in edits {
        if let Some((offset, record_index)) = edit.record_index {
            let start = usize::try_from(offset).map_err(|_| {
                CodecError::Malformed("construction-recipe offset exceeds address space".into())
            })?;
            bytes
                .get_mut(start..start + 4)
                .ok_or_else(|| {
                    CodecError::Malformed("construction-recipe record index is truncated".into())
                })?
                .copy_from_slice(&record_index.to_le_bytes());
        }
        if let Some((offset, encoded)) = &edit.design_id {
            let start = usize::try_from(*offset).map_err(|_| {
                CodecError::Malformed(
                    "construction-recipe design-id offset exceeds address space".into(),
                )
            })?;
            bytes
                .get_mut(start..start + encoded.len())
                .ok_or_else(|| {
                    CodecError::Malformed("construction-recipe design id is truncated".into())
                })?
                .copy_from_slice(encoded);
        }
    }
    Ok(())
}

fn validate_persistent_reference_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<PersistentReferenceEdit>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.persistent_references[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.persistent_references[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|reference| (reference.id.as_str(), reference))
        .collect::<BTreeMap<_, _>>();
    let target_by_id = target
        .iter()
        .map(|reference| (reference.id.as_str(), reference))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id.keys().ne(target_by_id.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D persistent-reference regeneration requires the unchanged reference-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<PersistentReferenceEdit>> = BTreeMap::new();
    for (id, before) in baseline_by_id {
        let after = target_by_id[id];
        let mut normalized = after.clone();
        normalized.value = before.value;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D persistent-reference edit changes fields other than value: {id}"
            )));
        }
        if after.value == before.value {
            continue;
        }
        let stream = id
            .strip_prefix("f3d:")
            .and_then(|id| id.rsplit_once(":persistent-reference#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::Malformed(format!("invalid persistent-reference id {id}"))
            })?;
        edits
            .entry(stream)
            .or_default()
            .push((after.byte_offset, after.value_offset, after.value));
    }
    Ok(edits)
}

fn patch_persistent_references(
    bytes: &mut [u8],
    edits: &[PersistentReferenceEdit],
) -> Result<(), CodecError> {
    for &(record_offset, value_offset, value) in edits {
        let start = usize::try_from(record_offset)
            .ok()
            .and_then(|offset| offset.checked_add(value_offset as usize))
            .ok_or_else(|| {
                CodecError::Malformed("persistent-reference offset exceeds address space".into())
            })?;
        bytes
            .get_mut(start..start + 8)
            .ok_or_else(|| CodecError::Malformed("persistent-reference value is truncated".into()))?
            .copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

#[derive(Default)]
struct HistoryEdits {
    preamble: Option<AsmHistory>,
    states: Vec<AsmDeltaState>,
    boards: Vec<AsmBulletinBoard>,
    changes: Vec<AsmEntityChange>,
}

fn validate_history_state_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, HistoryEdits>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.asm_histories[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.asm_histories[..])
        .unwrap_or_default();
    let baseline_by_id = baseline
        .iter()
        .map(|history| (history.id.as_str(), history))
        .collect::<BTreeMap<_, _>>();
    if baseline_by_id
        .keys()
        .copied()
        .ne(target.iter().map(|history| history.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D history regeneration requires the unchanged history-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, HistoryEdits> = BTreeMap::new();
    for history in target {
        let before = baseline_by_id[history.id.as_str()];
        if before.states.len() != history.states.len() {
            return Err(CodecError::NotImplemented(format!(
                "F3D history edit changes the state count: {}",
                history.id
            )));
        }
        let mut normalized = history.clone();
        normalized.stream_size = before.stream_size;
        normalized.high_water_mark = before.high_water_mark;
        for (state, before_state) in normalized.states.iter_mut().zip(&before.states) {
            state.state_id = before_state.state_id;
            state.version_flag = before_state.version_flag;
            state.state_flag = before_state.state_flag;
            state.previous_ref = before_state.previous_ref;
            state.next_ref = before_state.next_ref;
            state.node_index = before_state.node_index;
            state.partner_ref = before_state.partner_ref;
            state.owner_ref = before_state.owner_ref;
            if state.bulletin_boards.len() != before_state.bulletin_boards.len() {
                return Err(CodecError::NotImplemented(format!(
                    "F3D history edit changes the bulletin-board count: {}",
                    history.id
                )));
            }
            for (board, before_board) in state
                .bulletin_boards
                .iter_mut()
                .zip(&before_state.bulletin_boards)
            {
                board.owner_ref = before_board.owner_ref;
                board.number = before_board.number;
                if board.changes.len() != before_board.changes.len() {
                    return Err(CodecError::NotImplemented(format!(
                        "F3D history edit changes the entity-change count: {}",
                        board.id
                    )));
                }
                for (change, before_change) in board.changes.iter_mut().zip(&before_board.changes) {
                    change.kind = before_change.kind;
                    change.old_ref = before_change.old_ref;
                    change.new_ref = before_change.new_ref;
                }
            }
        }
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D history edit changes fields outside the fixed delta-state header: {}",
                history.id
            )));
        }
        let stream = history
            .id
            .strip_prefix("f3d:")
            .and_then(|id| id.rsplit_once(":asm-history#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::Malformed(format!("invalid ASM history id {}", history.id))
            })?;
        if let Some(size) = history.stream_size {
            if history
                .states
                .first()
                .is_none_or(|state| state.state_id != size)
                || history
                    .high_water_mark
                    .is_none_or(|high_water| high_water < size)
            {
                return Err(CodecError::Malformed(format!(
                    "F3D history {} requires head state_id == stream_size <= high_water_mark",
                    history.id
                )));
            }
        }
        if history.stream_size != before.stream_size
            || history.high_water_mark != before.high_water_mark
        {
            let (Some(size), Some(high_water)) = (history.stream_size, history.high_water_mark)
            else {
                return Err(CodecError::NotImplemented(format!(
                    "cannot add or remove the F3D history preamble: {}",
                    history.id
                )));
            };
            if history.byte_offset == 0 || high_water < size {
                return Err(CodecError::Malformed(format!(
                    "F3D history {} requires head state_id == stream_size <= high_water_mark",
                    history.id
                )));
            }
            edits.entry(stream.clone()).or_default().preamble = Some(history.clone());
        }
        for (state, before_state) in history.states.iter().zip(&before.states) {
            if state != before_state {
                edits
                    .entry(stream.clone())
                    .or_default()
                    .states
                    .push(state.clone());
            }
            for (board, before_board) in state
                .bulletin_boards
                .iter()
                .zip(&before_state.bulletin_boards)
            {
                if board.owner_ref != before_board.owner_ref || board.number != before_board.number
                {
                    edits
                        .entry(stream.clone())
                        .or_default()
                        .boards
                        .push(board.clone());
                }
                for (change, before_change) in board.changes.iter().zip(&before_board.changes) {
                    if change != before_change {
                        if change.kind != history_change_kind(change.old_ref, change.new_ref)? {
                            return Err(CodecError::Malformed(format!(
                                "F3D entity change {} has a kind inconsistent with its references",
                                change.id
                            )));
                        }
                        edits
                            .entry(stream.clone())
                            .or_default()
                            .changes
                            .push(change.clone());
                    }
                }
            }
        }
    }
    Ok(edits)
}

fn patch_history_states(bytes: &mut [u8], edits: &HistoryEdits) -> Result<(), CodecError> {
    const DELTA_HEADER_LEN: usize = b"\x11\x0d\x0bdelta_state".len();
    const PREAMBLE_LEN: usize = b"\x0d\x0ehistory_stream".len();
    if let Some(history) = &edits.preamble {
        let start = usize::try_from(history.byte_offset)
            .ok()
            .and_then(|offset| offset.checked_add(PREAMBLE_LEN))
            .ok_or_else(|| {
                CodecError::Malformed("ASM preamble offset exceeds address space".into())
            })?;
        let size = history.stream_size.expect("validated history preamble");
        let high_water = history.high_water_mark.expect("validated history preamble");
        for (ordinal, value) in [(0, size), (1, size), (3, high_water)] {
            let tag = start + ordinal * 9;
            if bytes.get(tag) != Some(&0x04) {
                return Err(CodecError::Malformed(format!(
                    "ASM history-preamble field {ordinal} at byte {tag} is not a long token"
                )));
            }
            bytes
                .get_mut(tag + 1..tag + 9)
                .ok_or_else(|| CodecError::Malformed("ASM history preamble is truncated".into()))?
                .copy_from_slice(&value.to_le_bytes());
        }
    }
    for state in &edits.states {
        let first_tag = usize::try_from(state.byte_offset)
            .ok()
            .and_then(|offset| offset.checked_add(DELTA_HEADER_LEN))
            .ok_or_else(|| {
                CodecError::Malformed("ASM history offset exceeds address space".into())
            })?;
        let values = [
            (0, 0x04, state.state_id),
            (1, 0x04, state.version_flag),
            (2, 0x04, state.state_flag),
            (3, 0x0c, state.previous_ref.unwrap_or(-1)),
            (4, 0x0c, state.next_ref.unwrap_or(-1)),
            (5, 0x0c, state.node_index),
            (6, 0x0c, state.partner_ref.unwrap_or(-1)),
            (7, 0x0c, state.owner_ref),
        ];
        for (ordinal, expected_tag, value) in values {
            let tag = first_tag + ordinal * 9;
            if bytes.get(tag) != Some(&expected_tag) {
                return Err(CodecError::Malformed(format!(
                    "ASM delta-state field {ordinal} at byte {tag} has the wrong token tag"
                )));
            }
            bytes
                .get_mut(tag + 1..tag + 9)
                .ok_or_else(|| CodecError::Malformed("ASM delta-state field is truncated".into()))?
                .copy_from_slice(&value.to_le_bytes());
        }
    }
    for board in &edits.boards {
        patch_tagged_i64(bytes, board.byte_offset, 1, 0x0c, board.owner_ref)?;
        patch_tagged_i64(bytes, board.byte_offset, 2, 0x04, board.number)?;
    }
    for change in &edits.changes {
        patch_tagged_i64(
            bytes,
            change.byte_offset,
            1,
            0x0c,
            change.old_ref.unwrap_or(-1),
        )?;
        patch_tagged_i64(
            bytes,
            change.byte_offset,
            2,
            0x0c,
            change.new_ref.unwrap_or(-1),
        )?;
    }
    Ok(())
}

fn history_change_kind(
    old_ref: Option<i64>,
    new_ref: Option<i64>,
) -> Result<AsmEntityChangeKind, CodecError> {
    match (old_ref, new_ref) {
        (None, Some(_)) => Ok(AsmEntityChangeKind::Insert),
        (Some(_), None) => Ok(AsmEntityChangeKind::Delete),
        (Some(_), Some(_)) => Ok(AsmEntityChangeKind::Update),
        (None, None) => Err(CodecError::Malformed(
            "ASM entity change cannot have two null references".into(),
        )),
    }
}

fn patch_tagged_i64(
    bytes: &mut [u8],
    record_offset: u64,
    ordinal: usize,
    expected_tag: u8,
    value: i64,
) -> Result<(), CodecError> {
    let tag = usize::try_from(record_offset)
        .ok()
        .and_then(|offset| offset.checked_add(ordinal * 9))
        .ok_or_else(|| CodecError::Malformed("ASM record offset exceeds address space".into()))?;
    if bytes.get(tag) != Some(&expected_tag) {
        return Err(CodecError::Malformed(format!(
            "ASM field {ordinal} at byte {tag} has the wrong token tag"
        )));
    }
    bytes
        .get_mut(tag + 1..tag + 9)
        .ok_or_else(|| CodecError::Malformed("ASM tagged integer is truncated".into()))?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn validate_sketch_point_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<SketchPointEdit>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.sketch_points[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.sketch_points[..])
        .unwrap_or_default();
    let by_id = baseline
        .iter()
        .map(|point| (point.id.as_str(), point))
        .collect::<BTreeMap<_, _>>();
    if by_id
        .keys()
        .copied()
        .ne(target.iter().map(|point| point.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D sketch-point regeneration requires the unchanged point-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<SketchPointEdit>> = BTreeMap::new();
    for point in target {
        let before = by_id[point.id.as_str()];
        let mut normalized = point.clone();
        normalized.coordinates = before.coordinates;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D sketch-point edit changes fields other than coordinates: {}",
                point.id
            )));
        }
        if point.coordinates == before.coordinates {
            continue;
        }
        if !point.coordinates.u.is_finite() || !point.coordinates.v.is_finite() {
            return Err(CodecError::Malformed(format!(
                "F3D sketch point {} has non-finite coordinates",
                point.id
            )));
        }
        let stream = point
            .id
            .strip_prefix("f3d:")
            .and_then(|id| id.rsplit_once(":sketch-point#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::Malformed(format!("invalid sketch-point id {}", point.id))
            })?;
        edits.entry(stream).or_default().push((
            point.byte_offset,
            point.coordinate_offset,
            point.coordinates,
        ));
    }
    Ok(edits)
}

fn patch_sketch_points(bytes: &mut [u8], edits: &[SketchPointEdit]) -> Result<(), CodecError> {
    for (record_offset, coordinate_offset, coordinates) in edits {
        let start = usize::try_from(*record_offset)
            .ok()
            .and_then(|record| record.checked_add(*coordinate_offset as usize))
            .ok_or_else(|| {
                CodecError::Malformed("sketch-point offset exceeds address space".into())
            })?;
        let payload = bytes.get_mut(start..start + 16).ok_or_else(|| {
            CodecError::Malformed("sketch-point coordinate payload is outside BulkStream".into())
        })?;
        payload[..8].copy_from_slice(&(coordinates.u / 10.0).to_le_bytes());
        payload[8..].copy_from_slice(&(coordinates.v / 10.0).to_le_bytes());
    }
    Ok(())
}

type SketchCurveEdit = (u64, u32, SketchCurveGeometry);

fn validate_sketch_curve_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<SketchCurveEdit>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.sketch_curve_identities[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.sketch_curve_identities[..])
        .unwrap_or_default();
    let by_id = baseline
        .iter()
        .map(|curve| (curve.id.as_str(), curve))
        .collect::<BTreeMap<_, _>>();
    if by_id
        .keys()
        .copied()
        .ne(target.iter().map(|curve| curve.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D sketch-curve regeneration requires the unchanged curve-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<SketchCurveEdit>> = BTreeMap::new();
    for curve in target {
        let before = by_id[curve.id.as_str()];
        let mut normalized = curve.clone();
        normalized.geometry.clone_from(&before.geometry);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D sketch-curve edit changes fields other than geometry: {}",
                curve.id
            )));
        }
        if curve.geometry == before.geometry {
            continue;
        }
        let geometry = curve.geometry.clone().ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove sketch-curve geometry: {}", curve.id))
        })?;
        if !valid_sketch_geometry(&geometry)
            || !same_sketch_layout(before.geometry.as_ref(), &geometry)
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D sketch-curve edit requires valid geometry with the original native layout: {}",
                curve.id
            )));
        }
        let stream = curve
            .id
            .strip_prefix("f3d:")
            .and_then(|id| id.rsplit_once(":sketch-curve-identity#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::Malformed(format!("invalid sketch-curve id {}", curve.id))
            })?;
        edits
            .entry(stream)
            .or_default()
            .push((curve.byte_offset, curve.geometry_offset, geometry));
    }
    Ok(edits)
}

fn same_sketch_layout(before: Option<&SketchCurveGeometry>, after: &SketchCurveGeometry) -> bool {
    match (before, after) {
        (Some(SketchCurveGeometry::Line { .. }), SketchCurveGeometry::Line { .. })
        | (Some(SketchCurveGeometry::Arc { .. }), SketchCurveGeometry::Arc { .. }) => true,
        (
            Some(SketchCurveGeometry::Nurbs {
                carrier_reference: old_carrier,
                subtype_class_tag: old_tag,
                subtype_record_index: old_index,
                degree: old_degree,
                scalar_width: old_width,
                knots: old_knots,
                weights: old_weights,
                control_points: old_points,
                ..
            }),
            SketchCurveGeometry::Nurbs {
                carrier_reference,
                subtype_class_tag,
                subtype_record_index,
                degree,
                scalar_width,
                knots,
                weights,
                control_points,
                ..
            },
        ) => {
            old_carrier == carrier_reference
                && old_tag == subtype_class_tag
                && old_index == subtype_record_index
                && old_degree == degree
                && old_width == scalar_width
                && old_knots.len() == knots.len()
                && old_weights.len() == weights.len()
                && old_points.len() == control_points.len()
        }
        _ => false,
    }
}

fn valid_sketch_geometry(geometry: &SketchCurveGeometry) -> bool {
    match geometry {
        SketchCurveGeometry::Line {
            start,
            end,
            direction,
            normal,
        } => finite_point(*start) && finite_point(*end) && orthonormal_pair(*direction, *normal),
        SketchCurveGeometry::Arc {
            center,
            normal,
            reference_direction,
            radius,
            start_angle,
            end_angle,
        } => {
            finite_point(*center)
                && orthonormal_pair(*normal, *reference_direction)
                && radius.is_finite()
                && *radius > 0.0
                && start_angle.is_finite()
                && end_angle.is_finite()
        }
        SketchCurveGeometry::Nurbs {
            degree,
            fit_tolerance,
            scalar_width,
            knots,
            weights,
            control_points,
            ..
        } => {
            *scalar_width == 8
                && fit_tolerance.is_finite()
                && *fit_tolerance >= 0.0
                && knots.len() == control_points.len() + *degree as usize + 1
                && knots.iter().all(|knot| knot.is_finite())
                && knots.windows(2).all(|pair| pair[0] <= pair[1])
                && (weights.is_empty() || weights.len() == control_points.len())
                && weights
                    .iter()
                    .all(|weight| weight.is_finite() && *weight > 0.0)
                && control_points.iter().all(|point| finite_point(*point))
        }
    }
}

fn patch_sketch_curves(bytes: &mut [u8], edits: &[SketchCurveEdit]) -> Result<(), CodecError> {
    for (record_offset, geometry_offset, geometry) in edits {
        let start = usize::try_from(*record_offset)
            .ok()
            .and_then(|record| record.checked_add(*geometry_offset as usize))
            .ok_or_else(|| {
                CodecError::Malformed("sketch-curve offset exceeds address space".into())
            })?;
        if let SketchCurveGeometry::Nurbs {
            fit_tolerance,
            knots,
            weights,
            control_points,
            ..
        } = geometry
        {
            patch_sketch_nurbs(bytes, start, *fit_tolerance, knots, weights, control_points)?;
            continue;
        }
        let payload = bytes.get_mut(start..start + 96).ok_or_else(|| {
            CodecError::Malformed("sketch-curve analytic payload is outside BulkStream".into())
        })?;
        let values = match geometry {
            SketchCurveGeometry::Line {
                start,
                end,
                direction,
                normal,
            } => [
                start.x / 10.0,
                start.y / 10.0,
                start.z / 10.0,
                (end.x - start.x) / 10.0,
                (end.y - start.y) / 10.0,
                (end.z - start.z) / 10.0,
                direction.x,
                direction.y,
                direction.z,
                normal.x,
                normal.y,
                normal.z,
            ],
            SketchCurveGeometry::Arc {
                center,
                normal,
                reference_direction,
                radius,
                start_angle,
                end_angle,
            } => [
                center.x / 10.0,
                center.y / 10.0,
                center.z / 10.0,
                normal.x,
                normal.y,
                normal.z,
                reference_direction.x,
                reference_direction.y,
                reference_direction.z,
                radius / 10.0,
                *start_angle,
                *end_angle,
            ],
            SketchCurveGeometry::Nurbs { .. } => unreachable!("NURBS handled before fixed payload"),
        };
        for (ordinal, value) in values.into_iter().enumerate() {
            payload[ordinal * 8..ordinal * 8 + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(())
}

fn patch_sketch_nurbs(
    bytes: &mut [u8],
    start: usize,
    fit_tolerance: f64,
    knots: &[f64],
    weights: &[f64],
    control_points: &[Point3],
) -> Result<(), CodecError> {
    let fit_at = start + 94;
    let knots_at = start + 114;
    let weights_header = knots_at + knots.len() * 8;
    let weights_at = weights_header + 12;
    let points_header = weights_at + weights.len() * 8;
    let points_at = points_header + 12;
    let end = points_at + control_points.len() * 24;
    if end > bytes.len() {
        return Err(CodecError::Malformed(
            "sketch NURBS arrays extend beyond BulkStream".into(),
        ));
    }
    bytes[fit_at..fit_at + 8].copy_from_slice(&(fit_tolerance / 10.0).to_le_bytes());
    for (ordinal, value) in knots.iter().enumerate() {
        let at = knots_at + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    for (ordinal, value) in weights.iter().enumerate() {
        let at = weights_at + ordinal * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    for (ordinal, point) in control_points.iter().enumerate() {
        let at = points_at + ordinal * 24;
        for (component, value) in [point.x, point.y, point.z].into_iter().enumerate() {
            let component_at = at + component * 8;
            bytes[component_at..component_at + 8].copy_from_slice(&(value / 10.0).to_le_bytes());
        }
    }
    Ok(())
}

type SketchRelationEdit = Vec<(u64, u32)>;

fn validate_sketch_relation_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, Vec<SketchRelationEdit>>, CodecError> {
    let baseline_native = f3d_native(baseline)?;
    let target_native = f3d_native(target)?;
    let baseline = baseline_native
        .as_ref()
        .map(|native| &native.sketch_relations[..])
        .unwrap_or_default();
    let target = target_native
        .as_ref()
        .map(|native| &native.sketch_relations[..])
        .unwrap_or_default();
    let by_id = baseline
        .iter()
        .map(|relation| (relation.id.as_str(), relation))
        .collect::<BTreeMap<_, _>>();
    if by_id
        .keys()
        .copied()
        .ne(target.iter().map(|relation| relation.id.as_str()))
    {
        return Err(CodecError::NotImplemented(
            "F3D sketch-relation regeneration requires the unchanged relation-id set".into(),
        ));
    }
    let mut edits: BTreeMap<String, Vec<SketchRelationEdit>> = BTreeMap::new();
    for relation in target {
        let before = by_id[relation.id.as_str()];
        let mut normalized = relation.clone();
        normalized.owner_reference = before.owner_reference;
        normalized
            .auxiliary_references
            .clone_from(&before.auxiliary_references);
        normalized.members.clone_from(&before.members);
        normalized.state = before.state;
        normalized
            .constraint_kinds
            .clone_from(&before.constraint_kinds);
        normalized.unknown_constraint_bits = before.unknown_constraint_bits;
        normalized.return_members.clone_from(&before.return_members);
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D sketch-relation edit changes fields outside its writable references and constraint mask: {}",
                relation.id
            )));
        }
        if relation.state == before.state
            && relation.owner_reference == before.owner_reference
            && relation.auxiliary_references == before.auxiliary_references
            && relation.members == before.members
            && relation.return_members == before.return_members
        {
            continue;
        }
        let (kinds, unknown) = crate::design::decode_constraint_kinds(relation.state);
        if kinds != relation.constraint_kinds || unknown != relation.unknown_constraint_bits {
            return Err(CodecError::Malformed(format!(
                "F3D sketch relation {} has a mask inconsistent with its typed constraint kinds",
                relation.id
            )));
        }
        let stream = relation
            .id
            .strip_prefix("f3d:")
            .and_then(|id| id.rsplit_once(":sketch-relation#"))
            .map(|(stream, _)| stream.to_owned())
            .ok_or_else(|| {
                CodecError::Malformed(format!("invalid sketch-relation id {}", relation.id))
            })?;
        let mut values = Vec::new();
        collect_sketch_reference_edits(
            relation,
            &before.members,
            &relation.members,
            &relation.member_offsets,
            &mut values,
        )?;
        collect_sketch_reference_edits(
            relation,
            &before.auxiliary_references,
            &relation.auxiliary_references,
            &relation.auxiliary_reference_offsets,
            &mut values,
        )?;
        if relation.owner_reference != before.owner_reference {
            values.push((
                relation.byte_offset + u64::from(relation.owner_reference_offset),
                relation.owner_reference,
            ));
        }
        collect_sketch_reference_edits(
            relation,
            &before.return_members,
            &relation.return_members,
            &relation.return_member_offsets,
            &mut values,
        )?;
        if relation.state != before.state {
            values.push((
                relation.byte_offset + u64::from(relation.state_offset),
                relation.state,
            ));
        }
        edits.entry(stream).or_default().push(values);
    }
    Ok(edits)
}

fn patch_sketch_relations(
    bytes: &mut [u8],
    edits: &[SketchRelationEdit],
) -> Result<(), CodecError> {
    for edit in edits {
        for (offset, value) in edit {
            patch_bytes_at(
                bytes,
                *offset,
                &value.to_le_bytes(),
                "sketch-relation value",
            )?;
        }
    }
    Ok(())
}

fn collect_sketch_reference_edits(
    relation: &crate::records::SketchRelation,
    before: &[u32],
    after: &[u32],
    offsets: &[u32],
    edits: &mut Vec<(u64, u32)>,
) -> Result<(), CodecError> {
    if before.len() != after.len() || after.len() != offsets.len() {
        return Err(CodecError::NotImplemented(format!(
            "F3D sketch relation {} must retain reference cardinality and offsets",
            relation.id
        )));
    }
    edits.extend(
        before
            .iter()
            .zip(after)
            .zip(offsets)
            .filter(|((before, after), _)| before != after)
            .map(|((_, after), offset)| (relation.byte_offset + u64::from(*offset), *after)),
    );
    Ok(())
}

fn validate_body_transform_edits(
    baseline: &[Body],
    target: &[Body],
) -> Result<BTreeMap<String, Transform>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|body| (body.id.as_str(), body))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|body| (body.id.as_str(), body))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D body-transform regeneration requires the unchanged body-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.transform = before.transform;
        normalized.color = before.color;
        normalized.visible = before.visible;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D body edit changes fields other than transform, color, or visibility: {id}"
            )));
        }
        if after.transform == before.transform {
            continue;
        }
        let transform = after.transform.ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove F3D body transform: {id}"))
        })?;
        if !valid_transform(transform) || before.transform.is_none() {
            return Err(CodecError::NotImplemented(format!(
                "F3D body transform {id} must replace an existing finite affine transform"
            )));
        }
        edits.insert(id.to_owned(), transform);
    }
    Ok(edits)
}

fn validate_body_color_edits(
    baseline: &[Body],
    target: &[Body],
) -> Result<BTreeMap<String, Color>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|body| (body.id.as_str(), body))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|body| (body.id.as_str(), body))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D body-color regeneration requires the unchanged body-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.color = before.color;
        normalized.transform = before.transform;
        normalized.visible = before.visible;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D body edit changes fields other than transform, color, or visibility: {id}"
            )));
        }
        if after.color == before.color {
            continue;
        }
        let color = after.color.ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove F3D body color: {id}"))
        })?;
        if before.color.is_none()
            || ![color.r, color.g, color.b, color.a]
                .into_iter()
                .all(|component| component.is_finite() && (0.0..=1.0).contains(&component))
            || color.a != 1.0
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D body color {id} must replace an existing opaque finite RGB color"
            )));
        }
        edits.insert(id.to_owned(), color);
    }
    Ok(edits)
}

fn validate_edge_range_edits(
    baseline: &[Edge],
    target: &[Edge],
) -> Result<BTreeMap<String, [f64; 2]>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D edge-range regeneration requires the unchanged edge-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.param_range = before.param_range;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D edge edit changes fields other than parameter range: {id}"
            )));
        }
        if after.param_range == before.param_range {
            continue;
        }
        let range = after.param_range.ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove F3D edge range: {id}"))
        })?;
        if before.param_range.is_none()
            || !range[0].is_finite()
            || !range[1].is_finite()
            || range[0] == range[1]
        {
            return Err(CodecError::Malformed(format!(
                "edited F3D edge range {id} must replace an existing finite non-degenerate range"
            )));
        }
        edits.insert(id.to_owned(), range);
    }
    Ok(edits)
}

fn validate_face_sense_edits(
    baseline_ir: &CadIr,
    target_ir: &CadIr,
) -> Result<BTreeMap<String, Sense>, CodecError> {
    let baseline = &baseline_ir.model.faces;
    let target = &target_ir.model.faces;
    let native_senses = f3d_native(baseline_ir)?
        .map(|native| {
            native
                .face_sidedness
                .into_iter()
                .map(|metadata| {
                    (
                        metadata.face,
                        (metadata.native_sense, metadata.normalized_sense),
                    )
                })
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let baseline = baseline
        .iter()
        .map(|face| (face.id.as_str(), face))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|face| (face.id.as_str(), face))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D face-sense regeneration requires the unchanged face-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.sense = before.sense;
        normalized.color = before.color;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D face edit changes fields other than sense: {id}"
            )));
        }
        if after.sense != before.sense {
            let (native_before, normalized_before) = native_senses
                .get(&before.id)
                .copied()
                .unwrap_or((before.sense, before.sense));
            let native_after =
                normalized_face_sense_to_native(after.sense, native_before, normalized_before);
            edits.insert(id.to_owned(), native_after);
        }
    }
    Ok(edits)
}

fn validate_face_color_edits(
    baseline: &[Face],
    target: &[Face],
) -> Result<BTreeMap<String, Color>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|face| (face.id.as_str(), face))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|face| (face.id.as_str(), face))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D face-color regeneration requires the unchanged face-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.color = before.color;
        normalized.sense = before.sense;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D face edit changes fields other than sense or color: {id}"
            )));
        }
        if after.color == before.color {
            continue;
        }
        let color = after.color.ok_or_else(|| {
            CodecError::NotImplemented(format!("cannot remove F3D face color: {id}"))
        })?;
        if before.color.is_none()
            || ![color.r, color.g, color.b, color.a]
                .into_iter()
                .all(|component| component.is_finite() && (0.0..=1.0).contains(&component))
            || color.a != 1.0
        {
            return Err(CodecError::NotImplemented(format!(
                "F3D face color {id} must replace an existing opaque finite RGB color"
            )));
        }
        edits.insert(id.to_owned(), color);
    }
    Ok(edits)
}

fn validate_coedge_sense_edits(
    baseline: &[Coedge],
    target: &[Coedge],
) -> Result<BTreeMap<String, Sense>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|coedge| (coedge.id.as_str(), coedge))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|coedge| (coedge.id.as_str(), coedge))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D coedge-sense regeneration requires the unchanged coedge-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        let mut normalized = after.clone();
        normalized.sense = before.sense;
        if &normalized != before {
            return Err(CodecError::NotImplemented(format!(
                "F3D coedge edit changes fields other than sense: {id}"
            )));
        }
        if after.sense != before.sense {
            edits.insert(id.to_owned(), after.sense);
        }
    }
    Ok(edits)
}

fn valid_transform(transform: Transform) -> bool {
    transform
        .rows
        .iter()
        .flatten()
        .all(|value| value.is_finite())
        && transform.rows[3][0] == 0.0
        && transform.rows[3][1] == 0.0
        && transform.rows[3][2] == 0.0
        && transform.rows[3][3] != 0.0
}

fn validate_curve_edits(
    baseline: &[Curve],
    target: &[Curve],
) -> Result<std::collections::BTreeSet<String>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|curve| (curve.id.as_str(), &curve.geometry))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|curve| (curve.id.as_str(), &curve.geometry))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D curve regeneration requires the unchanged curve-id set".into(),
        ));
    }
    let mut edited = std::collections::BTreeSet::new();
    for (id, before) in baseline {
        let after = target[id];
        if before == after {
            continue;
        }
        edited.insert(id.to_owned());
        let valid = match after {
            CurveGeometry::Line { origin, direction }
                if matches!(before, CurveGeometry::Line { .. }) =>
            {
                finite_point(*origin)
                    && finite_vector(*direction)
                    && (direction.norm() - 1.0).abs() <= 1e-9
            }
            CurveGeometry::Circle {
                center,
                axis,
                ref_direction,
                radius,
            } if matches!(before, CurveGeometry::Circle { .. }) => {
                finite_point(*center)
                    && orthonormal_pair(*axis, *ref_direction)
                    && radius.is_finite()
                    && *radius > 0.0
            }
            CurveGeometry::Ellipse {
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius,
            } if matches!(before, CurveGeometry::Ellipse { .. }) => {
                finite_point(*center)
                    && orthonormal_pair(*axis, *major_direction)
                    && major_radius.is_finite()
                    && minor_radius.is_finite()
                    && *major_radius > 0.0
                    && *minor_radius > 0.0
                    && *minor_radius <= *major_radius
            }
            CurveGeometry::Degenerate { point }
                if matches!(before, CurveGeometry::Degenerate { .. }) =>
            {
                finite_point(*point)
            }
            CurveGeometry::Nurbs(after) => {
                let CurveGeometry::Nurbs(before) = before else {
                    return Err(CodecError::NotImplemented(format!(
                        "F3D regeneration cannot change curve {id} into a NURBS carrier"
                    )));
                };
                (id.starts_with("f3d:brep:entity#")
                    || (id.starts_with("f3d:brep:procedural_surface#")
                        && (id.ends_with(":directrix") || id.ends_with(":spine"))))
                    && valid_edited_curve_structure(before, after)
                    && before.weights.is_some() == after.weights.is_some()
                    && before.control_points.len() == after.control_points.len()
                    && after.control_points.iter().copied().all(finite_point)
                    && after.weights.as_ref().is_none_or(|weights| {
                        weights.len() == after.control_points.len()
                            && weights
                                .iter()
                                .all(|weight| weight.is_finite() && *weight > 0.0)
                    })
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "F3D regeneration does not support edits to curve {id}"
                )));
            }
        };
        if !valid {
            return Err(CodecError::Malformed(format!(
                "edited F3D curve {id} has an invalid frame or radius"
            )));
        }
    }
    Ok(edited)
}

fn validate_pcurve_edits(
    baseline: &[Pcurve],
    target: &[Pcurve],
) -> Result<BTreeMap<String, NurbsPcurveEdit>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|pcurve| (pcurve.id.as_str(), pcurve))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|pcurve| (pcurve.id.as_str(), pcurve))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D pcurve regeneration requires the unchanged pcurve-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        if before == after {
            continue;
        }
        let (
            PcurveGeometry::Nurbs {
                degree: _,
                knots: before_knots,
                control_points: before_points,
                weights: before_weights,
                periodic: before_periodic,
            },
            PcurveGeometry::Nurbs {
                degree: after_degree,
                knots: after_knots,
                control_points: after_points,
                weights: after_weights,
                periodic: after_periodic,
            },
        ) = (&before.geometry, &after.geometry)
        else {
            return Err(CodecError::NotImplemented(format!(
                "F3D regeneration does not support this pcurve edit: {id}"
            )));
        };
        let valid = id.starts_with("f3d:brep:entity#")
            && valid_edited_nurbs_direction(
                before_knots,
                *after_degree,
                after_knots,
                after_points.len(),
            )
            && before_points.len() == after_points.len()
            && before_weights.is_some() == after_weights.is_some()
            && before_weights.as_ref().map(Vec::len) == after_weights.as_ref().map(Vec::len)
            && after_weights.as_ref().is_none_or(|weights| {
                weights
                    .iter()
                    .all(|weight| weight.is_finite() && *weight > 0.0)
            })
            && after_points
                .iter()
                .all(|point| point.u.is_finite() && point.v.is_finite());
        let contract_valid = before.wrapper_reversed.is_some() == after.wrapper_reversed.is_some()
            && before.native_tail_flags.is_some() == after.native_tail_flags.is_some()
            && before.parameter_range.is_some() == after.parameter_range.is_some()
            && before.fit_tolerance.is_some() == after.fit_tolerance.is_some()
            && after
                .parameter_range
                .is_none_or(|range| range.into_iter().all(f64::is_finite) && range[0] <= range[1])
            && after
                .fit_tolerance
                .is_none_or(|tolerance| tolerance.is_finite() && tolerance >= 0.0);
        if !valid || !contract_valid {
            return Err(CodecError::NotImplemented(format!(
                "F3D pcurve edit changes fixed cache structure: {id}"
            )));
        }
        edits.insert(
            id.to_owned(),
            NurbsPcurveEdit {
                geometry: after.geometry.clone(),
                periodic: (before_periodic != after_periodic).then_some(*after_periodic),
                wrapper_reversed: (before.wrapper_reversed != after.wrapper_reversed)
                    .then_some(after.wrapper_reversed)
                    .flatten(),
                native_tail_flags: (before.native_tail_flags != after.native_tail_flags)
                    .then_some(after.native_tail_flags)
                    .flatten(),
                parameter_range: (before.parameter_range != after.parameter_range)
                    .then_some(after.parameter_range)
                    .flatten(),
                fit_tolerance: (before.fit_tolerance != after.fit_tolerance)
                    .then_some(after.fit_tolerance)
                    .flatten(),
            },
        );
    }
    Ok(edits)
}

fn validate_surface_edits(
    baseline: &[Surface],
    target: &[Surface],
) -> Result<std::collections::BTreeSet<String>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|surface| (surface.id.as_str(), &surface.geometry))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|surface| (surface.id.as_str(), &surface.geometry))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D surface regeneration requires the unchanged surface-id set".into(),
        ));
    }
    let mut edited = std::collections::BTreeSet::new();
    for (id, before) in baseline {
        let after = target[id];
        if before == after {
            continue;
        }
        let valid = match after {
            SurfaceGeometry::Plane {
                origin,
                normal,
                u_axis,
            } if matches!(before, SurfaceGeometry::Plane { .. }) => {
                finite_point(*origin) && orthonormal_pair(*normal, *u_axis)
            }
            SurfaceGeometry::Sphere {
                center,
                axis,
                ref_direction,
                radius,
            } if matches!(before, SurfaceGeometry::Sphere { .. }) => {
                finite_point(*center)
                    && orthonormal_pair(*axis, *ref_direction)
                    && radius.is_finite()
                    && *radius != 0.0
            }
            SurfaceGeometry::Torus {
                center,
                axis,
                ref_direction,
                major_radius,
                minor_radius,
            } if matches!(before, SurfaceGeometry::Torus { .. }) => {
                finite_point(*center)
                    && orthonormal_pair(*axis, *ref_direction)
                    && major_radius.is_finite()
                    && minor_radius.is_finite()
                    && *major_radius != 0.0
                    && *minor_radius != 0.0
            }
            SurfaceGeometry::Cylinder {
                origin,
                axis,
                ref_direction,
                radius,
            } if matches!(before, SurfaceGeometry::Cylinder { .. }) => {
                finite_point(*origin)
                    && orthonormal_pair(*axis, *ref_direction)
                    && radius.is_finite()
                    && *radius != 0.0
            }
            SurfaceGeometry::Cone {
                origin,
                axis,
                ref_direction,
                radius,
                ratio,
                half_angle,
            } if matches!(before, SurfaceGeometry::Cone { .. }) => {
                finite_point(*origin)
                    && orthonormal_pair(*axis, *ref_direction)
                    && radius.is_finite()
                    && *radius != 0.0
                    && ratio.is_finite()
                    && *ratio > 0.0
                    && half_angle.is_finite()
                    && *half_angle >= 0.0
                    && *half_angle < std::f64::consts::FRAC_PI_2
            }
            SurfaceGeometry::Nurbs(after) => {
                let SurfaceGeometry::Nurbs(before) = before else {
                    return Err(CodecError::NotImplemented(format!(
                        "F3D regeneration cannot change surface {id} into a NURBS carrier"
                    )));
                };
                (id.starts_with("f3d:brep:entity#")
                    || (id.starts_with("f3d:brep:procedural_surface#")
                        && (id.ends_with(":support0") || id.ends_with(":support1"))))
                    && valid_edited_nurbs_direction(
                        &before.u_knots,
                        after.u_degree,
                        &after.u_knots,
                        usize::try_from(after.u_count).unwrap_or(usize::MAX),
                    )
                    && valid_edited_nurbs_direction(
                        &before.v_knots,
                        after.v_degree,
                        &after.v_knots,
                        usize::try_from(after.v_count).unwrap_or(usize::MAX),
                    )
                    && before.u_count == after.u_count
                    && before.v_count == after.v_count
                    && before.weights.is_some() == after.weights.is_some()
                    && after.control_points.len()
                        == usize::try_from(after.u_count)
                            .ok()
                            .and_then(|u| {
                                usize::try_from(after.v_count)
                                    .ok()
                                    .and_then(|v| u.checked_mul(v))
                            })
                            .unwrap_or(usize::MAX)
                    && after.control_points.iter().copied().all(finite_point)
                    && after.weights.as_ref().is_none_or(|weights| {
                        weights.len() == after.control_points.len()
                            && weights
                                .iter()
                                .all(|weight| weight.is_finite() && *weight > 0.0)
                    })
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "F3D regeneration does not support edits to surface {id}"
                )));
            }
        };
        if !valid {
            return Err(CodecError::Malformed(format!(
                "edited F3D surface {id} requires a finite orthonormal frame and valid radius"
            )));
        }
        edited.insert(id.to_owned());
    }
    Ok(edited)
}

fn validate_procedural_surface_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, ProceduralSurfaceEdit>, CodecError> {
    let baseline = baseline
        .model
        .procedural_surfaces
        .iter()
        .map(|surface| (surface.id.as_str(), surface))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .model
        .procedural_surfaces
        .iter()
        .map(|surface| (surface.id.as_str(), surface))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D procedural-surface regeneration requires the unchanged construction-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        if after == before {
            continue;
        }
        if before.id != after.id || before.surface != after.surface {
            return Err(CodecError::NotImplemented(format!(
                "F3D procedural-surface edit changes immutable carrier fields: {id}"
            )));
        }
        let edit = match (&before.definition, &after.definition) {
            (
                ProceduralSurfaceDefinition::Extrusion {
                    directrix: before_directrix,
                    parameter_interval: before_parameter_interval,
                    direction: before_direction,
                    native_position: before_native_position,
                },
                ProceduralSurfaceDefinition::Extrusion {
                    directrix: after_directrix,
                    parameter_interval: after_parameter_interval,
                    direction: after_direction,
                    native_position: after_native_position,
                },
            ) if before_directrix == after_directrix => {
                let interval = after_parameter_interval.ok_or_else(|| {
                    CodecError::Malformed(format!("F3D extrusion interval is missing: {id}"))
                })?;
                let position = after_native_position.ok_or_else(|| {
                    CodecError::Malformed(format!("F3D extrusion native position is missing: {id}"))
                })?;
                if !interval.into_iter().all(f64::is_finite) || interval[0] >= interval[1] {
                    return Err(CodecError::Malformed(format!(
                        "F3D extrusion interval must be finite and ordered: {id}"
                    )));
                }
                if !finite_vector(*after_direction) || after_direction.norm() == 0.0 {
                    return Err(CodecError::Malformed(format!(
                        "F3D extrusion direction must be finite and nonzero: {id}"
                    )));
                }
                if ![position.x, position.y, position.z]
                    .into_iter()
                    .all(f64::is_finite)
                {
                    return Err(CodecError::Malformed(format!(
                        "F3D extrusion native position must be finite: {id}"
                    )));
                }
                (before_parameter_interval != after_parameter_interval
                    || before_direction != after_direction
                    || before_native_position != after_native_position)
                    .then_some(ProceduralSurfaceEdit::Extrusion {
                        parameter_interval: interval,
                        direction: *after_direction,
                        native_position: position,
                    })
            }
            (
                ProceduralSurfaceDefinition::Blend {
                    supports: before_supports,
                    spine: before_spine,
                    radius: before_radius,
                    cross_section: before_cross_section,
                    native: before_native,
                },
                ProceduralSurfaceDefinition::Blend {
                    supports: after_supports,
                    spine: after_spine,
                    radius: after_radius,
                    cross_section: after_cross_section,
                    native: after_native,
                },
            ) if before_supports == after_supports
                && before_spine == after_spine
                && before_cross_section == after_cross_section
                && before_native == after_native =>
            {
                let values = match after_radius {
                    BlendRadiusLaw::Constant { signed_radius } => [*signed_radius; 2],
                    BlendRadiusLaw::Linear { start, end } => [*start, *end],
                    BlendRadiusLaw::Law { .. } => {
                        return Err(CodecError::NotImplemented(format!(
                            "F3D explicit blend-law regeneration is unsupported: {id}"
                        )));
                    }
                };
                if !values.into_iter().all(f64::is_finite) || values.contains(&0.0) {
                    return Err(CodecError::Malformed(format!(
                        "F3D rolling-ball radii must be finite and nonzero: {id}"
                    )));
                }
                (before_radius != after_radius).then_some(ProceduralSurfaceEdit::BlendRadii(values))
            }
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "F3D procedural-surface edit changes non-writable construction fields: {id}"
                )));
            }
        };
        if let Some(edit) = edit {
            edits.insert(id.to_owned(), edit);
        }
    }
    Ok(edits)
}

fn validate_procedural_surface_fit_edits(
    baseline: &CadIr,
    target: &CadIr,
) -> Result<BTreeMap<String, f64>, CodecError> {
    let baseline = baseline
        .model
        .procedural_surfaces
        .iter()
        .map(|surface| (surface.id.as_str(), surface))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .model
        .procedural_surfaces
        .iter()
        .map(|surface| (surface.id.as_str(), surface))
        .collect::<BTreeMap<_, _>>();
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        if after.cache_fit_tolerance == before.cache_fit_tolerance {
            continue;
        }
        let tolerance = after.cache_fit_tolerance.ok_or_else(|| {
            CodecError::NotImplemented(format!(
                "cannot remove F3D procedural-surface fit tolerance: {id}"
            ))
        })?;
        if before.cache_fit_tolerance.is_none() || !tolerance.is_finite() || tolerance < 0.0 {
            return Err(CodecError::Malformed(format!(
                "F3D procedural-surface fit tolerance must replace a finite nonnegative value: {id}"
            )));
        }
        edits.insert(id.to_owned(), tolerance);
    }
    Ok(edits)
}

fn validate_procedural_curve_edits(
    baseline: &[ProceduralCurve],
    target: &[ProceduralCurve],
) -> Result<BTreeMap<String, ProceduralCurveEdit>, CodecError> {
    let baseline = baseline
        .iter()
        .map(|curve| (curve.id.as_str(), curve))
        .collect::<BTreeMap<_, _>>();
    let target = target
        .iter()
        .map(|curve| (curve.id.as_str(), curve))
        .collect::<BTreeMap<_, _>>();
    if baseline.keys().ne(target.keys()) {
        return Err(CodecError::NotImplemented(
            "F3D procedural-curve regeneration requires the unchanged construction-id set".into(),
        ));
    }
    let mut edits = BTreeMap::new();
    for (id, before) in baseline {
        let after = target[id];
        if after.curve != before.curve {
            return Err(CodecError::NotImplemented(format!(
                "F3D procedural-curve edit changes its solved curve: {id}"
            )));
        }
        let definition = match (&before.definition, &after.definition) {
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix { .. },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix { .. },
            ) if before.definition != after.definition => Some(after.definition.clone()),
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::VectorOffset {
                    source: before_source,
                    labels: before_labels,
                    codes: before_codes,
                    ..
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::VectorOffset {
                    source: after_source,
                    labels: after_labels,
                    codes: after_codes,
                    ..
                },
            ) if before_source == after_source
                && before_labels == after_labels
                && before_codes == after_codes
                && before.definition != after.definition =>
            {
                Some(after.definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                    source: before_source,
                    ..
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
                    source: after_source,
                    ..
                },
            ) if before_source == after_source && before.definition != after.definition => {
                Some(after.definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::TwoSidedOffset {
                    context: before_context,
                    ..
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::TwoSidedOffset {
                    context: after_context,
                    ..
                },
            ) if before_context.sides == after_context.sides
                && before_context
                    .discontinuities
                    .iter()
                    .map(Vec::len)
                    .eq(after_context.discontinuities.iter().map(Vec::len))
                && before.definition != after.definition =>
            {
                Some(after.definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceOffset {
                    context: before_context,
                    base: before_base,
                    ..
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceOffset {
                    context: after_context,
                    base: after_base,
                    ..
                },
            ) if before_context.sides == after_context.sides
                && before_context
                    .discontinuities
                    .iter()
                    .map(Vec::len)
                    .eq(after_context.discontinuities.iter().map(Vec::len))
                && before_base == after_base
                && before.definition != after.definition =>
            {
                Some(after.definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Spring {
                    context: before_context,
                    surface_parameter_ranges: before_surface_ranges,
                    first_pcurve_parameter_range: before_pcurve_range,
                    ..
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Spring {
                    context: after_context,
                    surface_parameter_ranges: after_surface_ranges,
                    first_pcurve_parameter_range: after_pcurve_range,
                    ..
                },
            ) if before_context.sides == after_context.sides
                && before_context
                    .discontinuities
                    .iter()
                    .map(Vec::len)
                    .eq(after_context.discontinuities.iter().map(Vec::len))
                && before_surface_ranges == after_surface_ranges
                && before_pcurve_range == after_pcurve_range
                && before.definition != after.definition =>
            {
                Some(after.definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Projection {
                    context: before_context,
                    source: before_source,
                    tail: before_tail,
                    ..
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Projection {
                    context: after_context,
                    source: after_source,
                    tail: after_tail,
                    ..
                },
            ) if before_context.sides == after_context.sides
                && before_context
                    .discontinuities
                    .iter()
                    .map(Vec::len)
                    .eq(after_context.discontinuities.iter().map(Vec::len))
                && before_source == after_source
                && match (before_tail, after_tail) {
                    (
                        cadmpeg_ir::geometry::ProjectionTail::EarlyClose { .. },
                        cadmpeg_ir::geometry::ProjectionTail::EarlyClose { .. },
                    ) => true,
                    (
                        cadmpeg_ir::geometry::ProjectionTail::Ranged {
                            role: before_role, ..
                        },
                        cadmpeg_ir::geometry::ProjectionTail::Ranged {
                            role: after_role, ..
                        },
                    ) => {
                        before_role.len() == after_role.len()
                            && matches!(after_role.as_str(), "surf1" | "surf2")
                    }
                    _ => false,
                }
                && before.definition != after.definition =>
            {
                Some(after.definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection {
                    context: before_context,
                    ..
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection {
                    context: after_context,
                    ..
                },
            ) if before_context.sides == after_context.sides
                && before_context
                    .discontinuities
                    .iter()
                    .map(Vec::len)
                    .eq(after_context.discontinuities.iter().map(Vec::len))
                && before.definition != after.definition =>
            {
                Some(after.definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::ThreeSurfaceIntersection {
                    context: before_context,
                    third: before_third,
                    ..
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::ThreeSurfaceIntersection {
                    context: after_context,
                    third: after_third,
                    ..
                },
            ) if before_context.sides == after_context.sides
                && before_context
                    .discontinuities
                    .iter()
                    .map(Vec::len)
                    .eq(after_context.discontinuities.iter().map(Vec::len))
                && before_third == after_third
                && before.definition != after.definition =>
            {
                Some(after.definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve {
                    family: before_family,
                    context: before_context,
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve {
                    family: after_family,
                    context: after_context,
                },
            ) if before_family == after_family
                && before_context.sides == after_context.sides
                && before_context
                    .discontinuities
                    .iter()
                    .map(Vec::len)
                    .eq(after_context.discontinuities.iter().map(Vec::len))
                && before.definition != after.definition =>
            {
                Some(after.definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Silhouette {
                    context: before_context,
                    silhouette: before_silhouette,
                    cast_surface: before_cast,
                    ..
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Silhouette {
                    context: after_context,
                    silhouette: after_silhouette,
                    cast_surface: after_cast,
                    ..
                },
            ) if before_context == after_context
                && std::mem::discriminant(before_silhouette)
                    == std::mem::discriminant(after_silhouette)
                && before_cast == after_cast
                && before.definition != after.definition =>
            {
                Some(after.definition.clone())
            }
            (
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound {
                    components: before_components,
                    ..
                },
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound {
                    components: after_components,
                    ..
                },
            ) if before_components == after_components && before.definition != after.definition => {
                Some(after.definition.clone())
            }
            (before, after) if before == after => None,
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "F3D procedural-curve edit changes a non-writable definition: {id}"
                )))
            }
        };
        let fit_tolerance = if after.cache_fit_tolerance == before.cache_fit_tolerance {
            None
        } else {
            let tolerance = after.cache_fit_tolerance.ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "cannot remove F3D procedural-curve fit tolerance: {id}"
                ))
            })?;
            if before.cache_fit_tolerance.is_none() || !tolerance.is_finite() || tolerance < 0.0 {
                return Err(CodecError::Malformed(format!(
                    "F3D procedural-curve fit tolerance must replace a finite nonnegative value: {id}"
                )));
            }
            Some(tolerance)
        };
        if definition.is_some() || fit_tolerance.is_some() {
            edits.insert(
                id.to_owned(),
                ProceduralCurveEdit {
                    definition,
                    fit_tolerance,
                },
            );
        }
    }
    Ok(edits)
}

fn finite_point(point: Point3) -> bool {
    point.x.is_finite() && point.y.is_finite() && point.z.is_finite()
}

fn valid_edited_curve_structure(before: &NurbsCurve, after: &NurbsCurve) -> bool {
    valid_edited_nurbs_direction(
        &before.knots,
        after.degree,
        &after.knots,
        after.control_points.len(),
    )
}

fn valid_edited_nurbs_direction(
    before_knots: &[f64],
    after_degree: u32,
    after_knots: &[f64],
    control_count: usize,
) -> bool {
    let Ok(degree) = usize::try_from(after_degree) else {
        return false;
    };
    (1..=20).contains(&after_degree)
        && after_knots.len() == control_count + degree + 1
        && unique_knot_count(after_knots) == unique_knot_count(before_knots)
        && after_knots.iter().all(|value| value.is_finite())
        && after_knots.windows(2).all(|pair| pair[0] <= pair[1])
}

fn unique_knot_count(knots: &[f64]) -> usize {
    knots
        .iter()
        .enumerate()
        .filter(|(index, value)| *index == 0 || knots[*index - 1] != **value)
        .count()
}

fn finite_vector(vector: Vector3) -> bool {
    vector.x.is_finite() && vector.y.is_finite() && vector.z.is_finite()
}

fn orthonormal_pair(first: Vector3, second: Vector3) -> bool {
    finite_vector(first)
        && finite_vector(second)
        && (first.norm() - 1.0).abs() <= 1e-9
        && (second.norm() - 1.0).abs() <= 1e-9
        && (first.x * second.x + first.y * second.y + first.z * second.z).abs() <= 1e-9
}

#[allow(clippy::too_many_arguments)]
fn patch_geometry(
    bytes: &mut [u8],
    positions: &BTreeMap<String, Point3>,
    lines: &BTreeMap<String, (Point3, Vector3)>,
    conics: &BTreeMap<String, (Point3, Vector3, Vector3, f64, f64)>,
    degenerate_curves: &BTreeMap<String, Point3>,
    planes: &BTreeMap<String, (Point3, Vector3, Vector3)>,
    spheres: &BTreeMap<String, (Point3, Vector3, Vector3, f64)>,
    tori: &BTreeMap<String, (Point3, Vector3, Vector3, f64, f64)>,
    cones: &BTreeMap<String, (Point3, Vector3, Vector3, f64, f64, f64)>,
    body_transforms: &BTreeMap<String, Transform>,
    entity_colors: &BTreeMap<String, Color>,
    edge_ranges: &BTreeMap<String, [f64; 2]>,
    face_senses: &BTreeMap<String, Sense>,
    coedge_senses: &BTreeMap<String, Sense>,
    procedural_surface_edits: &BTreeMap<String, ProceduralSurfaceEdit>,
    nurbs_surfaces: &BTreeMap<String, NurbsSurfaceEdit>,
    nurbs_curves: &BTreeMap<String, NurbsCurveEdit>,
    pcurves: &BTreeMap<String, NurbsPcurveEdit>,
    procedural_curve_edits: &BTreeMap<String, ProceduralCurveEdit>,
    procedural_surface_fits: &BTreeMap<String, f64>,
    creation_timestamps: &BTreeMap<usize, f64>,
    edge_continuities: &BTreeMap<usize, (Sense, String)>,
    vertex_ownerships: &BTreeMap<usize, (i64, u8)>,
    face_sidedness: &BTreeMap<usize, crate::records::FaceContainment>,
    tolerant_vertices: &BTreeMap<usize, (f64, [f32; 2])>,
) -> Result<(), CodecError> {
    let start = asm_header::record_stream_start(bytes)
        .ok_or_else(|| CodecError::Malformed("active BREP has no SAB record stream".into()))?;
    let limit = asm_header::first_delta_state_offset(bytes).unwrap_or(bytes.len());
    let ref_width = asm_header::parse(bytes).map_or(8, |header| usize::from(header.width));
    let records = sab::frame(bytes, start, limit, ref_width)
        .map_err(|error| CodecError::Malformed(format!("cannot frame active BREP: {error}")))?;
    let header_scale = asm_header::parse(bytes)
        .and_then(|header| header.scale)
        .unwrap_or(1.0);
    patch_framed_geometry(
        bytes,
        &records,
        positions,
        lines,
        conics,
        degenerate_curves,
        planes,
        spheres,
        tori,
        cones,
        body_transforms,
        entity_colors,
        edge_ranges,
        face_senses,
        coedge_senses,
        procedural_surface_edits,
        nurbs_surfaces,
        nurbs_curves,
        pcurves,
        procedural_curve_edits,
        procedural_surface_fits,
        creation_timestamps,
        edge_continuities,
        vertex_ownerships,
        face_sidedness,
        tolerant_vertices,
        header_scale,
    )
}

#[allow(clippy::too_many_arguments)]
fn patch_framed_geometry(
    bytes: &mut [u8],
    records: &[sab::Record],
    positions: &BTreeMap<String, Point3>,
    lines: &BTreeMap<String, (Point3, Vector3)>,
    conics: &BTreeMap<String, (Point3, Vector3, Vector3, f64, f64)>,
    degenerate_curves: &BTreeMap<String, Point3>,
    planes: &BTreeMap<String, (Point3, Vector3, Vector3)>,
    spheres: &BTreeMap<String, (Point3, Vector3, Vector3, f64)>,
    tori: &BTreeMap<String, (Point3, Vector3, Vector3, f64, f64)>,
    cones: &BTreeMap<String, (Point3, Vector3, Vector3, f64, f64, f64)>,
    body_transforms: &BTreeMap<String, Transform>,
    entity_colors: &BTreeMap<String, Color>,
    edge_ranges: &BTreeMap<String, [f64; 2]>,
    face_senses: &BTreeMap<String, Sense>,
    coedge_senses: &BTreeMap<String, Sense>,
    procedural_surface_edits: &BTreeMap<String, ProceduralSurfaceEdit>,
    nurbs_surfaces: &BTreeMap<String, NurbsSurfaceEdit>,
    nurbs_curves: &BTreeMap<String, NurbsCurveEdit>,
    pcurves: &BTreeMap<String, NurbsPcurveEdit>,
    procedural_curve_edits: &BTreeMap<String, ProceduralCurveEdit>,
    procedural_surface_fits: &BTreeMap<String, f64>,
    creation_timestamps: &BTreeMap<usize, f64>,
    edge_continuities: &BTreeMap<usize, (Sense, String)>,
    vertex_ownerships: &BTreeMap<usize, (i64, u8)>,
    face_sidedness: &BTreeMap<usize, crate::records::FaceContainment>,
    tolerant_vertices: &BTreeMap<usize, (f64, [f32; 2])>,
    header_scale: f64,
) -> Result<(), CodecError> {
    let records_by_index = records
        .iter()
        .map(|record| (record.index, record))
        .collect::<BTreeMap<_, _>>();
    let transform_records = records
        .iter()
        .filter(|record| record.head == "body")
        .filter_map(|body| {
            body_transforms
                .get(&format!("f3d:brep:entity#{}", body.index))
                .and_then(|transform| {
                    body.ref_at(5)
                        .map(|reference| (reference as usize, *transform))
                })
        })
        .collect::<BTreeMap<_, _>>();
    let ref_pcurve_geometry = records
        .iter()
        .filter(|record| record.head == "pcurve")
        .filter_map(|record| {
            let edit = pcurves.get(&format!("f3d:brep:entity#{}", record.index))?;
            let target = usize::try_from(record.ref_at(4)?).ok()?;
            let mut geometry = edit.clone();
            geometry.wrapper_reversed = None;
            geometry.native_tail_flags = None;
            geometry.parameter_range = None;
            geometry.fit_tolerance = None;
            Some((target, geometry))
        })
        .collect::<BTreeMap<_, _>>();
    let mut color_records = BTreeMap::new();
    for entity in records
        .iter()
        .filter(|record| record.head == "body" || record.head == "face")
    {
        let id = format!("f3d:brep:entity#{}", entity.index);
        let Some(color) = entity_colors.get(&id) else {
            continue;
        };
        let mut next = entity.ref_at(0);
        let mut found = false;
        while let Some(index) = next.and_then(|index| usize::try_from(index).ok()) {
            let Some(attribute) = records_by_index.get(&index) else {
                break;
            };
            if attribute.head.contains("rgb_color") {
                color_records.insert(index, *color);
                found = true;
                break;
            }
            next = attribute.ref_at(0);
        }
        if !found {
            return Err(CodecError::NotImplemented(format!(
                "F3D entity color {id} has no writable rgb_color attribute"
            )));
        }
    }
    for record in records {
        if let Some(timestamp) = creation_timestamps.get(&record.index) {
            if !record.head.contains("ATTRIB_CUSTOM")
                || !record.tokens.iter().any(
                    |token| matches!(token, sab::Token::Str(value) if value == "Timestamp_attrib_def"),
                )
            {
                return Err(CodecError::Malformed(format!(
                    "F3D timestamp record {} has the wrong attribute family",
                    record.index
                )));
            }
            let family = record
                .tokens
                .iter()
                .position(
                    |token| matches!(token, sab::Token::Str(value) if value == "Timestamp_attrib_def"),
                )
                .expect("timestamp family was checked");
            if !matches!(record.chunk(family + 1), Some(sab::Token::Long(1))) {
                return Err(CodecError::Malformed(format!(
                    "F3D timestamp record {} lacks marker 1 after its family",
                    record.index
                )));
            }
            let offset =
                required_payload_field(bytes, record, active_ref_width(bytes), family + 2, 0x06)?;
            bytes[offset + 1..offset + 9].copy_from_slice(&timestamp.to_le_bytes());
            continue;
        }
        if let Some((sense, continuity)) = edge_continuities.get(&record.index) {
            if !matches!(record.head.as_str(), "edge" | "tedge") {
                return Err(CodecError::Malformed(format!(
                    "F3D edge-continuity record {} is not an edge",
                    record.index
                )));
            }
            let ref_width = active_ref_width(bytes);
            patch_sense_field(bytes, record, ref_width, 9, *sense)?;
            patch_ascii_field(bytes, record, ref_width, 10, continuity)?;
        }
        if let Some((owning_edge, endpoint_index)) = vertex_ownerships.get(&record.index) {
            if !matches!(record.head.as_str(), "vertex" | "tvertex") {
                return Err(CodecError::Malformed(format!(
                    "F3D vertex-ownership record {} is not a vertex",
                    record.index
                )));
            }
            let ref_width = active_ref_width(bytes);
            for (index, tag, value) in [
                (3usize, 0x0c, *owning_edge),
                (4, 0x04, i64::from(*endpoint_index)),
            ] {
                patch_integer_field(bytes, record, ref_width, index, tag, value)?;
            }
        }
        if let Some(containment) = face_sidedness.get(&record.index) {
            if record.head != "face" || !matches!(record.chunk(9), Some(sab::Token::True)) {
                return Err(CodecError::Malformed(format!(
                    "F3D face-sidedness record {} is not double-sided",
                    record.index
                )));
            }
            let sense = match containment {
                crate::records::FaceContainment::In => Sense::Reversed,
                crate::records::FaceContainment::Out => Sense::Forward,
            };
            patch_sense_field(bytes, record, active_ref_width(bytes), 10, sense)?;
        }
        if let Some((tolerance, trailing)) = tolerant_vertices.get(&record.index) {
            if record.head != "tvertex" {
                return Err(CodecError::Malformed(format!(
                    "F3D tolerant-vertex record {} is not a tvertex",
                    record.index
                )));
            }
            let ref_width = active_ref_width(bytes);
            let tolerance_offset = required_payload_field(bytes, record, ref_width, 6, 0x06)?;
            bytes[tolerance_offset + 1..tolerance_offset + 9]
                .copy_from_slice(&tolerance.to_le_bytes());
            for (index, value) in [(7usize, trailing[0]), (8, trailing[1])] {
                let offset = required_payload_field(bytes, record, ref_width, index, 0x05)?;
                bytes[offset + 1..offset + 5].copy_from_slice(&value.to_le_bytes());
            }
        }
        if let Some(color) = color_records.get(&record.index) {
            let ref_width = active_ref_width(bytes);
            for (index, value) in [
                (1usize, f64::from(color.r)),
                (2, f64::from(color.g)),
                (3, f64::from(color.b)),
            ] {
                let offset = required_payload_field(bytes, record, ref_width, index, 0x06)?;
                bytes[offset + 1..offset + 9].copy_from_slice(&value.to_le_bytes());
            }
            continue;
        }
        if let Some(transform) = transform_records.get(&record.index) {
            patch_transform_record(bytes, record, *transform, header_scale)?;
            continue;
        }
        let id = format!("f3d:brep:entity#{}", record.index);
        if let Some(edit) = ref_pcurve_geometry.get(&record.index) {
            patch_nurbs_pcurve_record(bytes, record, edit)?;
        }
        if let Some(edit) = pcurves.get(&id) {
            if record.ref_at(4).is_some() {
                patch_ref_pcurve_contract(bytes, record, edit)?;
            } else {
                patch_nurbs_pcurve_record(bytes, record, edit)?;
            }
        }
        if let Some(edit) = nurbs_curves.get(&id) {
            patch_nurbs_curve_record(bytes, record, edit, false)?;
        }
        let procedural_curve_id = format!("f3d:brep:procedural_curve#{}", record.index);
        if let Some(edit) = procedural_curve_edits.get(&procedural_curve_id) {
            if let Some(tolerance) = edit.fit_tolerance {
                patch_procedural_curve_fit(bytes, record, tolerance)?;
            }
            if let Some(definition) = &edit.definition {
                match definition {
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix { .. } => {
                        patch_helix_definition(bytes, record, definition)?;
                    }
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::VectorOffset { .. } => {
                        patch_vector_offset_definition(bytes, record, definition)?;
                    }
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset { .. } => {
                        patch_subset_definition(bytes, record, definition)?;
                    }
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound { .. } => {
                        patch_compound_definition(bytes, record, definition)?;
                    }
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::TwoSidedOffset { .. } => {
                        patch_two_sided_offset_definition(bytes, record, definition)?;
                    }
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceOffset { .. } => {
                        patch_surface_offset_definition(bytes, record, definition)?;
                    }
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::Spring { .. } => {
                        patch_spring_definition(bytes, record, definition)?;
                    }
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::Projection { .. } => {
                        patch_projection_definition(bytes, record, definition)?;
                    }
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection { .. } => {
                        patch_intersection_definition(bytes, record, definition)?;
                    }
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::ThreeSurfaceIntersection {
                        ..
                    } => patch_three_surface_intersection_definition(bytes, record, definition)?,
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve { .. } => {
                        patch_surface_curve_definition(bytes, record, definition)?;
                    }
                    cadmpeg_ir::geometry::ProceduralCurveDefinition::Silhouette { .. } => {
                        patch_silhouette_definition(bytes, record, definition)?;
                    }
                    _ => unreachable!("procedural edit validation limits writable definitions"),
                }
            }
        }
        let directrix_id = format!("f3d:brep:procedural_surface#{}:directrix", record.index);
        if let Some(edit) = nurbs_curves.get(&directrix_id) {
            patch_nurbs_curve_record(bytes, record, edit, false)?;
        }
        let spine_id = format!("f3d:brep:procedural_surface#{}:spine", record.index);
        if let Some(edit) = nurbs_curves.get(&spine_id) {
            patch_nurbs_curve_record(bytes, record, edit, true)?;
        }
        if let Some(edit) = nurbs_surfaces.get(&id) {
            patch_nurbs_surface_record(bytes, record, edit, None)?;
        }
        for side in 0..2 {
            let support_id = format!("f3d:brep:procedural_surface#{}:support{side}", record.index);
            if let Some(edit) = nurbs_surfaces.get(&support_id) {
                patch_nurbs_surface_record(bytes, record, edit, Some(side))?;
            }
        }
        let procedural_id = format!("f3d:brep:procedural_surface#{}", record.index);
        if let Some(tolerance) = procedural_surface_fits.get(&procedural_id) {
            patch_procedural_surface_fit(bytes, record, *tolerance)?;
        }
        if let Some(edit) = procedural_surface_edits.get(&procedural_id) {
            if record.head != "spline" {
                return Err(CodecError::Malformed(format!(
                    "F3D extrusion carrier {procedural_id} is not a spline record"
                )));
            }
            match edit {
                ProceduralSurfaceEdit::Extrusion {
                    parameter_interval,
                    direction,
                    native_position,
                } => {
                    patch_extrusion_definition(
                        bytes,
                        record,
                        *parameter_interval,
                        *direction,
                        *native_position,
                    )?;
                }
                ProceduralSurfaceEdit::BlendRadii(radii) => {
                    patch_blend_radius_tokens(bytes, record, *radii)?;
                }
            }
        }
        if record.head == "face" {
            if let Some(sense) = face_senses.get(&id) {
                patch_sense_field(bytes, record, active_ref_width(bytes), 8, *sense)?;
            }
        } else if matches!(record.head.as_str(), "coedge" | "tcoedge") {
            if let Some(sense) = coedge_senses.get(&id) {
                patch_sense_field(bytes, record, active_ref_width(bytes), 7, *sense)?;
            }
        } else if matches!(record.head.as_str(), "edge" | "tedge") {
            if let Some(range) = edge_ranges.get(&id) {
                let ref_width = active_ref_width(bytes);
                for (index, value) in [(4usize, range[0]), (6, range[1])] {
                    let offset = required_payload_field(bytes, record, ref_width, index, 0x06)?;
                    bytes[offset + 1..offset + 9].copy_from_slice(&value.to_le_bytes());
                }
            }
        } else if record.head == "point" {
            if let Some(position) = positions.get(&id) {
                let offset =
                    required_payload_field(bytes, record, active_ref_width(bytes), 3, 0x13)?;
                for (component, value) in [position.x / 10.0, position.y / 10.0, position.z / 10.0]
                    .into_iter()
                    .enumerate()
                {
                    let at = offset + 1 + component * 8;
                    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
                }
            }
        } else if record.head == "straight" {
            if let Some((origin, direction)) = lines.get(&id) {
                let field_indices = match record.name.as_str() {
                    "straight" => [0, 1],
                    "straight-curve" => [3, 4],
                    _ => {
                        return Err(CodecError::Malformed(format!(
                            "straight record {} has unsupported carrier name {}",
                            record.index, record.name
                        )))
                    }
                };
                let ref_width = active_ref_width(bytes);
                let fields = [
                    required_payload_field(bytes, record, ref_width, field_indices[0], 0x13)?,
                    required_payload_field(bytes, record, ref_width, field_indices[1], 0x14)?,
                ];
                for (offset, values) in fields.into_iter().zip([
                    [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0],
                    [direction.x, direction.y, direction.z],
                ]) {
                    for (component, value) in values.into_iter().enumerate() {
                        let at = offset + 1 + component * 8;
                        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
                    }
                }
            }
        } else if record.head == "degenerate_curve" {
            if let Some(point) = degenerate_curves.get(&id) {
                let field_index = match record.name.as_str() {
                    "degenerate_curve" => 0,
                    "degenerate_curve-curve" => 3,
                    _ => {
                        return Err(CodecError::Malformed(format!(
                            "degenerate-curve record {} has unsupported carrier name {}",
                            record.index, record.name
                        )))
                    }
                };
                let offset = required_payload_field(
                    bytes,
                    record,
                    active_ref_width(bytes),
                    field_index,
                    0x13,
                )?;
                for (component, value) in [point.x / 10.0, point.y / 10.0, point.z / 10.0]
                    .into_iter()
                    .enumerate()
                {
                    let at = offset + 1 + component * 8;
                    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
                }
            }
        } else if record.head == "ellipse" {
            if let Some((center, axis, direction, major_radius, minor_radius)) = conics.get(&id) {
                let field_indices = match record.name.as_str() {
                    "ellipse" => [0, 1, 2, 3],
                    "ellipse-curve" => [3, 4, 5, 6],
                    _ => {
                        return Err(CodecError::Malformed(format!(
                            "ellipse record {} has unsupported carrier name {}",
                            record.index, record.name
                        )))
                    }
                };
                let ref_width = active_ref_width(bytes);
                let fields = [
                    required_payload_field(bytes, record, ref_width, field_indices[0], 0x13)?,
                    required_payload_field(bytes, record, ref_width, field_indices[1], 0x14)?,
                    required_payload_field(bytes, record, ref_width, field_indices[2], 0x14)?,
                    required_payload_field(bytes, record, ref_width, field_indices[3], 0x06)?,
                ];
                let major = major_radius / 10.0;
                for (offset, values) in fields[..3].iter().zip([
                    [center.x / 10.0, center.y / 10.0, center.z / 10.0],
                    [axis.x, axis.y, axis.z],
                    [
                        direction.x * major,
                        direction.y * major,
                        direction.z * major,
                    ],
                ]) {
                    for (component, value) in values.into_iter().enumerate() {
                        let at = offset + 1 + component * 8;
                        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
                    }
                }
                let ratio = minor_radius / major_radius;
                let old_ratio = f64::from_le_bytes(
                    bytes[fields[3] + 1..fields[3] + 9]
                        .try_into()
                        .expect("framed ellipse ratio has eight payload bytes"),
                );
                let signed_ratio = if old_ratio.is_sign_negative() {
                    -ratio
                } else {
                    ratio
                };
                bytes[fields[3] + 1..fields[3] + 9].copy_from_slice(&signed_ratio.to_le_bytes());
            }
        } else if record.head == "plane" {
            if let Some((origin, normal, u_axis)) = planes.get(&id) {
                let field_indices = match record.name.as_str() {
                    "plane" => [0, 1, 2],
                    "plane-surface" => [3, 4, 5],
                    _ => {
                        return Err(CodecError::Malformed(format!(
                            "plane record {} has unsupported carrier name {}",
                            record.index, record.name
                        )))
                    }
                };
                let ref_width = active_ref_width(bytes);
                let fields = [
                    required_payload_field(bytes, record, ref_width, field_indices[0], 0x13)?,
                    required_payload_field(bytes, record, ref_width, field_indices[1], 0x14)?,
                    required_payload_field(bytes, record, ref_width, field_indices[2], 0x14)?,
                ];
                for (offset, values) in fields.into_iter().zip([
                    [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0],
                    [normal.x, normal.y, normal.z],
                    [u_axis.x, u_axis.y, u_axis.z],
                ]) {
                    for (component, value) in values.into_iter().enumerate() {
                        let at = offset + 1 + component * 8;
                        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
                    }
                }
            }
        } else if record.head == "sphere" {
            if let Some((center, axis, ref_direction, radius)) = spheres.get(&id) {
                let field_indices = match record.name.as_str() {
                    "sphere" => [0, 1, 2, 3],
                    "sphere-surface" => [3, 4, 5, 6],
                    _ => {
                        return Err(CodecError::Malformed(format!(
                            "sphere record {} has unsupported carrier name {}",
                            record.index, record.name
                        )))
                    }
                };
                let ref_width = active_ref_width(bytes);
                let fields = [
                    required_payload_field(bytes, record, ref_width, field_indices[0], 0x13)?,
                    required_payload_field(bytes, record, ref_width, field_indices[1], 0x06)?,
                    required_payload_field(bytes, record, ref_width, field_indices[2], 0x14)?,
                    required_payload_field(bytes, record, ref_width, field_indices[3], 0x14)?,
                ];
                for (offset, values) in [fields[0], fields[2], fields[3]].into_iter().zip([
                    [center.x / 10.0, center.y / 10.0, center.z / 10.0],
                    [ref_direction.x, ref_direction.y, ref_direction.z],
                    [axis.x, axis.y, axis.z],
                ]) {
                    for (component, value) in values.into_iter().enumerate() {
                        let at = offset + 1 + component * 8;
                        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
                    }
                }
                bytes[fields[1] + 1..fields[1] + 9].copy_from_slice(&(radius / 10.0).to_le_bytes());
            }
        } else if record.head == "torus" {
            if let Some((center, axis, ref_direction, major_radius, minor_radius)) = tori.get(&id) {
                let field_indices = match record.name.as_str() {
                    "torus" => [0, 1, 2, 3, 4],
                    "torus-surface" => [3, 4, 5, 6, 7],
                    _ => {
                        return Err(CodecError::Malformed(format!(
                            "torus record {} has unsupported carrier name {}",
                            record.index, record.name
                        )))
                    }
                };
                let ref_width = active_ref_width(bytes);
                let fields = [
                    required_payload_field(bytes, record, ref_width, field_indices[0], 0x13)?,
                    required_payload_field(bytes, record, ref_width, field_indices[1], 0x14)?,
                    required_payload_field(bytes, record, ref_width, field_indices[2], 0x06)?,
                    required_payload_field(bytes, record, ref_width, field_indices[3], 0x06)?,
                    required_payload_field(bytes, record, ref_width, field_indices[4], 0x14)?,
                ];
                for (offset, values) in [fields[0], fields[1], fields[4]].into_iter().zip([
                    [center.x / 10.0, center.y / 10.0, center.z / 10.0],
                    [axis.x, axis.y, axis.z],
                    [ref_direction.x, ref_direction.y, ref_direction.z],
                ]) {
                    for (component, value) in values.into_iter().enumerate() {
                        let at = offset + 1 + component * 8;
                        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
                    }
                }
                for (offset, value) in [fields[2], fields[3]]
                    .into_iter()
                    .zip([major_radius / 10.0, minor_radius / 10.0])
                {
                    bytes[offset + 1..offset + 9].copy_from_slice(&value.to_le_bytes());
                }
            }
        } else if record.head == "cone" {
            if let Some((origin, axis, ref_direction, radius, ratio, half_angle)) = cones.get(&id) {
                let ref_width = active_ref_width(bytes);
                let field_indices = match record.name.as_str() {
                    "cone" => [0, 1, 2, 3, 4, 5, 6],
                    "cone-surface" => [3, 4, 5, 6, 9, 10, 11],
                    _ => {
                        return Err(CodecError::Malformed(format!(
                            "cone record {} has unsupported carrier name {}",
                            record.index, record.name
                        )))
                    }
                };
                let fields = [
                    required_payload_field(bytes, record, ref_width, field_indices[0], 0x13)?,
                    required_payload_field(bytes, record, ref_width, field_indices[1], 0x14)?,
                    required_payload_field(bytes, record, ref_width, field_indices[2], 0x14)?,
                    required_payload_field(bytes, record, ref_width, field_indices[3], 0x06)?,
                    required_payload_field(bytes, record, ref_width, field_indices[4], 0x06)?,
                    required_payload_field(bytes, record, ref_width, field_indices[5], 0x06)?,
                    required_payload_field(bytes, record, ref_width, field_indices[6], 0x06)?,
                ];
                let old_sine = f64::from_le_bytes(
                    bytes[fields[4] + 1..fields[4] + 9]
                        .try_into()
                        .expect("framed cone sine has eight payload bytes"),
                );
                let old_cosine = f64::from_le_bytes(
                    bytes[fields[5] + 1..fields[5] + 9]
                        .try_into()
                        .expect("framed cone cosine has eight payload bytes"),
                );
                let sine_sign = if old_sine < 0.0 { -1.0 } else { 1.0 };
                let cosine_sign = if old_cosine < 0.0 { -1.0 } else { 1.0 };
                let native_axis = if *half_angle > 0.0 && sine_sign * cosine_sign < 0.0 {
                    Vector3::new(-axis.x, -axis.y, -axis.z)
                } else {
                    *axis
                };
                let scaled_radius = radius / 10.0;
                for (offset, values) in fields[..3].iter().zip([
                    [origin.x / 10.0, origin.y / 10.0, origin.z / 10.0],
                    [native_axis.x, native_axis.y, native_axis.z],
                    [
                        ref_direction.x * scaled_radius,
                        ref_direction.y * scaled_radius,
                        ref_direction.z * scaled_radius,
                    ],
                ]) {
                    for (component, value) in values.into_iter().enumerate() {
                        let at = offset + 1 + component * 8;
                        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
                    }
                }
                for (offset, value) in fields[3..].iter().zip([
                    *ratio,
                    sine_sign * half_angle.sin(),
                    cosine_sign * half_angle.cos(),
                    scaled_radius,
                ]) {
                    bytes[offset + 1..offset + 9].copy_from_slice(&value.to_le_bytes());
                }
            }
        }
    }
    Ok(())
}

fn required_payload_field(
    bytes: &[u8],
    record: &sab::Record,
    ref_width: usize,
    index: usize,
    tag: u8,
) -> Result<usize, CodecError> {
    let offset = sab::payload_token_offset(bytes, record, ref_width, index).ok_or_else(|| {
        CodecError::Malformed(format!(
            "{} record {} lacks payload field {index}",
            record.head, record.index
        ))
    })?;
    if bytes.get(offset) != Some(&tag) {
        return Err(CodecError::Malformed(format!(
            "{} record {} payload field {index} is not tag {tag:#04x}",
            record.head, record.index
        )));
    }
    Ok(offset)
}

fn patch_extrusion_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    parameter_interval: [f64; 2],
    direction: Vector3,
    native_position: cadmpeg_ir::math::Point3,
) -> Result<(), CodecError> {
    let end = record.offset.checked_add(record.len).ok_or_else(|| {
        CodecError::Malformed("extrusion record extent overflows address space".into())
    })?;
    let record_bytes = bytes
        .get(record.offset..end)
        .ok_or_else(|| CodecError::Malformed("extrusion record is truncated".into()))?;
    let layout = crate::nurbs::extrusion_patch_layout(record_bytes, active_ref_width(bytes))
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "spline record {} lacks writable extrusion fields",
                record.index
            ))
        })?;
    for (offset, value) in layout
        .parameter_interval
        .into_iter()
        .zip(parameter_interval)
    {
        let at = record.offset + offset;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    for (base, values) in [
        (
            layout.direction,
            [direction.x / 10.0, direction.y / 10.0, direction.z / 10.0],
        ),
        (
            layout.native_position,
            [
                native_position.x / 10.0,
                native_position.y / 10.0,
                native_position.z / 10.0,
            ],
        ),
    ] {
        for (component, value) in values.into_iter().enumerate() {
            let at = record.offset + base + component * 8;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(())
}

fn patch_ascii_field(
    bytes: &mut [u8],
    record: &sab::Record,
    ref_width: usize,
    index: usize,
    value: &str,
) -> Result<(), CodecError> {
    let offset = required_payload_field(bytes, record, ref_width, index, 0x07)?;
    let encoded_length = bytes.get(offset + 1).copied().ok_or_else(|| {
        CodecError::Malformed(format!("{} record string is truncated", record.head))
    })? as usize;
    if value.len() != encoded_length || !value.is_ascii() {
        return Err(CodecError::NotImplemented(format!(
            "{} record {} string edit must retain its encoded ASCII length",
            record.head, record.index
        )));
    }
    bytes[offset + 2..offset + 2 + encoded_length].copy_from_slice(value.as_bytes());
    Ok(())
}

fn patch_integer_field(
    bytes: &mut [u8],
    record: &sab::Record,
    ref_width: usize,
    index: usize,
    tag: u8,
    value: i64,
) -> Result<(), CodecError> {
    let offset = required_payload_field(bytes, record, ref_width, index, tag)?;
    if ref_width == 4 && i64::from(value as i32) != value {
        return Err(CodecError::NotImplemented(format!(
            "{} record {} integer edit exceeds BinaryFile4 range",
            record.head, record.index
        )));
    }
    bytes[offset + 1..offset + 1 + ref_width].copy_from_slice(&value.to_le_bytes()[..ref_width]);
    Ok(())
}

fn patch_transform_record(
    bytes: &mut [u8],
    record: &sab::Record,
    transform: Transform,
    header_scale: f64,
) -> Result<(), CodecError> {
    if header_scale == 0.0 {
        return Err(CodecError::Malformed(format!(
            "transform record {} has zero header scale",
            record.index
        )));
    }
    let ref_width = active_ref_width(bytes);
    let vectors = [
        [
            transform.rows[0][0],
            transform.rows[1][0],
            transform.rows[2][0],
        ],
        [
            transform.rows[0][1],
            transform.rows[1][1],
            transform.rows[2][1],
        ],
        [
            transform.rows[0][2],
            transform.rows[1][2],
            transform.rows[2][2],
        ],
        [
            transform.rows[0][3] / (header_scale * 10.0),
            transform.rows[1][3] / (header_scale * 10.0),
            transform.rows[2][3] / (header_scale * 10.0),
        ],
    ];
    for (index, vector) in vectors.into_iter().enumerate() {
        let offset = required_payload_field(bytes, record, ref_width, index, 0x14)?;
        for (component, value) in vector.into_iter().enumerate() {
            let at = offset + 1 + component * 8;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    let scale = required_payload_field(bytes, record, ref_width, 4, 0x06)?;
    bytes[scale + 1..scale + 9].copy_from_slice(&transform.rows[3][3].to_le_bytes());
    Ok(())
}

fn patch_sense_field(
    bytes: &mut [u8],
    record: &sab::Record,
    ref_width: usize,
    index: usize,
    sense: Sense,
) -> Result<(), CodecError> {
    let offset = sab::payload_token_offset(bytes, record, ref_width, index).ok_or_else(|| {
        CodecError::Malformed(format!(
            "{} record {} lacks payload field {index}",
            record.head, record.index
        ))
    })?;
    if !matches!(bytes.get(offset), Some(0x0a | 0x0b)) {
        return Err(CodecError::Malformed(format!(
            "{} record {} payload field {index} is not a sense token",
            record.head, record.index
        )));
    }
    bytes[offset] = match sense {
        Sense::Forward => 0x0b,
        Sense::Reversed => 0x0a,
    };
    Ok(())
}

fn active_ref_width(bytes: &[u8]) -> usize {
    asm_header::parse(bytes).map_or(8, |header| usize::from(header.width))
}

fn patch_blend_radius_tokens(
    bytes: &mut [u8],
    record: &sab::Record,
    radii: [f64; 2],
) -> Result<(), CodecError> {
    let end = record.offset.checked_add(record.len).ok_or_else(|| {
        CodecError::Malformed("rolling-ball record extent overflows address space".into())
    })?;
    let record_bytes = bytes
        .get(record.offset..end)
        .ok_or_else(|| CodecError::Malformed("rolling-ball record is truncated".into()))?;
    let layout = crate::nurbs::rolling_ball_patch_layout(record_bytes, active_ref_width(bytes))
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "spline record {} lacks a writable rolling-ball radius pair",
                record.index
            ))
        })?;
    for (offset, radius) in layout.radii.into_iter().zip(radii) {
        let payload = record.offset + offset;
        bytes[payload..payload + 8].copy_from_slice(&(radius / 10.0).to_le_bytes());
    }
    Ok(())
}

fn patch_nurbs_surface_record(
    bytes: &mut [u8],
    record: &sab::Record,
    edit: &NurbsSurfaceEdit,
    surface_ordinal: Option<usize>,
) -> Result<(), CodecError> {
    let surface = &edit.surface;
    let end = record.offset.checked_add(record.len).ok_or_else(|| {
        CodecError::Malformed("NURBS surface record extent overflows address space".into())
    })?;
    let record_bytes = bytes
        .get(record.offset..end)
        .ok_or_else(|| CodecError::Malformed("NURBS surface record is truncated".into()))?;
    let layout = surface_ordinal
        .map_or_else(
            || crate::nurbs::final_surface_patch_layout(record_bytes),
            |ordinal| crate::nurbs::surface_patch_layout_at(record_bytes, ordinal),
        )
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "spline record {} has no writable surface cache",
                record.index
            ))
        })?;
    let u_count = usize::try_from(surface.u_count)
        .map_err(|_| CodecError::Malformed("NURBS u pole count exceeds address space".into()))?;
    let v_count = usize::try_from(surface.v_count)
        .map_err(|_| CodecError::Malformed("NURBS v pole count exceeds address space".into()))?;
    if layout.u_count != u_count
        || layout.v_count != v_count
        || layout.rational != surface.weights.is_some()
    {
        return Err(CodecError::NotImplemented(format!(
            "spline record {} changed NURBS cache structure",
            record.index
        )));
    }
    patch_knot_structure(
        bytes,
        record.offset,
        &layout.u_knots,
        &surface.u_knots,
        layout.int_width,
    )?;
    patch_knot_structure(
        bytes,
        record.offset,
        &layout.v_knots,
        &surface.v_knots,
        layout.int_width,
    )?;
    for (offset, degree) in layout
        .degree_value_offsets
        .into_iter()
        .zip([surface.u_degree, surface.v_degree])
    {
        let at = record.offset + offset;
        patch_layout_integer(bytes, at, layout.int_width, i64::from(degree))?;
    }
    if let Some(periodic) = edit.periodic {
        for (offset, periodic) in layout.periodic_value_offsets.into_iter().zip(periodic) {
            let at = record.offset + offset;
            let value = if periodic { 2i64 } else { 0i64 };
            patch_layout_integer(bytes, at, layout.int_width, value)?;
        }
    }
    let components = if layout.rational { 4 } else { 3 };
    if layout.control_value_offsets.len() != u_count * v_count * components {
        return Err(CodecError::Malformed(format!(
            "spline record {} has an inconsistent NURBS control layout",
            record.index
        )));
    }
    let weights = surface.weights.as_deref();
    let mut ordinal = 0usize;
    for v in 0..v_count {
        for u in 0..u_count {
            let ir_index = u * v_count + v;
            let point = surface.control_points[ir_index];
            let values = [
                point.x / 10.0,
                point.y / 10.0,
                point.z / 10.0,
                weights.map_or(0.0, |weights| weights[ir_index]),
            ];
            for value in values.into_iter().take(components) {
                let at = record.offset + layout.control_value_offsets[ordinal];
                bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
                ordinal += 1;
            }
        }
    }
    Ok(())
}

fn patch_procedural_surface_fit(
    bytes: &mut [u8],
    record: &sab::Record,
    tolerance: f64,
) -> Result<(), CodecError> {
    let end = record.offset.checked_add(record.len).ok_or_else(|| {
        CodecError::Malformed("procedural-surface record extent overflows address space".into())
    })?;
    let record_bytes = bytes
        .get(record.offset..end)
        .ok_or_else(|| CodecError::Malformed("procedural-surface record is truncated".into()))?;
    let layout = crate::nurbs::final_surface_patch_layout(record_bytes).ok_or_else(|| {
        CodecError::Malformed(format!(
            "spline record {} has no solved surface cache",
            record.index
        ))
    })?;
    if record_bytes.get(layout.end) != Some(&0x06) {
        return Err(CodecError::NotImplemented(format!(
            "spline record {} has no writable fit-tolerance carrier",
            record.index
        )));
    }
    let at = record.offset + layout.end + 1;
    bytes[at..at + 8].copy_from_slice(&(tolerance / 10.0).to_le_bytes());
    Ok(())
}

fn patch_nurbs_curve_record(
    bytes: &mut [u8],
    record: &sab::Record,
    edit: &NurbsCurveEdit,
    final_cache: bool,
) -> Result<(), CodecError> {
    let curve = &edit.curve;
    let end = record.offset.checked_add(record.len).ok_or_else(|| {
        CodecError::Malformed("NURBS curve record extent overflows address space".into())
    })?;
    let record_bytes = bytes
        .get(record.offset..end)
        .ok_or_else(|| CodecError::Malformed("NURBS curve record is truncated".into()))?;
    let layout = if final_cache {
        crate::nurbs::final_curve_patch_layout(record_bytes)
    } else {
        crate::nurbs::first_curve_patch_layout(record_bytes)
    }
    .ok_or_else(|| {
        CodecError::Malformed(format!(
            "spline record {} has no writable curve cache",
            record.index
        ))
    })?;
    if layout.control_count != curve.control_points.len()
        || layout.rational != curve.weights.is_some()
    {
        return Err(CodecError::NotImplemented(format!(
            "spline record {} changed NURBS curve structure",
            record.index
        )));
    }
    patch_knot_structure(
        bytes,
        record.offset,
        &layout.knots,
        &curve.knots,
        layout.int_width,
    )?;
    let degree_at = record.offset + layout.degree_value_offset;
    patch_layout_integer(bytes, degree_at, layout.int_width, i64::from(curve.degree))?;
    if let Some(periodic) = edit.periodic {
        let periodic = if periodic { 2i64 } else { 0i64 };
        let periodic_at = record.offset + layout.periodic_value_offset;
        patch_layout_integer(bytes, periodic_at, layout.int_width, periodic)?;
    }
    let components = if layout.rational { 4 } else { 3 };
    if layout.control_value_offsets.len() != curve.control_points.len() * components {
        return Err(CodecError::Malformed(format!(
            "spline record {} has an inconsistent NURBS curve layout",
            record.index
        )));
    }
    let weights = curve.weights.as_deref();
    let mut ordinal = 0usize;
    for (index, point) in curve.control_points.iter().enumerate() {
        let values = [
            point.x / 10.0,
            point.y / 10.0,
            point.z / 10.0,
            weights.map_or(0.0, |weights| weights[index]),
        ];
        for value in values.into_iter().take(components) {
            let at = record.offset + layout.control_value_offsets[ordinal];
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            ordinal += 1;
        }
    }
    Ok(())
}

fn patch_procedural_curve_fit(
    bytes: &mut [u8],
    record: &sab::Record,
    tolerance: f64,
) -> Result<(), CodecError> {
    let end = record.offset.checked_add(record.len).ok_or_else(|| {
        CodecError::Malformed("procedural-curve record extent overflows address space".into())
    })?;
    let record_bytes = bytes
        .get(record.offset..end)
        .ok_or_else(|| CodecError::Malformed("procedural-curve record is truncated".into()))?;
    let layout = crate::nurbs::final_curve_patch_layout(record_bytes).ok_or_else(|| {
        CodecError::Malformed(format!(
            "intcurve record {} has no solved curve cache",
            record.index
        ))
    })?;
    if record_bytes.get(layout.end) != Some(&0x06) {
        return Err(CodecError::NotImplemented(format!(
            "intcurve record {} has no writable fit-tolerance carrier",
            record.index
        )));
    }
    let at = record.offset + layout.end + 1;
    bytes[at..at + 8].copy_from_slice(&(tolerance / 10.0).to_le_bytes());
    Ok(())
}

fn patch_helix_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
        angle_range,
        center,
        major,
        minor,
        pitch,
        apex_factor,
        axis,
    } = definition
    else {
        return Err(CodecError::Malformed(
            "helix patch received a non-helix definition".into(),
        ));
    };
    let end = record.offset.checked_add(record.len).ok_or_else(|| {
        CodecError::Malformed("helix record extent overflows address space".into())
    })?;
    let record_bytes = bytes
        .get(record.offset..end)
        .ok_or_else(|| CodecError::Malformed("helix record is truncated".into()))?;
    let layout = crate::nurbs::helix_patch_layout(record_bytes, active_ref_width(bytes))
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "procedural curve record {} lacks writable helix fields",
                record.index
            ))
        })?;
    for (offset, value) in layout.angle_range.into_iter().zip(*angle_range) {
        let at = record.offset + offset;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    for (offset, value) in layout.frame_vectors.into_iter().zip([
        [center.x / 10.0, center.y / 10.0, center.z / 10.0],
        [major.x / 10.0, major.y / 10.0, major.z / 10.0],
        [minor.x / 10.0, minor.y / 10.0, minor.z / 10.0],
        [pitch.x / 10.0, pitch.y / 10.0, pitch.z / 10.0],
    ]) {
        for (component, value) in value.into_iter().enumerate() {
            let at = record.offset + offset + component * 8;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    let apex_at = record.offset + layout.apex_factor;
    bytes[apex_at..apex_at + 8].copy_from_slice(&apex_factor.to_le_bytes());
    for (component, value) in [axis.x, axis.y, axis.z].into_iter().enumerate() {
        let at = record.offset + layout.axis + component * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

fn patch_vector_offset_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::VectorOffset {
        parameter_range,
        offset,
        ..
    } = definition
    else {
        return Err(CodecError::Malformed(
            "vector-offset patch received another definition".into(),
        ));
    };
    let end = record.offset + record.len;
    let record_bytes = bytes
        .get(record.offset..end)
        .ok_or_else(|| CodecError::Malformed("vector-offset record is truncated".into()))?;
    let layout = crate::nurbs::vector_offset_patch_layout(record_bytes, active_ref_width(bytes))
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "vector-offset record {} lacks writable construction fields",
                record.index
            ))
        })?;
    for (offset, value) in layout.parameter_range.into_iter().zip(*parameter_range) {
        let at = record.offset + offset;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    for (component, value) in [offset.x / 10.0, offset.y / 10.0, offset.z / 10.0]
        .into_iter()
        .enumerate()
    {
        let at = record.offset + layout.offset + component * 8;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

fn patch_subset_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset {
        parameter_range, ..
    } = definition
    else {
        return Err(CodecError::Malformed(
            "subset patch received another definition".into(),
        ));
    };
    let end = record.offset + record.len;
    let record_bytes = bytes
        .get(record.offset..end)
        .ok_or_else(|| CodecError::Malformed("subset record is truncated".into()))?;
    let layout = crate::nurbs::subset_patch_layout(record_bytes, active_ref_width(bytes))
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "subset record {} lacks writable construction fields",
                record.index
            ))
        })?;
    for (offset, value) in layout.parameter_range.into_iter().zip(*parameter_range) {
        let at = record.offset + offset;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

fn patch_compound_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Compound {
        parameters,
        component_parameters,
        ..
    } = definition
    else {
        return Err(CodecError::Malformed(
            "compound patch received another definition".into(),
        ));
    };
    let end = record.offset.checked_add(record.len).ok_or_else(|| {
        CodecError::Malformed("compound record extent overflows address space".into())
    })?;
    let record_bytes = bytes
        .get(record.offset..end)
        .ok_or_else(|| CodecError::Malformed("compound record is truncated".into()))?;
    let layout = crate::nurbs::compound_patch_layout(record_bytes, active_ref_width(bytes))
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "compound record {} lacks writable parameter arrays",
                record.index
            ))
        })?;
    if layout.parameters.len() != parameters.len()
        || layout.component_parameters.len() != component_parameters.len()
    {
        return Err(CodecError::NotImplemented(
            "compound edit changes native parameter cardinality".into(),
        ));
    }
    for (offset, value) in layout
        .parameters
        .into_iter()
        .chain(layout.component_parameters)
        .zip(parameters.iter().chain(component_parameters))
    {
        let at = record.offset + offset;
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

fn patch_two_sided_offset_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::TwoSidedOffset {
        context,
        discontinuity_flag,
        offsets,
    } = definition
    else {
        return Err(CodecError::Malformed(
            "two-sided offset patch received another definition".into(),
        ));
    };
    let record_bytes = bytes
        .get(record.offset..record.offset + record.len)
        .ok_or_else(|| CodecError::Malformed("two-sided offset record is truncated".into()))?;
    let layout = [8usize, 4]
        .into_iter()
        .filter_map(|width| crate::nurbs::two_sided_offset_patch_layout(record_bytes, width))
        .find(|layout| {
            layout
                .discontinuities
                .iter()
                .map(Vec::len)
                .eq(context.discontinuities.iter().map(Vec::len))
        })
        .ok_or_else(|| CodecError::Malformed("two-sided offset layout is malformed".into()))?;
    for (at, value) in layout
        .parameter_range
        .into_iter()
        .zip(context.parameter_range)
    {
        patch_f64_payload(bytes, record.offset + at, value)?;
    }
    for (locations, values) in layout.discontinuities.iter().zip(&context.discontinuities) {
        for (at, value) in locations.iter().zip(values) {
            patch_f64_payload(bytes, record.offset + *at, *value)?;
        }
    }
    bytes[record.offset + layout.discontinuity_flag] = native_bool(*discontinuity_flag);
    for (at, value) in layout.offsets.into_iter().zip(offsets) {
        patch_f64_payload(bytes, record.offset + at, *value / 10.0)?;
    }
    Ok(())
}

fn patch_f64_payload(bytes: &mut [u8], at: usize, value: f64) -> Result<(), CodecError> {
    let payload = bytes
        .get_mut(at..at + 8)
        .ok_or_else(|| CodecError::Malformed("native double payload is truncated".into()))?;
    payload.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn patch_surface_offset_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceOffset {
        context,
        discontinuity_flag,
        base_u_range,
        base_v_range,
        base_range,
        distance,
        shift,
        scale,
        ..
    } = definition
    else {
        return Err(CodecError::Malformed(
            "surface-offset patch received another definition".into(),
        ));
    };
    if !distance.is_finite() || !shift.is_finite() || !scale.is_finite() {
        return Err(CodecError::Malformed(
            "surface-offset scalars must be finite".into(),
        ));
    }
    if [base_u_range, base_v_range, base_range]
        .into_iter()
        .flatten()
        .any(|value| !value.is_finite())
    {
        return Err(CodecError::Malformed(
            "surface-offset ranges must be finite".into(),
        ));
    }
    if context
        .parameter_range
        .into_iter()
        .chain(context.discontinuities.iter().flatten().copied())
        .any(|value| !value.is_finite())
    {
        return Err(CodecError::Malformed(
            "surface-offset context values must be finite".into(),
        ));
    }
    let end = record.offset.checked_add(record.len).ok_or_else(|| {
        CodecError::Malformed("surface-offset record extent overflows address space".into())
    })?;
    let record_bytes = bytes
        .get(record.offset..end)
        .ok_or_else(|| CodecError::Malformed("surface-offset record is truncated".into()))?;
    let layout = crate::nurbs::surface_offset_patch_layout(record_bytes, active_ref_width(bytes))
        .ok_or_else(|| {
        CodecError::Malformed("surface-offset construction is malformed".into())
    })?;
    if layout
        .discontinuities
        .iter()
        .map(Vec::len)
        .ne(context.discontinuities.iter().map(Vec::len))
    {
        return Err(CodecError::Malformed(
            "surface-offset context is incomplete".into(),
        ));
    }
    for (offset, value) in layout
        .parameter_range
        .into_iter()
        .chain(layout.discontinuities.into_iter().flatten())
        .chain(layout.base_u_range)
        .chain(layout.base_v_range)
        .chain(layout.base_range)
        .chain([layout.distance, layout.shift, layout.scale])
        .zip(
            context
                .parameter_range
                .into_iter()
                .chain(context.discontinuities.iter().flatten().copied())
                .chain(base_u_range.iter().copied())
                .chain(base_v_range.iter().copied())
                .chain(
                    base_range
                        .iter()
                        .copied()
                        .chain([distance / 10.0, *shift, *scale]),
                ),
        )
    {
        let offset = record.offset + offset;
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes[record.offset + layout.discontinuity_flag] = native_bool(*discontinuity_flag);
    Ok(())
}

fn patch_spring_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Spring {
        context,
        discontinuity_flag,
        direction,
        ..
    } = definition
    else {
        return Err(CodecError::Malformed(
            "spring patch received another definition".into(),
        ));
    };
    if context
        .parameter_range
        .into_iter()
        .chain(context.discontinuities.iter().flatten().copied())
        .any(|value| !value.is_finite())
    {
        return Err(CodecError::Malformed(
            "spring context values must be finite".into(),
        ));
    }
    let end = record.offset.checked_add(record.len).ok_or_else(|| {
        CodecError::Malformed("spring record extent overflows address space".into())
    })?;
    let record_bytes = bytes
        .get(record.offset..end)
        .ok_or_else(|| CodecError::Malformed("spring record is truncated".into()))?;
    let int_width = active_ref_width(bytes);
    let layout = crate::nurbs::spring_patch_layout(record_bytes, int_width)
        .ok_or_else(|| CodecError::Malformed("spring construction is malformed".into()))?;
    if layout
        .discontinuities
        .iter()
        .map(Vec::len)
        .ne(context.discontinuities.iter().map(Vec::len))
    {
        return Err(CodecError::Malformed("spring context is incomplete".into()));
    }
    for (offset, value) in layout
        .parameter_range
        .into_iter()
        .chain(layout.discontinuities.into_iter().flatten())
        .zip(
            context
                .parameter_range
                .into_iter()
                .chain(context.discontinuities.iter().flatten().copied()),
        )
    {
        let offset = record.offset + offset;
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes[record.offset + layout.discontinuity_flag] = native_bool(*discontinuity_flag);
    patch_tagged_integer_at(
        bytes,
        record.offset + layout.direction,
        int_width,
        *direction,
    )?;
    Ok(())
}

fn patch_projection_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Projection {
        context,
        discontinuity_flag,
        tail,
        ..
    } = definition
    else {
        return Err(CodecError::Malformed(
            "projection patch received another definition".into(),
        ));
    };
    if context
        .parameter_range
        .into_iter()
        .chain(context.discontinuities.iter().flatten().copied())
        .any(|value| !value.is_finite())
    {
        return Err(CodecError::Malformed(
            "projection context values must be finite".into(),
        ));
    }
    let end = record.offset.checked_add(record.len).ok_or_else(|| {
        CodecError::Malformed("projection record extent overflows address space".into())
    })?;
    let record_bytes = bytes
        .get(record.offset..end)
        .ok_or_else(|| CodecError::Malformed("projection record is truncated".into()))?;
    let layout = crate::nurbs::projection_patch_layout(record_bytes, active_ref_width(bytes))
        .ok_or_else(|| CodecError::Malformed("projection construction is malformed".into()))?;
    if layout
        .discontinuities
        .iter()
        .map(Vec::len)
        .ne(context.discontinuities.iter().map(Vec::len))
    {
        return Err(CodecError::Malformed(
            "projection context is incomplete".into(),
        ));
    }
    match (&layout.tail, tail) {
        (
            crate::nurbs::ProjectionTailPatchLayout::EarlyClose { flag: offset },
            cadmpeg_ir::geometry::ProjectionTail::EarlyClose { flag },
        ) => bytes[record.offset + offset] = native_bool(*flag),
        (
            crate::nurbs::ProjectionTailPatchLayout::Ranged {
                flag: flag_offset,
                parameter_range: range_offsets,
                role: role_range,
            },
            cadmpeg_ir::geometry::ProjectionTail::Ranged {
                flag,
                parameter_range,
                role,
            },
        ) => {
            if !parameter_range.iter().copied().all(f64::is_finite) {
                return Err(CodecError::Malformed(
                    "projection tail range must be finite".into(),
                ));
            }
            if !role.is_ascii() || role.len() != role_range.len() {
                return Err(CodecError::NotImplemented(
                    "projection role edit must retain its encoded ASCII length".into(),
                ));
            }
            bytes[record.offset + flag_offset] = native_bool(*flag);
            for (offset, value) in range_offsets.iter().zip(parameter_range) {
                let offset = record.offset + offset;
                bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
            }
            let role_target = record.offset + role_range.start..record.offset + role_range.end;
            bytes[role_target].copy_from_slice(role.as_bytes());
        }
        _ => {
            return Err(CodecError::NotImplemented(
                "projection edit cannot change native tail form".into(),
            ))
        }
    }
    for (offset, value) in layout
        .parameter_range
        .into_iter()
        .chain(layout.discontinuities.into_iter().flatten())
        .zip(
            context
                .parameter_range
                .into_iter()
                .chain(context.discontinuities.iter().flatten().copied()),
        )
    {
        let offset = record.offset + offset;
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes[record.offset + layout.discontinuity_flag] = native_bool(*discontinuity_flag);
    Ok(())
}

fn patch_intersection_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Intersection {
        context,
        discontinuity_flag,
    } = definition
    else {
        return Err(CodecError::Malformed(
            "intersection patch received another definition".into(),
        ));
    };
    if context
        .parameter_range
        .into_iter()
        .chain(context.discontinuities.iter().flatten().copied())
        .any(|value| !value.is_finite())
    {
        return Err(CodecError::Malformed(
            "intersection context values must be finite".into(),
        ));
    }
    let end = record.offset.checked_add(record.len).ok_or_else(|| {
        CodecError::Malformed("intersection record extent overflows address space".into())
    })?;
    let record_bytes = bytes
        .get(record.offset..end)
        .ok_or_else(|| CodecError::Malformed("intersection record is truncated".into()))?;
    let layout = crate::nurbs::intersection_patch_layout(record_bytes, active_ref_width(bytes))
        .ok_or_else(|| CodecError::Malformed("intersection construction is malformed".into()))?;
    if layout
        .discontinuities
        .iter()
        .map(Vec::len)
        .ne(context.discontinuities.iter().map(Vec::len))
    {
        return Err(CodecError::Malformed(
            "intersection context is incomplete".into(),
        ));
    }
    for (offset, value) in layout
        .parameter_range
        .into_iter()
        .chain(layout.discontinuities.into_iter().flatten())
        .zip(
            context
                .parameter_range
                .into_iter()
                .chain(context.discontinuities.iter().flatten().copied()),
        )
    {
        let offset = record.offset + offset;
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes[record.offset + layout.discontinuity_flag] = native_bool(*discontinuity_flag);
    Ok(())
}

fn patch_three_surface_intersection_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::ThreeSurfaceIntersection {
        context,
        selector,
        ..
    } = definition
    else {
        return Err(CodecError::Malformed(
            "three-surface intersection patch received another definition".into(),
        ));
    };
    if context
        .parameter_range
        .into_iter()
        .chain(context.discontinuities.iter().flatten().copied())
        .any(|value| !value.is_finite())
    {
        return Err(CodecError::Malformed(
            "three-surface intersection context values must be finite".into(),
        ));
    }
    let end = record.offset.checked_add(record.len).ok_or_else(|| {
        CodecError::Malformed(
            "three-surface intersection record extent overflows address space".into(),
        )
    })?;
    let record_bytes = bytes.get(record.offset..end).ok_or_else(|| {
        CodecError::Malformed("three-surface intersection record is truncated".into())
    })?;
    let int_width = active_ref_width(bytes);
    let layout = crate::nurbs::three_surface_patch_layout(record_bytes, int_width)
        .ok_or_else(|| CodecError::Malformed("three-surface construction is malformed".into()))?;
    if layout
        .discontinuities
        .iter()
        .map(Vec::len)
        .ne(context.discontinuities.iter().map(Vec::len))
    {
        return Err(CodecError::Malformed(
            "three-surface intersection context is incomplete".into(),
        ));
    }
    for (offset, value) in layout
        .parameter_range
        .into_iter()
        .chain(layout.discontinuities.into_iter().flatten())
        .zip(
            context
                .parameter_range
                .into_iter()
                .chain(context.discontinuities.iter().flatten().copied()),
        )
    {
        let offset = record.offset + offset;
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    patch_tagged_integer_at(bytes, record.offset + layout.selector, int_width, *selector)?;
    Ok(())
}

fn patch_surface_curve_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::SurfaceCurve { family, context } =
        definition
    else {
        return Err(CodecError::Malformed(
            "surface-curve patch received another definition".into(),
        ));
    };
    if context
        .parameter_range
        .into_iter()
        .chain(context.discontinuities.iter().flatten().copied())
        .any(|value| !value.is_finite())
    {
        return Err(CodecError::Malformed(
            "surface-curve context values must be finite".into(),
        ));
    }
    let end = record.offset.checked_add(record.len).ok_or_else(|| {
        CodecError::Malformed("surface-curve record extent overflows address space".into())
    })?;
    let record_bytes = bytes
        .get(record.offset..end)
        .ok_or_else(|| CodecError::Malformed("surface-curve record is truncated".into()))?;
    let layout =
        crate::nurbs::surface_curve_patch_layout(record_bytes, active_ref_width(bytes), family)
            .ok_or_else(|| {
                CodecError::Malformed("surface-curve construction is malformed".into())
            })?;
    if layout
        .discontinuities
        .iter()
        .map(Vec::len)
        .ne(context.discontinuities.iter().map(Vec::len))
    {
        return Err(CodecError::Malformed(
            "surface-curve context is incomplete".into(),
        ));
    }
    for (offset, value) in layout
        .parameter_range
        .into_iter()
        .chain(layout.discontinuities.into_iter().flatten())
        .zip(
            context
                .parameter_range
                .into_iter()
                .chain(context.discontinuities.iter().flatten().copied()),
        )
    {
        let offset = record.offset + offset;
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

fn patch_silhouette_definition(
    bytes: &mut [u8],
    record: &sab::Record,
    definition: &cadmpeg_ir::geometry::ProceduralCurveDefinition,
) -> Result<(), CodecError> {
    let cadmpeg_ir::geometry::ProceduralCurveDefinition::Silhouette {
        silhouette,
        light_direction,
        ..
    } = definition
    else {
        return Err(CodecError::Malformed(
            "silhouette patch received another definition".into(),
        ));
    };
    if !finite_vector(*light_direction) {
        return Err(CodecError::Malformed(
            "silhouette light direction must be finite".into(),
        ));
    }
    let draft_factor = match silhouette {
        cadmpeg_ir::geometry::SilhouetteKind::Standard
        | cadmpeg_ir::geometry::SilhouetteKind::Parametric => None,
        cadmpeg_ir::geometry::SilhouetteKind::Taper { draft_factor } => {
            if !draft_factor.is_finite() {
                return Err(CodecError::Malformed(
                    "silhouette draft factor must be finite".into(),
                ));
            }
            Some(*draft_factor)
        }
    };
    let end = record.offset.checked_add(record.len).ok_or_else(|| {
        CodecError::Malformed("silhouette record extent overflows address space".into())
    })?;
    let record_bytes = bytes
        .get(record.offset..end)
        .ok_or_else(|| CodecError::Malformed("silhouette record is truncated".into()))?;
    let layout =
        crate::nurbs::silhouette_patch_layout(record_bytes, active_ref_width(bytes), silhouette)
            .ok_or_else(|| CodecError::Malformed("silhouette construction is malformed".into()))?;
    for (component, value) in [light_direction.x, light_direction.y, light_direction.z]
        .into_iter()
        .enumerate()
    {
        let start = record.offset + layout.light_direction + component * 8;
        bytes[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    if let Some(draft_factor) = draft_factor {
        let draft_offset = layout
            .draft_factor
            .ok_or_else(|| CodecError::Malformed("silhouette draft factor is missing".into()))?;
        let draft_offset = record.offset + draft_offset;
        bytes[draft_offset..draft_offset + 8].copy_from_slice(&draft_factor.to_le_bytes());
    }
    Ok(())
}

fn patch_nurbs_pcurve_record(
    bytes: &mut [u8],
    record: &sab::Record,
    edit: &NurbsPcurveEdit,
) -> Result<(), CodecError> {
    let geometry = &edit.geometry;
    let PcurveGeometry::Nurbs {
        degree,
        control_points,
        weights,
        ..
    } = geometry
    else {
        return Err(CodecError::NotImplemented(format!(
            "pcurve record {} is not a writable NURBS cache",
            record.index
        )));
    };
    let ref_width = asm_header::parse(bytes).map_or(8, |header| usize::from(header.width));
    let scope = if record.head == "pcurve" {
        sab::payload_subtype_range(bytes, record, 5, ref_width, "exp_par_cur").ok_or_else(|| {
            CodecError::Malformed(format!(
                "pcurve record {} has no exp_par_cur payload",
                record.index
            ))
        })?
    } else if record.head == "intcurve" {
        record.offset..record.offset.checked_add(record.len).ok_or_else(|| {
            CodecError::Malformed("NURBS pcurve record extent overflows address space".into())
        })?
    } else {
        return Err(CodecError::Malformed(format!(
            "record {} is not a pcurve carrier",
            record.index
        )));
    };
    let layout =
        crate::nurbs::final_pcurve_patch_layout(bytes.get(scope.clone()).ok_or_else(|| {
            CodecError::Malformed("NURBS pcurve subtype extent is truncated".into())
        })?)
        .ok_or_else(|| {
            CodecError::Malformed(format!(
                "pcurve record {} has no writable UV cache",
                record.index
            ))
        })?;
    if layout.control_count != control_points.len()
        || layout.control_value_offsets.len() != control_points.len() * 2
        || layout.weight_value_offsets.len() != weights.as_ref().map_or(0, Vec::len)
    {
        return Err(CodecError::NotImplemented(format!(
            "pcurve record {} changed UV cache structure",
            record.index
        )));
    }
    let PcurveGeometry::Nurbs { knots, .. } = geometry else {
        unreachable!()
    };
    patch_knot_structure(bytes, scope.start, &layout.knots, knots, layout.int_width)?;
    let at = scope.start + layout.degree_value_offset;
    patch_layout_integer(bytes, at, layout.int_width, i64::from(*degree))?;
    if let Some(periodic) = edit.periodic {
        let value = if periodic { 2i64 } else { 0i64 };
        let at = scope.start + layout.periodic_value_offset;
        patch_layout_integer(bytes, at, layout.int_width, value)?;
    }
    if record.head == "pcurve" {
        if let Some(reversed) = edit.wrapper_reversed {
            let offset =
                sab::payload_token_offset(bytes, record, ref_width, 4).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "pcurve record {} lacks wrapper-reversal carrier",
                        record.index
                    ))
                })?;
            if !matches!(bytes.get(offset), Some(0x0a | 0x0b)) {
                return Err(CodecError::Malformed(format!(
                    "pcurve record {} has a non-boolean wrapper-reversal carrier",
                    record.index
                )));
            }
            bytes[offset] = if reversed { 0x0a } else { 0x0b };
        }
        if bytes.get(scope.end) != Some(&0x10) {
            return Err(CodecError::Malformed(format!(
                "pcurve record {} lacks the exp_par_cur close",
                record.index
            )));
        }
        let suffix_start = record.tokens.len().checked_sub(6).ok_or_else(|| {
            CodecError::Malformed(format!(
                "pcurve record {} lacks its native metadata suffix",
                record.index
            ))
        })?;
        let suffix_offsets = (suffix_start..record.tokens.len())
            .map(|index| {
                sab::payload_token_offset(bytes, record, ref_width, index).ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "pcurve record {} has an incomplete native metadata suffix",
                        record.index
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(flags) = edit.native_tail_flags {
            for (offset, flag) in suffix_offsets[..4].iter().zip(flags) {
                if !matches!(bytes.get(*offset), Some(0x0a | 0x0b)) {
                    return Err(CodecError::Malformed(format!(
                        "pcurve record {} has an incomplete native boolean tail",
                        record.index
                    )));
                }
                bytes[*offset] = native_bool(flag);
            }
        } else {
            for offset in &suffix_offsets[..4] {
                if !matches!(bytes.get(*offset), Some(0x0a | 0x0b)) {
                    return Err(CodecError::Malformed(format!(
                        "pcurve record {} has an incomplete native boolean tail",
                        record.index
                    )));
                }
            }
        }
        if let Some(range) = edit.parameter_range {
            for (offset, value) in suffix_offsets[4..].iter().zip(range) {
                if bytes.get(*offset) != Some(&0x06) {
                    return Err(CodecError::Malformed(format!(
                        "pcurve record {} has an incomplete parameter range",
                        record.index
                    )));
                }
                bytes[*offset + 1..*offset + 9].copy_from_slice(&value.to_le_bytes());
            }
        }
    }
    if let Some(tolerance) = edit.fit_tolerance {
        if bytes.get(scope.start + layout.control_end) != Some(&0x06) {
            return Err(CodecError::NotImplemented(format!(
                "pcurve record {} has no writable fit-tolerance carrier",
                record.index
            )));
        }
        let at = scope.start + layout.control_end + 1;
        bytes[at..at + 8].copy_from_slice(&tolerance.to_le_bytes());
    }
    for (point, offsets) in control_points
        .iter()
        .zip(layout.control_value_offsets.chunks_exact(2))
    {
        for (value, offset) in [point.u, point.v].into_iter().zip(offsets) {
            let at = scope.start + offset;
            bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    if let Some(weights) = weights {
        for (weight, offset) in weights.iter().zip(&layout.weight_value_offsets) {
            let at = scope.start + offset;
            bytes[at..at + 8].copy_from_slice(&weight.to_le_bytes());
        }
    }
    Ok(())
}

fn patch_ref_pcurve_contract(
    bytes: &mut [u8],
    record: &sab::Record,
    edit: &NurbsPcurveEdit,
) -> Result<(), CodecError> {
    if edit.wrapper_reversed.is_some()
        || edit.native_tail_flags.is_some()
        || edit.fit_tolerance.is_some()
    {
        return Err(CodecError::NotImplemented(format!(
            "ref-form pcurve record {} cannot carry wrapper or inline fit edits",
            record.index
        )));
    }
    let Some(range) = edit.parameter_range else {
        return Ok(());
    };
    let ref_width = active_ref_width(bytes);
    for (index, value) in [5usize, 6].into_iter().zip(range) {
        let offset =
            sab::payload_token_offset(bytes, record, ref_width, index).ok_or_else(|| {
                CodecError::Malformed(format!(
                    "ref-form pcurve record {} lacks parameter-range field {index}",
                    record.index
                ))
            })?;
        if bytes.get(offset) != Some(&0x06) {
            return Err(CodecError::Malformed(format!(
                "ref-form pcurve record {} parameter-range field {index} is not a double",
                record.index
            )));
        }
        bytes[offset + 1..offset + 9].copy_from_slice(&value.to_le_bytes());
    }
    Ok(())
}

fn patch_knot_structure(
    bytes: &mut [u8],
    record_offset: usize,
    layout: &crate::nurbs::KnotPatchLayout,
    knots: &[f64],
    int_width: usize,
) -> Result<(), CodecError> {
    let mut runs: Vec<(f64, usize)> = Vec::new();
    for knot in knots {
        if let Some((value, count)) = runs.last_mut() {
            if *value == *knot {
                *count += 1;
                continue;
            }
        }
        runs.push((*knot, 1));
    }
    if runs.len() != layout.value_offsets.len() || runs.len() != layout.multiplicity_offsets.len() {
        return Err(CodecError::NotImplemented(
            "F3D NURBS curve edit changes the unique-knot count".into(),
        ));
    }
    for (ordinal, ((value, expanded_count), (value_offset, multiplicity_offset))) in runs
        .into_iter()
        .zip(
            layout
                .value_offsets
                .iter()
                .zip(&layout.multiplicity_offsets),
        )
        .enumerate()
    {
        let endpoint_extra = usize::from(ordinal == 0 || ordinal + 1 == layout.value_offsets.len());
        let stored = expanded_count
            .checked_sub(endpoint_extra)
            .filter(|count| *count > 0)
            .ok_or_else(|| {
                CodecError::NotImplemented(
                    "F3D NURBS curve knot multiplicity is not writable".into(),
                )
            })?;
        let stored = i64::try_from(stored).map_err(|_| {
            CodecError::Malformed("F3D NURBS curve knot multiplicity exceeds i64".into())
        })?;
        let value_at = record_offset + *value_offset;
        bytes[value_at..value_at + 8].copy_from_slice(&value.to_le_bytes());
        let multiplicity_at = record_offset + *multiplicity_offset;
        patch_layout_integer(bytes, multiplicity_at, int_width, stored)?;
    }
    Ok(())
}

fn patch_layout_integer(
    bytes: &mut [u8],
    offset: usize,
    width: usize,
    value: i64,
) -> Result<(), CodecError> {
    if width == 4 && i64::from(value as i32) != value {
        return Err(CodecError::NotImplemented(
            "F3D NURBS integer edit exceeds BinaryFile4 range".into(),
        ));
    }
    let target = bytes
        .get_mut(offset..offset + width)
        .ok_or_else(|| CodecError::Malformed("F3D NURBS integer payload is truncated".into()))?;
    target.copy_from_slice(&value.to_le_bytes()[..width]);
    Ok(())
}

fn patch_tagged_integer_at(
    bytes: &mut [u8],
    tag_offset: usize,
    width: usize,
    value: i64,
) -> Result<(), CodecError> {
    if !matches!(bytes.get(tag_offset), Some(0x04 | 0x0c | 0x15)) {
        return Err(CodecError::Malformed(
            "F3D tagged integer carrier is missing".into(),
        ));
    }
    patch_layout_integer(bytes, tag_offset + 1, width, value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f3d_intcurve_writer_rejects_independent_pcurve_parameter_mapping() {
        let context = cadmpeg_ir::geometry::IntcurveSupportContext {
            sides: [
                cadmpeg_ir::geometry::IntcurveSupportSide {
                    surface: None,
                    pcurve: Some(cadmpeg_ir::geometry::PcurveGeometry::Line {
                        origin: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                        direction: cadmpeg_ir::math::Point2::new(1.0, 0.0),
                    }),
                    pcurve_parameter_range: Some([2.0, 5.0]),
                },
                cadmpeg_ir::geometry::IntcurveSupportSide {
                    surface: None,
                    pcurve: None,
                    pcurve_parameter_range: None,
                },
            ],
            parameter_range: [0.0, 1.0],
            discontinuities: std::array::from_fn(|_| Vec::new()),
        };
        let error = native_intcurve_support_context(
            &mut Vec::new(),
            &CadIr::empty(cadmpeg_ir::units::Units::default()),
            &context,
        )
        .expect_err("independent mapping is not writable");

        assert!(matches!(
            error,
            CodecError::NotImplemented(message)
                if message.contains("independent support-pcurve parameter intervals")
        ));
    }

    #[test]
    fn generated_face_sense_edit_preserves_native_normalization_relation() {
        assert_eq!(
            normalized_face_sense_to_native(Sense::Reversed, Sense::Forward, Sense::Forward,),
            Sense::Reversed
        );
        assert_eq!(
            normalized_face_sense_to_native(Sense::Reversed, Sense::Reversed, Sense::Forward,),
            Sense::Forward
        );
    }

    #[test]
    fn generated_straight_record_patches_by_token_boundaries() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x0d\x08straight");
        bytes.push(0x13);
        for value in [1.0f64, 2.0, 3.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x14);
        for value in [1.0f64, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x11);
        let records = sab::frame(&bytes, 0, bytes.len(), 8).expect("generated straight record");
        let lines = BTreeMap::from([(
            "f3d:brep:entity#0".to_string(),
            (Point3::new(40.0, 50.0, 60.0), Vector3::new(0.0, 1.0, 0.0)),
        )]);
        patch_framed_geometry(
            &mut bytes,
            &records,
            &BTreeMap::new(),
            &lines,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            1.0,
        )
        .expect("generated line edit");
        let decoded =
            sab::frame(&bytes, 0, bytes.len(), 8).expect("patched generated straight record");
        assert!(matches!(
            crate::brep::decode_curve(&decoded[0]),
            Some(CurveGeometry::Line { origin, direction })
                if origin == Point3::new(40.0, 50.0, 60.0)
                    && direction == Vector3::new(0.0, 1.0, 0.0)
        ));
    }

    #[test]
    fn generated_signed_sphere_patches_exact_frame_and_radius() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x0d\x06sphere");
        bytes.push(0x13);
        for value in [0.0f64, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x06);
        bytes.extend_from_slice(&1.0f64.to_le_bytes());
        for vector in [[1.0f64, 0.0, 0.0], [0.0, 0.0, 1.0]] {
            bytes.push(0x14);
            for value in vector {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes.push(0x11);
        let records = sab::frame(&bytes, 0, bytes.len(), 8).expect("generated sphere record");
        let spheres = BTreeMap::from([(
            "f3d:brep:entity#0".to_string(),
            (
                Point3::new(10.0, 20.0, 30.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                -25.0,
            ),
        )]);
        patch_framed_geometry(
            &mut bytes,
            &records,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &spheres,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            1.0,
        )
        .expect("generated sphere edit");
        let decoded = sab::frame(&bytes, 0, bytes.len(), 8).expect("patched sphere record");
        assert!(matches!(
            crate::brep::decode_surface(&decoded[0]),
            Some((SurfaceGeometry::Sphere { center, axis, ref_direction, radius }, false))
                if center == Point3::new(10.0, 20.0, 30.0)
                    && axis == Vector3::new(0.0, 1.0, 0.0)
                    && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                    && radius == -25.0
        ));
    }

    #[test]
    fn generated_torus_preserves_signed_self_intersecting_radii() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x0d\x05torus");
        bytes.push(0x13);
        for value in [0.0f64, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x14);
        for value in [0.0f64, 0.0, 1.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [1.0f64, 0.25] {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x14);
        for value in [1.0f64, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x11);
        let records = sab::frame(&bytes, 0, bytes.len(), 8).expect("generated torus record");
        let tori = BTreeMap::from([(
            "f3d:brep:entity#0".to_string(),
            (
                Point3::new(10.0, 20.0, 30.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                20.0,
                -35.0,
            ),
        )]);
        patch_framed_geometry(
            &mut bytes,
            &records,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &tori,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            1.0,
        )
        .expect("generated torus edit");
        let decoded = sab::frame(&bytes, 0, bytes.len(), 8).expect("patched torus record");
        assert!(matches!(
            crate::brep::decode_surface(&decoded[0]),
            Some((SurfaceGeometry::Torus {
                center,
                axis,
                ref_direction,
                major_radius,
                minor_radius,
            }, false))
                if center == Point3::new(10.0, 20.0, 30.0)
                    && axis == Vector3::new(0.0, 1.0, 0.0)
                    && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                    && major_radius == 20.0
                    && minor_radius == -35.0
        ));
    }

    #[test]
    fn generated_cylinder_preserves_native_angle_branch() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x0d\x04cone");
        bytes.push(0x13);
        for value in [0.0f64, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for vector in [[0.0f64, 0.0, 1.0], [1.0, 0.0, 0.0]] {
            bytes.push(0x14);
            for value in vector {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        for value in [1.0f64, 0.0, -1.0, 1.0] {
            bytes.push(0x06);
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0x11);
        let records = sab::frame(&bytes, 0, bytes.len(), 8).expect("generated cylinder record");
        let cones = BTreeMap::from([(
            "f3d:brep:entity#0".to_string(),
            (
                Point3::new(10.0, 20.0, 30.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                40.0,
                1.0,
                0.0,
            ),
        )]);
        patch_framed_geometry(
            &mut bytes,
            &records,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &cones,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            1.0,
        )
        .expect("generated cylinder edit");
        let decoded = sab::frame(&bytes, 0, bytes.len(), 8).expect("patched cylinder record");
        // The patch preserves the record's native negative-cosine angle
        // branch, so decode reports the inward-normal flag.
        assert!(matches!(
            crate::brep::decode_surface(&decoded[0]),
            Some((SurfaceGeometry::Cylinder {
                origin,
                axis,
                ref_direction,
                radius,
            }, true))
                if origin == Point3::new(10.0, 20.0, 30.0)
                    && axis == Vector3::new(0.0, 1.0, 0.0)
                    && ref_direction == Vector3::new(1.0, 0.0, 0.0)
                    && radius == 40.0
        ));
    }

    #[test]
    fn generated_ellipse_preserves_negative_ratio_phase() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x0d\x07ellipse");
        bytes.push(0x13);
        for value in [0.0f64, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for vector in [[0.0f64, 0.0, 1.0], [1.0, 0.0, 0.0]] {
            bytes.push(0x14);
            for value in vector {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes.push(0x06);
        bytes.extend_from_slice(&(-0.5f64).to_le_bytes());
        bytes.push(0x11);
        let records = sab::frame(&bytes, 0, bytes.len(), 8).expect("generated ellipse record");
        let conics = BTreeMap::from([(
            "f3d:brep:entity#0".to_string(),
            (
                Point3::new(10.0, 20.0, 30.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                40.0,
                10.0,
            ),
        )]);
        patch_framed_geometry(
            &mut bytes,
            &records,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &conics,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
            1.0,
        )
        .expect("generated ellipse edit");
        let ratio_offset =
            sab::payload_token_offsets(&bytes, &records[0], 8, 0x06).expect("ellipse tokens")[0];
        assert_eq!(
            f64::from_le_bytes(
                bytes[ratio_offset + 1..ratio_offset + 9]
                    .try_into()
                    .expect("ratio payload"),
            ),
            -0.25
        );
        let decoded = sab::frame(&bytes, 0, bytes.len(), 8).expect("patched ellipse record");
        assert!(matches!(
            crate::brep::decode_curve(&decoded[0]),
            Some(CurveGeometry::Ellipse {
                center,
                axis,
                major_direction,
                major_radius,
                minor_radius,
            })
                if center == Point3::new(10.0, 20.0, 30.0)
                    && axis == Vector3::new(0.0, 1.0, 0.0)
                    && major_direction == Vector3::new(1.0, 0.0, 0.0)
                    && major_radius == 40.0
                    && minor_radius == 10.0
        ));
    }

    #[test]
    fn generated_binaryfile4_integer_patch_preserves_following_token() {
        let mut bytes = vec![0x15];
        bytes.extend_from_slice(&(-3i32).to_le_bytes());
        bytes.extend_from_slice(&[0x0d, 0x03, b'n', b'e', b'x']);

        patch_tagged_integer_at(&mut bytes, 0, 4, 7).expect("width-4 enum patch");

        assert_eq!(&bytes[1..5], &7i32.to_le_bytes());
        assert_eq!(&bytes[5..], &[0x0d, 0x03, b'n', b'e', b'x']);
    }
}
