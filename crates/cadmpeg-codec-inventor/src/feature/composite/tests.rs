// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::ids::BodyId;

use super::{definition, operands, project, resolve, Joins, MemberLink, Operands, CLASS_ID};
use crate::feature::body_selection::tests::{body, carrier, metadata, root};
use crate::feature::body_selection::Scope;
use crate::feature::tests::{test_feature, test_label, test_property};
use crate::feature::{
    FeatureInventory, PmDcFeature, PmDcFeatureLabel, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcKernelEntityLink, PmDcKernelEntityLinkPayload, BODY_SET_TYPE, BOOLEAN_TYPE,
    KERNEL_ENTITY_LINK_TYPE,
};
use crate::native::ActiveCarrierRecord;
use crate::pmdc::{PmDcListMetadata, PmDcReference, PmDcReferenceList};
use crate::record_identity::{Located, RecordTypeId};

/// The record ordinal of the stored kernel carrier that the source links select.
pub(crate) const STORED_CARRIER: u32 = 20;

/// Two paired source and result selections of one composite in design segment
/// `dc`, with the current carrier in segment `seg`.
pub(crate) struct Fixture {
    pub(crate) feature: PmDcFeature,
    pub(crate) label: PmDcFeatureLabel,
    pub(crate) properties: Vec<PmDcFeatureProperty>,
    pub(crate) links: Vec<PmDcKernelEntityLink>,
    pub(crate) members: Vec<MemberLink>,
    pub(crate) keys: Vec<BodyNativeKey>,
    pub(crate) carrier: ActiveCarrierRecord,
    pub(crate) metadata: Vec<crate::native::SegmentMetaRecord>,
    pub(crate) roots: Vec<crate::design::scope::PmDcModelRoot>,
    pub(crate) bodies: Vec<BodyId>,
}

fn list(references: Vec<PmDcReference>) -> PmDcReferenceList {
    PmDcReferenceList::new(
        2,
        Some(PmDcListMetadata::U32([
            u32::try_from(references.len()).expect("fixture length"),
            0,
        ])),
        references,
    )
    .expect("fixture list")
}

fn typed(ordinal: u32, kind: PmDcFeaturePropertyKind, type_id: [u8; 16]) -> PmDcFeatureProperty {
    let mut property = test_property(ordinal, kind);
    property.identity.type_id = RecordTypeId::from_bytes(type_id);
    property.identity.segment_token = cadmpeg_ir::identity_key!("dc");
    property
}

fn link(
    segment: cadmpeg_ir::ids::IdentityKey,
    ordinal: u32,
    header_id: u16,
    selection_source: u32,
    body: u32,
    entity_id: u32,
) -> PmDcKernelEntityLink {
    Located::new(
        PmDcKernelEntityLinkPayload {
            save_version_major: 16,
            header_value: 0,
            header_id,
            selection_source,
            state: 3,
            body: PmDcReference::from_packed(body),
            entity_id,
            entity_type: 38,
        },
        RecordTypeId::from_bytes(KERNEL_ENTITY_LINK_TYPE),
        segment,
        ordinal,
    )
}

fn with_references(feature: &PmDcFeature, references: Vec<PmDcReference>) -> PmDcFeature {
    let mut payload = (**feature).clone();
    payload.properties = list(references);
    Located::new(
        payload,
        feature.identity.type_id.clone(),
        feature.identity.segment_token.clone(),
        feature.identity.record_ordinal,
    )
}

pub(crate) fn fixture() -> Fixture {
    let mut feature = test_feature(0, 3, &[(0, 1), (1, 2), (2, 3)]);
    feature.identity.segment_token = cadmpeg_ir::identity_key!("dc");
    let mut label = test_label(0, 1, CLASS_ID, &[]);
    label.identity.segment_token = cadmpeg_ir::identity_key!("dc");
    let stored = 0x8000_0000 | (STORED_CARRIER + 1);
    let links = vec![
        link(cadmpeg_ir::identity_key!("dc"), 10, 1, 100, stored, 3),
        link(cadmpeg_ir::identity_key!("dc"), 11, 2, 101, stored, 4),
        link(cadmpeg_ir::identity_key!("seg"), 30, 5, 100, 0x8000_0001, 7),
        link(cadmpeg_ir::identity_key!("seg"), 31, 6, 101, 0x8000_0001, 8),
    ];
    let references = [
        PmDcReference::new(1, false).expect("external reference"),
        PmDcReference::new(2, false).expect("external reference"),
    ];
    let properties = vec![
        typed(
            1,
            PmDcFeaturePropertyKind::BodySet {
                value: 0,
                items: list(vec![
                    PmDcReference::from_packed(0x8000_000b),
                    PmDcReference::from_packed(0x8000_000c),
                ]),
            },
            BODY_SET_TYPE,
        ),
        typed(
            2,
            PmDcFeaturePropertyKind::BodySet {
                value: 0,
                items: list(references.to_vec()),
            },
            BODY_SET_TYPE,
        ),
        typed(
            3,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: true,
            },
            BOOLEAN_TYPE,
        ),
    ];
    let members = [0_u32, 1]
        .into_iter()
        .zip(references)
        .zip(&links[2..])
        .map(|((index, reference), link)| MemberLink {
            id: format!("test:composite-member#{index}"),
            set: properties[1].id(),
            index,
            link: link.id(),
            reference,
        })
        .collect();
    let keys = [(7, "target"), (8, "other")]
        .into_iter()
        .zip(0_u32..)
        .map(|((asm_body_key, name), record_index)| BodyNativeKey {
            source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                cadmpeg_asm::asm_format!("inventor"),
            ),
            record_index,
            body: body(name),
            body_ordinal: 0,
            source_brep: Some("Bseg:record:0".into()),
            asm_body_key: Some(asm_body_key),
        })
        .collect();
    Fixture {
        feature,
        label,
        properties,
        links,
        members,
        keys,
        carrier: carrier(),
        metadata: vec![metadata()],
        roots: vec![root()],
        bodies: vec![body("target"), body("other")],
    }
}

impl Fixture {
    pub(crate) fn operands(&self) -> Result<Operands<'_>, &'static str> {
        operands(&self.feature, |ordinal| {
            self.properties
                .iter()
                .find(|property| property.identity.record_ordinal == ordinal)
        })
    }

    pub(crate) fn resolve(&self) -> Result<Vec<(String, BodyId)>, &'static str> {
        let operands = self.operands()?;
        let scope = Scope {
            metadata: &self.metadata,
            roots: &self.roots,
        };
        let joins = Joins::new(
            &self.links,
            &self.members,
            &self.keys,
            &self.carrier,
            &scope,
        );
        resolve(
            &operands,
            &joins,
            |_, ordinal| ordinal == STORED_CARRIER,
            |candidate| self.bodies.contains(candidate),
        )
    }

    pub(crate) fn inventory(&self) -> FeatureInventory {
        let mut inventory = FeatureInventory {
            features: vec![self.feature.clone()],
            labels: vec![self.label.clone()],
            properties: self.properties.clone(),
            kernel_entity_links: self.links.clone(),
            ..FeatureInventory::default()
        };
        inventory.record_types.insert(
            cadmpeg_ir::identity_key!("dc"),
            [(
                STORED_CARRIER,
                Some(crate::pmdc::type_id_string(
                    crate::kernel::KERNEL_RECORD_TYPE_ID,
                )),
            )]
            .into_iter()
            .collect(),
        );
        inventory
    }

    /// A model that holds the two current bodies and the native feature.
    pub(crate) fn model(&self, ctx: &DecodeContext<'_>) -> cadmpeg_ir::document::Model {
        let mut model = cadmpeg_ir::document::Model::default();
        for id in &self.bodies {
            model.bodies.push(cadmpeg_ir::topology::Body {
                id: id.clone(),
                kind: cadmpeg_ir::topology::BodyKind::default(),
                regions: vec![],
                transform: None,
                name: None,
                color: None,
                visible: None,
            });
        }
        model.features.push(
            crate::feature::native_feature(
                ctx,
                &self.feature.identity,
                Some(&self.label),
                self.feature.id(),
            )
            .expect("native history feature"),
        );
        model
    }

    pub(crate) fn project(
        &self,
        ctx: &DecodeContext<'_>,
        model: &mut cadmpeg_ir::document::Model,
    ) -> super::Projection {
        let scope = Scope {
            metadata: &self.metadata,
            roots: &self.roots,
        };
        project(
            ctx,
            &self.inventory(),
            &self.members,
            &self.carrier,
            &scope,
            &self.keys,
            model,
        )
        .expect("composite projection")
    }
}

pub(crate) fn with_context<T>(run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    run(&ctx)
}

#[test]
fn composite_pairs_sources_and_results_into_distinct_current_bodies() {
    let fixture = fixture();
    let resolved = fixture.resolve().expect("complete join");
    assert_eq!(
        resolved.iter().map(|(_, body)| body).collect::<Vec<_>>(),
        [&body("target"), &body("other")]
    );
    assert_eq!(resolved[0].0, fixture.links[2].id());
    assert_eq!(resolved[1].0, fixture.links[3].id());
}

#[test]
fn composite_projects_stored_geometry_with_ordered_result_bodies() {
    with_context(|ctx| {
        let fixture = fixture();
        let mut model = fixture.model(ctx);
        let projection = fixture.project(ctx, &mut model);
        assert_eq!(projection.transferred, 1);
        assert!(projection.losses.is_empty());
        let feature = &model.features[0];
        assert!(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {})
        ));
        assert_eq!(feature.source_tag.as_deref(), Some("composite"));
        assert_eq!(feature.source_properties.len(), 3);
        for (key, ordinal) in [
            ("source_body_set", 0),
            ("result_body_set", 1),
            ("control", 2),
        ] {
            assert_eq!(
                feature.source_properties.get(key),
                Some(&fixture.properties[ordinal].id())
            );
        }
        let [result] = model.feature_result_topologies.as_slice() else {
            panic!("one result topology")
        };
        assert_eq!(result.output_of, feature.id);
        assert_eq!(
            result.native_ref.as_deref(),
            Some(fixture.properties[1].id().as_str())
        );
        assert_eq!(
            result
                .bodies()
                .iter()
                .map(|body| body.as_str().to_owned())
                .collect::<Vec<_>>(),
            [fixture.links[2].id(), fixture.links[3].id()]
        );
        assert!(result.faces().is_empty() && result.edges().is_empty());
        let joined = projection.bodies.into_iter().collect::<HashMap<_, _>>();
        crate::feature::body_selection::project(&mut model, &joined, &fixture.inventory());
        assert_eq!(
            model.features[0].evaluation.outputs().as_slice(),
            [body("target"), body("other")]
        );
    });
}

#[test]
fn composite_keeps_native_construction_with_the_first_refused_join() {
    type Change = fn(&mut Fixture);
    let cases: [(Change, &str); 12] = [
        (
            |f| f.links[0].edit(|link| link.state = 0),
            "slot 0 member requires a state-3 selection of a stored kernel carrier",
        ),
        (
            |f| f.links[0].identity.record_ordinal = 99,
            "slot 0 member requires one kernel-entity link",
        ),
        (
            |f| f.members.truncate(1),
            "slot 1 member requires one external-reference join",
        ),
        (
            |f| f.members[1].reference = PmDcReference::new(9, false).expect("reference"),
            "slot 1 member requires one external-reference join",
        ),
        (
            |f| f.links[2].edit(|link| link.state = 0),
            "slot 1 member requires one state-3 current body key",
        ),
        (
            |f| f.links[3].edit(|link| link.body = PmDcReference::from_packed(0x8000_0002)),
            "slot 1 member requires a unique link on the current carrier",
        ),
        (
            |f| {
                let header_id = f.links[2].header_id;
                f.links[3].edit(|link| link.header_id = header_id);
            },
            "slot 1 member requires a unique link on the current carrier",
        ),
        (
            |f| {
                f.keys.remove(1);
            },
            "slot 1 member requires one state-3 current body key",
        ),
        (
            |f| f.bodies.truncate(1),
            "slot 1 member requires a current model body",
        ),
        (
            |f| f.links[1].edit(|link| link.selection_source = 102),
            "slot 0 and slot 1 members require equal distinct selection sources",
        ),
        (
            |f| {
                f.links[1].edit(|link| link.selection_source = 100);
                f.links[3].edit(|link| link.selection_source = 100);
            },
            "slot 0 and slot 1 members require equal distinct selection sources",
        ),
        (
            |f| f.roots.clear(),
            "the current carrier requires one document segment",
        ),
    ];
    for (change, reason) in cases {
        let mut fixture = fixture();
        change(&mut fixture);
        assert_eq!(fixture.resolve(), Err(reason), "{reason}");
        with_context(|ctx| {
            let mut model = fixture.model(ctx);
            let projection = fixture.project(ctx, &mut model);
            assert_eq!(projection.transferred, 0);
            assert!(projection.bodies.is_empty());
            assert!(model.feature_result_topologies.is_empty());
            assert_eq!(model.features[0].evaluation.definition(), &definition(true));
            assert_eq!(projection.losses.len(), 1);
            assert_eq!(
                projection.losses[0].code.local_code(),
                "feature.composite-operands-native"
            );
            assert_eq!(
                projection.losses[0].message,
                format!("1 Composite feature(s) remain native: {reason}.")
            );
        });
    }
    let mut fixture = fixture();
    fixture.carrier = ActiveCarrierRecord::NotApplicable { id: String::new() };
    assert_eq!(
        fixture.resolve(),
        Err("the active carrier requires one selected record")
    );
    let mut fixture = self::fixture();
    fixture.links[3].edit(|link| link.entity_id = 7);
    assert_eq!(
        fixture.resolve(),
        Err("slot 1 members require distinct current bodies")
    );
}

fn set_value(property: &mut PmDcFeatureProperty, value: u32) {
    property.edit(|property| {
        let PmDcFeaturePropertyKind::BodySet { value: current, .. } = &mut property.kind else {
            panic!("body set")
        };
        *current = value;
    });
}

fn set_members(property: &mut PmDcFeatureProperty, references: Vec<PmDcReference>) {
    property.edit(|property| {
        property.kind = PmDcFeaturePropertyKind::BodySet {
            value: 0,
            items: list(references),
        }
    });
}

#[test]
fn composite_operand_checks_report_the_first_refused_slot() {
    type Change = fn(&mut Fixture);
    let cases: [(Change, &str); 14] = [
        (
            |f| {
                let mut payload = (*f.feature).clone();
                payload.properties = PmDcReferenceList::new(3, None, vec![]).expect("list");
                f.feature = Located::new(
                    payload,
                    f.feature.identity.type_id.clone(),
                    f.feature.identity.segment_token.clone(),
                    0,
                );
            },
            "property-vector marker requires the type-2 form",
        ),
        (
            |f| {
                f.feature =
                    with_references(&f.feature, vec![PmDcReference::from_packed(0x8000_0002)]);
            },
            "property vector requires three slots",
        ),
        (
            |f| {
                f.properties.remove(0);
            },
            "slot 0 requires one stored-body-set record",
        ),
        (
            |f| f.properties[1].identity.type_id = RecordTypeId::from_bytes(BOOLEAN_TYPE),
            "slot 1 requires one result-body-set record",
        ),
        (
            |f| f.properties[2].identity.type_id = RecordTypeId::from_bytes(BODY_SET_TYPE),
            "slot 2 requires one Boolean record",
        ),
        (
            |f| f.properties[0].edit(|property| property.save_version_major = 17),
            "slot 0 requires one stored-body-set record",
        ),
        (
            |f| {
                f.properties[0].edit(|property| {
                    property.kind = PmDcFeaturePropertyKind::BodySet {
                        value: 0,
                        items: PmDcReferenceList::new(3, None, vec![]).expect("list"),
                    }
                });
            },
            "slot 0 requires the type-2 body-set form",
        ),
        (
            |f| {
                f.properties[1].edit(|property| {
                    property.kind = PmDcFeaturePropertyKind::BodySet {
                        value: 0,
                        items: PmDcReferenceList::new(3, None, vec![]).expect("list"),
                    }
                });
            },
            "slot 1 requires the type-2 body-set form",
        ),
        (
            |f| set_value(&mut f.properties[0], 1),
            "slot 0 body-set value requires its construction meaning",
        ),
        (
            |f| set_value(&mut f.properties[1], 1),
            "slot 1 body-set value requires its construction meaning",
        ),
        (
            |f| {
                f.properties[2].edit(|property| {
                    property.kind = PmDcFeaturePropertyKind::Boolean {
                        name: "named".into(),
                        name_value: 0,
                        value: false,
                    }
                });
            },
            "slot 2 requires an unnamed Boolean",
        ),
        (
            |f| {
                set_members(
                    &mut f.properties[0],
                    vec![PmDcReference::from_packed(0x8000_000b)],
                );
            },
            "slot 0 and slot 1 require equal nonempty member counts",
        ),
        (
            |f| {
                set_members(
                    &mut f.properties[0],
                    vec![
                        PmDcReference::from_packed(0x8000_000b),
                        PmDcReference::from_packed(0x0000_000c),
                    ],
                );
            },
            "slot 0 members require qualified references",
        ),
        (
            |f| {
                set_members(
                    &mut f.properties[1],
                    vec![
                        PmDcReference::from_packed(1),
                        PmDcReference::from_packed(0x8000_0002),
                    ],
                );
            },
            "slot 1 members require unqualified external references",
        ),
    ];
    for (change, reason) in cases {
        let mut fixture = fixture();
        change(&mut fixture);
        assert_eq!(fixture.operands().err(), Some(reason), "{reason}");
    }
    for (slot, reason) in [
        "slot 0 requires a qualified stored-body-set reference",
        "slot 1 requires a qualified result-body-set reference",
        "slot 2 requires a qualified Boolean reference",
    ]
    .into_iter()
    .enumerate()
    {
        for packed in [0, 2, 0x8000_0000] {
            let mut fixture = fixture();
            let mut references = fixture.feature.properties.references().to_vec();
            references[slot] = PmDcReference::from_packed(packed);
            fixture.feature = with_references(&fixture.feature, references);
            assert_eq!(fixture.operands().err(), Some(reason));
        }
    }
    assert!(fixture().operands().is_ok());
}

#[test]
fn composite_boolean_value_does_not_change_the_join() {
    for value in [false, true] {
        let mut fixture = fixture();
        fixture.properties[2].edit(|property| {
            property.kind = PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value,
            }
        });
        assert!(fixture.resolve().is_ok());
    }
}

#[test]
fn composite_losses_report_missing_and_ambiguous_owners_once() {
    with_context(|ctx| {
        let fixture = fixture();
        let mut model = fixture.model(ctx);
        let scope = Scope {
            metadata: &fixture.metadata,
            roots: &fixture.roots,
        };
        let mut inventory = fixture.inventory();
        inventory.features.clear();
        let projection = project(
            ctx,
            &inventory,
            &fixture.members,
            &fixture.carrier,
            &scope,
            &fixture.keys,
            &mut model,
        )
        .expect("projection");
        assert_eq!(
            projection.losses[0].message,
            "1 Composite feature(s) remain native: label owner requires one typed feature record."
        );
        let mut inventory = fixture.inventory();
        let mut duplicate = inventory.labels[0].clone();
        duplicate.identity.record_ordinal += 1;
        inventory.labels.push(duplicate);
        let projection = project(
            ctx,
            &inventory,
            &fixture.members,
            &fixture.carrier,
            &scope,
            &fixture.keys,
            &mut model,
        )
        .expect("projection");
        assert_eq!(projection.losses.len(), 1);
        assert_eq!(
            projection.losses[0].message,
            "1 Composite feature(s) remain native: label owner requires one feature label."
        );
        assert!(project(
            ctx,
            &FeatureInventory::default(),
            &[],
            &fixture.carrier,
            &scope,
            &[],
            &mut model
        )
        .expect("projection")
        .losses
        .is_empty());
    });
}

#[test]
fn composite_native_definition_names_the_class_only_for_a_unique_label() {
    assert!(matches!(
        definition(true),
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Other(ref name),
            ref parameters,
        }) if name == "Composite" && parameters.is_empty()
    ));
    assert!(matches!(
        definition(false),
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: cadmpeg_ir::features::NativeFeatureKind::Other(ref name),
            ..
        }) if name == "InventorFeature"
    ));
}
