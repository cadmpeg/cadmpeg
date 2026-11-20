// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::feature::PmDcFeatureReferenceFamily as Family;
use crate::pmdc::{PmDcListMetadata, PmDcU32List};
use crate::test_support::sheet_metal::{list, property};

fn edge_item(sources: &[u32]) -> Kind {
    Kind::EdgeItem {
        index_references: PmDcU32List::new(
            2,
            (!sources.is_empty()).then_some(PmDcListMetadata::U32([1, 0])),
            sources.to_vec(),
        )
        .expect("indexes"),
        index_reference_value: -1,
        value: 0,
    }
}

fn attached() -> ContourFixture {
    let mut fixture = ContourFixture::new();
    fixture.plate = feature(
        100,
        &[1, 2, 3, 50, 5, 0, 0, 6, 7, 8, 9, 10, 121, 12, 0, 13, 14],
    );
    fixture.properties.extend([
        property(
            121,
            &Kind::References {
                family: Family::SheetMetalAttachmentSelections,
                items: list(&[122, 125]),
            },
        ),
        property(
            122,
            &Kind::SheetMetal {
                definition: SheetMetalProperty::EdgeSelection {
                    edges: reference(123),
                },
            },
        ),
        property(
            123,
            &Kind::References {
                family: Family::EdgeCollection,
                items: list(&[124]),
            },
        ),
        property(124, &edge_item(&[903, 901])),
        property(
            125,
            &Kind::SheetMetal {
                definition: SheetMetalProperty::EdgeSelection {
                    edges: reference(126),
                },
            },
        ),
        property(
            126,
            &Kind::References {
                family: Family::EdgeCollection,
                items: list(&[127, 128]),
            },
        ),
        property(127, &edge_item(&[902])),
        property(128, &edge_item(&[904])),
    ]);
    fixture
}

fn resolved(item: PmDcReference, source: u32) -> Option<i64> {
    match (item.index(), source) {
        (124, 903) => Some(43),
        (124, 901) => Some(41),
        (127, 902) => Some(42),
        (128, 904) => Some(44),
        _ => None,
    }
}

fn read(fixture: &ContourFixture) -> Option<AttachmentEdges> {
    attachment_edges(&fixture.plate, |r| fixture.property(r), resolved)
}

fn admission(fixture: &ContourFixture, attachment: Option<&AttachmentEdges>) -> ContourRefusal {
    let mut children = fixture.children();
    children.attachment_edges = attachment;
    operands(
        children,
        Some(&fixture.style),
        |r| fixture.property(r),
        ContourFixture::length,
        ContourFixture::direction,
        ContourFixture::stored,
        UnitVector3::Z_AXIS,
    )
    .expect_err("attached contour remains native")
}

#[test]
fn attachment_edges_keep_wrapper_item_and_selector_order() {
    let fixture = attached();
    let attachment = read(&fixture).expect("decoded attachment");
    assert_eq!(attachment.selection, reference(121));
    assert_eq!(
        attachment.edges,
        [
            AttachmentEdge {
                item: reference(124),
                selector_source: 903,
                entity_index: 43,
                revision: None
            },
            AttachmentEdge {
                item: reference(124),
                selector_source: 901,
                entity_index: 41,
                revision: None
            },
            AttachmentEdge {
                item: reference(127),
                selector_source: 902,
                entity_index: 42,
                revision: None
            },
            AttachmentEdge {
                item: reference(128),
                selector_source: 904,
                entity_index: 44,
                revision: None
            },
        ]
    );
    let wire = serde_json::to_value(&attachment).expect("native wire");
    assert_eq!(
        serde_json::from_value::<AttachmentEdges>(wire).expect("native read"),
        attachment
    );
    assert_eq!(
        admission(&fixture, Some(&attachment)),
        ContourRefusal::EdgeAttached(4)
    );
    assert_eq!(admission(&fixture, None), ContourRefusal::ReservedSlot);
}

#[test]
fn undecoded_attachment_keeps_the_reserved_slot_refusal() {
    for index in [121, 122, 123, 124] {
        let mut fixture = attached();
        fixture.edit(index, |kind| *kind = boolean(false));
        assert_eq!(read(&fixture), None);
        assert_eq!(admission(&fixture, None), ContourRefusal::ReservedSlot);
    }
    let fixture = attached();
    for missing in [901, 902, 903, 904] {
        assert_eq!(
            attachment_edges(
                &fixture.plate,
                |r| fixture.property(r),
                |item, source| {
                    (source != missing)
                        .then(|| resolved(item, source))
                        .flatten()
                }
            ),
            None
        );
    }
}

#[test]
fn duplicate_empty_and_unresolved_attachment_edges_stay_reserved() {
    for (index, kind) in [
        (
            121,
            Kind::References {
                family: Family::SheetMetalAttachmentSelections,
                items: list(&[]),
            },
        ),
        (
            123,
            Kind::References {
                family: Family::EdgeCollection,
                items: list(&[]),
            },
        ),
        (124, edge_item(&[])),
        (124, edge_item(&[903, 903])),
        (127, edge_item(&[901])),
    ] {
        let mut fixture = attached();
        fixture.edit(index, |value| *value = kind);
        assert_eq!(read(&fixture), None);
    }
    let fixture = attached();
    for owner in [-1, 43] {
        assert_eq!(
            attachment_edges(&fixture.plate, |r| fixture.property(r), |_, _| Some(owner)),
            None
        );
    }
    let mut attachment = read(&fixture).expect("attachment");
    attachment.selection = reference(999);
    assert_eq!(
        admission(&fixture, Some(&attachment)),
        ContourRefusal::ReservedSlot
    );
    attachment.selection = reference(121);
    attachment.edges.clear();
    assert_eq!(
        admission(&fixture, Some(&attachment)),
        ContourRefusal::ReservedSlot
    );
}

#[test]
fn decoded_attachment_preserves_other_reserved_slot_checks() {
    for slot in [5, 6, 14] {
        let mut fixture = attached();
        let attachment = read(&fixture).expect("attachment");
        let mut slots: Vec<_> = fixture
            .plate
            .properties
            .references()
            .iter()
            .map(|r| r.index())
            .collect();
        slots[slot] = 121;
        fixture.plate = feature(100, &slots);
        assert_eq!(
            admission(&fixture, Some(&attachment)),
            ContourRefusal::ReservedSlot
        );
    }
}

#[test]
fn edge_attached_loss_names_the_decoded_edge_count() {
    let mut refusals = Refusals::default();
    refusals.insert("plate".to_owned(), ContourRefusal::EdgeAttached(4));
    let losses = refusals.losses(&[native("attached", "plate")]);
    assert_eq!(losses.len(), 1);
    let text = serde_json::to_string(&losses).expect("loss");
    assert!(text.contains("contour-flange check is edge-attached"));
    assert!(text.contains("decoded attachment selects 4 input edge(s)"));
}

#[test]
fn native_contour_retains_unique_item_matched_brep_revisions() {
    use crate::feature::edge_selection::{geometry::EdgeGeometry, historical::SelectedEdge};
    use cadmpeg_ir::features::FinitePoint3;

    context_run(|ctx| {
        for invalid in [None, Some("item"), Some("owners")] {
            let fixture = attached();
            let expected = read(&fixture).expect("attachment");
            let mut inventory = FeatureInventory {
                features: vec![fixture.plate],
                properties: fixture.properties,
                sheet_metal_flanges: vec![flange(
                    crate::feature::sheet_metal::flange::CONTOUR_TYPE,
                )],
                ..Default::default()
            };
            for edge in &expected.edges {
                let point = |x| FinitePoint3::new(Point3::new(x, 0.0, 0.0)).expect("point");
                inventory.edge_inputs.historical.insert(
                    ("fixture".to_owned(), edge.selector_source),
                    SelectedEdge {
                        owner: edge.entity_index,
                        owners: if invalid == Some("owners") { 2 } else { 1 },
                        generators: 1,
                        item: if invalid == Some("item") {
                            0
                        } else {
                            edge.item.record_ordinal().expect("item")
                        },
                        geometry: EdgeGeometry {
                            start: point(0.0),
                            end: point(1.0),
                            interior: point(0.5),
                            tangent: UnitVector3::X_AXIS,
                        },
                        faces: None,
                    },
                );
            }
            let mut resolved = std::collections::HashMap::new();
            if let Some(decoded) = attachment_edges(
                &inventory.features[0],
                |reference| {
                    inventory.properties.iter().find(|property| {
                        property.identity.record_ordinal
                            == reference.record_ordinal().expect("property")
                    })
                },
                |item, source| {
                    attachment::selected_owner(
                        item,
                        inventory
                            .edge_inputs
                            .historical
                            .get(&("fixture".to_owned(), source))?,
                    )
                },
            ) {
                resolved.insert(("fixture".to_owned(), 100), decoded);
            }
            retain_attachments(ctx, &mut inventory, &resolved).expect("native edges");
            let retained = inventory.sheet_metal_flanges[0].attachment_edges.as_ref();
            if invalid.is_none() {
                assert_eq!(retained, Some(&expected));
                let wire = serde_json::to_value(&inventory.sheet_metal_flanges[0])
                    .expect("native contour");
                let decoded: Flange = serde_json::from_value(wire).expect("native contour read");
                assert_eq!(decoded.attachment_edges.as_ref(), Some(&expected));
            } else {
                assert_eq!(retained, None);
            }
        }
    });
}
