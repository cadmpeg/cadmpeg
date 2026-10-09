// SPDX-License-Identifier: Apache-2.0
//! Borrowed iteration over admitted tessellation vertex rows.

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::tessellation::TessellationMesh;

/// An admitted tessellation mesh: finite positions and normals.
pub(super) type AdmittedMesh = TessellationMesh<FinitePoint3, cadmpeg_ir::features::FiniteVector3>;

/// The strip rows of a strip mesh, borrowed in mesh order.
type StripRows<'m, V> = std::iter::FlatMap<
    std::slice::Iter<'m, cadmpeg_ir::tessellation::Strip<V>>,
    &'m [V],
    fn(&'m cadmpeg_ir::tessellation::Strip<V>) -> &'m [V],
>;

/// The vertex rows of a mesh in mesh order, borrowed: each position and, when
/// the mesh carries per-vertex normals, its normal. Each step is fixed work.
pub(crate) enum MeshRows<'m> {
    Positions(std::slice::Iter<'m, FinitePoint3>),
    Shaded(std::slice::Iter<'m, ShadedRow>),
    PositionStrips(StripRows<'m, FinitePoint3>),
    ShadedStrips(StripRows<'m, ShadedRow>),
}

pub(super) type ShadedRow =
    cadmpeg_ir::tessellation::ShadedVertex<FinitePoint3, cadmpeg_ir::features::FiniteVector3>;

impl<'m> MeshRows<'m> {
    pub(crate) fn new(mesh: &'m AdmittedMesh) -> Self {
        match mesh {
            TessellationMesh::List { vertices, .. }
            | TessellationMesh::CornerShadedList { vertices, .. } => {
                Self::Positions(vertices.iter())
            }
            TessellationMesh::ShadedList { vertices, .. } => Self::Shaded(vertices.iter()),
            TessellationMesh::Strips { strips } => {
                let rows: fn(
                    &'m cadmpeg_ir::tessellation::Strip<FinitePoint3>,
                ) -> &'m [FinitePoint3] = cadmpeg_ir::tessellation::Strip::vertices;
                Self::PositionStrips(strips.as_slice().iter().flat_map(rows))
            }
            TessellationMesh::ShadedStrips { strips } => {
                let rows: fn(&'m cadmpeg_ir::tessellation::Strip<ShadedRow>) -> &'m [ShadedRow] =
                    cadmpeg_ir::tessellation::Strip::vertices;
                Self::ShadedStrips(strips.as_slice().iter().flat_map(rows))
            }
        }
    }

    /// Whether every vertex row carries a shading normal and there is at
    /// least one row.
    pub(super) fn shaded(mesh: &'m AdmittedMesh) -> bool {
        let mut rows = Self::new(mesh);
        matches!(rows, Self::Shaded(_) | Self::ShadedStrips(_)) && rows.next().is_some()
    }
}

impl Iterator for MeshRows<'_> {
    type Item = (FinitePoint3, Option<cadmpeg_ir::features::FiniteVector3>);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Positions(rows) => rows.next().map(|position| (*position, None)),
            Self::PositionStrips(rows) => rows.next().map(|position| (*position, None)),
            Self::Shaded(rows) => rows.next().map(|row| (row.position, Some(row.normal))),
            Self::ShadedStrips(rows) => rows.next().map(|row| (row.position, Some(row.normal))),
        }
    }
}

