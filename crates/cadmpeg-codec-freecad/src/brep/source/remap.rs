// SPDX-License-Identifier: Apache-2.0
//! Resolve source table indices without rebinding a deleted row to its neighbor.

use super::*;

#[derive(Default)]
pub(crate) struct Indices(BTreeMap<u8, BTreeMap<usize, usize>>);
impl Indices {
    pub(crate) fn get(&self, table: u8, index: usize) -> Option<usize> {
        self.0.get(&table)?.get(&index).copied()
    }
    fn location(&self, index: usize) -> Option<usize> {
        if index == 0 {
            Some(0)
        } else {
            self.get(0, index)
        }
    }
    pub(crate) fn bind(&mut self, table: u8, source: usize, target: usize) {
        self.0.entry(table).or_default().insert(source, target);
    }
    pub(crate) fn resolve(&self, mut value: BrepValue) -> Option<BrepValue> {
        match &mut value {
            BrepValue::Location(location) => {
                for factor in &mut location.factors {
                    factor.location = self.get(0, factor.location)?;
                }
            }
            BrepValue::TShape(shapes) => {
                let shape = shapes.0.first_mut()?;
                match &mut shape.geometry {
                    TextTShapeGeometry::Vertex {
                        representations, ..
                    } => {
                        *representations = representations
                            .iter()
                            .cloned()
                            .filter_map(|mut value| {
                                self.point(&mut value)?;
                                Some(value)
                            })
                            .collect();
                    }
                    TextTShapeGeometry::Edge {
                        representations, ..
                    } => {
                        *representations = representations
                            .iter()
                            .cloned()
                            .filter_map(|mut value| {
                                self.edge(&mut value)?;
                                Some(value)
                            })
                            .collect();
                    }
                    TextTShapeGeometry::Face {
                        surface,
                        location,
                        triangulation,
                        ..
                    } => {
                        if let Some(index) = surface {
                            *surface = TableRef::optional(self.get(5, index.index())?);
                        }
                        *location = self.location(location.index())?.into();
                        if let Some(index) = triangulation {
                            *triangulation = TableRef::optional(self.get(6, index.index())?);
                        }
                    }
                    _ => {}
                }
                shape.children = shape
                    .children
                    .iter()
                    .cloned()
                    .filter_map(|mut child| {
                        self.get(7, child.shape)?;
                        child.location = self.location(child.location.index())?.into();
                        Some(child)
                    })
                    .collect();
            }
            BrepValue::Roots(roots) => {
                *roots = roots
                    .iter()
                    .cloned()
                    .filter_map(|mut root| {
                        self.get(7, root.shape)?;
                        root.location = self.location(root.location.index())?.into();
                        Some(root)
                    })
                    .collect();
            }
            _ => {}
        }
        Some(value)
    }
    fn point(&self, value: &mut TextPointRepresentation) -> Option<()> {
        match value {
            TextPointRepresentation::Curve3d {
                curve, location, ..
            } => {
                *curve = self.get(2, *curve)?;
                *location = self.location(*location)?;
            }
            TextPointRepresentation::Pcurve {
                curve,
                surface,
                location,
                ..
            } => {
                *curve = self.get(1, *curve)?;
                *surface = self.get(5, *surface)?;
                *location = self.location(*location)?;
            }
            TextPointRepresentation::Surface {
                surface, location, ..
            } => {
                *surface = self.get(5, *surface)?;
                *location = self.location(*location)?;
            }
        }
        Some(())
    }
    pub(super) fn edge(&self, value: &mut TextEdgeRepresentation) -> Option<()> {
        match value {
            TextEdgeRepresentation::Curve3d {
                curve, location, ..
            } => {
                *curve = self.get(2, *curve)?;
                *location = self.location(*location)?;
            }
            TextEdgeRepresentation::Pcurve {
                curve,
                surface,
                location,
                ..
            } => {
                *curve = self.get(1, *curve)?;
                *surface = self.get(5, *surface)?;
                *location = self.location(*location)?;
            }
            TextEdgeRepresentation::PcurvePair {
                curves,
                surface,
                location,
                ..
            } => {
                for curve in curves {
                    *curve = self.get(1, *curve)?;
                }
                *surface = self.get(5, *surface)?;
                *location = self.location(*location)?;
            }
            TextEdgeRepresentation::Regularity {
                surfaces,
                locations,
                ..
            } => {
                for surface in surfaces {
                    *surface = self.get(5, *surface)?;
                }
                for location in locations {
                    *location = self.location(*location)?;
                }
            }
            TextEdgeRepresentation::Polygon3d { polygon, location } => {
                *polygon = self.get(3, *polygon)?;
                *location = self.location(*location)?;
            }
            TextEdgeRepresentation::PolygonOnTriangulation {
                polygon,
                triangulation,
                location,
            } => {
                *polygon = self.get(4, *polygon)?;
                *triangulation = self.get(6, *triangulation)?;
                *location = self.location(*location)?;
            }
            TextEdgeRepresentation::PolygonPair {
                polygons,
                triangulation,
                location,
            } => {
                for polygon in polygons {
                    *polygon = self.get(4, *polygon)?;
                }
                *triangulation = self.get(6, *triangulation)?;
                *location = self.location(*location)?;
            }
        }
        Some(())
    }
}
