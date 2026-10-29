// SPDX-License-Identifier: Apache-2.0

use crate::brep::{TextShapeKind, TextTShapeGeometry};

pub(crate) fn geometry_for_kind(kind: TextShapeKind) -> TextTShapeGeometry {
    match kind {
        TextShapeKind::Vertex => TextTShapeGeometry::Vertex {
            tolerance: cadmpeg_ir::scalar::FiniteReal::ZERO,
            point: cadmpeg_ir::features::FinitePoint3::ZERO,
            representations: Vec::new(),
        },
        TextShapeKind::Edge => TextTShapeGeometry::Edge {
            tolerance: cadmpeg_ir::scalar::FiniteReal::ZERO,
            same_parameter: false,
            same_range: false,
            degenerated: false,
            representations: Vec::new(),
        },
        TextShapeKind::Face => TextTShapeGeometry::Face {
            natural_restriction: false,
            tolerance: cadmpeg_ir::scalar::FiniteReal::ZERO,
            surface: None,
            location: crate::brep::LocationRef::Identity,
            triangulation: None,
        },
        TextShapeKind::Wire => TextTShapeGeometry::Wire,
        TextShapeKind::Shell => TextTShapeGeometry::Shell,
        TextShapeKind::Solid => TextTShapeGeometry::Solid,
        TextShapeKind::CompSolid => TextTShapeGeometry::CompSolid,
        TextShapeKind::Compound => TextTShapeGeometry::Compound,
    }
}
