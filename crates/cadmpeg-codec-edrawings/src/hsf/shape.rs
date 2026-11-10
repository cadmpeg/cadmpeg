// SPDX-License-Identifier: Apache-2.0
//! Shell decoding: vertices, triangle strips, and instance affines.
//!
//! A shell carries `f32` vertices in metres and a face list of triangle
//! strips. Strip triangle `i` is `(s[i], s[i+1], s[i+2])` for even `i` and
//! `(s[i+1], s[i], s[i+2])` for odd `i`, which makes every triangle
//! counter-clockwise seen from outside. A shell instance applies an affine
//! to the points of a source shell and reuses its strips unchanged. Lengths
//! leave this module in millimetres.

use cadmpeg_core::decode::View;

use super::scene::{Object, Scene};
use super::walk::Shell;

/// Millimetres per metre.
pub(super) const MILLIMETRES_PER_METRE: f64 = 1000.0;
/// Deepest chain of shell instances followed to a source shell.
const MAX_INSTANCE_CHAIN: usize = 8;
/// Index width marker of an 8-bit face list.
const WIDTH_8: u8 = 8;
/// Index width marker of a 16-bit face list.
const WIDTH_16: u8 = 16;

/// A point set with triangles.
#[derive(Debug, Default)]
pub(super) struct MeshData {
    /// Positions in millimetres.
    pub(super) positions: Vec<[f64; 3]>,
    /// Counter-clockwise triangles.
    pub(super) triangles: Vec<[u32; 3]>,
}

/// Why a shell could not be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShellError {
    /// The object id is not in the store.
    Missing,
    /// The object is not a shell and not an instance of one.
    NotShell,
    /// The face list is malformed.
    FaceList,
    /// A strip index lies outside the shell.
    Index,
    /// The mesh would exceed the caps.
    TooLarge,
}

impl ShellError {
    /// Text for loss messages.
    pub(super) const fn text(self) -> &'static str {
        match self {
            Self::Missing => "object is missing from the store",
            Self::NotShell => "object is not a shell",
            Self::FaceList => "face list is malformed",
            Self::Index => "strip index lies outside the shell",
            Self::TooLarge => "mesh exceeds the size caps",
        }
    }
}

/// An affine in the stream convention: `p' = p * R + t`.
pub(super) fn apply(matrix: &[f32; 12], point: [f64; 3]) -> [f64; 3] {
    let m: [f64; 12] = matrix.map(f64::from);
    let [x, y, z] = point;
    let [r0, r1, r2, r3, r4, r5, r6, r7, r8, tx, ty, tz] = m;
    [
        x * r0 + y * r3 + z * r6 + tx,
        x * r1 + y * r4 + z * r7 + ty,
        x * r2 + y * r5 + z * r8 + tz,
    ]
}

/// Determinant of the linear part of `matrix`.
pub(super) fn determinant(matrix: &[f32; 12]) -> f64 {
    let [r0, r1, r2, r3, r4, r5, r6, r7, r8, _, _, _] = matrix.map(f64::from);
    r0 * (r4 * r8 - r5 * r7) - r1 * (r3 * r8 - r5 * r6) + r2 * (r3 * r7 - r4 * r6)
}

/// The vertices of `shell` in metres.
fn vertices(shell: &Shell<'_>) -> Vec<[f64; 3]> {
    let mut view = View::over_retained(shell.points);
    let mut points = Vec::new();
    while let (Some(x), Some(y), Some(z)) = (view.f32_le(), view.f32_le(), view.f32_le()) {
        points.push([f64::from(x), f64::from(y), f64::from(z)]);
    }
    points
}

/// The strips of a face list as index lists.
fn strips(shell: &Shell<'_>) -> Result<Vec<Vec<u32>>, ShellError> {
    let Some((&width, body)) = shell.facelist.split_first() else {
        return Err(ShellError::FaceList);
    };
    let values: Vec<u32> = match width {
        WIDTH_8 => body.iter().map(|&b| u32::from(b)).collect(),
        WIDTH_16 => {
            if body.len() % 2 != 0 {
                return Err(ShellError::FaceList);
            }
            let mut view = View::over_retained(body);
            std::iter::from_fn(|| view.u16_le().map(u32::from)).collect()
        }
        _ => return Err(ShellError::FaceList),
    };
    let mut out = Vec::new();
    let mut rest: &[u32] = &values;
    while let Some((&count, tail)) = rest.split_first() {
        let count = usize::try_from(count).map_err(|_| ShellError::FaceList)?;
        if count > tail.len() {
            return Err(ShellError::FaceList);
        }
        let (strip, after) = tail.split_at(count);
        out.push(strip.to_vec());
        rest = after;
    }
    Ok(out)
}

/// The triangles of one strip, counter-clockwise.
pub(super) fn strip_triangles(strip: &[u32], out: &mut Vec<[u32; 3]>) {
    for (i, window) in strip.windows(3).enumerate() {
        if let [a, b, c] = *window {
            out.push(if i % 2 == 0 { [a, b, c] } else { [b, a, c] });
        }
    }
}

/// Resolves object `id` to its source shell and the affine to apply.
fn resolve<'a>(scene: &Scene<'a>, id: u32) -> Result<(Shell<'a>, Option<[f32; 12]>), ShellError> {
    let mut current = id;
    let mut affine: Option<[f32; 12]> = None;
    for _ in 0..MAX_INSTANCE_CHAIN {
        match scene.objects.get(&current) {
            None => return Err(ShellError::Missing),
            Some(Object::Shell(shell)) => return Ok((*shell, affine)),
            Some(Object::Instance(instance)) => {
                if affine.is_some() {
                    // A chained instance would need a composed affine; the
                    // stream never nests them.
                    return Err(ShellError::NotShell);
                }
                affine = Some(instance.matrix);
                current = u32::try_from(instance.source).map_err(|_| ShellError::Missing)?;
            }
        }
    }
    Err(ShellError::NotShell)
}

/// Appends the shell of object `id` to `mesh`. `world` is applied after the
/// instance affine. Triangles flip when `world` mirrors.
pub(super) fn append_shell(
    scene: &Scene<'_>,
    id: u32,
    world: Option<&[f32; 12]>,
    limits: (usize, usize),
    mesh: &mut MeshData,
) -> Result<(), ShellError> {
    let (shell, affine) = resolve(scene, id)?;
    let strips = strips(&shell)?;
    let points = vertices(&shell);
    let mut triangles = Vec::new();
    for strip in &strips {
        strip_triangles(strip, &mut triangles);
    }
    if triangles
        .iter()
        .flatten()
        .any(|&index| usize::try_from(index).map_or(true, |i| i >= points.len()))
    {
        return Err(ShellError::Index);
    }
    let (max_positions, max_triangles) = limits;
    let positions_fit = mesh
        .positions
        .len()
        .checked_add(points.len())
        .is_some_and(|total| total <= max_positions);
    let triangles_fit = mesh
        .triangles
        .len()
        .checked_add(triangles.len())
        .is_some_and(|total| total <= max_triangles);
    if !positions_fit || !triangles_fit {
        return Err(ShellError::TooLarge);
    }
    let base = u32::try_from(mesh.positions.len()).map_err(|_| ShellError::TooLarge)?;
    let flip = world.is_some_and(|matrix| determinant(matrix) < 0.0);
    for point in points {
        let local = match &affine {
            Some(matrix) => apply(matrix, point),
            None => point,
        };
        let placed = match world {
            Some(matrix) => apply(matrix, local),
            None => local,
        };
        mesh.positions
            .push(placed.map(|v| v * MILLIMETRES_PER_METRE));
    }
    for [a, b, c] in triangles {
        let shifted = |index: u32| base.checked_add(index).ok_or(ShellError::TooLarge);
        let (a, b, c) = (shifted(a)?, shifted(b)?, shifted(c)?);
        mesh.triangles
            .push(if flip { [a, c, b] } else { [a, b, c] });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
