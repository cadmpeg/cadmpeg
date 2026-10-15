// SPDX-License-Identifier: Apache-2.0
//! Borrowed validation traversal of admitted mesh rows.

use cadmpeg_core::decode::{index_from_u32, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::tessellation::{Strips, Tessellation, TessellationMesh};

pub(super) fn origin(mesh: &Tessellation) -> Option<FinitePoint3> {
    match mesh.mesh() {
        TessellationMesh::List {
            vertices,
            triangles,
        } => vertices.get(index_from_u32(triangles.first()?[0])).copied(),
        TessellationMesh::ShadedList {
            vertices,
            triangles,
        } => vertices
            .get(index_from_u32(triangles.first()?[0]))
            .map(|row| row.position),
        TessellationMesh::CornerShadedList {
            vertices,
            triangles,
        } => vertices
            .get(index_from_u32(triangles.first()?.corners[0]))
            .copied(),
        TessellationMesh::Strips { strips } => {
            strips.as_slice().first()?.vertices().first().copied()
        }
        TessellationMesh::ShadedStrips { strips } => strips
            .as_slice()
            .first()?
            .vertices()
            .first()
            .map(|row| row.position),
    }
}

pub(super) fn extent(
    mesh: &Tessellation,
    origin: FinitePoint3,
    ctx: &DecodeContext<'_>,
) -> Result<f64, CodecError> {
    let mut extent = 0.0_f64;
    let mut visit = |point: FinitePoint3| {
        extent = extent
            .max((point.x - origin.x).abs())
            .max((point.y - origin.y).abs())
            .max((point.z - origin.z).abs());
    };
    match mesh.mesh() {
        TessellationMesh::List { vertices, .. }
        | TessellationMesh::CornerShadedList { vertices, .. } => {
            for &point in ctx.admit_iter(vertices, "STEP validation vertex extent traversal")? {
                visit(point);
            }
        }
        TessellationMesh::ShadedList { vertices, .. } => {
            for row in ctx.admit_iter(vertices, "STEP validation vertex extent traversal")? {
                visit(row.position);
            }
        }
        TessellationMesh::Strips { strips } => {
            let mut source = strips.as_slice().iter();
            for _ in 0..source.len() {
                let strip = ctx
                    .next_charged(&mut source, "STEP validation strip extent traversal")?
                    .ok_or_else(|| {
                        CodecError::malformed("STEP validation strip source ended early")
                    })?;
                for &point in
                    ctx.admit_iter(strip.vertices(), "STEP validation vertex extent traversal")?
                {
                    visit(point);
                }
            }
        }
        TessellationMesh::ShadedStrips { strips } => {
            let mut source = strips.as_slice().iter();
            for _ in 0..source.len() {
                let strip = ctx
                    .next_charged(&mut source, "STEP validation strip extent traversal")?
                    .ok_or_else(|| {
                        CodecError::malformed("STEP validation strip source ended early")
                    })?;
                for row in
                    ctx.admit_iter(strip.vertices(), "STEP validation vertex extent traversal")?
                {
                    visit(row.position);
                }
            }
        }
    }
    Ok(extent)
}

/// Visit triangle indices and their three positions in source order.
/// Returning false stops before the next triangle. List indexing borrows its
/// rows; strip windows preserve winding without materializing either lane.
pub(super) fn triangles(
    mesh: &Tessellation,
    ctx: &DecodeContext<'_>,
    mut visitor: impl FnMut([u32; 3], [FinitePoint3; 3]) -> Result<bool, CodecError>,
) -> Result<bool, CodecError> {
    match mesh.mesh() {
        TessellationMesh::List {
            vertices,
            triangles,
        } => list(
            vertices,
            triangles,
            |row| *row,
            |point| *point,
            ctx,
            &mut visitor,
        ),
        TessellationMesh::ShadedList {
            vertices,
            triangles,
        } => list(
            vertices,
            triangles,
            |row| *row,
            |row| row.position,
            ctx,
            &mut visitor,
        ),
        TessellationMesh::CornerShadedList {
            vertices,
            triangles,
        } => list(
            vertices,
            triangles,
            |row| row.corners,
            |point| *point,
            ctx,
            &mut visitor,
        ),
        TessellationMesh::Strips { strips } => strip(strips, |point| *point, ctx, &mut visitor),
        TessellationMesh::ShadedStrips { strips } => {
            strip(strips, |row| row.position, ctx, &mut visitor)
        }
    }
}

fn list<V, T>(
    vertices: &[V],
    triangles: &[T],
    corners: impl Fn(&T) -> [u32; 3],
    position: impl Fn(&V) -> FinitePoint3,
    ctx: &DecodeContext<'_>,
    visitor: &mut impl FnMut([u32; 3], [FinitePoint3; 3]) -> Result<bool, CodecError>,
) -> Result<bool, CodecError> {
    let mut source = triangles.iter();
    for _ in 0..source.len() {
        let row = ctx
            .next_charged(&mut source, "step_validation_mesh_triangles")?
            .ok_or_else(|| CodecError::malformed("STEP validation traversal source ended early"))?;
        let indices = corners(row);
        let [Some(a), Some(b), Some(c)] =
            indices.map(|index| vertices.get(index_from_u32(index)).map(&position))
        else {
            return Ok(false);
        };
        if !visitor(indices, [a, b, c])? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn strip<V>(
    strips: &Strips<V>,
    position: impl Fn(&V) -> FinitePoint3,
    ctx: &DecodeContext<'_>,
    visitor: &mut impl FnMut([u32; 3], [FinitePoint3; 3]) -> Result<bool, CodecError>,
) -> Result<bool, CodecError> {
    let mut base = 0u32;
    let mut strips_source = strips.as_slice().iter();
    for _ in 0..strips_source.len() {
        let strip = ctx
            .next_charged(
                &mut strips_source,
                "STEP validation triangle strip traversal",
            )?
            .ok_or_else(|| CodecError::malformed("STEP validation strip source ended early"))?;
        let mut source = strip.vertices().windows(3);
        let mut index = 0u32;
        for _ in 0..source.len() {
            let rows = ctx
                .next_charged(&mut source, "step_validation_mesh_triangles")?
                .ok_or_else(|| {
                    CodecError::malformed("STEP validation traversal source ended early")
                })?;
            let points = [position(&rows[0]), position(&rows[1]), position(&rows[2])];
            let a = base + index;
            let (indices, points) = if index.is_multiple_of(2) {
                ([a, a + 1, a + 2], points)
            } else {
                ([a, a + 2, a + 1], [points[0], points[2], points[1]])
            };
            if !visitor(indices, points)? {
                return Ok(false);
            }
            index += 1;
        }
        // Strips::new proves the total vertex span fits u32.
        base += index + 2;
    }
    Ok(true)
}

#[cfg(test)]
mod tests;
