// SPDX-License-Identifier: Apache-2.0
//! A physical prefix keeps complete BEFORE slots, with independently known arity.

use crate::native::entry_contents::EntryContents;
use crate::native::EntryRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::tessellation::TessellationMesh;
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

type Parts = BTreeMap<String, Vec<(String, u64, u64)>>;

fn keep_complete<T>(slots: &mut [Option<T>], name: &str, level: &str, cut: usize, parts: &Parts) {
    let rows = parts[level]
        .iter()
        .filter(|(member, _, _)| member == name)
        .collect::<Vec<_>>();
    assert_eq!(
        slots.len(),
        rows.len(),
        "original slots and independent rows"
    );
    for (slot, (_, start, length)) in slots.iter_mut().zip(rows) {
        if start + length > u64::try_from(cut).unwrap() {
            *slot = None;
        }
    }
}

pub(super) fn prefix(
    ctx: &DecodeContext<'_>,
    before: &CadIr,
    name: &str,
    cut: usize,
    parts: &Parts,
) -> CadIr {
    let mut expected = before.clone();
    let mut entries = before
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<EntryRecord>("entries")
        .unwrap();
    let entry = entries
        .iter_mut()
        .find(|entry| entry.name() == name)
        .unwrap();
    let mesh = name == "MeshKernel.bms";
    if (mesh && cut < 272) || (!mesh && cut < 4) {
        entry.contents.retain(|contents| {
            !matches!(
                contents,
                EntryContents::Mesh { .. } | EntryContents::Points { .. }
            )
        });
    } else {
        for contents in &mut entry.contents {
            match contents {
                EntryContents::Mesh {
                    vertices,
                    facets,
                    bounds,
                    ..
                } => {
                    keep_complete(vertices, name, "freecad.mesh_point", cut, parts);
                    keep_complete(facets, name, "freecad.mesh_facet", cut, parts);
                    let (_, start, length) = &parts["freecad.mesh_bounding_box"][0];
                    if start + length > u64::try_from(cut).unwrap() {
                        *bounds = None;
                    }
                }
                EntryContents::Points { points } => {
                    keep_complete(points, name, "freecad.point_kernel_point", cut, parts);
                }
                _ => {}
            }
        }
    }
    let current = entry.clone();
    expected
        .native
        .namespace_mut("fcstd")
        .set_arena(ctx, "entries", &entries)
        .unwrap();
    let shape = current
        .contents()
        .iter()
        .find_map(|contents| match contents {
            EntryContents::Mesh {
                vertices, facets, ..
            } => Some((vertices, facets)),
            _ => None,
        });
    expected.model.tessellations.retain_mut(|mesh| {
        if mesh.native_ref.as_deref() != Some(current.id()) {
            return true;
        }
        let Some((vertices, facets)) = shape else {
            return false;
        };
        let mut positions = Vec::new();
        let indices = vertices
            .iter()
            .map(|position| {
                position.map(|position| {
                    let index = u32::try_from(positions.len()).unwrap();
                    positions.push(position);
                    index
                })
            })
            .collect::<Vec<_>>();
        let triangles = facets
            .iter()
            .flatten()
            .filter_map(|facet| {
                let [a, b, c] = facet
                    .points
                    .map(|index| indices[usize::try_from(index).unwrap()]);
                Some([a?, b?, c?])
            })
            .collect();
        *mesh = mesh
            .clone()
            .with_mesh(
                TessellationMesh::List {
                    vertices: positions,
                    triangles,
                },
                mesh.channels().to_vec(),
            )
            .unwrap();
        true
    });
    let points = current
        .contents()
        .iter()
        .find_map(|contents| match contents {
            EntryContents::Points { points } => Some(points),
            _ => None,
        });
    let mut slot = 0;
    expected.model.points.retain(|point| {
        if point.native_ref.as_deref() != Some(current.id()) {
            return true;
        }
        let held = points.is_some_and(|points| points[slot].is_some());
        slot += 1;
        held
    });
    expected
}
