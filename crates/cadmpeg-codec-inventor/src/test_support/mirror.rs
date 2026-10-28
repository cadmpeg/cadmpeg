// SPDX-License-Identifier: Apache-2.0

use crate::feature::mirror::{MirrorIndex, Property, IDENTITY};
use crate::feature::tests::{reference, reference_list, segment, test_header, test_label};
use crate::feature::{
    history_feature_id, ClassId, PmDcFeatureLabel, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcFeatureReferenceFamily, PmDcPatternFamily, PmDcPatternFeature, PmDcWorkPlane,
};
use crate::feature::{
    PmDcFeaturePropertyPayload, PmDcPatternFeaturePayload, PmDcWorkPlanePayload,
    MIRROR_FEATURE_TYPE, WORK_PLANE_TYPE,
};
use crate::record_identity::{Located, RecordTypeId};
use crate::sketch::PmDcTransform;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::scalar::FiniteReal;

pub(crate) fn pattern(
    version: u8,
    body: bool,
    seeds: &[u32],
    overrides: &[(usize, u32)],
) -> PmDcPatternFeature {
    let mut slots = vec![reference(0); 13];
    for (slot, ordinal) in [
        (0, 100),
        (1, 111),
        (2, 107),
        (4, 112),
        (5, 110),
        (6, 101),
        (7, 102),
        (12, 103),
    ] {
        slots[slot] = reference(ordinal + 1);
    }
    slots[if body { 10 } else { 9 }] = reference(105);
    for &(slot, packed) in overrides {
        slots[slot] = reference(packed);
    }
    let mut extension_values = if version >= 29 {
        vec![0; 14]
    } else if version >= 21 {
        vec![0; 6]
    } else {
        Vec::new()
    };
    if version >= 21 {
        extension_values[0] = 103;
    }
    if version >= 29 {
        extension_values[11] = 2;
    }
    Located::new(
        PmDcPatternFeaturePayload {
            save_version_major: version,
            header: test_header(),
            state: 0,
            outline_value: 0,
            properties: reference_list(&[]),
            value: 0,
            participants: reference_list(seeds),
            family: PmDcPatternFamily::Mirror,
            property_slots: slots,
            control: 0,
            extension_values,
        },
        RecordTypeId::from_bytes(MIRROR_FEATURE_TYPE),
        segment(),
        20,
    )
}

pub(crate) fn property(ordinal: u32, kind: PmDcFeaturePropertyKind) -> PmDcFeatureProperty {
    let type_id = match &kind {
        PmDcFeaturePropertyKind::Mirror { definition } => match definition {
            Property::Selection { .. } => crate::feature::mirror::SELECTION_TYPE,
            Property::Options { .. } => crate::feature::mirror::OPTIONS_TYPE,
        },
        PmDcFeaturePropertyKind::Boolean { .. } => crate::feature::BOOLEAN_TYPE,
        PmDcFeaturePropertyKind::References { family, .. } => match family {
            PmDcFeatureReferenceFamily::ObjectCollection => crate::feature::OBJECT_COLLECTION_TYPE,
            PmDcFeatureReferenceFamily::FeatureDimensions => {
                crate::feature::FEATURE_DIMENSIONS_TYPE
            }
            _ => panic!("fixture reference family"),
        },
        PmDcFeaturePropertyKind::SurfaceBody { .. } => crate::feature::SURFACE_BODY_TYPE,
        PmDcFeaturePropertyKind::RdxVariable { .. } => crate::feature::RDX_VARIABLE_TYPE,
        _ => panic!("fixture property kind"),
    };
    Located::new(
        PmDcFeaturePropertyPayload {
            save_version_major: 19,
            header: test_header(),
            kind,
        },
        RecordTypeId::from_bytes(type_id),
        segment(),
        ordinal,
    )
}

pub(crate) struct Fixture {
    pub(crate) properties: Vec<PmDcFeatureProperty>,
    pub(crate) transforms: Vec<PmDcTransform>,
    pub(crate) planes: Vec<PmDcWorkPlane>,
    pub(crate) label: PmDcFeatureLabel,
}

impl Fixture {
    pub(crate) fn new(body: bool) -> Self {
        let properties = vec![
            property(
                101,
                PmDcFeaturePropertyKind::Boolean {
                    name: String::new(),
                    name_value: 0,
                    value: body,
                },
            ),
            property(
                102,
                PmDcFeaturePropertyKind::Boolean {
                    name: String::new(),
                    name_value: 0,
                    value: false,
                },
            ),
            property(
                104,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::ObjectCollection,
                    items: reference_list(&[106]),
                },
            ),
            property(
                105,
                PmDcFeaturePropertyKind::SurfaceBody { body: reference(1) },
            ),
            property(
                107,
                PmDcFeaturePropertyKind::RdxVariable {
                    name: String::new(),
                    name_value: 0,
                    nominal_value: 0,
                    model_value: 0,
                },
            ),
            property(
                110,
                PmDcFeaturePropertyKind::References {
                    family: PmDcFeatureReferenceFamily::FeatureDimensions,
                    items: reference_list(&[]),
                },
            ),
            property(
                111,
                PmDcFeaturePropertyKind::Mirror {
                    definition: Property::Selection {
                        control: 0x3000_0004,
                        value: 0,
                    },
                },
            ),
            property(
                112,
                PmDcFeaturePropertyKind::Mirror {
                    definition: Property::Options {
                        bytes: [0; 9].into(),
                    },
                },
            ),
        ];
        let transforms = vec![Located::new(
            crate::sketch::PmDcTransformPayload {
                save_version_major: 19,
                header: test_header(),
                prefix_present: false,
                matrix: crate::compact_matrix::CompactMatrix::try_from_rows(
                    0x8421, 0x7bde, IDENTITY,
                )
                .expect("identity compact matrix"),
            },
            RecordTypeId::from_bytes(crate::pmdc::inventor_id(0x9087_4d18)),
            segment(),
            100,
        )];
        let planes = vec![Located::new(
            PmDcWorkPlanePayload {
                save_version_major: 19,
                header: test_header(),
                entity_flags: 0,
                values: vec![0; 3],
                marker: false,
                origin: [
                    FiniteReal::new(2.0).expect("finite"),
                    FiniteReal::ZERO,
                    FiniteReal::ZERO,
                ],
                u_axis: [FiniteReal::ZERO, FiniteReal::ONE, FiniteReal::ZERO],
                v_axis: [FiniteReal::ZERO, FiniteReal::ZERO, FiniteReal::ONE],
            },
            RecordTypeId::from_bytes(WORK_PLANE_TYPE),
            segment(),
            103,
        )];
        let class =
            ClassId::try_from("a326c2b6d211263660002cab01f31bb0".to_owned()).expect("mirror class");
        Self {
            properties,
            transforms,
            planes,
            label: test_label(20, 2, class, &[]),
        }
    }

    pub(crate) fn at_version(mut self, version: u8) -> Self {
        for property in &mut self.properties {
            let identity = property.identity.clone();
            let mut payload = (**property).clone();
            payload.save_version_major = version;
            payload.header.flags_extension = (version >= 28).then_some(0);
            payload.header.stamps = if version >= 25 {
                vec![-1, -1]
            } else if version >= 23 {
                vec![-1]
            } else {
                Vec::new()
            };
            *property = Located::new(
                payload,
                identity.type_id,
                identity.segment_token,
                identity.record_ordinal,
            );
        }
        let mut plane = (*self.planes[0]).clone();
        plane.save_version_major = version;
        plane.values = vec![0; if version > 21 { 4 } else { 3 }];
        self.planes[0] = Located::new(
            plane,
            RecordTypeId::from_bytes(WORK_PLANE_TYPE),
            segment(),
            103,
        );
        self
    }

    pub(crate) fn index(&self) -> MirrorIndex<'_> {
        MirrorIndex::new(&self.properties, &self.transforms, &self.planes)
    }
}

pub(crate) fn seed(ordinal: u32) -> Option<FeatureId> {
    [2, 4].contains(&ordinal).then(|| {
        history_feature_id(&crate::feature::tests::test_feature(ordinal, 0, &[]).identity)
            .expect("feature id")
    })
}

pub(crate) fn refusal_cases() -> Vec<(String, Fixture, PmDcPatternFeature)> {
    let mut cases = Vec::new();
    for slot in [0, 1, 2, 4, 5, 6, 7, 9, 12] {
        cases.push((
            format!("missing slot {slot}"),
            Fixture::new(false),
            pattern(19, false, &[3], &[(slot, 0)]),
        ));
    }
    for slot in [3, 8, 10, 11] {
        cases.push((
            format!("unexpected slot {slot}"),
            Fixture::new(false),
            pattern(19, false, &[3], &[(slot, 106)]),
        ));
    }
    for definition in [
        Property::Selection {
            control: 0,
            value: 0,
        },
        Property::Selection {
            control: 0x3000_0004,
            value: 1,
        },
    ] {
        let mut fixture = Fixture::new(false);
        fixture.properties[6] = property(111, PmDcFeaturePropertyKind::Mirror { definition });
        cases.push((
            "selection control".into(),
            fixture,
            pattern(19, false, &[3], &[]),
        ));
    }
    for index in 0..9 {
        let mut fixture = Fixture::new(false);
        let mut bytes = [0; 9];
        bytes[index] = 1;
        fixture.properties[7] = property(
            112,
            PmDcFeaturePropertyKind::Mirror {
                definition: Property::Options {
                    bytes: bytes.into(),
                },
            },
        );
        cases.push((
            format!("option byte {index}"),
            fixture,
            pattern(19, false, &[3], &[]),
        ));
    }
    for (nominal_value, model_value) in [(1, 0), (0, 1)] {
        let mut fixture = Fixture::new(false);
        fixture.properties[4] = property(
            107,
            PmDcFeaturePropertyKind::RdxVariable {
                name: String::new(),
                name_value: 0,
                nominal_value,
                model_value,
            },
        );
        cases.push((
            "variable control".into(),
            fixture,
            pattern(19, false, &[3], &[]),
        ));
    }
    let mut fixture = Fixture::new(false);
    fixture.properties[5] = property(
        110,
        PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::FeatureDimensions,
            items: reference_list(&[106]),
        },
    );
    cases.push(("dimensions".into(), fixture, pattern(19, false, &[3], &[])));
    for version in [19, 21, 29] {
        let source = pattern(version, false, &[3], &[]);
        let mut changes = Vec::new();
        let mut payload = (*source).clone();
        payload.control = 1;
        changes.push(payload);
        let mut payload = (*source).clone();
        payload.value = 1;
        changes.push(payload);
        let mut payload = (*source).clone();
        payload.properties = reference_list(&[106]);
        changes.push(payload);
        for i in 0..source.extension_values.len() {
            let mut payload = (*source).clone();
            payload.extension_values[i] = if i == 0 { 106 } else { 1 };
            changes.push(payload);
        }
        let mut payload = (*source).clone();
        payload.extension_values.push(0);
        changes.push(payload);
        let mut payload = (*source).clone();
        payload.property_slots.pop();
        changes.push(payload);
        let mut payload = (*source).clone();
        payload.property_slots.push(reference(0));
        changes.push(payload);
        for payload in changes {
            cases.push((
                format!("version {version} suffix"),
                Fixture::new(false).at_version(version),
                Located::new(
                    payload,
                    RecordTypeId::from_bytes(MIRROR_FEATURE_TYPE),
                    segment(),
                    20,
                ),
            ));
        }
    }
    for i in 0..7 {
        let mut fixture = Fixture::new(false);
        let mut payload = (*fixture.planes[0]).clone();
        match i {
            0 => payload.entity_flags = 1,
            1 => payload.marker = true,
            2 => payload.values.push(0),
            3..=5 => payload.values[i - 3] = 1,
            _ => payload.u_axis = payload.v_axis,
        }
        fixture.planes[0] = Located::new(
            payload,
            RecordTypeId::from_bytes(WORK_PLANE_TYPE),
            segment(),
            103,
        );
        cases.push((
            "plane controls".into(),
            fixture,
            pattern(19, false, &[3], &[]),
        ));
    }
    cases.extend(boolean_refusal_cases());
    cases.extend(additional_refusal_cases());
    cases.extend(marker_refusal_cases());
    cases
}

pub(crate) fn boolean_refusal_cases() -> Vec<(String, Fixture, PmDcPatternFeature)> {
    let mut cases = Vec::new();
    for (name, property_index, slot) in [
        ("true slot 7", 1, None),
        ("true slot 11", 1, Some(11)),
        ("mode disagrees with operands", 0, None),
    ] {
        let mut fixture = Fixture::new(false);
        let ordinal = if property_index == 0 { 101 } else { 102 };
        fixture.properties[property_index] = property(
            ordinal,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: true,
            },
        );
        let overrides = slot
            .map(|slot| (slot, ordinal + 1))
            .into_iter()
            .collect::<Vec<_>>();
        cases.push((name.into(), fixture, pattern(19, false, &[3], &overrides)));
    }
    for version in [21, 29] {
        let mut fixture = Fixture::new(false).at_version(version);
        fixture.properties.push(property(
            113,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value: true,
            },
        ));
        let source = pattern(version, false, &[3], &[]);
        let mut payload = (*source).clone();
        payload.extension_values[0] = 114;
        cases.push((
            "true extension option".into(),
            fixture,
            Located::new(
                payload,
                RecordTypeId::from_bytes(MIRROR_FEATURE_TYPE),
                segment(),
                20,
            ),
        ));
    }
    cases
}

fn additional_refusal_cases() -> Vec<(String, Fixture, PmDcPatternFeature)> {
    let mut cases = Vec::new();
    for version in [22, 29] {
        for missing in [false, true] {
            let mut fixture = Fixture::new(false).at_version(version);
            let mut plane = (*fixture.planes[0]).clone();
            if missing {
                plane.values.pop();
            } else {
                plane.values[3] = 1;
            }
            fixture.planes[0] = Located::new(
                plane,
                RecordTypeId::from_bytes(WORK_PLANE_TYPE),
                segment(),
                103,
            );
            cases.push((
                "four-value plane controls".into(),
                fixture,
                pattern(version, false, &[3], &[]),
            ));
        }
    }
    for axis in [true, false] {
        let mut fixture = Fixture::new(false);
        let mut plane = (*fixture.planes[0]).clone();
        if axis {
            plane.u_axis[1] = FiniteReal::new(2.0).expect("finite");
        } else {
            plane.v_axis[2] = FiniteReal::new(2.0).expect("finite");
        }
        fixture.planes[0] = Located::new(
            plane,
            RecordTypeId::from_bytes(WORK_PLANE_TYPE),
            segment(),
            103,
        );
        cases.push((
            "nonunit plane axes".into(),
            fixture,
            pattern(19, false, &[3], &[]),
        ));
    }
    for i in [0, 1, 4, 6, 7] {
        let mut fixture = Fixture::new(false);
        let ordinal = fixture.properties[i].identity.record_ordinal;
        if matches!(i, 0 | 1) {
            fixture.properties[i] = property(
                ordinal,
                PmDcFeaturePropertyKind::Boolean {
                    name: String::new(),
                    name_value: 1,
                    value: false,
                },
            );
        } else if i == 4 {
            fixture.properties[i] = property(
                ordinal,
                PmDcFeaturePropertyKind::RdxVariable {
                    name: String::new(),
                    name_value: 1,
                    nominal_value: 0,
                    model_value: 0,
                },
            );
        } else {
            fixture.properties[i].identity.type_id = RecordTypeId::from_bytes(WORK_PLANE_TYPE);
        }
        cases.push((
            "operand name value or record type".into(),
            fixture,
            pattern(19, false, &[3], &[]),
        ));
    }
    for values in [&[][..], &[106, 106][..], &[0][..], &[103][..]] {
        let mut fixture = Fixture::new(false);
        fixture.properties[2] = property(
            104,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::ObjectCollection,
                items: reference_list(values),
            },
        );
        cases.push((
            "result collection members".into(),
            fixture,
            pattern(19, false, &[3], &[]),
        ));
    }
    for changed in 0..3 {
        let mut fixture = Fixture::new(false);
        fixture.label = match changed {
            0 => test_label(21, 2, fixture.label.class_id(), &[]),
            1 => test_label(20, 2, fixture.label.class_id(), &[104]),
            _ => test_label(
                20,
                2,
                ClassId::try_from("00000000000000000000000000000000".to_owned()).expect("class"),
                &[],
            ),
        };
        cases.push((
            "label owner, participants, or class".into(),
            fixture,
            pattern(19, false, &[3], &[]),
        ));
    }
    cases
}

impl Fixture {
    pub(crate) fn project_history(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        source: PmDcPatternFeature,
    ) -> Result<crate::feature::FeatureProjection, cadmpeg_core::CodecError> {
        use crate::design::scope::{BrepSegmentName, BrepSegmentRole, PmDcModelRootPayload};

        let seeds = source
            .participants
            .references()
            .iter()
            .filter_map(|r| r.record_ordinal())
            .collect::<Vec<_>>();
        let mut participants = seeds.iter().map(|ordinal| ordinal + 1).collect::<Vec<_>>();
        participants.push(source.identity.record_ordinal + 1);
        let mut inventory = crate::feature::FeatureInventory::default();
        inventory.features.extend(
            seeds
                .iter()
                .map(|&ordinal| crate::feature::tests::test_feature(ordinal, 0, &[])),
        );
        inventory.pattern_features.push(source);
        inventory.properties.extend(self.properties.clone());
        inventory.work_planes.extend(self.planes.clone());
        inventory.labels.extend([
            self.label.clone(),
            test_label(
                0,
                0,
                ClassId::try_from("00000000000000000000000000000000".to_owned()).expect("class"),
                &participants,
            ),
        ]);
        let mut values = [0; 15];
        values[12] = 1;
        let root = Located::new(
            PmDcModelRootPayload {
                save_version_major: 19,
                header_value: 0,
                header_id: 0,
                next: reference(0),
                flags: 0x40,
                flags_extension: None,
                identifier: "0".repeat(32),
                name: String::new(),
                members: reference_list(&[]),
                values,
                parameter_table: reference(0),
                brep_segment: "Model".to_owned(),
                scale: FiniteReal::ONE,
                terminal_value: 0,
            },
            RecordTypeId::from_bytes(crate::design::scope::MODEL_ROOT_TYPE),
            segment(),
            0,
        );
        let design = crate::design::DesignInventory {
            parameters: Vec::new(),
            expressions: Vec::new(),
            units: Vec::new(),
            parameter_tables: Vec::new(),
            model_roots: vec![root],
            brep_segments: vec![BrepSegmentName {
                display_name: "Model".to_owned(),
                role: BrepSegmentRole::Document,
            }],
            issues: Vec::new(),
        };
        let sketch = crate::sketch::SketchInventory {
            sketches: Vec::new(),
            entities: Vec::new(),
            transforms: self.transforms.clone(),
            directions: Vec::new(),
            constraints: Vec::new(),
            issues: Vec::new(),
        };
        crate::feature::project(
            ctx,
            &inventory,
            &design,
            &sketch,
            &[],
            &crate::feature::ProjectedSketches {
                sketches: &[],
                entities: &[],
                profile_bodies: &[],
                result_model: None,
            },
        )
    }
}

fn marker_refusal_cases() -> Vec<(String, Fixture, PmDcPatternFeature)> {
    use crate::pmdc::{PmDcListMetadata, PmDcReferenceList};

    let mut cases = Vec::new();
    let empty = || PmDcReferenceList::new(8, None, Vec::new()).expect("empty type-8 list");
    let nonempty = || {
        PmDcReferenceList::new(8, Some(PmDcListMetadata::U16([1, 0])), vec![reference(106)])
            .expect("type-8 list")
    };
    for change in 0..5 {
        let mut fixture = Fixture::new(false);
        let source = pattern(19, false, &[3], &[]);
        let mut payload = (*source).clone();
        match change {
            0 => payload.properties = empty(),
            1 => {
                payload.participants = PmDcReferenceList::new(
                    8,
                    Some(PmDcListMetadata::U16([1, 0])),
                    vec![reference(3)],
                )
                .expect("type-8 seeds");
            }
            2 => {
                fixture.properties[5] = property(
                    110,
                    PmDcFeaturePropertyKind::References {
                        family: PmDcFeatureReferenceFamily::FeatureDimensions,
                        items: empty(),
                    },
                );
            }
            3 => {
                fixture.properties[2] = property(
                    104,
                    PmDcFeaturePropertyKind::References {
                        family: PmDcFeatureReferenceFamily::ObjectCollection,
                        items: nonempty(),
                    },
                );
            }
            _ => {
                let mut label = (*fixture.label).clone();
                label.participants = empty();
                fixture.label = Located::new(
                    label,
                    fixture.label.identity.type_id.clone(),
                    segment(),
                    fixture.label.identity.record_ordinal,
                );
            }
        }
        cases.push((
            "reference-list marker".into(),
            fixture,
            Located::new(
                payload,
                RecordTypeId::from_bytes(MIRROR_FEATURE_TYPE),
                segment(),
                20,
            ),
        ));
    }
    cases
}
