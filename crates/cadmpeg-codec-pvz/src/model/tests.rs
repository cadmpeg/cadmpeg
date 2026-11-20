// SPDX-License-Identifier: Apache-2.0

use std::rc::Rc;
use std::time::{Duration, Instant};

use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use super::{definitions, display_geometry, occurrences, placement, Placed};
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

/// Occurrence limit of the tests that reach it.
const LIMIT: usize = 8;

fn expand_within(structure: &Structure, max_occurrences: usize) -> (CadIr, Vec<LossNote>) {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut ir = CadIr::empty();
    let mut losses = Vec::new();
    definitions(&ctx, structure, &mut ir, &mut losses).unwrap();
    occurrences(&ctx, structure, max_occurrences, &mut ir, &mut losses).unwrap();
    (ir, losses)
}

fn codes(losses: &[LossNote]) -> Vec<cadmpeg_ir::report::loss::LossKind> {
    losses.iter().map(|loss| loss.code.clone()).collect()
}

/// Expands `structure` with no occurrence limit.
fn expand(structure: &Structure) -> (CadIr, Vec<cadmpeg_ir::report::loss::LossKind>) {
    let (ir, losses) = expand_within(structure, usize::MAX);
    (ir, codes(&losses))
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
fn a_shared_definition_lists_its_occurrences_once() {
    let (ir, losses) = expand(&structure(vec![
        definition("TOP", &[1, 1, 2]),
        definition("SUB", &[2]),
        definition("LEAF", &[]),
    ]));
    let owned: Vec<_> = ir
        .model
        .product_definitions
        .iter()
        .map(|definition| {
            (
                definition.id.to_string(),
                definition.is_root(),
                definition
                    .occurrences
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    let id = |index: usize| format!("pvz:model:occurrence#{index}");
    let definition = |index: usize| format!("pvz:model:definition#{index}");
    assert_eq!(
        owned,
        [
            (definition(0), true, vec![id(0), id(2), id(3)]),
            (definition(1), false, vec![id(1)]),
            (definition(2), false, vec![]),
        ]
    );
    assert_eq!(ir.model.occurrences.len(), 4);
    assert!(losses.is_empty());
    let mut ir = ir;
    ir.finalize();
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

#[test]
fn exponential_instancing_stops_at_the_occurrence_limit() {
    let (ir, losses) = expand_within(
        &structure(vec![
            repeated("A", 1, LIMIT),
            repeated("B", 2, LIMIT),
            definition("C", &[]),
        ]),
        LIMIT,
    );
    assert_eq!(ir.model.occurrences.len(), LIMIT);
    assert_eq!(
        codes(&losses),
        [PvzLossCode::StructureOccurrenceTruncated.kind()]
    );
}

/// Each definition's children are walked once (§8.1), so the walk is linear
/// in the stored children, and cycle-closing children are counted and left
/// out without cutting off the other children.
#[test]
fn cycle_closing_children_are_left_out_without_truncating_the_rest() {
    let (ir, losses) = expand_within(
        &structure(vec![
            repeated("ROOT", 1, LIMIT / 2),
            repeated("LOOP", 1, LIMIT),
        ]),
        LIMIT,
    );
    assert_eq!(ir.model.occurrences.len(), LIMIT / 2);
    assert_eq!(
        codes(&losses),
        [PvzLossCode::StructureOccurrenceCycle.kind()]
    );
    assert_eq!(
        losses[0].message,
        format!("{LIMIT} child occurrences close a definition cycle and were left out")
    );
}

/// Bound on the expansion time of [`MANY_ROOTS`] root definitions.
const EXPANSION_DEADLINE: Duration = Duration::from_secs(20);
/// Root count at which attaching occurrences in quadratic time runs past
/// [`EXPANSION_DEADLINE`].
const MANY_ROOTS: usize = 100_001;

#[test]
fn roots_count_toward_the_occurrence_limit() {
    let started = Instant::now();
    let roots: Vec<_> = (0..MANY_ROOTS).map(|_| definition("ROOT", &[])).collect();
    let (ir, losses) = expand_within(&structure(roots), LIMIT);
    assert!(started.elapsed() < EXPANSION_DEADLINE);
    assert_eq!(ir.model.occurrences.len(), LIMIT);
    assert_eq!(
        codes(&losses),
        [
            PvzLossCode::StructureRootAmbiguous.kind(),
            PvzLossCode::StructureOccurrenceTruncated.kind(),
        ]
    );
}

#[test]
fn the_document_root_over_several_leaf_tops_is_charged_once() {
    let tops = structure(vec![definition("A", &[]), definition("B", &[])]);
    // Two definitions, the two occurrences placing them, and the document.
    let expand_within = |max_entities| {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_entities = max_entities;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root fits the service input limit");
        let mut ir = CadIr::empty();
        let mut losses = Vec::new();
        definitions(&ctx, &tops, &mut ir, &mut losses)?;
        occurrences(&ctx, &tops, LIMIT, &mut ir, &mut losses)?;
        Ok::<_, CodecError>(ir)
    };
    let error = expand_within(4).expect_err("five entities exceed four");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::Entities
            && limit.operation == "pvz_document"));
    let ir = expand_within(5).expect("five entities fit five");
    assert_eq!(ir.entity_count(), 5);
    let roots: Vec<_> = ir
        .model
        .product_definitions
        .iter()
        .filter(|definition| definition.is_root())
        .map(|definition| definition.id.to_string())
        .collect();
    assert_eq!(roots, ["pvz:model:document#0"]);
}

fn retained_refusal(result: &Result<(), CodecError>) -> bool {
    matches!(
        result,
        Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes
    )
}

#[test]
fn a_shared_name_is_charged_each_time_it_enters_the_model() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    // One charge of the name fits the retained allowance, and two do not.
    let length = usize::try_from(ctx.retained_bytes_available() / 2 + 1).unwrap();
    let name: Rc<str> = "N".repeat(length).into();
    let shared: Vec<_> = (0..2)
        .map(|_| Definition {
            name: Rc::clone(&name),
            ..definition("", &[])
        })
        .collect();
    let mut ir = CadIr::empty();
    let mut losses = Vec::new();
    assert!(retained_refusal(&definitions(
        &ctx,
        &structure(shared),
        &mut ir,
        &mut losses
    )));

    let mut root = repeated("ROOT", 1, 2);
    for child in &mut root.occurrences {
        child.feature_id = Rc::clone(&name);
    }
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut ir = CadIr::empty();
    assert!(retained_refusal(&occurrences(
        &ctx,
        &structure(vec![root, definition("LEAF", &[])]),
        LIMIT,
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
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
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
    // §8.1: the root definition owns its two children; it has no occurrence.
    assert_eq!(ir.model.occurrences.len(), 2);
    assert_eq!(
        losses,
        [
            PvzLossCode::StructurePlacementNotSimilarity.kind(),
            PvzLossCode::StructurePlacementInvalid.kind(),
        ]
    );

    let depth = usize::try_from(
        cadmpeg_core::decode::test_support::service_decode_context()
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
    // Each definition above the depth limit places the next; the root
    // definition, at the top, is placed by no occurrence (§8.1).
    assert_eq!(ir.model.occurrences.len(), depth - 1);
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
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
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
