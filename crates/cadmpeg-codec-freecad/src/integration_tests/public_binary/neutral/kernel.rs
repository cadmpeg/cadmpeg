// SPDX-License-Identifier: Apache-2.0
//! Expected kernel projections from independently decoded native source facts.
//!
//! This edits only the BEFORE document. Neither an AFTER document nor the
//! production kernel projection routines supply expected geometry.

use crate::native::entry_contents::EntryContents;
use crate::native::{EntryRecord, GuiPropertyRecord, PropertyRecord, ValueRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::tessellation::TessellationMesh;
use cadmpeg_ir::CadIr;
use cadmpeg_test_support::framing::freecad::binary::BinaryKind;

pub(super) fn update(
    ir: &mut CadIr,
    entry: &EntryRecord,
    kind: BinaryKind,
) -> Result<(), CodecError> {
    if !matches!(kind, BinaryKind::Mesh | BinaryKind::Points) {
        return Ok(());
    }
    let namespace = ir.native.namespace("fcstd").unwrap();
    let records = namespace.arena_as::<EntryRecord>("entries").unwrap();
    let current = records
        .iter()
        .find(|current| current.id() == entry.id())
        .expect("actual independently updated entry");
    let properties = namespace.arena_as::<PropertyRecord>("properties").unwrap();
    let gui_properties = namespace
        .arena_as::<GuiPropertyRecord>("gui_properties")
        .unwrap();
    for contents in current.contents() {
        match contents {
            EntryContents::Mesh {
                vertices, facets, ..
            } => {
                assert!(matches!(kind, BinaryKind::Mesh));
                assert!(vertices.iter().all(Option::is_some));
                assert!(facets.iter().all(Option::is_some));
                // Facets address original source slots. Compact only readable
                // points, retaining a separate original-slot correspondence.
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
                    .map(|facet| {
                        facet.points.map(|index| {
                            indices
                                .get(usize::try_from(index).unwrap())
                                .copied()
                                .flatten()
                                .expect("readable facet references a readable source vertex")
                        })
                    })
                    .collect::<Vec<_>>();
                for mesh in ir
                    .model
                    .tessellations
                    .iter_mut()
                    .filter(|mesh| mesh.native_ref.as_deref() == Some(entry.id()))
                {
                    *mesh = mesh
                        .clone()
                        .with_mesh(
                            TessellationMesh::List {
                                vertices: positions.clone(),
                                triangles: triangles.clone(),
                            },
                            mesh.channels().to_vec(),
                        )
                        .unwrap();
                }
                // Bounds and neighbour indices remain native kernel fields;
                // the neutral mesh has neither a stored bounds field nor a
                // second neighbour table to update.
            }
            EntryContents::Points { points } => {
                assert!(matches!(kind, BinaryKind::Points));
                assert!(points.iter().all(Option::is_some));
                let owners = properties
                    .iter()
                    .filter(|property| {
                        property.type_name == "Points::PropertyPointKernel"
                            && property
                                .side_entries()
                                .iter()
                                .any(|name| name == entry.name())
                    })
                    .map(PropertyRecord::values)
                    .chain(
                        gui_properties
                            .iter()
                            .filter(|property| {
                                property.type_name == "Points::PropertyPointKernel"
                                    && property
                                        .side_entries
                                        .iter()
                                        .any(|name| name == entry.name())
                            })
                            .map(|property| property.values.as_ref()),
                    )
                    .filter(|values| {
                        values.iter().any(|value| {
                            value.parent.is_none()
                                && value.tag == "Points"
                                && value
                                    .attributes
                                    .get("file")
                                    .is_some_and(|name| name == entry.name())
                        })
                    })
                    .collect::<Vec<&[ValueRecord]>>();
                assert_eq!(
                    owners.len(),
                    1,
                    "public source has one actual point-kernel transform carrier"
                );
                let values = owners[0];
                let matrix = point_matrix(values);
                let linked = ir
                    .model
                    .points
                    .iter()
                    .filter(|point| point.native_ref.as_deref() == Some(entry.id()))
                    .count();
                if linked == 0 {
                    continue;
                }
                assert_eq!(
                    linked,
                    points.len(),
                    "public baseline has one point per original source slot"
                );
                let mut slot = 0;
                ir.model.points.retain_mut(|point| {
                    if point.native_ref.as_deref() != Some(entry.id()) {
                        return true;
                    }
                    let position = points[slot].map(|point| {
                        place(matrix, point)
                            .expect("readable source point has a finite transformed position")
                    });
                    slot += 1;
                    if let Some(position) = position {
                        point.set_position(position);
                        true
                    } else {
                        false
                    }
                });
            }
            _ => {}
        }
    }
    Ok(())
}

fn point_matrix(values: &[ValueRecord]) -> [[f64; 4]; 4] {
    let Some(text) = values
        .iter()
        .find(|value| value.parent.is_none() && value.tag == "Points")
        .and_then(|value| value.attributes.get("mtrx"))
    else {
        return std::array::from_fn(|row| {
            std::array::from_fn(|column| if row == column { 1.0 } else { 0.0 })
        });
    };
    let scalars = text
        .split_whitespace()
        .map(|value| value.parse::<f64>().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(scalars.len(), 16);
    assert!(scalars.iter().all(|value| value.is_finite()));
    std::array::from_fn(|row| std::array::from_fn(|column| scalars[row * 4 + column]))
}

fn place(matrix: [[f64; 4]; 4], point: FinitePoint3) -> Option<FinitePoint3> {
    let point = point.get();
    let coordinates: [f64; 3] = std::array::from_fn(|row| {
        matrix[row][0] * point.x
            + matrix[row][1] * point.y
            + matrix[row][2] * point.z
            + matrix[row][3]
    });
    FinitePoint3::new(Point3::new(coordinates[0], coordinates[1], coordinates[2]))
}
