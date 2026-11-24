// SPDX-License-Identifier: Apache-2.0
//! The neutral model of a decoded package: product definitions with the
//! occurrences each places, and the display geometry of each geometry member.
//!
//! PVS placements and OL coordinates are in metres (§8.21). The model stores
//! millimetres, so every length is scaled by [`MILLIMETRES_PER_METRE`].

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::features::geometry::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::products::{
    ExternalDocument, Occurrence, ProductDefinition, ProductDefinitionKind, Storage,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::tessellation::{Tessellation, TessellationMesh, TessellationTriangleGroup};
use cadmpeg_ir::topology::{Body, BodyKind, Color};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::brep::frames::MILLIMETRES_PER_METRE;
use crate::loss::PvzLossCode;
use crate::ol::{MemberGeometry, Mesh};
use crate::pvs::{DefinitionRole, Placement, Structure};

/// Relative tolerance on the perpendicularity and the common length of the
/// three placement basis vectors (§3.10).
const EPS_BASIS_SIMILARITY: f64 = 1e-5;
/// Distance of a basis length from 1 below which the placement is rigid.
const EPS_UNIT_SCALE: f64 = 1e-6;
/// Operation that charges the text a definition carries into the model.
const RETAIN_DEFINITION_TEXT: &str = "pvz_definition_text";
/// Operation that charges the name an occurrence carries into the model.
const RETAIN_OCCURRENCE_NAME: &str = "pvz_occurrence_name";

/// Adds one product definition per structure definition.
pub(crate) fn definitions(
    ctx: &DecodeContext<'_>,
    structure: &Structure,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut admitted = u64_from_index(ir.entity_count());
    let mut dropped = Vec::new();
    for (index, definition) in structure.definitions.iter().enumerate() {
        let mut bom_properties = BTreeMap::new();
        for property in &definition.properties {
            let key = if property.stream.is_empty() {
                property.name.to_string()
            } else {
                format!("{}/{}", property.stream, property.name)
            };
            let Some(key) = NonBlankString::new(key.clone()) else {
                let note = format!("{key:?} on definition {index}");
                ctx.retain_bytes(u64_from_index(note.len()), RETAIN_DEFINITION_TEXT)?;
                dropped.push(note);
                continue;
            };
            match bom_properties.entry(key) {
                Entry::Vacant(slot) => {
                    ctx.retain_bytes(
                        u64_from_index(slot.key().as_str().len() + property.value.len()),
                        RETAIN_DEFINITION_TEXT,
                    )?;
                    slot.insert(property.value.to_string());
                }
                Entry::Occupied(kept) if *kept.get() == *property.value => {}
                Entry::Occupied(kept) => {
                    let note = format!(
                        "{:?} = {:?} on definition {index}, which keeps {:?}",
                        kept.key().as_str(),
                        property.value,
                        kept.get()
                    );
                    ctx.retain_bytes(u64_from_index(note.len()), RETAIN_DEFINITION_TEXT)?;
                    dropped.push(note);
                }
            }
        }
        ctx.retain_bytes(
            u64_from_index(definition.name.len()),
            RETAIN_DEFINITION_TEXT,
        )?;
        ir.model.product_definitions.push(ProductDefinition {
            sketches: Vec::default(),

            tessellations: Vec::new(),
            id: crate::ids::definition(index),
            kind: Some(match definition.role {
                DefinitionRole::Component => ProductDefinitionKind::Part,
                DefinitionRole::IllustrationRoot => ProductDefinitionKind::Group,
            }),
            source_name: (!definition.name.is_empty()).then(|| definition.name.to_string()),
            label: None,
            description: None,
            part_number: None,
            bom_properties,
            bodies: Vec::new(),
            native_ref: Some(format!("definition {index}")),
            generated_from: None,
            storage: cadmpeg_ir::products::Storage::Embedded,
            occurrences: Vec::new(),
            instance_bodies: Vec::new(),
            representations: Vec::new(),
            units: None,
            general_tolerance: None,
            active: None,
        });
        ctx.admit_entities(
            u64_from_index(ir.entity_count()),
            &mut admitted,
            "pvz_definition",
        )?;
    }
    if !dropped.is_empty() {
        losses.push(PvzLossCode::StructurePropertyKeyDropped.note(format!(
            "{} properties with a blank name, or with the name of a kept property and another \
             value, were not transferred: {}",
            dropped.len(),
            dropped.join(", ")
        )));
    }
    Ok(())
}

/// How far the walk from the roots has reached a definition.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reached {
    No,
    /// On the path from a root to the definition being walked.
    OnPath,
    Done,
}

/// Adds the occurrences each definition places, walking the definition graph
/// from its roots (§3.11). A definition reached from a root lists its child
/// occurrences once, in stored order; a child occurrence of a definition on
/// the path to it closes a cycle and is left out. The walk stops after
/// `max_occurrences` occurrences.
pub(crate) fn occurrences(
    ctx: &DecodeContext<'_>,
    structure: &Structure,
    max_occurrences: usize,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let definitions = &structure.definitions;
    let targeted: BTreeSet<usize> = definitions
        .iter()
        .flat_map(|definition| definition.occurrences.iter().map(|child| child.definition))
        .collect();
    let roots: Vec<usize> = (0..definitions.len())
        .filter(|index| !targeted.contains(index))
        .collect();
    let max_depth = ctx.policy().limits.max_recursion_depth;
    let depth_limit = usize::try_from(max_depth).ok();
    let mut admitted = u64_from_index(ir.entity_count());
    let mut emitted = 0_usize;
    let mut cycles = 0_usize;
    let mut stopped = false;
    let mut past_depth = 0_usize;
    let mut not_similar = 0_usize;
    let mut invalid = 0_usize;
    let mut owned: Vec<Vec<cadmpeg_ir::ids::OccurrenceId>> = vec![Vec::new(); definitions.len()];
    match roots.as_slice() {
        [] => {}
        [root] => set_root(ir, &crate::ids::definition(*root)),
        _ => {
            losses.push(PvzLossCode::StructureRootAmbiguous.note(format!(
                "the structure has {} root definitions, not one; a document definition places \
                 each root",
                roots.len()
            )));
            let mut document =
                ProductDefinition::root(crate::ids::document(), ExternalDocument::default());
            for root in roots.iter().copied() {
                if emitted >= max_occurrences {
                    stopped = true;
                    break;
                }
                let id = crate::ids::occurrence(emitted);
                let mut occurrence = Occurrence::new(
                    id.clone(),
                    crate::ids::definition(root),
                    Some(Transform::identity()),
                );
                occurrence.native_ref = Some(format!("definition {root}"));
                ir.model.occurrences.push(occurrence);
                document.occurrences.push(id);
                emitted += 1;
                ctx.admit_entities(
                    u64_from_index(ir.entity_count()),
                    &mut admitted,
                    "pvz_occurrence",
                )?;
            }
            ir.model.product_definitions.push(document);
            ctx.admit_entities(
                u64_from_index(ir.entity_count()),
                &mut admitted,
                "pvz_document",
            )?;
        }
    }
    let mut reached = vec![Reached::No; definitions.len()];
    'roots: for root in roots.iter().copied() {
        if stopped {
            break;
        }
        // Each frame is a definition, its depth, and its next child.
        let mut stack = vec![(root, 1_usize, 0_usize)];
        reached[root] = Reached::OnPath;
        while let Some(frame) = stack.last_mut() {
            let (parent_definition, depth, next) = *frame;
            let Some(definition) = definitions.get(parent_definition) else {
                stack.pop();
                continue;
            };
            if next == 0 && depth_limit.is_some_and(|limit| depth >= limit) {
                past_depth += definition.occurrences.len();
                reached[parent_definition] = Reached::Done;
                stack.pop();
                continue;
            }
            let Some(child) = definition.occurrences.get(next) else {
                reached[parent_definition] = Reached::Done;
                stack.pop();
                continue;
            };
            frame.2 += 1;
            if emitted >= max_occurrences {
                stopped = true;
                break 'roots;
            }
            if reached.get(child.definition) == Some(&Reached::OnPath) {
                cycles += 1;
                continue;
            }
            let transform = match placement(&child.placement) {
                Placed::Similarity(transform, scale) => transform
                    .compose(uniform_scale_transform(scale)?)
                    .map_err(|error| CodecError::malformed(error.to_string()))?,
                Placed::Affine(transform) => {
                    not_similar += 1;
                    transform
                }
                Placed::Invalid => {
                    invalid += 1;
                    Transform::identity()
                }
            };
            ctx.retain_bytes(
                u64_from_index(child.feature_id.len()),
                RETAIN_OCCURRENCE_NAME,
            )?;
            let id = crate::ids::occurrence(emitted);
            let mut occurrence = Occurrence::new(
                id.clone(),
                crate::ids::definition(child.definition),
                Some(transform),
            );
            occurrence.name = (!child.feature_id.is_empty()).then(|| child.feature_id.to_string());
            occurrence.native_ref =
                Some(format!("definition {parent_definition} occurrence {next}"));
            ir.model.occurrences.push(occurrence);
            owned[parent_definition].push(id);
            emitted += 1;
            ctx.admit_entities(
                u64_from_index(ir.entity_count()),
                &mut admitted,
                "pvz_occurrence",
            )?;
            if reached.get(child.definition) == Some(&Reached::No) {
                reached[child.definition] = Reached::OnPath;
                stack.push((child.definition, depth + 1, 0));
            }
        }
    }
    let mut owned: BTreeMap<_, _> = owned
        .into_iter()
        .enumerate()
        .map(|(index, occurrences)| (crate::ids::definition(index), occurrences))
        .collect();
    for definition in &mut ir.model.product_definitions {
        if let Some(occurrences) = owned.remove(&definition.id) {
            definition.occurrences = occurrences;
        }
    }
    if cycles > 0 {
        losses.push(PvzLossCode::StructureOccurrenceCycle.note(format!(
            "{cycles} child occurrences close a definition cycle and were left out"
        )));
    }
    let mut truncation = Vec::new();
    if stopped {
        truncation.push(format!(
            "decoding stopped at the limit of {max_occurrences} occurrences"
        ));
    }
    if past_depth > 0 {
        truncation.push(format!(
            "{past_depth} child occurrences lie past the recursion depth limit of \
             {max_depth} definitions and were left out"
        ));
    }
    if !truncation.is_empty() {
        losses.push(PvzLossCode::StructureOccurrenceTruncated.note(truncation.join("; ")));
    }
    if not_similar > 0 {
        losses.push(PvzLossCode::StructurePlacementNotSimilarity.note(format!(
            "{not_similar} placements have a basis that is not a uniformly scaled right-handed \
             rotation; each was kept as a general affine transform"
        )));
    }
    if invalid > 0 {
        losses.push(PvzLossCode::StructurePlacementInvalid.note(format!(
            "{invalid} placements hold non-finite values; each occurrence was placed at the \
             identity"
        )));
    }
    Ok(())
}

/// Marks the definition the file holds at its top as the root definition.
fn set_root(ir: &mut CadIr, id: &cadmpeg_ir::ids::ProductDefinitionId) {
    if let Some(definition) = ir
        .model
        .product_definitions
        .iter_mut()
        .find(|definition| definition.id == *id)
    {
        definition.storage = Storage::File {
            document: ExternalDocument::default(),
            source: None,
        };
    }
}

/// The uniform scale a similarity applies before its rigid transform.
fn uniform_scale_transform(scale: FiniteReal) -> Result<Transform, CodecError> {
    let s = scale.get();
    Transform::affine([[s, 0.0, 0.0, 0.0], [0.0, s, 0.0, 0.0], [0.0, 0.0, s, 0.0]])
        .ok_or_else(|| CodecError::malformed("occurrence scale must be finite"))
}

/// A placement in model terms.
enum Placed {
    /// A rigid transform and the uniform scale applied before it.
    Similarity(Transform, FiniteReal),
    /// A general affine transform.
    Affine(Transform),
    /// The placement holds a non-finite value.
    Invalid,
}

/// Maps a placement to an occurrence transform and uniform scale (§3.10).
fn placement(placement: &Placement) -> Placed {
    let [tx, ty, tz] = placement
        .translation
        .unwrap_or_default()
        .map(|value| value * MILLIMETRES_PER_METRE);
    let Some(basis) = placement.basis else {
        return Transform::affine([
            [1.0, 0.0, 0.0, tx],
            [0.0, 1.0, 0.0, ty],
            [0.0, 0.0, 1.0, tz],
        ])
        .map_or(Placed::Invalid, |transform| {
            Placed::Similarity(transform, FiniteReal::ONE)
        });
    };
    let [xx, xy, xz, yx, yy, yz, zx, zy, zz] = basis;
    let affine = [[xx, yx, zx, tx], [xy, yy, zy, ty], [xz, yz, zz, tz]];
    let Some(general) = Transform::affine(affine) else {
        return Placed::Invalid;
    };
    let Some(scale) = uniform_scale([[xx, xy, xz], [yx, yy, yz], [zx, zy, zz]]) else {
        return Placed::Affine(general);
    };
    if (scale - 1.0).abs() <= EPS_UNIT_SCALE {
        return Placed::Similarity(general, FiniteReal::ONE);
    }
    let rigid = affine.map(|[a, b, c, t]| [a / scale, b / scale, c / scale, t]);
    match (Transform::affine(rigid), FiniteReal::new(scale)) {
        (Some(rigid), Some(scale)) => Placed::Similarity(rigid, scale),
        _ => Placed::Affine(general),
    }
}

/// The common length of three mutually perpendicular, right-handed axes, or
/// `None` when the axes are not a uniformly scaled rotation.
fn uniform_scale(axes: [[f64; 3]; 3]) -> Option<f64> {
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let [x, y, z] = axes;
    let lengths = [dot(x, x).sqrt(), dot(y, y).sqrt(), dot(z, z).sqrt()];
    let scale = (lengths[0] + lengths[1] + lengths[2]) / 3.0;
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let square = scale * scale;
    let equal = lengths
        .iter()
        .all(|length| (length - scale).abs() <= EPS_BASIS_SIMILARITY * scale);
    let perpendicular = [dot(x, y), dot(y, z), dot(z, x)]
        .iter()
        .all(|product| product.abs() <= EPS_BASIS_SIMILARITY * square);
    let cross = [
        x[1] * y[2] - x[2] * y[1],
        x[2] * y[0] - x[0] * y[2],
        x[0] * y[1] - x[1] * y[0],
    ];
    let right_handed = dot(cross, z) > 0.0;
    (equal && perpendicular && right_handed).then_some(scale)
}

/// Adds the display geometry of one geometry member: its body, one
/// tessellation per mesh, and the appearances the meshes use. Returns the
/// body, or `None` when the member holds no display geometry.
pub(crate) fn display_geometry(
    ctx: &DecodeContext<'_>,
    member: &str,
    geometry: &MemberGeometry,
    exact: Option<BodyId>,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<Option<BodyId>, CodecError> {
    let body = match exact {
        Some(body) => {
            let written = ir
                .geometry
                .bodies
                .iter_mut()
                .rev()
                .find(|written| written.id == body)
                .ok_or_else(|| {
                    CodecError::malformed(format_args!("B-rep body {body} was not written"))
                })?;
            written.name = Some(member.to_owned());
            body
        }
        None if geometry.meshes.is_empty() && !geometry.has_exact => return Ok(None),
        None => {
            let body = crate::ids::body(member);
            ir.geometry.bodies.push(Body {
                id: body.clone(),
                kind: BodyKind::General,
                transform: None,
                name: Some(member.to_owned()),
                color: None,
                visible: None,
                native_ref: None,
                unstated: None,
            });
            body
        }
    };
    let mut admitted = u64_from_index(ir.entity_count());
    let mut used = BTreeSet::new();
    let mut rejected = Vec::new();
    let mut untriangulated = 0_usize;
    for (index, mesh) in geometry.meshes.iter().enumerate() {
        if mesh.triangles.is_empty() {
            untriangulated += mesh.positions.len();
            continue;
        }
        let id = crate::ids::tessellation(member, index);
        match tessellation(id.clone(), mesh) {
            Ok(tessellation) => {
                ir.model
                    .tessellations
                    .push(tessellation.with_body(Some(body.clone())));
                if let Some(base) = mesh.appearance {
                    used.insert(base);
                    ir.model.appearance_bindings.push(AppearanceBinding {
                        id: crate::ids::appearance_binding(member, index),
                        target: AppearanceTarget::Tessellation(id.to_string()).into(),
                        appearance: crate::ids::appearance(member, base),
                        precedence: None,
                        source_entity_id: None,
                        object_type: None,
                        visible: None,
                        channels: BTreeMap::new(),
                        native_ref: None,
                    });
                }
            }
            Err(reason) => rejected.push(format!("mesh {index}: {reason}")),
        }
        ctx.admit_entities(
            u64_from_index(ir.entity_count()),
            &mut admitted,
            "pvz_tessellation",
        )?;
    }
    let mut colors_rejected = 0_usize;
    for base in used {
        let Some(defined) = geometry.appearances.defined.get(&base) else {
            continue;
        };
        let (appearance, rejected_values) = appearance(member, base, defined);
        colors_rejected += rejected_values;
        ir.model.appearances.push(appearance);
    }
    if untriangulated > 0 {
        losses.push(PvzLossCode::TessellationRecordsUndecoded.note(format!(
            "geometry member {member:?}: {untriangulated} vertices that no triangle primitive \
             indexes were not transferred; line primitives of the surface stream (PVZ-15) index \
             them"
        )));
    }
    if !rejected.is_empty() {
        losses.push(PvzLossCode::TessellationMeshRejected.note(format!(
            "geometry member {member:?}: {}",
            rejected.join("; ")
        )));
    }
    if colors_rejected > 0 {
        losses.push(PvzLossCode::AttributeAppearanceColorRejected.note(format!(
            "geometry member {member:?}: {colors_rejected} appearance values lie outside their \
             range and were not transferred"
        )));
    }
    Ok(Some(body))
}

/// Builds the tessellation of one mesh.
fn tessellation(
    id: cadmpeg_ir::tessellation::TessellationId,
    mesh: &Mesh,
) -> Result<Tessellation, String> {
    let positions = mesh
        .positions
        .iter()
        .map(|&[x, y, z]| {
            FinitePoint3::new(Point3::new(
                f64::from(x) * MILLIMETRES_PER_METRE,
                f64::from(y) * MILLIMETRES_PER_METRE,
                f64::from(z) * MILLIMETRES_PER_METRE,
            ))
        })
        .collect::<Option<Vec<_>>>()
        .ok_or("a vertex position is not finite")?;
    let normals = mesh
        .normals
        .as_ref()
        .map(|normals| {
            normals
                .iter()
                .map(|&[x, y, z]| {
                    FiniteVector3::new(Vector3::new(f64::from(x), f64::from(y), f64::from(z)))
                })
                .collect::<Option<Vec<_>>>()
                .ok_or("a vertex normal is not finite")
        })
        .transpose()?;
    let lanes =
        TessellationMesh::from_checked_list_lanes(positions, mesh.triangles.clone(), normals)
            .map_err(|error| error.to_string())?;
    let groups = mesh
        .patches
        .iter()
        .map(|(patch, triangles)| {
            TessellationTriangleGroup::new(Some(format!("patch {patch}")), triangles.clone())
        })
        .collect();
    Tessellation::from_parts(id, lanes, Vec::new())
        .and_then(|tessellation| tessellation.with_triangle_groups(groups))
        .map_err(|error| error.to_string())
}

/// Builds one appearance (§5.1) and counts the values it could not admit.
fn appearance(member: &str, base: u32, defined: &crate::ol::Appearance) -> (Appearance, usize) {
    let values = &defined.values;
    let mut rejected = 0_usize;
    let [r, g, b] = values.base;
    let base_color = Color::new(
        f64::from(r),
        f64::from(g),
        f64::from(b),
        f64::from(values.opacity),
    );
    if base_color.is_none() {
        rejected += 1;
    }
    let mut properties = BTreeMap::new();
    let channels: [(&str, &[f32]); 6] = [
        ("diffuse", &values.diffuse),
        ("specular", &values.specular),
        ("shininess", std::slice::from_ref(&values.shininess)),
        ("emissive", &values.emissive),
        ("opacity", std::slice::from_ref(&values.opacity)),
        ("base", &values.base),
    ];
    for (name, components) in channels {
        let suffixes: &[&str] = if components.len() == 1 {
            &[""]
        } else {
            &[".r", ".g", ".b"]
        };
        for (suffix, value) in suffixes.iter().zip(components) {
            let key = NonBlankString::new(format!("{name}{suffix}"));
            match (key, FiniteReal::new(f64::from(*value))) {
                (Some(key), Some(value)) => {
                    properties.insert(key, value);
                }
                _ => rejected += 1,
            }
        }
    }
    let appearance = Appearance {
        id: crate::ids::appearance(member, base),
        name: (!defined.name.is_empty()).then(|| defined.name.clone()),
        asset_guid: None,
        library_id: None,
        visual_guid: None,
        physical_token: None,
        schema: None,
        category: None,
        base_color,
        properties,
        textures: Vec::new(),
        native_ref: None,
    };
    (appearance, rejected)
}

#[cfg(test)]
mod tests;
