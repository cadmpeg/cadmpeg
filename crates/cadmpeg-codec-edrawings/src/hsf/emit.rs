// SPDX-License-Identifier: Apache-2.0
//! Emission of a [`Scene`] into CADIR.
//!
//! Each library body that an instance references becomes one body, built from
//! the triangles of all its shells by the mesh B-rep module. An instance
//! references a list of bodies, its body set. A file with one instance and no
//! sub-assembly is a plain part: one part definition holds the bodies, and the
//! instance matrix moves the points. Any other file is an assembly: one part
//! definition per distinct body set, one group definition for the assembly and
//! one per sub-assembly, and one occurrence per tree node. The stream stores
//! absolute placements on the instances and none on sub-assemblies, so a
//! sub-assembly occurrence places itself at the identity and a leaf occurrence
//! carries its world matrix.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::transform::Transform;

use super::scene::{Component, Scene};
use super::shape::{append_shell, MeshData, MILLIMETRES_PER_METRE};
use crate::dialect::Grammar;
use crate::loss::EdrawingsLossCode;
use crate::mesh_brep::{add_mesh_body, MeshBrepOptions, MeshInput, MAX_POSITIONS, MAX_TRIANGLES};

/// Operation name for the retained mesh bytes.
const RETAIN_MESH: &str = "edrawings_mesh";
/// Operation name for the retained entities.
const ADMIT_ENTITIES: &str = "edrawings_entity";
/// Bytes one retained position takes.
const POSITION_BYTES: u64 = 24;
/// Bytes one retained triangle takes.
const TRIANGLE_BYTES: u64 = 12;
/// Most shell failures a loss message lists.
const MAX_LISTED: usize = 5;

/// A body that was built.
struct Built {
    id: BodyId,
}

/// Totals over all bodies, for the loss notes.
#[derive(Default)]
struct Totals {
    bodies: usize,
    faces: usize,
    triangles: usize,
    open_bodies: Vec<String>,
    open_edges: usize,
    dropped: usize,
    dropped_zero: usize,
    dropped_thin: usize,
    dropped_component_triangles: usize,
    dropped_sliver: usize,
    collapsed_faces: usize,
    dropped_flat_shells: usize,
    t_junction_splits: usize,
    crack_merged_points: usize,
    fine_weld_bodies: usize,
    reversed_bodies: Vec<String>,
    reversed_components: usize,
}

/// The note for dropped triangles and dropped closed components; each clause
/// is present only when its count is not zero.
fn dropped_message(totals: &Totals) -> Option<String> {
    let mut clauses = Vec::new();
    if totals.dropped > 0 {
        clauses.push(format!(
            "{} degenerate triangles were dropped",
            totals.dropped
        ));
    }
    let components = totals.dropped_zero + totals.dropped_thin;
    if components > 0 {
        let mut kinds = Vec::new();
        if totals.dropped_thin > 0 {
            kinds.push(format!("{} thin cavities", totals.dropped_thin));
        }
        if totals.dropped_zero > 0 {
            kinds.push(format!("{} zero-volume pieces", totals.dropped_zero));
        }
        clauses.push(format!(
            "{components} closed components without a usable volume were dropped ({}; {} triangles)",
            kinds.join(", "),
            totals.dropped_component_triangles
        ));
    }
    if totals.dropped_sliver > 0 {
        clauses.push(format!(
            "{} open slivers below the part size were dropped",
            totals.dropped_sliver
        ));
    }
    if totals.dropped_flat_shells > 0 {
        clauses.push(format!(
            "{} closed shells of fewer than four faces without a volume were dropped",
            totals.dropped_flat_shells
        ));
    }
    if totals.collapsed_faces > 0 {
        clauses.push(format!(
            "{} faces without an area were removed and their neighbours joined",
            totals.collapsed_faces
        ));
    }
    (!clauses.is_empty()).then(|| clauses.join("; "))
}

/// The display name of tree node `index`.
fn display_name(scene: &Scene<'_>, index: usize) -> String {
    let Some(node) = scene.nodes.get(index) else {
        return String::new();
    };
    [node.refname_slot, node.name_slot]
        .into_iter()
        .flatten()
        .filter_map(|slot| scene.texts.get(&slot))
        .map(|text| text.trim())
        .find(|text| !text.is_empty())
        .map(str::to_owned)
        .or_else(|| node.user_name.clone().filter(|name| !name.is_empty()))
        .unwrap_or_else(|| node.segment.clone())
}

/// The component that lists node `index` in the active display state.
fn listing<'a>(scene: &'a Scene<'_>, index: usize) -> Option<&'a Component> {
    let state = scene.current_state.as_ref()?;
    let node = scene.nodes.get(index)?;
    let suffix = format!("/{}", node.segment);
    scene
        .components
        .iter()
        .find(|c| &c.state == state && (c.path == node.segment || c.path.ends_with(&suffix)))
}

/// Visibility of tree node `index` in the active display state.
fn visibility(scene: &Scene<'_>, index: usize) -> Option<bool> {
    if scene.grammar.is_some_and(Grammar::solidworks_layout) {
        return inherited_visibility(scene, index);
    }
    listing(scene, index).map(|c| c.visible)
}

/// Visibility under the rules of `SolidWorks` streams. A leaf follows its own
/// listing, then its own `defaultvisibility=off`, then the nearest listed
/// ancestor. A sub-assembly shows only its own listing.
fn inherited_visibility(scene: &Scene<'_>, index: usize) -> Option<bool> {
    let node = scene.nodes.get(index)?;
    if let Some(component) = listing(scene, index) {
        return Some(component.visible);
    }
    if !node.is_instance {
        return None;
    }
    if node.hidden_by_default {
        return Some(false);
    }
    let mut cursor = node.parent;
    while let Some(current) = cursor {
        if let Some(component) = listing(scene, current) {
            return Some(component.visible);
        }
        cursor = scene.nodes.get(current).and_then(|n| n.parent);
    }
    None
}

/// The occurrence transform of a stream matrix: the column-vector form, in
/// millimetres. `None` when a value is not finite.
fn placement(matrix: Option<&[f32; 12]>) -> Option<Transform> {
    let Some(matrix) = matrix else {
        return Some(Transform::identity());
    };
    let [r0, r1, r2, r3, r4, r5, r6, r7, r8, tx, ty, tz] = matrix.map(f64::from);
    let scale = MILLIMETRES_PER_METRE;
    Transform::affine([
        [r0, r3, r6, tx * scale],
        [r1, r4, r7, ty * scale],
        [r2, r5, r8, tz * scale],
    ])
}

/// The last path component of a library body path.
fn body_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn lead(items: &[String]) -> String {
    let shown: Vec<&str> = items.iter().take(MAX_LISTED).map(String::as_str).collect();
    let more = items.len() - shown.len();
    if more > 0 {
        format!("{} and {more} more", shown.join(", "))
    } else {
        shown.join(", ")
    }
}

/// The body set of every instance: the library bodies that its includes name,
/// in order of first mention, and the instances that name none.
struct BodySets {
    /// Distinct body sets in order of first use.
    sets: Vec<Vec<usize>>,
    /// Index into `sets` by instance node.
    of_instance: BTreeMap<usize, usize>,
    /// Instances that name no library body.
    unresolved: Vec<String>,
    /// Include references that name no library body in instances that name
    /// another one.
    dangling: usize,
}

fn body_sets(scene: &Scene<'_>, instances: &[usize]) -> BodySets {
    let mut by_path: BTreeMap<&str, usize> = BTreeMap::new();
    for (index, body) in scene.bodies.iter().enumerate() {
        by_path.entry(body.path.as_str()).or_insert(index);
    }
    let mut result = BodySets {
        sets: Vec::new(),
        of_instance: BTreeMap::new(),
        unresolved: Vec::new(),
        dangling: 0,
    };
    let mut known: BTreeMap<Vec<usize>, usize> = BTreeMap::new();
    for &index in instances {
        let Some(node) = scene.nodes.get(index) else {
            continue;
        };
        let mut set: Vec<usize> = Vec::new();
        let mut missing = 0_usize;
        for path in &node.body_paths {
            match by_path.get(path.as_str()) {
                Some(&body) if !set.contains(&body) => set.push(body),
                Some(_) => {}
                None => missing += 1,
            }
        }
        if set.is_empty() {
            result.unresolved.push(display_name(scene, index));
            continue;
        }
        result.dangling += missing;
        let next = result.sets.len();
        let slot = *known.entry(set.clone()).or_insert_with(|| {
            result.sets.push(set);
            next
        });
        result.of_instance.insert(index, slot);
    }
    result
}

/// Appearance that the stream holds and the output cannot carry.
fn appearance_losses(scene: &Scene<'_>, built: &[usize], losses: &mut Vec<LossNote>) {
    if !scene.grammar.is_some_and(Grammar::solidworks_layout) {
        return;
    }
    let mut faces = 0_usize;
    let mut bodies = 0_usize;
    for library in built.iter().filter_map(|&i| scene.bodies.get(i)) {
        let differing = library
            .sub_colours
            .iter()
            .filter(|colour| colour.is_some() && **colour != library.colour)
            .count();
        if differing > 0 {
            bodies += 1;
            faces += differing;
        }
    }
    if faces > 0 {
        losses.push(EdrawingsLossCode::AppearanceFaceColourReduced.note(format!(
            "{faces} face group colours in {bodies} bodies were reduced to the colour of their body"
        )));
    }
    let leaves = scene
        .nodes
        .iter()
        .filter(|n| n.is_instance && (n.colour.is_some() || n.default_colour.is_some()))
        .count();
    let states = scene
        .components
        .iter()
        .filter(|c| c.overrides_appearance)
        .count();
    if leaves + states > 0 {
        losses.push(EdrawingsLossCode::AppearanceOverrideDropped.note(format!(
            "{leaves} part colour overrides and {states} display state colour or transparency \
             overrides were not transferred"
        )));
    }
}

/// Emits `scene` into `ir`. Returns whether any body was built.
pub(super) fn emit(
    ctx: &DecodeContext<'_>,
    scene: &Scene<'_>,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<bool, CodecError> {
    let instances: Vec<usize> = (0..scene.nodes.len())
        .filter(|&i| scene.nodes.get(i).is_some_and(|n| n.is_instance))
        .collect();
    let single = instances.len() == 1 && scene.nodes.len() == 1;
    let sets = body_sets(scene, &instances);

    // The library bodies in order of first use.
    let mut order: Vec<usize> = Vec::new();
    let mut position: BTreeMap<usize, usize> = BTreeMap::new();
    for set in &sets.sets {
        for &body in set {
            position.entry(body).or_insert_with(|| {
                order.push(body);
                order.len() - 1
            });
        }
    }
    let first_instance_of_set = |set: usize| {
        sets.of_instance
            .iter()
            .find(|(_, &slot)| slot == set)
            .map(|(&node, _)| node)
    };
    let single_node = single
        .then(|| sets.of_instance.keys().next().copied())
        .flatten();

    // Bodies.
    let mut built: Vec<Option<Built>> = Vec::new();
    let mut totals = Totals::default();
    let mut shell_failures: Vec<String> = Vec::new();
    let mut shell_failure_count = 0_usize;
    let mut failed_bodies: Vec<String> = Vec::new();
    let mut admitted = u64_from_index(ir.model.entity_count());
    let options = MeshBrepOptions::default();
    let lone_set = matches!(sets.sets.as_slice(), [set] if set.len() == 1);
    let world = single_node
        .and_then(|node| scene.nodes.get(node))
        .and_then(|n| n.matrix);
    for (k, &body_index) in order.iter().enumerate() {
        let Some(library) = scene.bodies.get(body_index) else {
            built.push(None);
            continue;
        };
        let name = match single_node {
            Some(node) if lone_set => display_name(scene, node),
            _ => body_name(&library.path).to_owned(),
        };
        let mut mesh = MeshData::default();
        for &slot in &library.slots {
            if let Err(error) = append_shell(
                scene,
                slot,
                world.as_ref(),
                (MAX_POSITIONS, MAX_TRIANGLES),
                &mut mesh,
            ) {
                shell_failure_count += 1;
                if shell_failures.len() < MAX_LISTED {
                    shell_failures.push(format!("object {slot} of {name}: {}", error.text()));
                }
            }
        }
        ctx.retain_bytes(
            u64_from_index(mesh.positions.len()) * POSITION_BYTES
                + u64_from_index(mesh.triangles.len()) * TRIANGLE_BYTES,
            RETAIN_MESH,
        )?;
        let scope = format!("body{k}");
        let colour = library
            .colour
            .map(|[r, g, b]| Color::from_rgba8(r, g, b, u8::MAX));
        let input = MeshInput {
            scope: &scope,
            name: Some(&name),
            color: colour,
            positions: &mesh.positions,
            triangles: &mesh.triangles,
        };
        match add_mesh_body(ir, &input, options) {
            Ok(summary) => {
                totals.bodies += 1;
                totals.faces += summary.faces;
                totals.triangles += summary.triangles;
                totals.dropped += summary.dropped_degenerate_triangles;
                totals.dropped_zero += summary.dropped_zero_components;
                totals.dropped_thin += summary.dropped_thin_components;
                totals.dropped_component_triangles += summary.dropped_component_triangles;
                totals.dropped_sliver += summary.dropped_sliver_components;
                totals.collapsed_faces += summary.collapsed_faces;
                totals.dropped_flat_shells += summary.dropped_flat_shells;
                totals.t_junction_splits += summary.t_junction_splits;
                totals.crack_merged_points += summary.crack_merged_points;
                totals.fine_weld_bodies += usize::from(summary.fine_weld);
                if summary.reversed {
                    totals.reversed_bodies.push(name.clone());
                    totals.reversed_components += summary.reversed_components;
                }
                if !summary.closed {
                    totals.open_bodies.push(name.clone());
                    totals.open_edges += summary.open_edges + summary.non_manifold_edges;
                }
                built.push(Some(Built { id: summary.body }));
            }
            Err(error) => {
                failed_bodies.push(format!("{name} ({error})"));
                built.push(None);
            }
        }
        ctx.admit_entities(
            u64_from_index(ir.model.entity_count()),
            &mut admitted,
            ADMIT_ENTITIES,
        )?;
    }

    if shell_failure_count > 0 {
        losses.push(EdrawingsLossCode::ModelShellInvalid.note(format!(
            "{shell_failure_count} shells were left out of their bodies: {}",
            lead(&shell_failures)
        )));
    }
    if !failed_bodies.is_empty() {
        losses.push(EdrawingsLossCode::GeometryBodyFailed.note(format!(
            "{} bodies could not be built and were left out: {}",
            failed_bodies.len(),
            lead(&failed_bodies)
        )));
    }
    if !sets.unresolved.is_empty() {
        losses.push(EdrawingsLossCode::ModelObjectInvalid.note(format!(
            "{} instances name no library body and were left out: {}",
            sets.unresolved.len(),
            lead(&sets.unresolved)
        )));
    }
    if sets.dangling > 0 {
        losses.push(EdrawingsLossCode::ModelObjectInvalid.note(format!(
            "{} body references of instances name no library body and were left out",
            sets.dangling
        )));
    }
    if totals.bodies == 0 {
        losses.push(
            EdrawingsLossCode::ModelNoGeometry
                .note("the model stream holds no instance with a body that could be built"),
        );
        return Ok(false);
    }
    let mut clauses: Vec<String> = Vec::new();
    if totals.t_junction_splits + totals.crack_merged_points > 0 {
        clauses.push(format!(
            "{} T-junction edge splits, {} crack points merged",
            totals.t_junction_splits, totals.crack_merged_points
        ));
    }
    if totals.fine_weld_bodies > 0 {
        clauses.push(format!(
            "{} bodies welded finer to keep features as thin as the weld tolerance",
            totals.fine_weld_bodies
        ));
    }
    let repairs = if clauses.is_empty() {
        String::new()
    } else {
        format!("; {}", clauses.join("; "))
    };
    losses.push(EdrawingsLossCode::GeometryReconstructedFromMesh.note(format!(
        "{} bodies were rebuilt from display meshes ({} triangles{repairs}) as {} planar faces with straight \
         edges; the source stores no analytic surfaces or curves",
        totals.bodies, totals.triangles, totals.faces
    )));
    if !totals.open_bodies.is_empty() {
        losses.push(EdrawingsLossCode::GeometryOpenBody.note(format!(
            "{} bodies are not watertight and are sheets ({} open or non-manifold edges): {}",
            totals.open_bodies.len(),
            totals.open_edges,
            lead(&totals.open_bodies)
        )));
    }
    if !totals.reversed_bodies.is_empty() {
        losses.push(EdrawingsLossCode::GeometryBodyReversed.note(format!(
            "{} closed components of {} bodies faced the wrong way for their nesting and were reversed: {}",
            totals.reversed_components,
            totals.reversed_bodies.len(),
            lead(&totals.reversed_bodies)
        )));
    }
    if let Some(message) = dropped_message(&totals) {
        losses.push(EdrawingsLossCode::GeometryDegenerateTriangles.note(message));
    }
    let built_libraries: Vec<usize> = order
        .iter()
        .zip(&built)
        .filter(|(_, body)| body.is_some())
        .map(|(&library, _)| library)
        .collect();
    appearance_losses(scene, &built_libraries, losses);

    // Definitions: one part per body set with at least one built body.
    let mut part_definition: BTreeMap<usize, usize> = BTreeMap::new();
    for (set_index, set) in sets.sets.iter().enumerate() {
        let bodies: Vec<BodyId> = set
            .iter()
            .filter_map(|body| position.get(body))
            .filter_map(|&k| built.get(k).and_then(Option::as_ref))
            .map(|body| body.id.clone())
            .collect();
        if bodies.is_empty() {
            continue;
        }
        let label = first_instance_of_set(set_index).map(|node| display_name(scene, node));
        let only = match set.as_slice() {
            [body] => scene.bodies.get(*body),
            _ => None,
        };
        let (source_name, label) = match (single, only) {
            (true, _) => (label, None),
            (false, Some(library)) => (Some(body_name(&library.path).to_owned()), label),
            (false, None) => (None, label),
        };
        let index = ir.model.product_definitions.len();
        ir.model.product_definitions.push(ProductDefinition {
            id: crate::ids::definition(index),
            kind: ProductDefinitionKind::Part,
            source_name,
            label,
            description: None,
            part_number: None,
            bom_properties: BTreeMap::new(),
            bodies,
            sketches: Vec::new(),
            native_ref: only.map(|library| library.path.clone()),
        });
        part_definition.insert(set_index, index);
    }
    if single {
        return finish(ctx, ir, &mut admitted);
    }

    // The assembly root and sub-assemblies.
    let root_definition = ir.model.product_definitions.len();
    let document = scene
        .document_slot
        .and_then(|slot| scene.texts.get(&slot))
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty());
    ir.model.product_definitions.push(ProductDefinition {
        id: crate::ids::definition(root_definition),
        kind: ProductDefinitionKind::Group,
        source_name: document,
        label: None,
        description: None,
        part_number: None,
        bom_properties: BTreeMap::new(),
        bodies: Vec::new(),
        sketches: Vec::new(),
        native_ref: None,
    });
    let mut assembly_definition: BTreeMap<usize, usize> = BTreeMap::new();
    for (index, node) in scene.nodes.iter().enumerate() {
        if node.is_instance {
            continue;
        }
        let definition = ir.model.product_definitions.len();
        ir.model.product_definitions.push(ProductDefinition {
            id: crate::ids::definition(definition),
            kind: ProductDefinitionKind::Group,
            source_name: Some(display_name(scene, index)),
            label: None,
            description: None,
            part_number: None,
            bom_properties: BTreeMap::new(),
            bodies: Vec::new(),
            sketches: Vec::new(),
            native_ref: Some(node.segment.clone()),
        });
        assembly_definition.insert(index, definition);
    }

    // Occurrences, parents before children.
    let mut occurrence_of: BTreeMap<usize, usize> = BTreeMap::new();
    let mut ordinals: BTreeMap<Option<usize>, u32> = BTreeMap::new();
    ir.model.occurrences.push(Occurrence {
        id: crate::ids::occurrence(0),
        prototype: PrototypeReference::Local {
            definition: crate::ids::definition(root_definition),
        },
        parent: OccurrenceParent::Root {},
        ordinal: 0,
        transform: Transform::identity(),
        linked_prototype: None,
        scale: [FiniteReal::ONE; 3],
        name: None,
        visible: None,
        link: None,
        native_ref: None,
    });
    let mut invalid_placements = Vec::new();
    for (index, node) in scene.nodes.iter().enumerate() {
        let definition = if node.is_instance {
            sets.of_instance
                .get(&index)
                .and_then(|set| part_definition.get(set))
        } else {
            assembly_definition.get(&index)
        };
        let Some(&definition) = definition else {
            continue;
        };
        let parent_occurrence = node
            .parent
            .and_then(|parent| occurrence_of.get(&parent).copied())
            .unwrap_or(0);
        let ordinal = ordinals.entry(Some(parent_occurrence)).or_insert(0);
        let transform = placement(node.matrix.as_ref()).unwrap_or_else(|| {
            invalid_placements.push(display_name(scene, index));
            Transform::identity()
        });
        let number = ir.model.occurrences.len();
        ir.model.occurrences.push(Occurrence {
            id: crate::ids::occurrence(number),
            prototype: PrototypeReference::Local {
                definition: crate::ids::definition(definition),
            },
            parent: OccurrenceParent::Occurrence {
                occurrence: crate::ids::occurrence(parent_occurrence),
            },
            ordinal: *ordinal,
            transform,
            linked_prototype: None,
            scale: [FiniteReal::ONE; 3],
            name: Some(display_name(scene, index)),
            visible: visibility(scene, index),
            link: None,
            native_ref: Some(node.segment.clone()),
        });
        *ordinal += 1;
        occurrence_of.insert(index, number);
    }
    if !invalid_placements.is_empty() {
        losses.push(EdrawingsLossCode::StructurePlacementInvalid.note(format!(
            "{} placements hold a non-finite value and were set to the identity: {}",
            invalid_placements.len(),
            lead(&invalid_placements)
        )));
    }
    finish(ctx, ir, &mut admitted)
}

fn finish(ctx: &DecodeContext<'_>, ir: &mut CadIr, admitted: &mut u64) -> Result<bool, CodecError> {
    ctx.admit_entities(
        u64_from_index(ir.model.entity_count()),
        admitted,
        ADMIT_ENTITIES,
    )?;
    Ok(true)
}
