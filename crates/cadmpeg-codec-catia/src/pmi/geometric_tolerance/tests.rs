// SPDX-License-Identifier: Apache-2.0

use super::{characteristic, transfer};
use crate::decode::ModelingGraphScope;
use crate::native::CatiaNative;
use cadmpeg_core::decode::View;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::pmi::{GeometricToleranceKind, PmiDefinition, PmiTarget};
use cadmpeg_ir::Exactness;

const VALUE_TOLERANCE: f64 = 1.0e-12;
const CATALOG: &[&str] = &[
    "CATCatalogManager",
    "catalogManager",
    "catalogLinks",
    "",
    "tolerance",
    "CATTPSToleranceWithDRF",
    "TolBloc",
    "CATTPSToleranceData",
    "TolWiDRF",
    "CATTPSToleranceWiDRFData",
    "LENGTH",
    "TolNumValue",
    "String",
    "TolTabValue",
    "frame",
    "CATTPSReferenceFrame",
    "FrameBloc",
    "CATTPSFrameData",
    "datum",
    "CATTPSDatum",
    "LabelBloc",
    "CATTPSCkeData",
    "Label",
    "TTRSBloc",
    "CATTPSTTRSData",
    "aspect",
    "CATMmrTTRS",
    "A",
    "B",
    "C",
    "RefTolNumValue",
    "RefTolTabValue",
    "blob",
    "blob-data",
    ".005/.25X.25",
];

fn selector(name: &str, compact: bool) -> Vec<u8> {
    let index = CATALOG
        .iter()
        .position(|value| *value == name)
        .expect("fixture catalog entry");
    if compact {
        vec![u8::try_from(index + 0x34).expect("small catalog")]
    } else {
        let mut bytes = vec![0x32];
        bytes.extend(
            u32::try_from(index + 1)
                .expect("small catalog")
                .to_le_bytes(),
        );
        bytes
    }
}

fn reference(value: u32, wide: bool) -> Vec<u8> {
    if wide {
        let mut bytes = vec![0x80];
        bytes.extend(value.to_le_bytes());
        bytes
    } else if value <= 79 {
        vec![u8::try_from(value + 0x81).expect("small reference")]
    } else {
        vec![
            u8::try_from(value / 256 + 0xd1).expect("paged reference"),
            u8::try_from(value % 256).expect("low reference"),
        ]
    }
}

fn frame(marker: u8, body: Vec<u8>) -> Vec<u8> {
    let mut bytes = vec![0x7c, marker];
    bytes.extend(
        u32::try_from(body.len() + 6)
            .expect("bounded fixture frame")
            .to_le_bytes(),
    );
    bytes.extend(body);
    bytes
}

fn key(id: u32) -> u32 {
    0x1122_0000 + id
}

#[derive(Clone, Copy)]
enum FixtureCase {
    Valid,
    WrongOwner,
    WrongMainKey,
    DuplicateField,
    CommonDatums,
    MalformedTransition,
    InstanceBlobDecoy,
    NegativeMain,
    SecondaryFrameKey,
    UnrelatedSecondaryKey,
    MalformedSecondaryKey,
    Tabulated,
    NullTarget,
    AmbiguousTargets,
    MultipleTargets,
    UnsupportedLastDatum,
    KeylessMain,
    AmbiguousSecondaryKey,
    UnresolvableSecondaryMember,
    MissingSecondaryField,
    RepeatedSecondaryField,
}

#[derive(Clone, Copy)]
struct Options {
    case: FixtureCase,
    base: u32,
    wide: bool,
    compact_schema: bool,
    subtype: u32,
    secondary: u32,
    main_literal: Option<u8>,
    secondary_literal: Option<u8>,
    trailer: u8,
    magnitude_trailer: Option<u8>,
    main_value: f64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            case: FixtureCase::Valid,
            base: 0,
            wide: false,
            compact_schema: true,
            subtype: 6,
            secondary: 0,
            main_literal: None,
            secondary_literal: None,
            trailer: 0x92,
            magnitude_trailer: None,
            main_value: 0.762,
        }
    }
}

fn field_pair(names: [&str; 2], options: Options) -> Vec<u8> {
    names
        .into_iter()
        .flat_map(|name| selector(name, options.compact_schema))
        .collect()
}

fn scalar_declaration(
    names: [&str; 2],
    values: &[u32],
    stored_key: Option<u32>,
    options: Options,
) -> Vec<u8> {
    let mut bytes = field_pair(names, options);
    bytes.push(u8::try_from(values.len() + 0x81).expect("small scalar vector"));
    for value in values {
        bytes.extend(reference(*value, options.wide));
    }
    bytes.extend([
        0x81,
        0x81,
        0x81,
        0x81,
        0x81,
        if names[0] == "FrameBloc" { 2 } else { 0x12 },
        0xd5,
        0,
        0x82,
        0x82,
    ]);
    match stored_key {
        Some(stored_key) => {
            bytes.push(0x80);
            bytes.extend(stored_key.to_le_bytes());
        }
        None => bytes.push(0x81),
    }
    bytes
}

fn inline_entity(id: u32, names: [&str; 2], declarations: Vec<u8>, options: Options) -> Vec<u8> {
    let mut bytes = vec![3];
    bytes.extend(field_pair(names, options));
    bytes.push(0xea);
    bytes.extend(id.to_le_bytes());
    bytes.extend([0x81, 0x80]);
    bytes.extend(key(id).to_le_bytes());
    bytes.extend([0, 0x89, 0x80]);
    bytes.extend(123_u32.to_le_bytes());
    bytes.push(0x4e);
    bytes.extend(declarations);
    bytes
}

fn instance(fields: Vec<u8>, count: u8) -> Vec<u8> {
    let mut bytes = vec![0x10, 0x96, 0x81 + count];
    bytes.extend(fields);
    bytes
}

fn instance_field(
    names: [&str; 2],
    references: &[Option<u32>],
    parameters: &[Option<u32>],
    options: Options,
) -> Vec<u8> {
    let mut bytes = field_pair(names, options);
    for group in [references, parameters] {
        bytes.push(u8::try_from(group.len() + 0x81).expect("small reference group"));
        for value in group {
            if let Some(value) = value {
                bytes.extend(reference(*value, options.wide));
            } else {
                bytes.push(0xfe);
            }
        }
    }
    bytes.push(u8::from(!references.is_empty()) * 2 + u8::from(!parameters.is_empty()) * 4);
    bytes
}

fn primitive(
    id: u32,
    owner: u32,
    role: &str,
    value: Option<f64>,
    label: Option<&str>,
    options: Options,
) -> (Vec<u8>, Vec<u8>) {
    let kind = if value.is_some() { "LENGTH" } else { "String" };
    let mut definition = vec![0, 0];
    definition.extend(field_pair([kind, role], options));
    definition.push(0xea);
    definition.extend(id.to_le_bytes());
    for word in [key(id), 0] {
        definition.push(0x80);
        definition.extend(word.to_le_bytes());
    }
    definition.extend([0x81, 3, 0x91]);
    let mut entity = vec![2];
    entity.extend(frame(6, definition));
    entity.extend(frame(7, vec![0xfe]));
    entity.extend([
        0x84,
        if value.is_some() { 0x96 } else { 0x93 },
        if label.is_some() || value.is_some() {
            0x82
        } else {
            0x81
        },
    ]);
    entity.extend(selector(kind, options.compact_schema));
    let literal = match role {
        "TolNumValue" => options.main_literal,
        "RefTolNumValue" => options.secondary_literal,
        _ => None,
    };
    if let Some(literal) = literal {
        entity.push(literal);
    } else if let Some(value) = value {
        entity.push(0xe6);
        entity.extend(value.to_le_bytes());
    } else if let Some(label) = label {
        entity.extend(selector(label, false));
    } else {
        entity.push(0x37);
    }
    let trailer = if value.is_some() {
        options.magnitude_trailer.unwrap_or(options.trailer)
    } else {
        options.trailer
    };
    entity.extend([0x81, trailer]);
    let mut object = vec![0x14, 1, 0x91];
    object.extend(reference(owner, options.wide));
    object.extend(frame(0x0a, vec![0xfe]));
    (entity, object)
}

fn fixture(options: Options) -> Vec<u8> {
    let id = |value| options.base + value;
    let mut declarations = scalar_declaration(
        ["TolBloc", "CATTPSToleranceData"],
        &[options.subtype, 0, 0, 1, 9],
        (!matches!(options.case, FixtureCase::KeylessMain)).then(|| {
            key(id(if matches!(options.case, FixtureCase::WrongMainKey) {
                3
            } else {
                2
            }))
        }),
        options,
    );
    let mut secondary_declaration = scalar_declaration(
        ["TolWiDRF", "CATTPSToleranceWiDRFData"],
        &[0, options.secondary, 0, 0],
        Some(key(id(
            if matches!(options.case, FixtureCase::UnrelatedSecondaryKey) {
                11
            } else if matches!(options.case, FixtureCase::SecondaryFrameKey) {
                4
            } else {
                12
            },
        ))),
        options,
    );
    if matches!(options.case, FixtureCase::MalformedSecondaryKey) {
        let at = secondary_declaration.len() - 5;
        secondary_declaration[at] = 0x7f;
    }
    if !matches!(options.case, FixtureCase::MissingSecondaryField) {
        declarations.extend(secondary_declaration);
    }
    let tol = instance_field(
        ["TolBloc", "CATTPSToleranceData"],
        &[None, None, None],
        &[Some(id(2)), Some(id(3))],
        options,
    );
    let target_refs = match options.case {
        FixtureCase::NullTarget => vec![None],
        FixtureCase::AmbiguousTargets => vec![Some(id(11)), Some(id(999))],
        FixtureCase::MultipleTargets => vec![Some(id(11)), Some(id(14))],
        _ => vec![Some(id(11))],
    };
    let mut fields = instance_field(["TTRSBloc", "CATTPSTTRSData"], &target_refs, &[], options);
    if matches!(options.case, FixtureCase::InstanceBlobDecoy) {
        fields.extend(field_pair(["blob", "blob-data"], options));
        fields.extend([0x82, 0xe5]);
        fields.extend(u32::try_from(tol.len()).expect("small blob").to_le_bytes());
        fields.extend(&tol);
        fields.extend([0x81, 0x81, 2]);
    }
    fields.extend(&tol);
    if matches!(options.case, FixtureCase::DuplicateField) {
        fields.extend(tol);
    }
    let with_drf = instance_field(
        ["TolWiDRF", "CATTPSToleranceWiDRFData"],
        &[Some(id(4))],
        &[
            Some(id(12)),
            Some(id(13)),
            matches!(options.case, FixtureCase::UnresolvableSecondaryMember).then(|| id(999)),
            None,
        ],
        options,
    );
    let with_drf_fields = match options.case {
        FixtureCase::MissingSecondaryField => 0,
        FixtureCase::RepeatedSecondaryField => 2,
        _ => 1,
    };
    for _ in 0..with_drf_fields {
        fields.extend(&with_drf);
    }
    let mut records = vec![(
        inline_entity(
            id(1),
            ["tolerance", "CATTPSToleranceWithDRF"],
            declarations,
            options,
        ),
        instance(
            fields,
            2 + with_drf_fields
                + u8::from(matches!(options.case, FixtureCase::InstanceBlobDecoy))
                + u8::from(matches!(options.case, FixtureCase::DuplicateField)),
        ),
    )];
    records.push(primitive(
        id(2),
        id(if matches!(options.case, FixtureCase::WrongOwner) {
            4
        } else {
            1
        }),
        "TolNumValue",
        Some(if matches!(options.case, FixtureCase::NegativeMain) {
            -0.762
        } else {
            options.main_value
        }),
        None,
        options,
    ));
    records.push(primitive(
        id(3),
        id(1),
        "TolTabValue",
        None,
        matches!(options.case, FixtureCase::Tabulated).then_some(".005/.25X.25"),
        options,
    ));
    let transitions = [
        1,
        if matches!(options.case, FixtureCase::MalformedTransition) {
            3
        } else if matches!(options.case, FixtureCase::CommonDatums) {
            2
        } else {
            1
        },
    ];
    let mut frame_entity = inline_entity(
        id(4),
        ["frame", "CATTPSReferenceFrame"],
        scalar_declaration(
            ["FrameBloc", "CATTPSFrameData"],
            &transitions,
            Some(key(id(10))),
            options,
        ),
        options,
    );
    if matches!(options.case, FixtureCase::AmbiguousSecondaryKey) {
        let mut header = vec![0xea];
        header.extend(id(4).to_le_bytes());
        header.extend([0x81, 0x80]);
        let at = frame_entity
            .windows(header.len())
            .position(|window| window == header.as_slice())
            .expect("frame identity header")
            + header.len();
        frame_entity[at..at + 4].copy_from_slice(&key(id(12)).to_le_bytes());
    }
    records.push((
        frame_entity,
        instance(
            instance_field(
                ["FrameBloc", "CATTPSFrameData"],
                &[Some(id(5)), Some(id(7)), Some(id(9))],
                &[],
                options,
            ),
            1,
        ),
    ));
    for (datum, label) in [(5, "A"), (7, "B"), (9, "C")] {
        let mut fields = instance_field(
            ["TTRSBloc", "CATTPSTTRSData"],
            &[Some(id(11))],
            &[],
            options,
        );
        fields.extend(instance_field(
            ["LabelBloc", "CATTPSCkeData"],
            &[],
            &[Some(id(datum + 1))],
            options,
        ));
        records.push((
            inline_entity(id(datum), ["datum", "CATTPSDatum"], Vec::new(), options),
            instance(fields, 2),
        ));
        records.push(primitive(
            id(datum + 1),
            id(datum),
            if datum == 9 && matches!(options.case, FixtureCase::UnsupportedLastDatum) {
                "TolTabValue"
            } else {
                "Label"
            },
            None,
            Some(label),
            options,
        ));
    }
    records.push((
        inline_entity(id(11), ["aspect", "CATMmrTTRS"], Vec::new(), options),
        instance(Vec::new(), 0),
    ));
    records.push(primitive(
        id(12),
        id(1),
        "RefTolNumValue",
        Some(if options.secondary > 0 { 0.127 } else { 0.05 }),
        None,
        options,
    ));
    records.push(primitive(
        id(13),
        id(1),
        "RefTolTabValue",
        None,
        None,
        options,
    ));
    let mut bytes = Vec::new();
    let mut objects = Vec::new();
    if matches!(options.case, FixtureCase::MultipleTargets) {
        records.push((
            inline_entity(id(14), ["aspect", "CATMmrTTRS"], Vec::new(), options),
            instance(Vec::new(), 0),
        ));
    }
    for (entity, object) in records {
        bytes.extend(frame(5, entity));
        objects.extend(frame(9, object));
    }
    bytes.push(0xde);
    bytes.extend(frame(8, objects));
    let mut catalog = vec![u8::try_from(CATALOG.len() + 0x81).expect("small catalog")];
    for value in CATALOG {
        catalog.push(u8::try_from(value.len() + 1).expect("short catalog entry"));
        catalog.extend(value.as_bytes());
    }
    bytes.extend(frame(2, catalog));
    bytes
}

fn decode(bytes: &[u8]) -> (CadIr, super::Transfer) {
    let native = CatiaNative::decode_with_record_ranges(bytes, &[]);
    let before = native.clone();
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let stats = crate::test_support::with_service_context(|ctx| {
        transfer(
            ctx,
            View::over_retained(bytes),
            &mut ir,
            &mut annotations,
            &native,
            &ModelingGraphScope::Unscoped,
        )
        .expect("source-bound PMI")
    });
    assert_eq!(native, before);
    (ir, stats)
}

#[test]
fn transfers_length_magnitude_ordered_datums_and_source_aspect_across_encodings() {
    for (base, wide, compact_schema) in
        [(0, false, true), (256, false, false), (20_000, true, true)]
    {
        let bytes = fixture(Options {
            base,
            wide,
            compact_schema,
            ..Options::default()
        });
        let (ir, stats) = decode(&bytes);
        assert_eq!(stats.main_tolerances, 1);
        assert_eq!(stats.secondary_tolerances, 0);
        let annotation = ir
            .model
            .pmi
            .iter()
            .find(|p| matches!(p.definition, PmiDefinition::GeometricTolerance { .. }))
            .expect("geometric tolerance");
        let PmiDefinition::GeometricTolerance {
            tolerance,
            magnitude,
            datum_system: Some(system),
            ..
        } = &annotation.definition
        else {
            panic!("tolerance with datum system");
        };
        assert_eq!(tolerance, &GeometricToleranceKind::Parallelism);
        assert!((magnitude.get().value.get() - 0.762).abs() < VALUE_TOLERANCE);
        let native = CatiaNative::decode_with_record_ranges(&bytes, &[]);
        let expected_source_id = native
            .entity_records
            .iter()
            .find(|record| record.entity_id == base + 11)
            .expect("selected source TTRS")
            .id
            .as_str();
        assert!(matches!(
            annotation.targets.as_slice(),
            [PmiTarget::ShapeAspect { source_id }]
                if source_id.as_str() == expected_source_id
        ));
        assert_eq!(stats.unresolved_bindings, 4);
        assert_eq!(stats.unresolved_target_bindings, 0);
        let system = ir
            .model
            .pmi
            .iter()
            .find(|p| &p.id == system)
            .expect("emitted system");
        let PmiDefinition::DatumSystem { references } = &system.definition else {
            panic!("datum system");
        };
        let labels = references
            .as_slice()
            .iter()
            .map(|r| {
                let datum = ir
                    .model
                    .pmi
                    .iter()
                    .find(|p| p.id == r.datum)
                    .expect("datum");
                let PmiDefinition::Datum { identification } = &datum.definition else {
                    panic!("datum definition");
                };
                (identification.as_str(), r.precedence.get(), r.common_group)
            })
            .collect::<Vec<_>>();
        assert_eq!(labels, [("A", 1, None), ("B", 2, None), ("C", 3, None)]);
    }
}

#[test]
fn active_secondary_reuses_source_target_and_inactive_defaults_stay_native() {
    for active in [0, 1, 3] {
        let (ir, stats) = decode(&fixture(Options {
            subtype: 22,
            secondary: active,
            ..Options::default()
        }));
        assert_eq!(stats.main_tolerances, 1);
        assert_eq!(stats.secondary_tolerances, usize::from(active > 0));
        assert_eq!(stats.unresolved_composite_links, usize::from(active > 0));
        let tolerances = ir
            .model
            .pmi
            .iter()
            .filter(|p| matches!(p.definition, PmiDefinition::GeometricTolerance { .. }))
            .collect::<Vec<_>>();
        assert_eq!(tolerances.len(), if active > 0 { 2 } else { 1 });
        if active > 0 {
            assert_eq!(tolerances[0].targets, tolerances[1].targets);
            let PmiDefinition::GeometricTolerance {
                magnitude,
                datum_system: Some(system),
                ..
            } = &tolerances[1].definition
            else {
                panic!("secondary tolerance");
            };
            assert!((magnitude.get().value.get() - 0.127).abs() < VALUE_TOLERANCE);
            let frame = ir
                .model
                .pmi
                .iter()
                .find(|p| &p.id == system)
                .expect("secondary frame");
            let PmiDefinition::DatumSystem { references } = &frame.definition else {
                panic!("secondary datum system");
            };
            assert_eq!(
                references.as_slice().len(),
                usize::try_from(active).expect("small count")
            );
        }
    }
}

#[test]
fn common_compartment_keeps_shared_precedence() {
    let (ir, stats) = decode(&fixture(Options {
        case: FixtureCase::CommonDatums,
        ..Options::default()
    }));
    assert_eq!(stats.main_tolerances, 1);
    let frame = ir
        .model
        .pmi
        .iter()
        .find_map(|p| match &p.definition {
            PmiDefinition::DatumSystem { references } => Some(references),
            _ => None,
        })
        .expect("datum frame");
    assert_eq!(
        frame
            .as_slice()
            .iter()
            .map(|r| (r.precedence.get(), r.common_group))
            .collect::<Vec<_>>(),
        [(1, None), (2, Some(2)), (2, Some(2))]
    );
}

#[test]
fn unknown_subtype_owner_key_duplicate_and_negative_stay_native() {
    for options in [
        Options {
            subtype: 99,
            ..Options::default()
        },
        Options {
            case: FixtureCase::WrongOwner,
            ..Options::default()
        },
        Options {
            case: FixtureCase::WrongMainKey,
            ..Options::default()
        },
        Options {
            case: FixtureCase::DuplicateField,
            ..Options::default()
        },
        Options {
            case: FixtureCase::NegativeMain,
            ..Options::default()
        },
    ] {
        let (ir, stats) = decode(&fixture(options));
        assert!(ir.model.pmi.is_empty());
        assert_eq!(stats.annotations, 0);
        assert_eq!(stats.unresolved_candidates, 1);
    }
}

#[test]
fn blob_containing_a_complete_tolerance_field_does_not_create_a_duplicate() {
    let (_, stats) = decode(&fixture(Options {
        case: FixtureCase::InstanceBlobDecoy,
        ..Options::default()
    }));
    assert_eq!(stats.main_tolerances, 1);
}

#[test]
fn source_frame_mismatch_rejects_retained_native_values() {
    let bytes = fixture(Options::default());
    let native = CatiaNative::decode_with_record_ranges(&bytes, &[]);
    let mut mismatch = bytes.clone();
    mismatch[2..6].copy_from_slice(&6_u32.to_le_bytes());
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let stats = crate::test_support::with_service_context(|ctx| {
        transfer(
            ctx,
            View::over_retained(&mismatch),
            &mut ir,
            &mut annotations,
            &native,
            &ModelingGraphScope::Unscoped,
        )
        .expect("withheld mismatched frame")
    });
    assert_eq!(stats.annotations, 0);
    assert!(ir.model.pmi.is_empty());
}

#[test]
fn characteristic_domain_is_scoped_and_does_not_use_instance_names() {
    assert_eq!(
        characteristic("CATTPSToleranceWithDRF", 6),
        Some(GeometricToleranceKind::Parallelism)
    );
    assert_eq!(
        characteristic("CATTPSToleranceWithoutDRF", 6),
        Some(GeometricToleranceKind::Flatness)
    );
    assert_eq!(
        characteristic("CATTPSToleranceWithDRF", 22),
        Some(GeometricToleranceKind::SurfaceProfile)
    );
    assert!(characteristic("CATTPSToleranceWithDRF", 16).is_none());
}

#[test]
fn every_characteristic_row_of_the_tolerance_table_is_admitted() {
    use GeometricToleranceKind::{
        Angularity, Flatness, Parallelism, Perpendicularity, Position, SurfaceProfile,
    };
    for (kind, value, expected) in [
        ("CATTPSToleranceWithDRF", 0, Angularity),
        ("CATTPSToleranceWithDRF", 3, Perpendicularity),
        ("CATTPSToleranceWithDRF", 6, Parallelism),
        ("CATTPSToleranceWithDRF", 9, Position),
        ("CATTPSToleranceWithDRF", 11, Position),
        ("CATTPSToleranceWithDRF", 12, Position),
        ("CATTPSToleranceWithDRF", 21, SurfaceProfile),
        ("CATTPSToleranceWithDRF", 22, SurfaceProfile),
        ("CATTPSToleranceWithoutDRF", 6, Flatness),
        ("CATTPSToleranceWithoutDRF", 16, SurfaceProfile),
    ] {
        assert_eq!(
            characteristic(kind, value),
            Some(expected),
            "{kind} {value}"
        );
    }
}

#[test]
fn no_tolerance_does_not_allocate_under_zero_resource_limits() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty source");
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let stats = transfer(
        &ctx,
        View::over_retained(&[]),
        &mut ir,
        &mut annotations,
        &CatiaNative::default(),
        &ModelingGraphScope::Unscoped,
    )
    .expect("no allocation");
    assert_eq!(stats.annotations, 0);
}

#[test]
fn contradictory_inline_field_count_withholds_the_tolerance() {
    let mut bytes = fixture(Options::default());
    let mut native = CatiaNative::decode_with_record_ranges(&bytes, &[]);
    let object = &mut native.object_graphs[0].records[0];
    object.inline_body.as_mut().expect("inline object")[2] += 1;
    let at = usize::try_from(object.byte_offset).expect("fixture offset") + 8;
    bytes[at] += 1;
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let stats = crate::test_support::with_service_context(|ctx| {
        transfer(
            ctx,
            View::over_retained(&bytes),
            &mut ir,
            &mut annotations,
            &native,
            &ModelingGraphScope::Unscoped,
        )
        .expect("contradictory field count")
    });
    assert_eq!(stats.annotations, 0);
    assert!(ir.model.pmi.is_empty());
}

#[test]
fn conflicting_existing_dimension_is_preserved_and_cannot_supply_a_datum() {
    use cadmpeg_ir::pmi::{DimensionKind, PmiAnnotation, PmiDimension, PmiQuantity, PmiValue};
    let bytes = fixture(Options::default());
    let native = CatiaNative::decode_with_record_ranges(&bytes, &[]);
    let datum = native
        .entity_records
        .iter()
        .find(|e| e.entity_id == 5)
        .expect("datum source");
    let existing = PmiAnnotation {
        id: super::identifier(datum.byte_offset, "datum", 0).expect("fixture identity"),
        name: Some("existing dimension".to_owned()),
        visible: None,
        targets: Vec::new(),
        definition: PmiDefinition::Dimension(
            PmiDimension::new(
                DimensionKind::Size,
                Some(PmiValue::new(35.0, PmiQuantity::Length).expect("finite length")),
                None,
            )
            .expect("dimension"),
        ),
    };
    let mut ir = CadIr::empty();
    ir.model.pmi.push(existing.clone());
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let stats = crate::test_support::with_service_context(|ctx| {
        transfer(
            ctx,
            View::over_retained(&bytes),
            &mut ir,
            &mut annotations,
            &native,
            &ModelingGraphScope::Unscoped,
        )
        .expect("collision leaves source native")
    });
    assert_eq!(stats.main_tolerances, 1);
    assert_eq!(stats.unresolved_identity_collisions, 1);
    assert_eq!(stats.unresolved_datum_systems, 1);
    assert_eq!(ir.model.pmi[0], existing);
    assert!(matches!(
        ir.model.pmi[1].definition,
        PmiDefinition::GeometricTolerance {
            datum_system: None,
            ..
        }
    ));
}

#[test]
fn retained_pmi_is_idempotent_and_carries_source_provenance() {
    let bytes = fixture(Options::default());
    let native = CatiaNative::decode_with_record_ranges(&bytes, &[]);
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::test_support::with_service_context(|ctx| {
        let first = transfer(
            ctx,
            View::over_retained(&bytes),
            &mut ir,
            &mut annotations,
            &native,
            &ModelingGraphScope::Unscoped,
        )
        .expect("first transfer");
        assert_eq!(first.main_tolerances, 1);
        let before = ir.clone();
        let second = transfer(
            ctx,
            View::over_retained(&bytes),
            &mut ir,
            &mut annotations,
            &native,
            &ModelingGraphScope::Unscoped,
        )
        .expect("repeated transfer");
        assert_eq!(second.annotations, 0);
        assert_eq!(second.unresolved_identity_collisions, 0);
        assert_eq!(second.unresolved_candidates, 0);
        assert_eq!(ir, before);
    });
    let annotations = annotations.build();
    assert_eq!(annotations.provenance.len(), ir.model.pmi.len());
    assert!(ir
        .model
        .pmi
        .iter()
        .all(|p| annotations.provenance.contains_key(p.id.as_str())));
}

#[test]
fn entity_limit_refuses_before_mutating_the_model() {
    let bytes = fixture(Options::default());
    let native = CatiaNative::decode_with_record_ranges(&bytes, &[]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("bounded source");
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let error = transfer(
        &ctx,
        View::over_retained(&bytes),
        &mut ir,
        &mut annotations,
        &native,
        &ModelingGraphScope::Unscoped,
    )
    .err()
    .expect("entity refusal");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    assert!(ir.model.pmi.is_empty());
}

#[test]
fn active_secondary_survives_a_negative_main_with_the_same_source_joins() {
    let (ir, stats) = decode(&fixture(Options {
        case: FixtureCase::NegativeMain,
        secondary: 1,
        subtype: 22,
        ..Options::default()
    }));
    assert_eq!(stats.main_tolerances, 0);
    assert_eq!(stats.secondary_tolerances, 1);
    assert_eq!(stats.unresolved_candidates, 1);
    let tolerance = ir
        .model
        .pmi
        .iter()
        .find_map(|p| match &p.definition {
            PmiDefinition::GeometricTolerance {
                tolerance,
                magnitude,
                ..
            } => Some((tolerance, magnitude)),
            _ => None,
        })
        .expect("source-closed secondary");
    assert_eq!(tolerance.0, &GeometricToleranceKind::SurfaceProfile);
    assert!((tolerance.1.get().value.get() - 0.127).abs() < VALUE_TOLERANCE);
}

#[test]
fn secondary_value_survives_without_a_system_that_would_split_a_common_compartment() {
    let (ir, stats) = decode(&fixture(Options {
        case: FixtureCase::CommonDatums,
        secondary: 2,
        subtype: 22,
        ..Options::default()
    }));
    assert_eq!(stats.main_tolerances, 1);
    assert_eq!(stats.secondary_tolerances, 1);
    assert_eq!(stats.unresolved_datum_systems, 1);
    assert_eq!(
        ir.model
            .pmi
            .iter()
            .filter(|p| matches!(p.definition, PmiDefinition::GeometricTolerance { .. }))
            .count(),
        2
    );
}

#[test]
fn secondary_metadata_key_binds_its_frame_or_reports_unresolved_meaning() {
    for unrelated in [false, true] {
        let (_, stats) = decode(&fixture(Options {
            secondary: 1,
            case: if unrelated {
                FixtureCase::UnrelatedSecondaryKey
            } else {
                FixtureCase::SecondaryFrameKey
            },
            ..Options::default()
        }));
        assert_eq!(stats.main_tolerances, 1);
        assert_eq!(stats.secondary_tolerances, 1);
        assert_eq!(stats.unresolved_field_keys, usize::from(unrelated));
    }
}

#[test]
fn ambiguous_or_unresolvable_secondary_key_members_keep_the_secondary() {
    for (case, unresolved_keys) in [
        (FixtureCase::AmbiguousSecondaryKey, 1),
        (FixtureCase::UnresolvableSecondaryMember, 1),
    ] {
        let (_, stats) = decode(&fixture(Options {
            secondary: 1,
            case,
            ..Options::default()
        }));
        assert_eq!(stats.main_tolerances, 1);
        assert_eq!(stats.secondary_tolerances, 1);
        assert_eq!(stats.unresolved_field_keys, unresolved_keys);
        assert_eq!(stats.unresolved_secondaries, 0);
    }
}

#[test]
fn unreadable_secondary_declaration_charges_an_untransferred_secondary() {
    let (_, stats) = decode(&fixture(Options {
        secondary: 1,
        case: FixtureCase::MalformedSecondaryKey,
        ..Options::default()
    }));
    assert_eq!(stats.main_tolerances, 1);
    assert_eq!(stats.secondary_tolerances, 0);
    assert_eq!(stats.unresolved_secondaries, 1);
}

#[test]
fn missing_or_repeated_secondary_field_charges_an_untransferred_secondary() {
    for case in [
        FixtureCase::MissingSecondaryField,
        FixtureCase::RepeatedSecondaryField,
    ] {
        let (_, stats) = decode(&fixture(Options {
            secondary: 1,
            case,
            ..Options::default()
        }));
        assert_eq!(stats.main_tolerances, 1);
        assert_eq!(stats.secondary_tolerances, 0);
        assert_eq!(stats.unresolved_secondaries, 1);
    }
}

#[test]
fn unreadable_secondary_magnitude_charges_an_untransferred_secondary() {
    let (_, stats) = decode(&fixture(Options {
        secondary: 1,
        secondary_literal: Some(0x55),
        ..Options::default()
    }));
    assert_eq!(stats.main_tolerances, 1);
    assert_eq!(stats.secondary_tolerances, 0);
    assert_eq!(stats.unresolved_secondaries, 1);
}

#[test]
fn exactness_omits_an_absent_datum_system() {
    let (mut ir, _) = decode(&fixture(Options::default()));
    let annotation = ir
        .model
        .pmi
        .iter_mut()
        .find(|p| matches!(p.definition, PmiDefinition::GeometricTolerance { .. }))
        .expect("tolerance");
    let PmiDefinition::GeometricTolerance { datum_system, .. } = &mut annotation.definition else {
        panic!("tolerance definition");
    };
    *datum_system = None;
    assert!(!super::exact_fields(annotation)
        .iter()
        .any(|(field, _)| *field == "definition.datum_system"));
}

#[test]
fn admitted_8193_trailer_and_unsupported_trailer_are_distinct() {
    let (_, admitted) = decode(&fixture(Options {
        trailer: 0x93,
        ..Options::default()
    }));
    assert_eq!(admitted.main_tolerances, 1);
    let (ir, unsupported) = decode(&fixture(Options {
        trailer: 0x55,
        secondary: 1,
        ..Options::default()
    }));
    assert_eq!(unsupported.annotations, 0);
    assert!(ir.model.pmi.is_empty());
}

#[test]
fn every_admitted_magnitude_trailer_transfers_the_tolerance() {
    for trailer in [0x92, 0x93, 0x9b, 0xdb] {
        let (ir, stats) = decode(&fixture(Options {
            trailer,
            ..Options::default()
        }));
        assert_eq!(stats.main_tolerances, 1, "trailer 81 {trailer:02x}");
        assert!(
            ir.model.pmi.iter().any(|p| matches!(
                p.definition,
                PmiDefinition::GeometricTolerance {
                    datum_system: Some(_),
                    ..
                }
            )),
            "trailer 81 {trailer:02x} keeps the datum labels"
        );
    }
}

#[test]
fn keyless_main_declaration_transfers_the_tolerance() {
    let (ir, stats) = decode(&fixture(Options {
        case: FixtureCase::KeylessMain,
        ..Options::default()
    }));
    assert_eq!(stats.main_tolerances, 1);
    assert!(ir.model.pmi.iter().any(|p| matches!(
        &p.definition,
        PmiDefinition::GeometricTolerance { magnitude, .. }
            if (magnitude.get().value.get() - 0.762).abs() < VALUE_TOLERANCE
    )));
}

#[test]
fn unsupported_magnitude_trailer_alone_withholds_the_tolerance() {
    let (ir, stats) = decode(&fixture(Options {
        magnitude_trailer: Some(0x55),
        ..Options::default()
    }));
    assert_eq!(stats.main_tolerances, 0);
    assert!(!ir
        .model
        .pmi
        .iter()
        .any(|p| matches!(p.definition, PmiDefinition::GeometricTolerance { .. })));
}

#[test]
fn source_closed_length_tolerances_decode_one_and_zero_control_values() {
    for (literal, expected) in [(0xe7, 1.0), (0xe8, 0.0)] {
        let (ir, stats) = decode(&fixture(Options {
            main_literal: Some(literal),
            ..Options::default()
        }));
        assert_eq!(stats.main_tolerances, 1);
        let magnitude = ir
            .model
            .pmi
            .iter()
            .find_map(|p| match &p.definition {
                PmiDefinition::GeometricTolerance { magnitude, .. } => {
                    Some(magnitude.get().value.get())
                }
                _ => None,
            })
            .expect("control-valued magnitude");
        assert!((magnitude - expected).abs() < VALUE_TOLERANCE);
    }
}

#[test]
fn control_valued_magnitudes_are_derived_and_binary64_magnitudes_are_byte_exact() {
    for (literal, expected) in [
        (None, Exactness::ByteExact),
        (Some(0xe7), Exactness::Derived),
        (Some(0xe8), Exactness::Derived),
    ] {
        let bytes = fixture(Options {
            main_literal: literal,
            ..Options::default()
        });
        let native = CatiaNative::decode_with_record_ranges(&bytes, &[]);
        let mut ir = CadIr::empty();
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        crate::test_support::with_service_context(|ctx| {
            transfer(
                ctx,
                View::over_retained(&bytes),
                &mut ir,
                &mut annotations,
                &native,
                &ModelingGraphScope::Unscoped,
            )
            .expect("source-bound PMI")
        });
        let tolerance = ir
            .model
            .pmi
            .iter()
            .find(|p| matches!(p.definition, PmiDefinition::GeometricTolerance { .. }))
            .expect("geometric tolerance");
        let annotations = annotations.build();
        let note = annotations
            .exactness()
            .get(tolerance.id.as_str())
            .expect("tolerance exactness");
        assert_eq!(
            note.fields().get("definition.magnitude").copied(),
            Some(expected)
        );
    }
}

#[test]
fn zero_secondary_preserves_unknown_field_key_with_an_explicit_loss_count() {
    let (ir, stats) = decode(&fixture(Options {
        secondary: 1,
        subtype: 12,
        secondary_literal: Some(0xe8),
        case: FixtureCase::UnrelatedSecondaryKey,
        ..Options::default()
    }));
    assert_eq!(stats.main_tolerances, 1);
    assert_eq!(stats.secondary_tolerances, 1);
    assert_eq!(stats.unresolved_field_keys, 1);
    assert!(ir.model.pmi.iter().any(|p| match &p.definition {
        PmiDefinition::GeometricTolerance { magnitude, .. } =>
            magnitude.get().value.get().abs() < VALUE_TOLERANCE,
        _ => false,
    }));
}

#[test]
fn malformed_secondary_metadata_key_does_not_become_an_unresolved_key() {
    let (_, stats) = decode(&fixture(Options {
        secondary: 1,
        case: FixtureCase::MalformedSecondaryKey,
        ..Options::default()
    }));
    assert_eq!(stats.main_tolerances, 1);
    assert_eq!(stats.secondary_tolerances, 0);
    assert_eq!(stats.unresolved_field_keys, 0);
}

fn declaration_fixture_with_decoy(
    prefix: &[u8],
    fake_tail: &[u8],
) -> (
    crate::native::entity_record::CatiaEntityRecord,
    super::Field<'static>,
) {
    let bytes = fixture(Options::default());
    let native = CatiaNative::decode_with_record_ranges(&bytes, &[]);
    let mut entity = native
        .entity_records
        .iter()
        .find(|e| e.entity_id == 1)
        .expect("tolerance source")
        .clone();
    let names = ["TolBloc", "CATTPSToleranceData"];
    let mut raw = prefix.to_vec();
    raw.extend(scalar_declaration(
        names,
        &[99, 0, 0, 0, 0],
        Some(key(2)),
        Options::default(),
    ));
    raw.extend(fake_tail);
    raw.extend(scalar_declaration(
        names,
        &[6, 0, 0, 1, 9],
        Some(key(2)),
        Options::default(),
    ));
    raw.extend(field_pair(
        ["TolWiDRF", "CATTPSToleranceWiDRFData"],
        Options::default(),
    ));
    entity.body = crate::native::entity_record::CatiaEntityRecordBody::Inline(inline_entity(
        entity.entity_id,
        ["tolerance", "CATTPSToleranceWithDRF"],
        raw,
        Options::default(),
    ));
    let mut schemas = [[0; 2]; super::MAX_INLINE_VALUES];
    let selected_pair = field_pair(names, Options::default());
    let mut selected = View::over_retained(&selected_pair);
    let schema = [
        super::schema(&mut selected).expect("field schema"),
        super::schema(&mut selected).expect("field storage"),
    ];
    let next_pair = field_pair(["TolWiDRF", "CATTPSToleranceWiDRFData"], Options::default());
    let mut next = View::over_retained(&next_pair);
    schemas[0] = schema;
    schemas[1] = [
        super::schema(&mut next).expect("next schema"),
        super::schema(&mut next).expect("next storage"),
    ];
    let empty = super::Group {
        view: View::over_retained(&[]),
        count: 0,
    };
    (
        entity,
        super::Field {
            schema,
            schemas,
            schema_count: 2,
            schema_position: 0,
            references: empty,
            parameters: empty,
        },
    )
}

#[test]
fn paged_operand_continuation_cannot_start_a_scalar_declaration() {
    let next = field_pair(["TolWiDRF", "CATTPSToleranceWiDRFData"], Options::default());
    let (entity, field) = declaration_fixture_with_decoy(&[0xd1], &next);
    let selected =
        super::scalar_declaration(&entity, &field).expect("real declaration after paged operand");
    assert_eq!(selected.values[0], 6);
}

#[test]
fn complete_inline_items_hide_declaration_prefix_bytes() {
    let next = field_pair(["TolWiDRF", "CATTPSToleranceWiDRFData"], Options::default());
    let (entity, field) = declaration_fixture_with_decoy(&[0x8e, 0xef, 0x84], &next);
    assert_eq!(
        super::scalar_declaration(&entity, &field)
            .expect("real declaration after inline packet")
            .values[0],
        6
    );
}

#[test]
fn inline_identity_payload_does_not_start_a_declaration_token() {
    for code in [0xe5_u32, 0xe6, 0x80, 0x32] {
        let (_, stats) = decode(&fixture(Options {
            base: code - 4,
            ..Options::default()
        }));
        assert_eq!(stats.main_tolerances, 1);
    }
}

#[test]
fn tabulated_display_preserves_the_independent_numeric_magnitude() {
    let (ir, stats) = decode(&fixture(Options {
        main_value: 0.127,
        case: FixtureCase::Tabulated,
        ..Options::default()
    }));
    assert_eq!(stats.main_tolerances, 1);
    assert_eq!(stats.unresolved_semantics, 1);
    let magnitude = ir
        .model
        .pmi
        .iter()
        .find_map(|p| match &p.definition {
            PmiDefinition::GeometricTolerance { magnitude, .. } => {
                Some(magnitude.get().value.get())
            }
            _ => None,
        })
        .expect("typed numeric magnitude");
    assert!((magnitude - 0.127).abs() < VALUE_TOLERANCE);
}

#[test]
fn unresolved_scope_and_another_graph_cannot_supply_pmi() {
    let bytes = fixture(Options::default());
    let native = CatiaNative::decode_with_record_ranges(&bytes, &[]);
    for scope in [
        ModelingGraphScope::Unresolved,
        ModelingGraphScope::Scoped("another graph".to_owned()),
    ] {
        let mut ir = CadIr::empty();
        let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
        let stats = crate::test_support::with_service_context(|ctx| {
            transfer(
                ctx,
                View::over_retained(&bytes),
                &mut ir,
                &mut annotations,
                &native,
                &scope,
            )
            .expect("source scope")
        });
        assert_eq!(stats.annotations, 0);
        assert!(ir.model.pmi.is_empty());
    }
}

#[test]
fn another_model_arena_reports_a_generated_tolerance_id_collision() {
    let bytes = fixture(Options::default());
    let native = CatiaNative::decode_with_record_ranges(&bytes, &[]);
    let datum = native
        .entity_records
        .iter()
        .find(|e| e.entity_id == 1)
        .expect("tolerance source");
    let id =
        super::identifier(datum.byte_offset, "geometric-tolerance", 0).expect("tolerance identity");
    let mut ir = CadIr::empty();
    ir.model.points.push(cadmpeg_ir::topology::Point::new(
        cadmpeg_ir::ids::PointId::mint(id.as_str().to_owned()).expect("fixture identity"),
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(1.0, 2.0, 3.0))
            .expect("finite point"),
        None,
    ));
    let before = ir.clone();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let stats = crate::test_support::with_service_context(|ctx| {
        transfer(
            ctx,
            View::over_retained(&bytes),
            &mut ir,
            &mut annotations,
            &native,
            &ModelingGraphScope::Unscoped,
        )
        .expect("cross-arena collision")
    });
    assert_eq!(stats.annotations, 0);
    assert_eq!(stats.unresolved_identity_collisions, 1);
    assert_eq!(stats.unresolved_candidates, 1);
    assert_eq!(ir, before);
}

#[test]
fn conflicting_existing_tolerance_reports_a_collision_and_preserves_the_model() {
    let bytes = fixture(Options::default());
    let native = CatiaNative::decode_with_record_ranges(&bytes, &[]);
    let (mut ir, _) = decode(&bytes);
    let tolerance = ir
        .model
        .pmi
        .iter_mut()
        .find_map(|p| match &mut p.definition {
            PmiDefinition::GeometricTolerance { magnitude, .. } => Some(magnitude),
            _ => None,
        })
        .expect("existing tolerance");
    *tolerance = cadmpeg_ir::pmi::PmiMagnitude::new(
        cadmpeg_ir::pmi::PmiValue::new(2.0, cadmpeg_ir::pmi::PmiQuantity::Length)
            .expect("finite length"),
    )
    .expect("nonnegative magnitude");
    let before = ir.clone();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let stats = crate::test_support::with_service_context(|ctx| {
        transfer(
            ctx,
            View::over_retained(&bytes),
            &mut ir,
            &mut annotations,
            &native,
            &ModelingGraphScope::Unscoped,
        )
        .expect("reported collision")
    });
    assert_eq!(stats.annotations, 0);
    assert_eq!(stats.unresolved_identity_collisions, 1);
    assert_eq!(stats.unresolved_candidates, 1);
    assert_eq!(ir, before);
}

#[test]
fn provenance_retention_refusal_keeps_model_and_annotation_tables_unchanged() {
    use cadmpeg_ir::pmi::DatumReference;
    let bytes = fixture(Options::default());
    let native = CatiaNative::decode_with_record_ranges(&bytes, &[]);
    let (admitted, _) = decode(&bytes);
    let mut model_bytes = 0;
    for pmi in &admitted.model.pmi {
        model_bytes +=
            std::mem::size_of::<cadmpeg_ir::pmi::PmiAnnotation>() + pmi.id.as_str().len();
        for target in &pmi.targets {
            model_bytes += std::mem::size_of::<PmiTarget>();
            if let PmiTarget::ShapeAspect { source_id } = target {
                model_bytes += source_id.as_str().len();
            }
        }
        match &pmi.definition {
            PmiDefinition::Datum { identification } => model_bytes += identification.len(),
            PmiDefinition::DatumSystem { references } => {
                for reference in references.as_slice() {
                    model_bytes +=
                        std::mem::size_of::<DatumReference>() + reference.datum.as_str().len();
                }
            }
            PmiDefinition::GeometricTolerance {
                datum_system: Some(system),
                ..
            } => model_bytes += system.as_str().len(),
            _ => {}
        }
    }
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(model_bytes).expect("bounded model size");
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("bounded source");
    let mut ir = CadIr::empty();
    let before = ir.clone();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let error = transfer(
        &ctx,
        View::over_retained(&bytes),
        &mut ir,
        &mut annotations,
        &native,
        &ModelingGraphScope::Unscoped,
    )
    .err()
    .expect("provenance needs additional retained bytes");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
    assert_eq!(ir, before);
    assert_eq!(annotations.build(), cadmpeg_ir::Annotations::default());
}

#[test]
fn actual_output_budget_admits_incremental_staging_and_reuse_at_zero_remaining_budget() {
    let bytes = fixture(Options {
        secondary: 1,
        ..Options::default()
    });
    let native = CatiaNative::decode_with_record_ranges(&bytes, &[]);
    let (expected, expected_stats) = decode(&bytes);
    let retained = expected
        .model
        .pmi
        .iter()
        .map(|p| super::retained_size(p).expect("output size"))
        .sum::<usize>();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_entities = u64::try_from(expected.model.pmi.len()).expect("output count");
    policy.limits.max_retained_bytes = u64::try_from(retained).expect("output bytes");
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("bounded source");
    let mut ir = CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    let first = transfer(
        &ctx,
        View::over_retained(&bytes),
        &mut ir,
        &mut annotations,
        &native,
        &ModelingGraphScope::Unscoped,
    )
    .expect("actual output budget");
    assert_eq!(first.annotations, expected_stats.annotations);
    assert_eq!(ir, expected);
    let before = annotations.build();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::resume(before.clone());
    let second = transfer(
        &ctx,
        View::over_retained(&bytes),
        &mut ir,
        &mut annotations,
        &native,
        &ModelingGraphScope::Unscoped,
    )
    .expect("identical records consume no retained budget");
    assert_eq!(second.annotations, 0);
    assert_eq!(second.unresolved_candidates, 0);
    assert_eq!(ir, expected);
    assert_eq!(annotations.build(), before);
}

#[test]
fn closed_target_members_and_tolerance_values_survive_partial_or_multiple_rosters() {
    for (case, target_count, unresolved_roster) in [
        (FixtureCase::NullTarget, 0, 1),
        (FixtureCase::AmbiguousTargets, 1, 1),
        (FixtureCase::MultipleTargets, 2, 0),
    ] {
        let (ir, stats) = decode(&fixture(Options {
            case,
            ..Options::default()
        }));
        assert_eq!(stats.main_tolerances, 1);
        assert_eq!(stats.unresolved_candidates, 0);
        assert_eq!(stats.unresolved_target_bindings, unresolved_roster);
        assert_eq!(stats.unresolved_bindings, 3 + target_count);
        assert_eq!(stats.unresolved_datum_systems, 0);
        let annotation = ir
            .model
            .pmi
            .iter()
            .find(|p| matches!(p.definition, PmiDefinition::GeometricTolerance { .. }))
            .expect("source quantity");
        assert_eq!(annotation.targets.len(), target_count);
        if target_count == 2 {
            assert_ne!(annotation.targets[0], annotation.targets[1]);
        }
        let PmiDefinition::GeometricTolerance {
            magnitude,
            datum_system,
            ..
        } = &annotation.definition
        else {
            panic!("tolerance")
        };
        assert!((magnitude.get().value.get() - 0.762).abs() < VALUE_TOLERANCE);
        assert!(datum_system.is_some());
    }
}

#[test]
fn an_unsupported_later_datum_keeps_the_main_value_and_independently_closed_secondary_prefix() {
    let (ir, stats) = decode(&fixture(Options {
        case: FixtureCase::UnsupportedLastDatum,
        secondary: 1,
        ..Options::default()
    }));
    assert_eq!(stats.main_tolerances, 1);
    assert_eq!(stats.secondary_tolerances, 1);
    assert_eq!(stats.unresolved_datum_systems, 1);
    assert_eq!(stats.unresolved_candidates, 0);
    let tolerances = ir
        .model
        .pmi
        .iter()
        .filter_map(|p| match &p.definition {
            PmiDefinition::GeometricTolerance {
                magnitude,
                datum_system,
                ..
            } => Some((magnitude.get().value.get(), datum_system)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(tolerances.len(), 2);
    assert!(tolerances
        .iter()
        .any(|(v, s)| (*v - 0.762).abs() < VALUE_TOLERANCE && s.is_none()));
    let secondary = tolerances
        .iter()
        .find(|(v, _)| (*v - 0.127).abs() < VALUE_TOLERANCE)
        .expect("secondary value");
    let system = ir
        .model
        .pmi
        .iter()
        .find(|p| Some(&p.id) == secondary.1.as_ref())
        .expect("source prefix");
    let PmiDefinition::DatumSystem { references } = &system.definition else {
        panic!("datum system")
    };
    assert_eq!(references.as_slice().len(), 1);
}

#[test]
fn an_unknown_datum_transition_keeps_the_quantity_without_shortening_the_system() {
    let (ir, stats) = decode(&fixture(Options {
        case: FixtureCase::MalformedTransition,
        ..Options::default()
    }));
    assert_eq!(stats.main_tolerances, 1);
    assert_eq!(stats.unresolved_datum_systems, 1);
    assert!(matches!(
        ir.model.pmi.as_slice(),
        [cadmpeg_ir::pmi::PmiAnnotation {
            definition: PmiDefinition::GeometricTolerance {
                datum_system: None,
                ..
            },
            ..
        }]
    ));
}

#[test]
fn partial_tolerance_transfer_is_idempotent_and_resource_safe() {
    for case in [FixtureCase::NullTarget, FixtureCase::UnsupportedLastDatum] {
        let bytes = fixture(Options {
            case,
            secondary: 1,
            ..Options::default()
        });
        let native = CatiaNative::decode_with_record_ranges(&bytes, &[]);
        let mut ir = CadIr::empty();
        let mut notes = cadmpeg_ir::AnnotationBuilder::new();
        crate::test_support::with_service_context(|ctx| {
            transfer(
                ctx,
                View::over_retained(&bytes),
                &mut ir,
                &mut notes,
                &native,
                &ModelingGraphScope::Unscoped,
            )
            .expect("partial value");
            let before = ir.clone();
            let annotations = notes.annotations().clone();
            let stats = transfer(
                ctx,
                View::over_retained(&bytes),
                &mut ir,
                &mut notes,
                &native,
                &ModelingGraphScope::Unscoped,
            )
            .expect("idempotent partial value");
            assert_eq!(stats.annotations, 0);
            assert_eq!(stats.unresolved_candidates, 0);
            assert_eq!(ir, before);
            assert_eq!(notes.annotations(), &annotations);
        });
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                .expect("source");
        let mut ir = CadIr::empty();
        let before = ir.clone();
        let mut notes = cadmpeg_ir::AnnotationBuilder::new();
        let error = transfer(
            &ctx,
            View::over_retained(&bytes),
            &mut ir,
            &mut notes,
            &native,
            &ModelingGraphScope::Unscoped,
        );
        assert!(matches!(
            error,
            Err(cadmpeg_core::CodecError::ResourceLimit(_))
        ));
        assert_eq!(ir, before);
        assert_eq!(notes.build(), cadmpeg_ir::Annotations::default());
    }
}
