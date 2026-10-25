// SPDX-License-Identifier: Apache-2.0
//! Compare topology facts through actual typed source-slot dependencies.

use crate::brep::{ShapePayloadRecord, Tables, TextShapeUse, TextTShape, TextTShapeGeometry};
use cadmpeg_ir::scalar::FiniteReal;

mod representations;

#[derive(Debug, PartialEq)]
struct Shape {
    geometry: Geometry,
    flags: [bool; 7],
    children: Vec<TextShapeUse>,
}

#[derive(Debug, PartialEq)]
enum Geometry {
    Ordinary(TextTShapeGeometry),
    Face {
        natural_restriction: bool,
        tolerance: FiniteReal,
        surface: Option<usize>,
        location: usize,
        triangulation: Option<usize>,
    },
}

/// The current table supplies only the admitted source slots. No current
/// scalar or variant value supplies an expected fact.
pub(super) fn assert_shape(
    previous: &ShapePayloadRecord,
    current: &ShapePayloadRecord,
    source_index: usize,
    row_id: &str,
) {
    let original = Slots::new(previous);
    let surviving = Slots::new(current);
    let original_index = original
        .position(7, source_index)
        .expect("original topology source slot exists");
    let expected = project(
        &original,
        &surviving,
        original.tables.tshapes.resolve(original_index).unwrap(),
        true,
    );
    let actual_index = surviving.position(7, source_index);
    let Some(expected) = expected else {
        assert!(
            actual_index.is_none(),
            "topology row {row_id} with a missing required source target must be absent"
        );
        return;
    };
    let actual_index = actual_index.unwrap_or_else(|| {
        panic!("readable topology source slot {source_index} ({row_id}) survives")
    });
    assert_eq!(
        current.row_identity(7, actual_index),
        Some(current.id.as_str()),
        "topology source slot retains its actual canonical shape payload home"
    );
    let actual = project(
        &surviving,
        &surviving,
        surviving.tables.tshapes.resolve(actual_index).unwrap(),
        false,
    )
    .expect("a surviving topology row has all its required targets");
    assert_eq!(expected, actual, "accepted topology source facts {row_id}");
}

pub(super) fn assert_location(
    previous: &ShapePayloadRecord,
    current: &ShapePayloadRecord,
    source_index: usize,
    row_id: &str,
) {
    let original = Slots::new(previous);
    let surviving = Slots::new(current);
    let original_index = original.position(0, source_index).unwrap();
    let mut expected = original.tables.locations[original_index - 1].clone();
    let all_targets = expected.factors.iter_mut().all(|factor| {
        let Some(source) = original.reference(&surviving, 0, factor.location) else {
            return false;
        };
        factor.location = source;
        true
    });
    let actual_index = surviving.position(0, source_index);
    if !all_targets {
        assert!(
            actual_index.is_none(),
            "location {row_id} loses its missing required factor"
        );
        return;
    }
    let actual_index = actual_index.expect("readable location source slot survives");
    assert_eq!(
        current.row_identity(0, actual_index),
        Some(current.id.as_str()),
        "location source slot retains its actual canonical shape payload home"
    );
    let mut actual = surviving.tables.locations[actual_index - 1].clone();
    for factor in &mut actual.factors {
        factor.location = surviving
            .reference(&surviving, 0, factor.location)
            .expect("surviving location factor has a current source target");
    }
    assert_eq!(expected, actual, "accepted location source facts {row_id}");
}

struct Slots<'a> {
    payload: &'a ShapePayloadRecord,
    tables: Tables<'a>,
    counts: [usize; 8],
}

impl<'a> Slots<'a> {
    fn new(payload: &'a ShapePayloadRecord) -> Self {
        let tables = Tables::from_payload(payload).expect("canonical shape tables exist");
        let counts = [
            tables.locations.len(),
            tables.curve2ds.len(),
            tables.curves.len(),
            tables.polygons3d.len(),
            tables.polygons_on_triangulations.len(),
            tables.surfaces.len(),
            tables.triangulations.len(),
            tables.tshapes.len(),
        ];
        Self {
            payload,
            tables,
            counts,
        }
    }
    fn position(&self, table: u8, source: usize) -> Option<usize> {
        (1..=self.counts[usize::from(table)])
            .find(|index| self.payload.source_index(table, *index) == source)
    }
    fn reference(&self, admitted: &Self, table: u8, index: usize) -> Option<usize> {
        assert!(
            (1..=self.counts[usize::from(table)]).contains(&index),
            "typed table {table} reference {index} exceeds its actual owned table"
        );
        let source = self.payload.source_index(table, index);
        admitted.position(table, source).map(|_| source)
    }
    fn location(&self, admitted: &Self, index: usize) -> Option<usize> {
        if index == 0 {
            Some(0)
        } else {
            self.reference(admitted, 0, index)
        }
    }
}

fn project(
    original: &Slots<'_>,
    admitted: &Slots<'_>,
    shape: &TextTShape,
    prune: bool,
) -> Option<Shape> {
    let geometry = match &shape.geometry {
        TextTShapeGeometry::Face {
            natural_restriction,
            tolerance,
            surface,
            location,
            triangulation,
        } => Geometry::Face {
            natural_restriction: *natural_restriction,
            tolerance: *tolerance,
            surface: surface
                .map(|value| original.reference(admitted, 5, value.index()))
                .transpose_option()?,
            location: original.location(admitted, usize::from(*location))?,
            triangulation: triangulation
                .map(|value| original.reference(admitted, 6, value.index()))
                .transpose_option()?,
        },
        value => {
            let mut value = value.clone();
            match &mut value {
                TextTShapeGeometry::Vertex {
                    representations, ..
                } => {
                    representations.retain_mut(|value| {
                        let keep = representations::point(original, admitted, value).is_some();
                        assert!(
                            prune || keep,
                            "surviving point representation has no source target"
                        );
                        keep
                    });
                }
                TextTShapeGeometry::Edge {
                    representations, ..
                } => {
                    representations.retain_mut(|value| {
                        let keep = representations::edge(original, admitted, value).is_some();
                        assert!(
                            prune || keep,
                            "surviving edge representation has no source target"
                        );
                        keep
                    });
                }
                _ => {}
            }
            Geometry::Ordinary(value)
        }
    };
    let children = shape
        .children
        .iter()
        .filter_map(|value| {
            let value = original
                .reference(admitted, 7, value.shape)
                .zip(original.location(admitted, usize::from(value.location)))
                .map(|(shape, location)| TextShapeUse {
                    shape,
                    location: location.into(),
                    orientation: value.orientation,
                });
            assert!(
                prune || value.is_some(),
                "surviving child use has no source target"
            );
            value
        })
        .collect();
    Some(Shape {
        geometry,
        flags: shape.flags,
        children,
    })
}

trait TransposeOption<T> {
    fn transpose_option(self) -> Option<Option<T>>;
}
impl<T> TransposeOption<T> for Option<Option<T>> {
    fn transpose_option(self) -> Option<Option<T>> {
        match self {
            Some(value) => value.map(Some),
            None => Some(None),
        }
    }
}

#[cfg(test)]
mod tests;
