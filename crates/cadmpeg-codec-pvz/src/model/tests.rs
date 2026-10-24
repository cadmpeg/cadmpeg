// SPDX-License-Identifier: Apache-2.0

use std::rc::Rc;
use std::time::{Duration, Instant};

use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::products::OccurrenceParent;
use cadmpeg_ir::CadIr;

use super::{definitions, display_geometry, occurrences, placement, Placed, MAX_OCCURRENCES};
use crate::loss::PvzLossCode;
use crate::ol::{MemberGeometry, Mesh};
use crate::pvs::{ChildOccurrence, Definition, DefinitionRole, Placement, Property, Structure};
use crate::test_support::SCALED_TURN;

fn definition(name: &str, children: &[usize]) -> Definition {
    Definition {
        name: name.into(),
        role: DefinitionRole::Component,
        geometry: None,
        occurrences: children
            .iter()
            .map(|&definition| ChildOccurrence {
                definition,
                placement: Placement::default(),
                feature_id: "".into(),
            })
            .collect(),
        properties: Vec::new(),
    }
}

fn structure(definitions: Vec<Definition>) -> Structure {
    Structure {
        definitions,
        ..Structure::default()
    }
}

fn expand(structure: &Structure) -> (CadIr, Vec<cadmpeg_ir::report::loss::LossKind>) {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut ir = CadIr::empty();
    let mut losses = Vec::new();
    definitions(&ctx, structure, &mut ir, &mut losses).unwrap();
    occurrences(&ctx, structure, &mut ir, &mut losses).unwrap();
    (ir, losses.into_iter().map(|loss| loss.code).collect())
}

fn rows(placed: &Placed) -> ([[f64; 4]; 3], f64) {
    match placed {
        Placed::Similarity(transform, scale) => (transform.affine_rows(), scale.get()),
        Placed::Affine(transform) => (transform.affine_rows(), f64::NAN),
        Placed::Invalid => panic!("invalid placement"),
    }
}

#[test]
fn a_translation_is_scaled_to_millimetres() {
    let placed = placement(&Placement {
        translation: Some([0.1, -0.2, 0.0]),
        basis: None,
    });
    let (rows, scale) = rows(&placed);
    assert_eq!(
        rows,
        [
            [1.0, 0.0, 0.0, 100.0],
            [0.0, 1.0, 0.0, -200.0],
            [0.0, 0.0, 1.0, 0.0]
        ]
    );
    assert_eq!(scale, 1.0);
}

#[test]
fn basis_vectors_are_matrix_columns_and_their_length_is_the_scale() {
    let placed = placement(&Placement {
        translation: Some([0.0, 0.2, 0.0]),
        basis: Some(SCALED_TURN),
    });
    let (rows, scale) = rows(&placed);
    assert_eq!(
        rows,
        [
            [0.0, -1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 200.0],
            [0.0, 0.0, 1.0, 0.0]
        ]
    );
    assert_eq!(scale, 2.0);
}

#[test]
fn a_unit_basis_keeps_its_stored_values() {
    let turn = [0.6, 0.8, 0.0, -0.8, 0.6, 0.0, 0.0, 0.0, 1.000_000_1];
    let (rows, scale) = rows(&placement(&Placement {
        translation: None,
        basis: Some(turn),
    }));
    assert_eq!(rows[0], [0.6, -0.8, 0.0, 0.0]);
    assert_eq!(rows[2], [0.0, 0.0, 1.000_000_1, 0.0]);
    assert_eq!(scale, 1.0);
}

#[test]
fn mirrored_skewed_or_unequal_bases_stay_affine() {
    for basis in [
        [-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 0.5, 1.0, 0.0, 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 1.0],
        [0.0; 9],
    ] {
        let placed = placement(&Placement {
            translation: None,
            basis: Some(basis),
        });
        assert!(matches!(placed, Placed::Affine(_)), "{basis:?}");
    }
    assert!(matches!(
        placement(&Placement {
            translation: Some([f64::NAN, 0.0, 0.0]),
            basis: None,
        }),
        Placed::Invalid
    ));
}

#[test]
fn shared_definitions_expand_to_one_occurrence_per_path() {
    let (ir, losses) = expand(&structure(vec![
        definition("TOP", &[1, 1, 2]),
        definition("SUB", &[2]),
        definition("LEAF", &[]),
    ]));
    let tree: Vec<_> = ir
        .model
        .occurrences
        .iter()
        .map(|occurrence| {
            let parent = match &occurrence.parent {
                OccurrenceParent::Root {} => String::new(),
                OccurrenceParent::Occurrence { occurrence } => occurrence.to_string(),
            };
            (occurrence.id.to_string(), parent, occurrence.ordinal)
        })
        .collect();
    let id = |index: usize| format!("pvz:model:occurrence#{index}");
    assert_eq!(
        tree,
        [
            (id(0), String::new(), 0),
            (id(1), id(0), 0),
            (id(2), id(0), 1),
            (id(3), id(0), 2),
            (id(4), id(1), 0),
            (id(5), id(2), 0),
        ]
    );
    assert!(losses.is_empty());
    let mut ir = ir;
    ir.model.finalize();
    let report = cadmpeg_ir::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(report.is_ok(), "{report:?}");
}

#[test]
fn cycles_and_extra_roots_are_reported() {
    let (ir, losses) = expand(&structure(vec![
        definition("A", &[1]),
        definition("B", &[2]),
        definition("C", &[1]),
        definition("D", &[]),
    ]));
    assert_eq!(ir.model.occurrences.len(), 4);
    assert_eq!(
        losses,
        [
            PvzLossCode::StructureRootAmbiguous.kind(),
            PvzLossCode::StructureOccurrenceCycle.kind(),
        ]
    );
}

/// A definition that places `count` occurrences of definition `child`.
fn repeated(name: &str, child: usize, count: usize) -> Definition {
    let mut definition = definition(name, &[]);
    definition.occurrences = vec![
        ChildOccurrence {
            definition: child,
            placement: Placement::default(),
            feature_id: "".into(),
        };
        count
    ];
    definition
}

/// Expansion work that finishes well inside this bound is linear in the
/// occurrence limit.
const EXPANSION_DEADLINE: Duration = Duration::from_secs(20);

#[test]
fn exponential_instancing_stops_at_the_occurrence_limit() {
    let fan_out = 100_000;
    let started = Instant::now();
    let (ir, losses) = expand(&structure(vec![
        repeated("A", 1, fan_out),
        repeated("B", 2, fan_out),
        definition("C", &[]),
    ]));
    assert!(started.elapsed() < EXPANSION_DEADLINE);
    assert_eq!(ir.model.occurrences.len(), MAX_OCCURRENCES);
    assert_eq!(losses, [PvzLossCode::StructureOccurrenceTruncated.kind()]);
}

#[test]
fn cycle_closing_children_count_toward_the_occurrence_limit() {
    let started = Instant::now();
    let (ir, losses) = expand(&structure(vec![
        repeated("ROOT", 1, MAX_OCCURRENCES / 2),
        repeated("LOOP", 1, MAX_OCCURRENCES),
    ]));
    assert!(started.elapsed() < EXPANSION_DEADLINE);
    assert_eq!(ir.model.occurrences.len(), MAX_OCCURRENCES / 2 + 1);
    assert_eq!(
        losses,
        [
            PvzLossCode::StructureOccurrenceCycle.kind(),
            PvzLossCode::StructureOccurrenceTruncated.kind(),
        ]
    );
}

#[test]
fn roots_count_toward_the_occurrence_limit() {
    let roots: Vec<_> = (0..=MAX_OCCURRENCES)
        .map(|_| definition("ROOT", &[]))
        .collect();
    let (ir, losses) = expand(&structure(roots));
    assert_eq!(ir.model.occurrences.len(), MAX_OCCURRENCES);
    assert_eq!(
        losses,
        [
            PvzLossCode::StructureRootAmbiguous.kind(),
            PvzLossCode::StructureOccurrenceTruncated.kind(),
        ]
    );
}

fn retained_refusal(result: &Result<(), CodecError>) -> bool {
    matches!(
        result,
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes
    )
}

#[test]
fn a_shared_name_is_charged_each_time_it_enters_the_model() {
    let name: Rc<str> = "N".repeat(60_000).into();
    let shared: Vec<_> = (0..100_000)
        .map(|_| Definition {
            name: Rc::clone(&name),
            ..definition("", &[])
        })
        .collect();
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut ir = CadIr::empty();
    let mut losses = Vec::new();
    assert!(retained_refusal(&definitions(
        &ctx,
        &structure(shared),
        &mut ir,
        &mut losses
    )));

    let mut root = repeated("ROOT", 1, 100_000);
    for child in &mut root.occurrences {
        child.feature_id = Rc::clone(&name);
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut ir = CadIr::empty();
    assert!(retained_refusal(&occurrences(
        &ctx,
        &structure(vec![root, definition("LEAF", &[])]),
        &mut ir,
        &mut losses
    )));
}

#[test]
fn properties_become_bom_properties_keyed_by_stream() {
    let mut root = definition("TOP", &[]);
    for (stream, name, value) in [
        ("", "PART_NUMBER", "TOP-001"),
        ("sbom", "PART_NUMBER", "X"),
        ("", "PART_NUMBER", "again"),
        ("", "PART_NUMBER", "TOP-001"),
        ("", " ", "blank"),
    ] {
        root.properties.push(Property {
            stream: stream.into(),
            name: name.into(),
            value: value.into(),
        });
    }
    let (ir, losses) = expand(&structure(vec![root]));
    let definition = &ir.model.product_definitions[0];
    assert_eq!(definition.source_name.as_deref(), Some("TOP"));
    let properties: Vec<_> = definition
        .bom_properties
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    assert_eq!(
        properties,
        [("PART_NUMBER", "TOP-001"), ("sbom/PART_NUMBER", "X")]
    );
    assert_eq!(losses, [PvzLossCode::StructurePropertyKeyDropped.kind()]);
}

#[test]
fn a_mesh_with_a_non_finite_vertex_is_rejected() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let geometry = MemberGeometry {
        appearances: crate::ol::AppearanceTable::default(),
        meshes: vec![
            Mesh {
                positions: vec![[0.0, 0.0, 0.0], [0.001, 0.0, 0.0], [0.0, 0.001, 0.0]],
                triangles: vec![[0, 1, 2]],
                patches: vec![(0, vec![0])],
                ..Mesh::default()
            },
            Mesh {
                positions: vec![[f32::NAN, 0.0, 0.0]],
                triangles: vec![[0, 0, 0]],
                ..Mesh::default()
            },
            Mesh {
                positions: vec![[0.0, 0.0, 0.0]],
                ..Mesh::default()
            },
        ],
        has_exact: false,
        residue: crate::ol::MemberResidue::default(),
    };
    let mut ir = CadIr::empty();
    let mut losses = Vec::new();
    let body = display_geometry(&ctx, "part.ol", &geometry, None, &mut ir, &mut losses)
        .unwrap()
        .unwrap();
    assert_eq!(body.as_str(), "pvz:model:body#part.ol");
    assert_eq!(ir.model.tessellations.len(), 1);
    let mesh = &ir.model.tessellations[0];
    assert_eq!(mesh.body.as_ref(), Some(&body));
    assert_eq!(
        mesh.vertices()[1].get(),
        cadmpeg_ir::math::Point3::new(f64::from(0.001_f32) * 1000.0, 0.0, 0.0)
    );
    assert_eq!(
        losses.into_iter().map(|loss| loss.code).collect::<Vec<_>>(),
        [
            PvzLossCode::TessellationRecordsUndecoded.kind(),
            PvzLossCode::TessellationMeshRejected.kind(),
        ]
    );

    let empty = MemberGeometry {
        meshes: Vec::new(),
        ..geometry
    };
    assert!(
        display_geometry(&ctx, "e.ol", &empty, None, &mut ir, &mut Vec::new())
            .unwrap()
            .is_none()
    );
}

#[test]
fn placement_defects_and_deep_chains_are_reported() {
    let mirrored = ChildOccurrence {
        definition: 1,
        placement: Placement {
            translation: None,
            basis: Some([-1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]),
        },
        feature_id: "".into(),
    };
    let unbounded = ChildOccurrence {
        definition: 1,
        placement: Placement {
            translation: Some([f64::INFINITY, 0.0, 0.0]),
            basis: None,
        },
        feature_id: "".into(),
    };
    let mut root = definition("ROOT", &[]);
    root.occurrences = vec![mirrored, unbounded];
    let (ir, losses) = expand(&structure(vec![root, definition("LEAF", &[])]));
    assert_eq!(ir.model.occurrences.len(), 3);
    assert_eq!(
        losses,
        [
            PvzLossCode::StructurePlacementNotSimilarity.kind(),
            PvzLossCode::StructurePlacementInvalid.kind(),
        ]
    );

    let depth = usize::try_from(
        cadmpeg_test_support::service_decode_context()
            .policy()
            .limits
            .max_recursion_depth,
    )
    .unwrap();
    let chain: Vec<_> = (0..depth + 2)
        .map(|index| {
            let children = if index + 1 < depth + 2 {
                vec![index + 1]
            } else {
                Vec::new()
            };
            definition("LINK", &children)
        })
        .collect();
    let (ir, losses) = expand(&structure(chain));
    assert_eq!(ir.model.occurrences.len(), depth);
    assert_eq!(losses, [PvzLossCode::StructureOccurrenceTruncated.kind()]);
}

#[test]
fn an_appearance_colour_outside_the_unit_range_is_rejected() {
    use crate::layout::token::{STREAM_ATTRIBUTES, STREAM_SURFACE_TESSELLATION};
    use crate::test_support::ol::{
        appearance_reference, appearance_values, member, patch_header, primitive, stream_index,
        vertex_block,
    };
    let mut attributes = stream_index(STREAM_ATTRIBUTES);
    attributes.extend(appearance_values([
        2.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.5, 0.5, 0.5, 32.0, 0.0, 0.0, 0.0, 1.0,
    ]));
    attributes.extend(appearance_reference("hot", 1));
    let mut surface = stream_index(STREAM_SURFACE_TESSELLATION);
    surface.extend(patch_header(0, 0));
    surface.extend(vertex_block(
        &[[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        None,
    ));
    surface.extend(primitive(0x1b, &[0, 1, 2]));
    let bytes = member(
        "DIV-OL09BL",
        false,
        &[
            (STREAM_ATTRIBUTES, attributes),
            (STREAM_SURFACE_TESSELLATION, surface),
        ],
    );
    let ctx = cadmpeg_test_support::service_decode_context();
    let geometry =
        crate::ol::decode(&ctx, cadmpeg_core::decode::View::over_retained(&bytes)).unwrap();
    let mut ir = CadIr::empty();
    let mut losses = Vec::new();
    display_geometry(&ctx, "hot.ol", &geometry, None, &mut ir, &mut losses).unwrap();
    assert!(ir.model.appearances[0].base_color.is_none());
    assert_eq!(ir.model.appearance_bindings.len(), 1);
    assert_eq!(
        losses.into_iter().map(|loss| loss.code).collect::<Vec<_>>(),
        [PvzLossCode::AttributeAppearanceColorRejected.kind()]
    );
}
