// SPDX-License-Identifier: Apache-2.0
use super::Slots;
use crate::brep::{TextEdgeRepresentation, TextPointRepresentation};

pub(super) fn point(
    original: &Slots<'_>,
    admitted: &Slots<'_>,
    value: &mut TextPointRepresentation,
) -> Option<()> {
    match value {
        TextPointRepresentation::Curve3d {
            curve, location, ..
        } => {
            *curve = original.reference(admitted, 2, *curve)?;
            *location = original.location(admitted, *location)?;
        }
        TextPointRepresentation::Pcurve {
            curve,
            surface,
            location,
            ..
        } => {
            *curve = original.reference(admitted, 1, *curve)?;
            *surface = original.reference(admitted, 5, *surface)?;
            *location = original.location(admitted, *location)?;
        }
        TextPointRepresentation::Surface {
            surface, location, ..
        } => {
            *surface = original.reference(admitted, 5, *surface)?;
            *location = original.location(admitted, *location)?;
        }
    }
    Some(())
}

pub(super) fn edge(
    original: &Slots<'_>,
    admitted: &Slots<'_>,
    value: &mut TextEdgeRepresentation,
) -> Option<()> {
    match value {
        TextEdgeRepresentation::Curve3d {
            curve, location, ..
        } => {
            *curve = original.reference(admitted, 2, *curve)?;
            *location = original.location(admitted, *location)?;
        }
        TextEdgeRepresentation::Pcurve {
            curve,
            surface,
            location,
            ..
        } => {
            *curve = original.reference(admitted, 1, *curve)?;
            *surface = original.reference(admitted, 5, *surface)?;
            *location = original.location(admitted, *location)?;
        }
        TextEdgeRepresentation::PcurvePair {
            curves,
            surface,
            location,
            ..
        } => {
            for curve in curves {
                *curve = original.reference(admitted, 1, *curve)?;
            }
            *surface = original.reference(admitted, 5, *surface)?;
            *location = original.location(admitted, *location)?;
        }
        TextEdgeRepresentation::Regularity {
            surfaces,
            locations,
            ..
        } => {
            for surface in surfaces {
                *surface = original.reference(admitted, 5, *surface)?;
            }
            for location in locations {
                *location = original.location(admitted, *location)?;
            }
        }
        TextEdgeRepresentation::Polygon3d { polygon, location } => {
            *polygon = original.reference(admitted, 3, *polygon)?;
            *location = original.location(admitted, *location)?;
        }
        TextEdgeRepresentation::PolygonOnTriangulation {
            polygon,
            triangulation,
            location,
        } => {
            *polygon = original.reference(admitted, 4, *polygon)?;
            *triangulation = original.reference(admitted, 6, *triangulation)?;
            *location = original.location(admitted, *location)?;
        }
        TextEdgeRepresentation::PolygonPair {
            polygons,
            triangulation,
            location,
        } => {
            for polygon in polygons {
                *polygon = original.reference(admitted, 4, *polygon)?;
            }
            *triangulation = original.reference(admitted, 6, *triangulation)?;
            *location = original.location(admitted, *location)?;
        }
    }
    Some(())
}
