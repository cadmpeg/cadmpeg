// SPDX-License-Identifier: Apache-2.0
//! Application mesh kernels authored from current tessellations.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::products::{OccurrenceParent, PrototypeReference};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use super::xml::{self, Object};
use crate::loss::FreecadLossCode;
use crate::native::{PropertyBody, PropertyRecord};

fn placement(
    ir: &CadIr,
    property: &PropertyRecord,
    properties: &[PropertyRecord],
) -> Option<Transform> {
    let occurrence = ir.model.occurrences.iter().find(|occurrence| {
        occurrence.native_ref.as_deref() == Some(property.owner.as_str())
            && matches!(&occurrence.prototype, PrototypeReference::Local { definition }
                if ir.model.product_definitions.iter().any(|record| record.id == *definition && record.native_ref.as_deref() == Some(property.owner.as_str())))
    });
    let Some(mut occurrence) = occurrence else {
        return match properties
            .iter()
            .find(|candidate| candidate.owner == property.owner && candidate.name == "Placement")
        {
            Some(property) => crate::placement::placement_matrix(property)
                .ok()?
                .map(crate::native::frame::FiniteFrame::transform),
            None => Some(Transform::identity()),
        };
    };
    let mut chain = Vec::new();
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(occurrence.id.as_str()) {
            return None;
        }
        chain.push(occurrence.effective_transform().ok()??);
        match &occurrence.parent {
            OccurrenceParent::Root {} => break,
            OccurrenceParent::Occurrence { occurrence: parent } => {
                occurrence = ir
                    .model
                    .occurrences
                    .iter()
                    .find(|record| record.id == *parent)?;
            }
        }
    }
    chain
        .into_iter()
        .rev()
        .try_fold(Transform::identity(), |world, local| {
            world.compose(local).ok()
        })
}

fn encode(
    ctx: &DecodeContext<'_>,
    mesh: &Tessellation,
    transform: Transform,
    source: Option<&[u8]>,
) -> Result<Option<Vec<u8>>, CodecError> {
    let Ok(inverse) = transform.try_inverse_affine() else {
        return Ok(None);
    };
    let vertices = mesh.vertices();
    let triangles = mesh.triangles();
    let (Ok(point_count), Ok(facet_count)) = (
        u32::try_from(vertices.len()),
        u32::try_from(triangles.len()),
    ) else {
        return Ok(None);
    };
    let Some(capacity) = vertices
        .len()
        .checked_mul(12)
        .and_then(|n| {
            triangles
                .len()
                .checked_mul(24)
                .and_then(|m| n.checked_add(m))
        })
        .and_then(|n| n.checked_add(crate::layout::mesh_kernel_side_entry_header::LEN + 8 + 24))
    else {
        return Ok(None);
    };
    let mut coordinates = Vec::new();
    ctx.reserve_working(&mut coordinates, vertices.len(), "FreeCAD mesh coordinates")?;
    let mut bounds = [[f32::INFINITY, f32::NEG_INFINITY]; 3];
    let source_order = source.and_then(|bytes| {
        let magic = crate::layout::mesh_kernel_side_entry_header::MAGIC_VALUE;
        if cadmpeg_core::decode::View::u32_le_at(bytes, 0) == Some(magic) {
            Some(false)
        } else if cadmpeg_core::decode::View::u32_be_at(bytes, 0) == Some(magic) {
            Some(true)
        } else {
            None
        }
    });
    for (index, vertex) in vertices.into_iter().enumerate() {
        let Some(local) = inverse.apply_point(vertex.get()) else {
            return Ok(None);
        };
        let local = local.get();
        let (Some(x), Some(y), Some(z)) = (
            cadmpeg_core::convert::f32_from_f64(local.x),
            cadmpeg_core::convert::f32_from_f64(local.y),
            cadmpeg_core::convert::f32_from_f64(local.z),
        ) else {
            return Ok(None);
        };
        let mut values = [x, y, z];
        if let (Some(source), Some(big_endian)) = (source, source_order) {
            if let Some(start) = index.checked_mul(12).and_then(|offset| {
                offset.checked_add(crate::layout::mesh_kernel_side_entry_header::LEN + 8)
            }) {
                if let Some(bytes) = start.checked_add(12).and_then(|end| source.get(start..end)) {
                    let mut original = [0.0_f32; 3];
                    for (index, value) in original.iter_mut().enumerate() {
                        let read = if big_endian {
                            cadmpeg_core::decode::View::f32_be_at(bytes, index * 4)
                        } else {
                            cadmpeg_core::decode::View::f32_le_at(bytes, index * 4)
                        };
                        let Some(read) = read else {
                            return Ok(None);
                        };
                        *value = read;
                    }
                    let local = original.map(f64::from);
                    let world = transform.affine_rows().map(|row| {
                        row[0] * local[0] + row[1] * local[1] + row[2] * local[2] + row[3]
                    });
                    let point = vertex.get();
                    if original.iter().all(|value| value.is_finite())
                        && world == [point.x, point.y, point.z]
                    {
                        values = original;
                    }
                }
            }
        }
        let local = values.map(f64::from);
        let world = transform
            .affine_rows()
            .map(|row| row[0] * local[0] + row[1] * local[1] + row[2] * local[2] + row[3]);
        let point = vertex.get();
        if world != [point.x, point.y, point.z] {
            return Ok(None);
        }
        for axis in 0..3 {
            bounds[axis][0] = bounds[axis][0].min(values[axis]);
            bounds[axis][1] = bounds[axis][1].max(values[axis]);
        }
        coordinates.push(values);
    }
    if coordinates.is_empty() {
        bounds = [[0.0; 2]; 3];
    }
    let mut edges = BTreeMap::<(u32, u32), Vec<(usize, usize)>>::new();
    for (facet, triangle) in triangles.iter().enumerate() {
        for slot in 0..3 {
            let a = triangle[slot];
            let b = triangle[(slot + 1) % 3];
            edges
                .entry((a.min(b), a.max(b)))
                .or_default()
                .push((facet, slot));
        }
    }
    let mut neighbours = Vec::new();
    ctx.reserve_working(
        &mut neighbours,
        triangles.len(),
        "FreeCAD mesh facet neighbours",
    )?;
    neighbours.resize(triangles.len(), [u32::MAX; 3]);
    for uses in edges.values() {
        if let [(first, first_slot), (second, second_slot)] = uses.as_slice() {
            if first != second {
                neighbours[*first][*first_slot] =
                    u32::try_from(*second).map_err(CodecError::malformed)?;
                neighbours[*second][*second_slot] =
                    u32::try_from(*first).map_err(CodecError::malformed)?;
            }
        }
    }
    let mut bytes = Vec::new();
    ctx.reserve_working(&mut bytes, capacity, "FreeCAD mesh kernel bytes")?;
    bytes.extend_from_slice(
        &crate::layout::mesh_kernel_side_entry_header::MAGIC_VALUE.to_le_bytes(),
    );
    bytes.extend_from_slice(
        &crate::layout::mesh_kernel_side_entry_header::VERSION_VALUE.to_le_bytes(),
    );
    bytes.resize(crate::layout::mesh_kernel_side_entry_header::LEN, 0);
    if let Some(information) = source.and_then(|source| {
        source.get(
            crate::layout::mesh_kernel_side_entry_header::INFORMATION
                ..crate::layout::mesh_kernel_side_entry_header::LEN,
        )
    }) {
        bytes[crate::layout::mesh_kernel_side_entry_header::INFORMATION..]
            .copy_from_slice(information);
    }
    bytes.extend_from_slice(&point_count.to_le_bytes());
    bytes.extend_from_slice(&facet_count.to_le_bytes());
    for point in coordinates {
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    for (triangle, neighbours) in triangles.iter().zip(neighbours) {
        for value in triangle.iter().copied().chain(neighbours) {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    for pair in bounds {
        for value in pair {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    Ok(Some(bytes))
}

pub(super) fn write(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    properties: &[PropertyRecord],
    names: &HashMap<String, String>,
    (entry_names, retained): (&HashSet<String>, &HashMap<String, Vec<u8>>),
    (objects, entries): (&mut [Object], &mut BTreeMap<String, Vec<u8>>),
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    if !ir.scope.model {
        return Ok(());
    }
    for property in properties
        .iter()
        .filter(|property| property.type_name == "Mesh::PropertyMeshKernel")
    {
        let Some(name) = names.get(&property.owner) else {
            continue;
        };
        let Some(object) = objects.iter_mut().find(|object| object.name == *name) else {
            continue;
        };
        let mesh = ir
            .model
            .tessellations
            .iter()
            .find(|mesh| mesh.id.as_str() == format!("{}:mesh", property.id));
        let Some(mesh) = mesh else {
            object.properties.shift_remove(&property.name);
            continue;
        };
        let Some(original_file) = property
            .side_entries()
            .first()
            .filter(|_| property.side_entries().len() == 1)
        else {
            object.properties.shift_remove(&property.name);
            losses.push(FreecadLossCode::ExportGeometryNotTransferred.note(format!("tessellation {} cannot retain its mesh-kernel association because its side entry is absent", mesh.id)));
            continue;
        };
        let bytes = match placement(ir, property, properties) {
            Some(transform) => encode(
                ctx,
                mesh,
                transform,
                retained.get(original_file).map(Vec::as_slice),
            )?,
            None => None,
        };
        let Some(bytes) = bytes else {
            object.properties.shift_remove(&property.name);
            losses.push(FreecadLossCode::ExportGeometryNotTransferred.note(format!("tessellation {} cannot retain its mesh-kernel association because its current coordinates cannot be represented in the local binary32 frame", mesh.id)));
            continue;
        };
        let shared = properties
            .iter()
            .filter(|other| other.side_entries().contains(original_file))
            .count()
            != 1;
        let mut file = original_file.clone();
        if shared || entries.contains_key(&file) {
            let mut ordinal = 0_usize;
            loop {
                file = format!("MeshKernel{ordinal}.bms");
                if !entry_names.contains(&file) && !entries.contains_key(&file) {
                    break;
                }
                ordinal = ordinal
                    .checked_add(1)
                    .ok_or_else(|| CodecError::malformed("mesh entry name space exhausted"))?;
            }
        }
        let mut generated = property.clone();
        let Some(PropertyBody::Persisted {
            values,
            side_entries,
            ..
        }) = generated.body.as_mut()
        else {
            object.properties.shift_remove(&property.name);
            losses.push(FreecadLossCode::ExportGeometryNotTransferred.note(format!("tessellation {} cannot retain its mesh-kernel association because its property has no persisted value", mesh.id)));
            continue;
        };
        let Some(root) = values
            .iter_mut()
            .find(|value| value.parent.is_none() && value.tag == "Mesh")
        else {
            object.properties.shift_remove(&property.name);
            losses.push(FreecadLossCode::ExportGeometryNotTransferred.note(format!("tessellation {} cannot retain its mesh-kernel association because its property has no Mesh element", mesh.id)));
            continue;
        };
        root.attributes.insert("file".into(), file.clone());
        side_entries.clear();
        side_entries.push(file.clone());
        let available = HashSet::from([file.clone()]);
        let Some(content) = xml::native_named(&generated, names, &available)
            .ok()
            .flatten()
        else {
            object.properties.shift_remove(&property.name);
            losses.push(FreecadLossCode::ExportGeometryNotTransferred.note(format!("tessellation {} cannot retain its mesh-kernel association because its property cannot be emitted", mesh.id)));
            continue;
        };
        object.set(&property.name, content);
        object.mark_geometry_written();
        entries.insert(file, bytes);
    }
    Ok(())
}
