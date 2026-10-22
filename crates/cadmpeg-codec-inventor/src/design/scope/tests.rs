// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

use super::{
    parse_model_root, parse_parameter_table, project_model_features, resolve_scopes,
    BrepSegmentName, BrepSegmentRole, ParameterScope, PmDcModelRoot, PmDcModelRootPayload,
    PmDcParameterTable, PmDcParameterTablePayload, FLAT_PATTERN_KIND, MODEL_ROOT_TYPE,
    PARAMETER_TABLE_TYPE,
};
use crate::pmdc::PmDcReference;
use crate::record_identity::{Located, RecordTypeId};

const DOCUMENT_SEGMENT: &str = "PmBRepSegment";
const FLAT_SEGMENT: &str = "MbBrepSegment";

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_utf16(bytes: &mut Vec<u8>, value: &str) {
    let units = value.encode_utf16().collect::<Vec<_>>();
    push_u32(
        bytes,
        u32::try_from(units.len()).expect("fixture string fits u32"),
    );
    for unit in units {
        push_u16(bytes, unit);
    }
}

/// A model-root payload whose parameter table is the one-based `table`.
fn model_root_bytes(table: u32, repeat: u32, segment: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 0);
    push_u16(&mut bytes, 2);
    push_u32(&mut bytes, 0x8000_0002);
    push_u32(&mut bytes, 0x40);
    bytes.extend_from_slice(&[0x11; 16]);
    push_utf16(&mut bytes, "Part");
    push_u16(&mut bytes, 2);
    push_u16(&mut bytes, 0x3000);
    push_u32(&mut bytes, 0);
    for index in 0..15 {
        push_u32(
            &mut bytes,
            match index {
                3 => table,
                9 => repeat,
                12 => 1,
                _ => 0,
            },
        );
    }
    push_utf16(&mut bytes, segment);
    bytes.extend_from_slice(&1.0_f64.to_le_bytes());
    push_u32(&mut bytes, 0);
    bytes
}

/// A parameter-table payload with context `context` and one named member.
fn parameter_table_bytes(context: u32, member: u32, name: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_u32(&mut bytes, 0);
    push_u16(&mut bytes, 1);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0x100);
    push_u32(&mut bytes, context);
    push_u16(&mut bytes, 2);
    push_u16(&mut bytes, 0x3000);
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, member);
    push_u16(&mut bytes, 6);
    push_u16(&mut bytes, 0x3000);
    push_u32(&mut bytes, 1);
    push_u32(&mut bytes, 0);
    push_u32(&mut bytes, 0);
    push_utf16(&mut bytes, name);
    push_u32(&mut bytes, member);
    bytes.extend_from_slice(&[0x5a; 6]);
    bytes
}

fn with_view<T>(bytes: &[u8], test: impl FnOnce(&DecodeContext<'_>, View<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let (ctx, view) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
        .expect("synthetic payload fits policy");
    test(&ctx, view)
}

fn root(ordinal: u32, table: u32, segment: &str) -> PmDcModelRoot {
    let payload = with_view(&model_root_bytes(table, table, segment), |ctx, view| {
        parse_model_root(ctx, view, 19).expect("model root parses")
    });
    located(payload, MODEL_ROOT_TYPE, ordinal)
}

fn table(ordinal: u32, context: u32) -> PmDcParameterTable {
    let payload = with_view(
        &parameter_table_bytes(context, 0x8000_0010, "Thickness"),
        |ctx, view| parse_parameter_table(ctx, view, 19).expect("parameter table parses"),
    );
    located(payload, PARAMETER_TABLE_TYPE, ordinal)
}

fn located<T>(payload: T, type_id: [u8; 16], ordinal: u32) -> Located<T> {
    Located::new(
        payload,
        RecordTypeId::from_bytes(type_id),
        cadmpeg_ir::identity_key!("segment"),
        ordinal,
    )
}

fn segments() -> Vec<BrepSegmentName> {
    vec![
        BrepSegmentName {
            display_name: DOCUMENT_SEGMENT.into(),
            role: BrepSegmentRole::Document,
        },
        BrepSegmentName {
            display_name: FLAT_SEGMENT.into(),
            role: BrepSegmentRole::FlatPattern,
        },
    ]
}

#[test]
fn model_roots_and_parameter_tables_frame_their_references() {
    let bytes = model_root_bytes(0x8000_0003, 0, DOCUMENT_SEGMENT);
    let root: PmDcModelRootPayload = with_view(&bytes, |ctx, view| {
        parse_model_root(ctx, view, 19).expect("model root with a null table repeat parses")
    });
    assert_eq!(root.name, "Part");
    assert_eq!(root.parameter_table.record_ordinal(), Some(2));
    assert_eq!(root.brep_segment, DOCUMENT_SEGMENT);
    assert_eq!(root.scale.get(), 1.0);
    let bytes = model_root_bytes(0x8000_0003, 0x8000_0004, DOCUMENT_SEGMENT);
    with_view(&bytes, |ctx, view| {
        assert!(parse_model_root(ctx, view, 19)
            .expect_err("disagreeing repeat")
            .to_string()
            .contains("repeats parameter table"));
    });

    let bytes = parameter_table_bytes(0x8000_0001, 0x8000_0010, "Thickness");
    let table: PmDcParameterTablePayload = with_view(&bytes, |ctx, view| {
        parse_parameter_table(ctx, view, 19).expect("parameter table parses")
    });
    assert_eq!(table.context.record_ordinal(), Some(0));
    assert_eq!(
        table.members.references(),
        &[PmDcReference::from_packed(0x8000_0010)]
    );
    assert_eq!(table.names.len(), 1);
    assert_eq!(table.names[0].name, "Thickness");
    assert_eq!(table.suffix_len, 6);
    let mut bytes = bytes;
    // The name-map marker follows the 18-byte header and the 20-byte member list.
    bytes[38] = 7;
    with_view(&bytes, |ctx, view| {
        assert!(parse_parameter_table(ctx, view, 19)
            .expect_err("foreign marker")
            .to_string()
            .contains("name-map marker"));
    });
}

#[test]
fn tables_take_the_scope_of_the_model_root_that_owns_them() {
    let roots = vec![
        root(0, 0x8000_0003, DOCUMENT_SEGMENT),
        root(10, 0x8000_000c, FLAT_SEGMENT),
    ];
    let tables = vec![table(2, 0x8000_0001), table(11, 0x8000_000b), table(20, 0)];
    let scopes = resolve_scopes(&roots, &tables, &segments()).expect("scopes resolve");
    let of = |context: u32, ordinal: u32, name: &str| {
        scopes.of(
            "segment",
            PmDcReference::from_packed(context),
            ordinal,
            name,
        )
    };
    assert_eq!(
        of(0x8000_0003, 15, "Thickness"),
        Some(&ParameterScope::Document)
    );
    let Some(ParameterScope::FlatPattern(owner)) = of(0x8000_000c, 15, "Thickness") else {
        panic!("flat-pattern table is owned by its model feature");
    };
    // A table with a null context, or a parameter the table does not list under
    // its own name and ordinal, has no established scope.
    assert_eq!(of(0x8000_0015, 15, "Thickness"), None);
    assert_eq!(of(0, 15, "Thickness"), None);
    assert_eq!(of(0x8000_0003, 15, "BendRadius"), None);
    assert_eq!(of(0x8000_0003, 16, "Thickness"), None);

    // The flat-pattern root projects the native feature that owns its table,
    // after the document features.
    with_view(&[], |ctx, _| {
        let features =
            project_model_features(ctx, &roots, &segments(), 7).expect("features project");
        assert_eq!(features.len(), 1);
        assert_eq!(features[0].ordinal, 7);
        assert_eq!(&features[0].id, owner);
        assert_eq!(
            features[0].native_ref.as_deref(),
            Some("inventor:pmdc:model-root#segment-10")
        );
        assert!(matches!(
            features[0].evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Native { kind, parameters })
                if kind.as_str() == FLAT_PATTERN_KIND && parameters.is_empty()
        ));
    });

    // Two document roots in one segment establish no scope and no feature.
    let ambiguous = vec![
        root(0, 0x8000_0003, DOCUMENT_SEGMENT),
        root(5, 0x8000_0003, DOCUMENT_SEGMENT),
        root(10, 0x8000_000c, FLAT_SEGMENT),
    ];
    let scopes = resolve_scopes(&ambiguous, &tables, &segments()).expect("scopes resolve");
    assert_eq!(
        scopes.of(
            "segment",
            PmDcReference::from_packed(0x8000_0003),
            15,
            "Thickness"
        ),
        None
    );
    with_view(&[], |ctx, _| {
        assert!(project_model_features(ctx, &ambiguous, &segments(), 7)
            .expect("features project")
            .is_empty());
    });
}
