// SPDX-License-Identifier: Apache-2.0
//! Remap polygon samples to current triangulation node indices.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;

fn key(point: FinitePoint3) -> [u64; 3] {
    let point = point.get();
    [point.x, point.y, point.z].map(|value| if value == 0.0 { 0 } else { value.to_bits() })
}

fn at(vertices: &[FinitePoint3], node: u32) -> Option<FinitePoint3> {
    usize::try_from(node)
        .ok()
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| vertices.get(index).copied())
}

pub(super) fn match_source(
    points: &[FinitePoint3],
    nodes: &[u32],
    vertices: &[FinitePoint3],
) -> bool {
    points.len() == nodes.len()
        && points
            .iter()
            .zip(nodes)
            .all(|(point, node)| at(vertices, *node) == Some(*point))
}

pub(super) fn remap(
    ctx: &DecodeContext<'_>,
    original: &[u32],
    points: &[FinitePoint3],
    vertices: &[FinitePoint3],
) -> Result<Option<Vec<u32>>, CodecError> {
    let mut nodes = Vec::new();
    ctx.reserve_working(&mut nodes, points.len(), "FreeCAD indexed polygon nodes")?;
    if match_source(points, original, vertices) {
        nodes.extend_from_slice(original);
        return Ok(Some(nodes));
    }
    let mut indexes = Vec::new();
    ctx.reserve_working(
        &mut indexes,
        vertices.len(),
        "FreeCAD triangulation point index",
    )?;
    for (index, vertex) in vertices.iter().copied().enumerate() {
        let Some(index) = u32::try_from(index)
            .ok()
            .and_then(|index| index.checked_add(1))
        else {
            return Ok(None);
        };
        indexes.push((key(vertex), index));
    }
    indexes.sort_unstable_by_key(|row| row.0);
    for (position, point) in points.iter().copied().enumerate() {
        if let Some(node) = original
            .get(position)
            .copied()
            .filter(|node| at(vertices, *node) == Some(point))
        {
            nodes.push(node);
            continue;
        }
        let target = key(point);
        let position = indexes.partition_point(|row| row.0 < target);
        let Some((_, index)) = indexes.get(position).filter(|row| row.0 == target) else {
            return Ok(None);
        };
        if indexes.get(position + 1).is_some_and(|row| row.0 == target) {
            return Ok(None);
        }
        nodes.push(*index);
    }
    Ok(Some(nodes))
}
