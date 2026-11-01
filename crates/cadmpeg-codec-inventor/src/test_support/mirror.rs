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
            Property::InstanceMap { .. } => crate::feature::mirror::instance_map::TYPE,
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
    pub(crate) generated_features: Vec<crate::feature::PmDcFeature>,
    pub(crate) generated_labels: Vec<PmDcFeatureLabel>,
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
            generated_features: Vec::new(),
            generated_labels: Vec::new(),
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
            .with_generated_nodes(&self.generated_features, &self.generated_labels)
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
    cases.extend(body_boolean_refusal_cases());
    cases.extend(additional_refusal_cases());
    cases.extend(marker_refusal_cases());
    cases.extend(replacement_refusal_cases());
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
        let overrides = if let Some(slot) = slot {
            fixture.properties[1] = property(
                102,
                PmDcFeaturePropertyKind::Boolean {
                    name: String::new(),
                    name_value: 0,
                    value: false,
                },
            );
            fixture.properties.push(property(
                113,
                PmDcFeaturePropertyKind::Boolean {
                    name: String::new(),
                    name_value: 0,
                    value: true,
                },
            ));
            vec![(slot, 114)]
        } else {
            Vec::new()
        };
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

pub(crate) fn body_boolean_form(
    version: u8,
    slot7: bool,
    slot11: Option<bool>,
) -> (Fixture, PmDcPatternFeature) {
    let mut fixture = Fixture::new(true);
    fixture.properties[1] = property(
        102,
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value: slot7,
        },
    );
    fixture.properties.push(property(
        113,
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value: false,
        },
    ));
    if let Some(value) = slot11 {
        fixture.properties.push(property(
            114,
            PmDcFeaturePropertyKind::Boolean {
                name: String::new(),
                name_value: 0,
                value,
            },
        ));
    }
    let overrides = slot11.map(|_| (11, 115)).into_iter().collect::<Vec<_>>();
    let source = pattern(version, true, &[], &overrides);
    let mut payload = (*source).clone();
    if version >= 21 {
        payload.extension_values[0] = 114;
    }
    (
        fixture.at_version(version),
        Located::new(
            payload,
            RecordTypeId::from_bytes(MIRROR_FEATURE_TYPE),
            segment(),
            20,
        ),
    )
}

pub(crate) fn body_boolean_refusal_cases() -> Vec<(String, Fixture, PmDcPatternFeature)> {
    let mut cases = Vec::new();
    for version in [19, 21, 29] {
        for (slot7, slot11) in [
            (true, None),
            (true, Some(false)),
            (true, Some(true)),
            (false, Some(true)),
        ] {
            let (fixture, source) = body_boolean_form(version, slot7, slot11);
            cases.push((
                format!("body mode slot 7 {slot7}, slot 11 {slot11:?}, version {version}"),
                fixture,
                source,
            ));
        }
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
        inventory.features.extend(self.generated_features.clone());
        inventory.labels.extend(self.generated_labels.clone());
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

pub(crate) fn refusal_code(name: &str) -> &'static str {
    if matches!(name, "true slot 7" | "true slot 11")
        || name.starts_with("body mode slot 7 ")
        || name.starts_with("replacement ")
    {
        "feature.mirror-boolean-native"
    } else if name == "unexpected slot 8" {
        "feature.mirror-instance-map-native"
    } else {
        "feature.mirror-operands-native"
    }
}

pub(crate) fn replacement_form() -> (Fixture, PmDcPatternFeature) {
    let (mut fixture, source) = body_boolean_form(19, true, Some(false));
    let mut payload = (*source).clone();
    payload.header.flags = 0x6200;
    let source = Located::new(payload, source.identity.type_id.clone(), segment(), 20);
    let mut payload = (*fixture.properties[3]).clone();
    payload.header.flags = 0x0300_4200;
    payload.kind = PmDcFeaturePropertyKind::SurfaceBody {
        body: crate::pmdc::PmDcReference::from_packed(1),
    };
    fixture.properties[3] = Located::new(
        payload,
        RecordTypeId::from_bytes(crate::feature::SURFACE_BODY_TYPE),
        segment(),
        105,
    );
    (fixture, source)
}

pub(crate) fn replacement_refusal_cases() -> Vec<(String, Fixture, PmDcPatternFeature)> {
    let mut cases = Vec::new();
    for change in 0..19 {
        let (mut fixture, source) = replacement_form();
        let mut payload = (*source).clone();
        match change {
            0 => payload.header.flags = 0x6a04,
            1 => payload.save_version_major = 20,
            2 => payload.header.flags_extension = Some(0),
            3 => payload.header.stamps.push(0),
            4 => payload.property_slots[11] = reference(0),
            5 => payload.property_slots[8] = reference(106),
            6 => payload.property_slots[0] = crate::pmdc::PmDcReference::from_packed(101),
            7 => {
                let mut body = (*fixture.properties[3]).clone();
                body.header.flags = 0;
                fixture.properties[3] = Located::new(
                    body,
                    RecordTypeId::from_bytes(crate::feature::SURFACE_BODY_TYPE),
                    segment(),
                    105,
                );
            }
            8 | 9 => {
                let body = if change == 8 {
                    reference(1)
                } else {
                    reference(0)
                };
                let mut body_payload = (*fixture.properties[3]).clone();
                body_payload.kind = PmDcFeaturePropertyKind::SurfaceBody { body };
                fixture.properties[3] = Located::new(
                    body_payload,
                    RecordTypeId::from_bytes(crate::feature::SURFACE_BODY_TYPE),
                    segment(),
                    105,
                );
            }
            10 => {
                let mut collection = (*fixture.properties[2]).clone();
                collection.header.flags = 1;
                fixture.properties[2] = Located::new(
                    collection,
                    RecordTypeId::from_bytes(crate::feature::OBJECT_COLLECTION_TYPE),
                    segment(),
                    104,
                );
            }
            11 => {
                fixture.properties[2] = property(
                    104,
                    PmDcFeaturePropertyKind::References {
                        family: PmDcFeatureReferenceFamily::ObjectCollection,
                        items: reference_list(&[106, 106]),
                    },
                );
            }
            12 => {
                let mut label = (*fixture.label).clone();
                label.participants = reference_list(&[106]);
                fixture.label = Located::new(
                    label,
                    fixture.label.identity.type_id.clone(),
                    segment(),
                    fixture.label.identity.record_ordinal,
                );
            }
            13 => {
                fixture.properties[2] = property(
                    104,
                    PmDcFeaturePropertyKind::References {
                        family: PmDcFeatureReferenceFamily::ObjectCollection,
                        items: crate::pmdc::PmDcReferenceList::new(
                            2,
                            Some(crate::pmdc::PmDcListMetadata::U32([1, 1])),
                            vec![reference(106)],
                        )
                        .expect("collection metadata"),
                    },
                );
            }
            14 => {
                fixture.properties[2] = property(
                    104,
                    PmDcFeaturePropertyKind::References {
                        family: PmDcFeatureReferenceFamily::ObjectCollection,
                        items: crate::pmdc::PmDcReferenceList::new(
                            2,
                            Some(crate::pmdc::PmDcListMetadata::U32([1, 0])),
                            vec![crate::pmdc::PmDcReference::from_packed(106)],
                        )
                        .expect("unqualified member"),
                    },
                );
            }
            15..=17 => {
                let mut body = (*fixture.properties[3]).clone();
                match change {
                    15 => body.save_version_major = 20,
                    16 => body.header.flags_extension = Some(0),
                    _ => body.header.stamps.push(0),
                }
                fixture.properties[3] = Located::new(
                    body,
                    RecordTypeId::from_bytes(crate::feature::SURFACE_BODY_TYPE),
                    segment(),
                    105,
                );
            }
            _ => {
                let mut label = (*fixture.label).clone();
                label.header.owner = crate::pmdc::PmDcReference::from_packed(21);
                fixture.label = Located::new(
                    label,
                    fixture.label.identity.type_id.clone(),
                    segment(),
                    fixture.label.identity.record_ordinal,
                );
            }
        }
        cases.push((
            format!("replacement control {change}"),
            fixture,
            Located::new(payload, source.identity.type_id.clone(), segment(), 20),
        ));
    }
    cases
}

pub(crate) fn new_body_form() -> (Fixture, PmDcPatternFeature) {
    let mut fixture = Fixture::new(true);
    let mut source = pattern(19, true, &[], &[(8, 114), (11, 115)]);
    let mut payload = (*source).clone();
    payload.header.source_index = 9;
    source = Located::new(payload, source.identity.type_id.clone(), segment(), 20);
    let original = fixture.properties[3].identity.clone();
    let mut payload = (*fixture.properties[3]).clone();
    payload.header.source_index = 7;
    payload.kind = PmDcFeaturePropertyKind::SurfaceBody {
        body: crate::pmdc::PmDcReference::from_packed(1),
    };
    fixture.properties[3] = Located::new(
        payload,
        original.type_id,
        original.segment_token,
        original.record_ordinal,
    );
    let mut map_bytes = crate::test_support::test_fixtures::content_at(113, 19, &[]);
    for value in [
        u32::MAX,
        0x3000_0006,
        1,
        0,
        0x3000_0002,
        0x8000_006a,
        0x3000_0002,
        1,
        1,
        0,
        0x8000_00c9,
        2,
        1,
        0x8000_0015,
    ] {
        map_bytes.extend_from_slice(&value.to_le_bytes());
    }
    let map = crate::test_support::test_fixtures::parse(&map_bytes, |ctx, source| {
        crate::feature::mirror::instance_map::parse(ctx, source, 19)
    })
    .expect("map bytes")
    .expect("single-entry map");
    fixture.properties.push(Located::new(
        map,
        RecordTypeId::from_bytes(crate::feature::mirror::instance_map::TYPE),
        segment(),
        113,
    ));
    fixture.properties.push(property(
        114,
        PmDcFeaturePropertyKind::Boolean {
            name: String::new(),
            name_value: 0,
            value: true,
        },
    ));
    for (ordinal, value) in [(201, 2), (202, 7), (203, 9)] {
        fixture.properties.push(property(
            ordinal,
            PmDcFeaturePropertyKind::RdxVariable {
                name: String::new(),
                name_value: 0,
                nominal_value: value,
                model_value: value,
            },
        ));
    }
    fixture.properties.push(property(
        204,
        PmDcFeaturePropertyKind::SurfaceBody {
            body: crate::pmdc::PmDcReference::from_packed(2),
        },
    ));
    let node =
        crate::feature::tests::test_feature(200, 4, &[(0, 201), (1, 202), (2, 203), (3, 204)]);
    let mut wire = serde_json::to_value(node).expect("synthetic node");
    wire["save_version_major"] = serde_json::json!(19);
    wire["outline_value"] = serde_json::json!(15);
    wire["header"]["flags"] = serde_json::json!(0x6200);
    fixture
        .generated_features
        .push(serde_json::from_value(wire).expect("generated node"));
    fixture.generated_labels.push(test_label(
        200,
        1,
        crate::feature::mirror::body_result::CLASS,
        &[],
    ));
    fixture.label = test_label(20, 2, fixture.label.class_id(), &[201]);
    (fixture, source)
}

fn change_record<T: serde::Serialize + serde::de::DeserializeOwned>(
    record: &mut T,
    change: impl FnOnce(&mut serde_json::Value),
) {
    let mut wire = serde_json::to_value(&*record).expect("synthetic record");
    change(&mut wire);
    *record = serde_json::from_value(wire).expect("changed synthetic record");
}

pub(crate) fn body_copy_refusal_cases() -> Vec<(String, Fixture, PmDcPatternFeature)> {
    let mut cases = Vec::new();
    for field in ["source", "result", "owner"] {
        for (name, reference) in [
            ("null", crate::pmdc::PmDcReference::from_packed(0)),
            ("unqualified", crate::pmdc::PmDcReference::from_packed(21)),
            (
                "different",
                crate::pmdc::PmDcReference::from_packed(0x8000_0033),
            ),
        ] {
            let (mut fixture, source) = new_body_form();
            let map = fixture
                .properties
                .iter_mut()
                .find(|p| p.identity.record_ordinal == 113)
                .expect("map");
            change_record(map, |wire| {
                wire["kind"]["definition"][field] =
                    serde_json::to_value(reference).expect("reference");
            });
            cases.push((format!("map {field} {name}"), fixture, source));
        }
    }
    for ordinal in [201, 202, 203] {
        for field in ["name_value", "nominal_value", "model_value"] {
            let (mut fixture, source) = new_body_form();
            let property = fixture
                .properties
                .iter_mut()
                .find(|p| p.identity.record_ordinal == ordinal)
                .expect("variable");
            change_record(property, |wire| {
                let value = wire["kind"][field].as_u64().expect("word");
                wire["kind"][field] = serde_json::json!(value + 1);
            });
            cases.push((format!("variable {ordinal} {field}"), fixture, source));
        }
    }
    for (name, ordinal, value) in [
        ("source body is local", 105, 0x8000_0001),
        ("source body is null", 105, 0),
        ("result body is local", 204, 0x8000_0002),
        ("result body is null", 204, 0),
        ("result repeats source body", 204, 1),
    ] {
        let (mut fixture, source) = new_body_form();
        let property = fixture
            .properties
            .iter_mut()
            .find(|p| p.identity.record_ordinal == ordinal)
            .expect("wrapper");
        change_record(property, |wire| {
            wire["kind"]["body"] =
                serde_json::to_value(crate::pmdc::PmDcReference::from_packed(value))
                    .expect("reference");
        });
        cases.push((name.into(), fixture, source));
    }
    for change in 0..20 {
        let (mut fixture, mut source) = new_body_form();
        match change {
            0 => fixture.generated_features.clear(),
            1 => fixture.generated_labels.clear(),
            2 => fixture
                .generated_features
                .push(fixture.generated_features[0].clone()),
            3 => fixture
                .generated_labels
                .push(fixture.generated_labels[0].clone()),
            4 => change_record(&mut fixture.generated_features[0], |wire| {
                wire["header"]["flags"] = serde_json::json!(0x86200);
            }),
            5 => change_record(&mut fixture.generated_features[0], |wire| {
                wire["save_version_major"] = serde_json::json!(20);
            }),
            6 => change_record(&mut fixture.generated_features[0], |wire| {
                wire["properties"]["metadata"]["values"][1] = serde_json::json!(1);
            }),
            7 => change_record(&mut fixture.label, |wire| {
                wire["participants"]["metadata"]["values"][1] = serde_json::json!(1);
            }),
            8 => {
                let collection = fixture
                    .properties
                    .iter_mut()
                    .find(|p| p.identity.record_ordinal == 104)
                    .expect("collection");
                change_record(collection, |wire| {
                    wire["kind"]["items"]["metadata"]["values"][1] = serde_json::json!(1);
                });
            }
            9 => fixture
                .properties
                .retain(|property| property.identity.record_ordinal != 113),
            10 => change_record(&mut fixture.generated_features[0], |wire| {
                wire["header"]["flags_extension"] = serde_json::json!(0);
            }),
            11 => change_record(&mut fixture.generated_features[0], |wire| {
                wire["header"]["stamps"] = serde_json::json!([-1]);
            }),
            12..=17 => {
                let ordinal = match change {
                    12 | 13 => 113,
                    14 | 15 => 201,
                    _ => 204,
                };
                let property = fixture
                    .properties
                    .iter_mut()
                    .find(|property| property.identity.record_ordinal == ordinal)
                    .expect("property");
                change_record(property, |wire| {
                    if change % 2 == 0 {
                        wire["header"]["flags_extension"] = serde_json::json!(0);
                    } else {
                        wire["header"]["stamps"] = serde_json::json!([-1]);
                    }
                });
            }
            18 => change_record(&mut source, |wire| {
                wire["header"]["flags_extension"] = serde_json::json!(0);
            }),
            _ => change_record(&mut source, |wire| {
                wire["header"]["stamps"] = serde_json::json!([-1]);
            }),
        }
        cases.push((format!("generated form {change}"), fixture, source));
    }
    cases
}
