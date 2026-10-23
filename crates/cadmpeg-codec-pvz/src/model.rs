// SPDX-License-Identifier: Apache-2.0
//! The neutral model of a decoded package: product definitions, the expanded
//! occurrence tree, and the display geometry of each geometry member.
//!
//! PVS placements and OL coordinates are in metres (§8.21). The model stores
//! millimetres, so every length is scaled by [`MILLIMETRES_PER_METRE`].

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::features::{FinitePoint3, FiniteVector3};
use cadmpeg_ir::ids::{BodyId, OccurrenceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::tessellation::{Tessellation, TessellationMesh, TessellationTriangleGroup};
use cadmpeg_ir::topology::{Body, BodyKind, Color};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use crate::loss::PvzLossCode;
use crate::ol::{MemberGeometry, Mesh};
use crate::pvs::{DefinitionRole, Placement, Structure};

/// Model length units per stored length unit.
const MILLIMETRES_PER_METRE: f64 = 1000.0;
/// Most occurrences one package expands to.
const MAX_OCCURRENCES: usize = 100_000;
/// Relative tolerance on the perpendicularity and the common length of the
/// three placement basis vectors (§3.10).
const EPS_BASIS_SIMILARITY: f64 = 1e-5;
/// Distance of a basis length from 1 below which the placement is rigid.
const EPS_UNIT_SCALE: f64 = 1e-6;

/// Adds one product definition per structure definition.
pub(crate) fn definitions(structure: &Structure, ir: &mut CadIr, losses: &mut Vec<LossNote>) {
    let mut dropped = Vec::new();
    for (index, definition) in structure.definitions.iter().enumerate() {
        let mut bom_properties = BTreeMap::new();
        for property in &definition.properties {
            let key = if property.stream.is_empty() {
                property.name.clone()
            } else {
                format!("{}/{}", property.stream, property.name)
            };
            let Some(key) = NonBlankString::new(key.clone()) else {
                dropped.push(format!("{key:?} on definition {index}"));
                continue;
            };
            match bom_properties.entry(key) {
                Entry::Vacant(slot) => {
                    slot.insert(property.value.clone());
                }
                Entry::Occupied(kept) if *kept.get() == property.value => {}
                Entry::Occupied(kept) => dropped.push(format!(
                    "{:?} = {:?} on definition {index}, which keeps {:?}",
                    kept.key().as_str(),
                    property.value,
                    kept.get()
                )),
            }
        }
        ir.model.product_definitions.push(ProductDefinition {
            id: crate::ids::definition(index),
            kind: match definition.role {
                DefinitionRole::Component => ProductDefinitionKind::Part,
                DefinitionRole::IllustrationRoot => ProductDefinitionKind::Group,
            },
            source_name: (!definition.name.is_empty()).then(|| definition.name.clone()),
            label: None,
            description: None,
            part_number: None,
            bom_properties,
            bodies: Vec::new(),
            native_ref: Some(format!("definition {index}")),
        });
    }
    if !dropped.is_empty() {
        losses.push(PvzLossCode::StructurePropertyKeyDropped.note(format!(
            "{} properties with a blank name, or with the name of a kept property and another \
             value, were not transferred: {}",
            dropped.len(),
            dropped.join(", ")
        )));
    }
}

/// One occurrence awaiting expansion of its children.
struct Pending {
    id: OccurrenceId,
    path: String,
    definition: usize,
    /// Definitions from the root to this occurrence.
    ancestors: Vec<usize>,
}

/// Expands the definition graph from its root into the occurrence tree
/// (§3.11).
pub(crate) fn occurrences(
    ctx: &DecodeContext<'_>,
    structure: &Structure,
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
    if roots.len() != 1 && !definitions.is_empty() {
        losses.push(PvzLossCode::StructureRootAmbiguous.note(format!(
            "the structure has {} root definitions, not one; each root is placed as its own \
             root occurrence",
            roots.len()
        )));
    }
    let depth_limit = usize::try_from(ctx.policy().limits.max_recursion_depth).ok();
    let mut admitted = u64_from_index(ir.model.entity_count());
    let mut pending = Vec::new();
    for (ordinal, root) in roots.iter().copied().enumerate() {
        let path = format!("r{root}");
        let id = crate::ids::occurrence(&path);
        ir.model.occurrences.push(Occurrence {
            id: id.clone(),
            prototype: PrototypeReference::Local {
                definition: crate::ids::definition(root),
            },
            parent: OccurrenceParent::Root {},
            ordinal: u32::try_from(ordinal)
                .map_err(|_| CodecError::malformed("root occurrence ordinal exceeds u32"))?,
            transform: Transform::identity(),
            linked_prototype: None,
            scale: [FiniteReal::ONE; 3],
            name: None,
            visible: None,
            link: None,
            native_ref: Some(format!("definition {root}")),
        });
        pending.push(Pending {
            id,
            path,
            definition: root,
            ancestors: vec![root],
        });
    }
    pending.reverse();
    let mut cycles = 0_usize;
    let mut truncated = 0_usize;
    let mut not_similar = 0_usize;
    let mut invalid = 0_usize;
    while let Some(parent) = pending.pop() {
        let Some(definition) = definitions.get(parent.definition) else {
            continue;
        };
        let mut children = Vec::new();
        for (ordinal, child) in definition.occurrences.iter().enumerate() {
            if parent.ancestors.contains(&child.definition) {
                cycles += 1;
                continue;
            }
            if ir.model.occurrences.len() >= MAX_OCCURRENCES
                || depth_limit.is_some_and(|limit| parent.ancestors.len() >= limit)
            {
                truncated += 1;
                continue;
            }
            let path = format!("{}/{ordinal}", parent.path);
            let id = crate::ids::occurrence(&path);
            let (transform, scale) = match placement(&child.placement) {
                Placed::Similarity(transform, scale) => (transform, scale),
                Placed::Affine(transform) => {
                    not_similar += 1;
                    (transform, FiniteReal::ONE)
                }
                Placed::Invalid => {
                    invalid += 1;
                    (Transform::identity(), FiniteReal::ONE)
                }
            };
            ir.model.occurrences.push(Occurrence {
                id: id.clone(),
                prototype: PrototypeReference::Local {
                    definition: crate::ids::definition(child.definition),
                },
                parent: OccurrenceParent::Occurrence {
                    occurrence: parent.id.clone(),
                },
                ordinal: u32::try_from(ordinal)
                    .map_err(|_| CodecError::malformed("child occurrence ordinal exceeds u32"))?,
                transform,
                linked_prototype: None,
                scale: [scale; 3],
                name: (!child.feature_id.is_empty()).then(|| child.feature_id.clone()),
                visible: None,
                link: None,
                native_ref: Some(format!(
                    "definition {} occurrence {ordinal}",
                    parent.definition
                )),
            });
            let mut ancestors = parent.ancestors.clone();
            ancestors.push(child.definition);
            children.push(Pending {
                id,
                path,
                definition: child.definition,
                ancestors,
            });
        }
        ctx.admit_entities(
            u64_from_index(ir.model.entity_count()),
            &mut admitted,
            "pvz_occurrence",
        )?;
        children.reverse();
        pending.extend(children);
    }
    if cycles > 0 {
        losses.push(PvzLossCode::StructureOccurrenceCycle.note(format!(
            "{cycles} child occurrences close a definition cycle and were not expanded"
        )));
    }
    if truncated > 0 {
        losses.push(PvzLossCode::StructureOccurrenceTruncated.note(format!(
            "{truncated} child occurrences lie past the expansion limit of {MAX_OCCURRENCES} \
             occurrences or the recursion depth limit and were not expanded"
        )));
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
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<Option<BodyId>, CodecError> {
    if geometry.meshes.is_empty() && !geometry.has_exact {
        return Ok(None);
    }
    let mut admitted = u64_from_index(ir.model.entity_count());
    let body = crate::ids::body(member);
    ir.model.bodies.push(Body {
        id: body.clone(),
        kind: BodyKind::General,
        regions: Vec::new(),
        transform: None,
        name: Some(member.to_owned()),
        color: None,
        visible: None,
    });
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
                        target: AppearanceTarget::Tessellation(id.to_string()),
                        appearance: crate::ids::appearance(member, base),
                        source_entity_id: None,
                        object_type: None,
                        visible: None,
                        channels: BTreeMap::new(),
                    });
                }
            }
            Err(reason) => rejected.push(format!("mesh {index}: {reason}")),
        }
        ctx.admit_entities(
            u64_from_index(ir.model.entity_count()),
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
        .map(|(patch, triangles)| TessellationTriangleGroup {
            source_id: Some(format!("patch {patch}")),
            triangles: triangles.clone(),
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
    let base_color = Color::new(r, g, b, values.opacity);
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
    };
    (appearance, rejected)
}

#[cfg(test)]
mod tests;
