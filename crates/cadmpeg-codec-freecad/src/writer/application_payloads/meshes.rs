// SPDX-License-Identifier: Apache-2.0
//! Current meshes receive distinct target carriers without inferring source bindings.

use super::*;

pub(super) struct Emission {
    pub(super) name: String,
    pub(super) file: String,
    pub(super) mesh: Option<String>,
    pub(super) regenerated: bool,
}

pub(super) struct MeshRequest<'a> {
    pub(super) property: &'a str,
    pub(super) name: &'a str,
    pub(super) object: Option<&'a str>,
    pub(super) occupied: &'a HashSet<String>,
    pub(super) entry: &'a crate::native::EntryRecord,
    pub(super) value: &'a EntryContents,
}

pub(super) fn write(
    ir: &CadIr,
    request: MeshRequest<'_>,
    entries: &mut BTreeMap<String, Vec<u8>>,
    losses: &mut Vec<LossNote>,
) -> Result<Vec<Emission>, CodecError> {
    let MeshRequest {
        property,
        name,
        object,
        occupied,
        entry,
        value,
    } = request;
    let EntryContents::Mesh {
        big_endian,
        vertices,
        facets,
        bounds,
    } = value
    else {
        return Err(CodecError::malformed(
            "mesh target has no mesh interpretation",
        ));
    };
    let carriers = carriers(ir, entry.id(), object, "Mesh::PropertyMeshKernel", "Mesh")?;
    let index = carriers
        .iter()
        .position(|carrier| carrier.3 == property)
        .ok_or_else(|| CodecError::malformed("mesh target has no current native carrier"))?;
    let meshes = ir
        .model
        .tessellations
        .iter()
        .filter(|mesh| {
            mesh.native_ref.as_deref() == Some(entry.id())
                && belongs(mesh.source_object.as_ref(), object)
        })
        .collect::<Vec<_>>();
    let native_empty = vertices.is_empty() && facets.is_empty();
    let selected: Vec<Option<&cadmpeg_ir::tessellation::Tessellation>> = if meshes.is_empty()
        && native_empty
    {
        vec![None]
    } else {
        meshes
            .iter()
            .enumerate()
            .filter_map(|(ordinal, mesh)| {
                (ordinal == index || index == 0 && ordinal >= carriers.len()).then_some(Some(*mesh))
            })
            .collect()
    };
    if index == 0 && !meshes.is_empty() && (carriers.len() != 1 || meshes.len() != 1) {
        losses.push(crate::loss::FreecadLossCode::ExportKernelAssociationReduced.note(format!(
            "mesh carriers {} are regenerated as {} distinct current meshes; individual original carrier associations are not represented",
            carriers.iter().map(|carrier| carrier.3.as_str()).collect::<Vec<_>>().join(", "), meshes.len()
        )));
    }
    let namespace = ir
        .native
        .namespace("fcstd")
        .ok_or_else(|| CodecError::malformed("mesh entry namespace is missing"))?;
    let originals = namespace.arena_as::<crate::native::EntryRecord>("entries")?;
    let (original_points, original_facets) = compact_mesh(vertices, facets)?;
    let original_triangles = original_facets
        .iter()
        .map(|facet| facet.points)
        .collect::<Vec<_>>();
    let mut names = occupied.clone();
    names.extend(carriers.iter().map(|carrier| carrier.2.clone()));
    let mut emissions = Vec::new();
    for (ordinal, mesh) in selected.into_iter().enumerate() {
        let target_name = if ordinal == 0 {
            name.to_owned()
        } else {
            let mut index = 1_u64;
            loop {
                let candidate = format!("{name}_{index}");
                if names.insert(candidate.clone()) {
                    break candidate;
                }
                index = index
                    .checked_add(1)
                    .ok_or_else(|| CodecError::malformed("mesh property ordinal overflows"))?;
            }
        };
        let (points, facets, current_bounds, mesh_id, changed) = if let Some(mesh) = mesh {
            let points = mesh.vertices();
            let triangles = mesh.triangles();
            let complete = vertices.iter().all(Option::is_some)
                && !value_missing_facets(value)
                && bounds.is_some();
            let changed = !complete || points != original_points || triangles != original_triangles;
            let facets = if changed {
                neighbours(&triangles)?
            } else {
                original_facets.clone()
            };
            (
                points,
                facets,
                if changed { None } else { *bounds },
                Some(mesh.id.as_str().to_owned()),
                changed,
            )
        } else {
            (
                original_points.clone(),
                original_facets.clone(),
                *bounds,
                None,
                false,
            )
        };
        if !precision(&points, property, losses) {
            continue;
        }
        let mut regenerated = changed
            || vertices.iter().any(Option::is_none)
            || value_missing_facets(value)
            || bounds.is_none();
        if vertices.iter().any(Option::is_none) || value_missing_facets(value) || bounds.is_none() {
            omission(
                losses,
                property,
                "missing mesh fields are regenerated from the surviving geometry",
            );
        }
        let bytes = mesh_bytes(*big_endian, &points, &facets, current_bounds)?;
        let file = if ordinal > 0
            || changed && entry.referenced_by().len() > 1
            || entries
                .get(entry.name())
                .is_some_and(|existing| existing != &bytes)
        {
            fresh_member(property, entries, &originals, "bms")?
        } else {
            entry.name().to_owned()
        };
        regenerated |= file != entry.name();
        entries.insert(file.clone(), bytes);
        emissions.push(Emission {
            name: target_name,
            file,
            mesh: mesh_id,
            regenerated,
        });
    }
    Ok(emissions)
}

fn value_missing_facets(value: &EntryContents) -> bool {
    matches!(value, EntryContents::Mesh { facets, .. } if facets.iter().any(Option::is_none))
}

pub(super) fn omits_carrier(
    ir: &CadIr,
    property: &str,
    entry: &str,
    object: Option<&str>,
) -> Result<bool, CodecError> {
    let count = ir
        .model
        .tessellations
        .iter()
        .filter(|mesh| {
            mesh.native_ref.as_deref() == Some(entry)
                && belongs(mesh.source_object.as_ref(), object)
        })
        .count();
    if count == 0 {
        return Ok(false);
    }
    let carriers = carriers(ir, entry, object, "Mesh::PropertyMeshKernel", "Mesh")?;
    Ok(carriers
        .iter()
        .position(|carrier| carrier.3 == property)
        .is_some_and(|index| index >= count))
}
