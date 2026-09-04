// SPDX-License-Identifier: Apache-2.0
//! Native-namespace tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::*;
use crate::CatiaCodec;

#[test]
fn entity_suffix_values_accept_8193_trailers() {
    use crate::native::{
        CatiaEntityEvaluation, CatiaEntityEvaluationEncoding, CatiaEntitySuffixPayload,
        CatiaEntitySuffixTrailer,
    };

    let bits = 11.0_f64.to_bits();
    let mut suffix = vec![0x84, 0x96, 0x82, 0xd8, 0xe6];
    suffix.extend_from_slice(&bits.to_le_bytes());
    suffix.extend_from_slice(&[0x81, 0x93]);
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_two_selector_value(
        "Range",
        "CstAttr_Dimension",
        &suffix,
    ));
    let suffix_value = native.entity_records[0]
        .suffix_value
        .as_ref()
        .expect("81 93-terminated suffix value");
    assert_eq!(suffix_value.prefix_code, 0xd8);
    assert_eq!(suffix_value.trailer, CatiaEntitySuffixTrailer::Token8193);
    assert_eq!(
        suffix_value.payload,
        CatiaEntitySuffixPayload::Evaluation {
            opcode_offset: 4,
            evaluation: CatiaEntityEvaluation::Scalar { bits },
            encoding: CatiaEntityEvaluationEncoding::Direct,
        }
    );

    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    native
        .store(&mut namespace)
        .expect("store 81 93-terminated suffix value");
    namespace.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_SUFFIX_TRAILER_8193_VERSION - 1).unwrap(),
    );
    namespace
        .arenas
        .get_mut("entity_records")
        .expect("stored entity records")[0]
        .fields_mut()
        .remove("suffix_value");
    let migrated = crate::native::CatiaNative::load(&namespace)
        .expect("migrate 81 93-terminated suffix value");
    assert_eq!(
        migrated.entity_records[0].suffix_value.as_ref(),
        Some(suffix_value)
    );
}

#[test]
fn entity_suffix_values_accept_8192_trailers() {
    use crate::native::{
        CatiaEntityEvaluation, CatiaEntityEvaluationEncoding, CatiaEntitySuffixPayload,
        CatiaEntitySuffixTrailer,
    };

    let bits = (-6.25_f64).to_bits();
    let mut suffix = vec![0x84, 0x96, 0x82, 0xdf, 0xe6];
    suffix.extend_from_slice(&bits.to_le_bytes());
    suffix.extend_from_slice(&[0x81, 0x92]);
    let native = crate::native::CatiaNative::decode(&standard_catpart_with_two_selector_value(
        "Range",
        "CstAttr_Dimension",
        &suffix,
    ));
    let suffix_value = native.entity_records[0]
        .suffix_value
        .as_ref()
        .expect("81 92-terminated suffix value");
    assert_eq!(suffix_value.prefix_code, 0xdf);
    assert_eq!(suffix_value.trailer, CatiaEntitySuffixTrailer::Token8192);
    assert_eq!(
        suffix_value.payload,
        CatiaEntitySuffixPayload::Evaluation {
            opcode_offset: 4,
            evaluation: CatiaEntityEvaluation::Scalar { bits },
            encoding: CatiaEntityEvaluationEncoding::Direct,
        }
    );
}

#[test]
fn native_namespace_types_and_validates_generic_entity_suffix_values() {
    use crate::native::{
        CatiaEntityEvaluation, CatiaEntityEvaluationEncoding, CatiaEntitySuffixPayload,
        CatiaEntitySuffixTrailer, CatiaEntitySuffixValue,
    };

    let bits = 0.1_f64.to_bits();
    let mut suffix = vec![0x84, 0x96, 0x82, 0xad, 0xe6];
    suffix.extend_from_slice(&bits.to_le_bytes());
    suffix.extend_from_slice(&[0x81, 0x49]);
    let decoded = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&suffix)),
            &DecodeOptions::default(),
        )
        .expect("decode generic entity suffix");
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_SCALAR_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_UNSET_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONTROL_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONTROL_E8_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_CONTROL_E9_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_SEPARATOR_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_ATOM_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_SELECTED_ATOM_ENTITY_SUFFIX_VALUE_COUNT
        ),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_SELECTED_EVALUATION_ENTITY_SUFFIX_VALUE_COUNT
        ),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_SELECTED_CONTROL_ENTITY_SUFFIX_VALUE_COUNT
        ),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_SELECTED_SEPARATOR_ENTITY_SUFFIX_VALUE_COUNT
        ),
        0
    );
    assert_eq!(
        decoded.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_SELECTED_SCHEMA_ENTITY_SUFFIX_VALUE_COUNT
        ),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_SCHEMA_SELECTED_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    assert_eq!(
        decoded
            .report()
            .coverage_count(crate::coverage::DECODED_WIDE_PREFIX_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    let native = crate::native::CatiaNative::load(
        decoded.ir().native.namespace("catia").expect("namespace"),
    )
    .expect("load generic entity suffix");

    assert_eq!(native.entity_records[0].parameter_value, None);
    assert_eq!(
        native.entity_records[0].suffix_value,
        Some(CatiaEntitySuffixValue {
            prefix_atoms: [4, 22, 2],
            prefix_atom_widths: [1, 1, 1],
            prefix_code: 0xad,
            payload: CatiaEntitySuffixPayload::Evaluation {
                opcode_offset: 4,
                evaluation: CatiaEntityEvaluation::Scalar { bits },
                encoding: CatiaEntityEvaluationEncoding::Direct,
            },
            trailer: CatiaEntitySuffixTrailer::Token8149,
        })
    );
    let mut stale_evaluation_offset = native.clone();
    let CatiaEntitySuffixPayload::Evaluation { opcode_offset, .. } = &mut stale_evaluation_offset
        .entity_records[0]
        .suffix_value
        .as_mut()
        .expect("complete scalar suffix")
        .payload
    else {
        panic!("scalar suffix evaluation");
    };
    *opcode_offset = 0;
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    stale_evaluation_offset
        .store(&mut namespace)
        .expect("store stale evaluation offset");
    namespace.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_SUFFIX_EVALUATION_OFFSET_VERSION - 1)
            .unwrap(),
    );
    let migrated =
        crate::native::CatiaNative::load(&namespace).expect("migrate suffix evaluation offset");
    assert_eq!(
        migrated.entity_records[0].suffix_value,
        native.entity_records[0].suffix_value
    );

    let mut malformed_evaluation_offset = native.clone();
    let CatiaEntitySuffixPayload::Evaluation { opcode_offset, .. } =
        &mut malformed_evaluation_offset.entity_records[0]
            .suffix_value
            .as_mut()
            .expect("complete scalar suffix")
            .payload
    else {
        panic!("scalar suffix evaluation");
    };
    *opcode_offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    malformed_evaluation_offset
        .store(&mut namespace)
        .expect("store malformed evaluation offset");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let wide_scalar_bits = 0.001_f64.to_bits();
    let mut wide_scalar_suffix = vec![0xd1, 0x53, 0x96, 0x82, 0xa6, 0xe6];
    wide_scalar_suffix.extend_from_slice(&wide_scalar_bits.to_le_bytes());
    let wide_scalar = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&wide_scalar_suffix)),
            &DecodeOptions::default(),
        )
        .expect("decode wide-prefix scalar suffix");
    assert_eq!(
        wide_scalar
            .report()
            .coverage_count(crate::coverage::DECODED_WIDE_PREFIX_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );
    let wide_scalar = crate::native::CatiaNative::load(
        wide_scalar
            .ir()
            .native
            .namespace("catia")
            .expect("namespace"),
    )
    .expect("load wide-prefix scalar suffix");
    assert_eq!(
        wide_scalar.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete wide-prefix scalar")
            .prefix_atoms,
        [84, 22, 2]
    );
    assert_eq!(
        wide_scalar.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete wide-prefix scalar")
            .prefix_atom_widths,
        [2, 1, 1]
    );
    assert!(matches!(
        wide_scalar.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete wide-prefix scalar")
            .payload,
        CatiaEntitySuffixPayload::Evaluation {
            opcode_offset: 5,
            ..
        }
    ));
    let mut malformed_wide_scalar = wide_scalar;
    malformed_wide_scalar.entity_records[0]
        .suffix_value
        .as_mut()
        .expect("complete wide-prefix scalar")
        .prefix_atom_widths[0] = 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    malformed_wide_scalar
        .store(&mut namespace)
        .expect("store malformed wide-prefix scalar");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let wide_control =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0xd1, 0x67, 0x88, 0x81, 0xbd, 0xe8, 0x81, 0x49,
        ]));
    assert!(matches!(
        wide_control.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete wide-prefix control"),
        CatiaEntitySuffixValue {
            prefix_atoms: [104, 8, 1],
            prefix_atom_widths: [2, 1, 1],
            payload: CatiaEntitySuffixPayload::ControlE8,
            ..
        }
    ));

    let truncated_wide_prefix =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0xd1, 0x53, 0xd1,
        ]));
    assert_eq!(truncated_wide_prefix.entity_records[0].suffix_value, None);

    let unset = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&[
                0x84, 0x96, 0x82, 0xad, 0xe7, 0x81, 0x49,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode generic unset entity suffix");
    assert_eq!(
        unset
            .report()
            .coverage_count(crate::coverage::DECODED_SCALAR_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    assert_eq!(
        unset
            .report()
            .coverage_count(crate::coverage::DECODED_UNSET_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );

    let incomplete = crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
        0x84, 0x96, 0x82, 0xad, 0xe7, 0x81, 0x49, 0x00,
    ]));
    assert_eq!(incomplete.entity_records[0].suffix_value, None);

    let unknown_trailer =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x84, 0x96, 0x82, 0xad, 0xe7, 0x81, 0x50,
        ]));
    assert_eq!(unknown_trailer.entity_records[0].suffix_value, None);

    let invalid_prefix =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x7f, 0x96, 0x82, 0xad, 0xe7, 0x81, 0x49,
        ]));
    assert_eq!(invalid_prefix.entity_records[0].suffix_value, None);

    let control = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&[
                0x84, 0x96, 0x81, 0xa6, 0xe8,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode generic control entity suffix");
    assert_eq!(
        control
            .report()
            .coverage_count(crate::coverage::DECODED_CONTROL_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );
    assert_eq!(
        control
            .report()
            .coverage_count(crate::coverage::DECODED_CONTROL_E8_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );
    assert_eq!(
        control
            .report()
            .coverage_count(crate::coverage::DECODED_CONTROL_E9_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    let control = crate::native::CatiaNative::load(
        control.ir().native.namespace("catia").expect("namespace"),
    )
    .expect("load generic control suffix");
    assert!(matches!(
        control.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete control suffix")
            .payload,
        CatiaEntitySuffixPayload::ControlE8
    ));

    let control_e9 = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&[
                0x84, 0x88, 0x82, 0xf0, 0xe9, 0x81, 0x4a,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode E9 control entity suffix");
    assert_eq!(
        control_e9
            .report()
            .coverage_count(crate::coverage::DECODED_CONTROL_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );
    assert_eq!(
        control_e9
            .report()
            .coverage_count(crate::coverage::DECODED_CONTROL_E8_ENTITY_SUFFIX_VALUE_COUNT),
        0
    );
    assert_eq!(
        control_e9
            .report()
            .coverage_count(crate::coverage::DECODED_CONTROL_E9_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );
    let control_e9 = crate::native::CatiaNative::load(
        control_e9
            .ir()
            .native
            .namespace("catia")
            .expect("namespace"),
    )
    .expect("load E9 control suffix");
    assert!(matches!(
        control_e9.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete E9 control suffix")
            .payload,
        CatiaEntitySuffixPayload::ControlE9
    ));
    let mut malformed_control_e9 = control_e9.clone();
    malformed_control_e9.entity_records[0]
        .suffix_value
        .as_mut()
        .expect("complete E9 control suffix")
        .payload = CatiaEntitySuffixPayload::ControlE8;
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    malformed_control_e9
        .store(&mut namespace)
        .expect("store malformed E9 control suffix");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
    let malformed_control_e9 =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x84, 0x88, 0x82, 0xf0, 0xe9, 0x81, 0x4a, 0x00,
        ]));
    assert_eq!(malformed_control_e9.entity_records[0].suffix_value, None);

    let malformed_control =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x84, 0x96, 0x81, 0xa6, 0xe8, 0x81,
        ]));
    assert_eq!(malformed_control.entity_records[0].suffix_value, None);

    let separator = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&[
                0x84, 0x93, 0x81, 0xa1, 0x37, 0x81, 0x49,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode generic separator entity suffix");
    assert_eq!(
        separator
            .report()
            .coverage_count(crate::coverage::DECODED_SEPARATOR_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );
    let separator = crate::native::CatiaNative::load(
        separator.ir().native.namespace("catia").expect("namespace"),
    )
    .expect("load generic separator suffix");
    assert!(matches!(
        separator.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete separator suffix")
            .payload,
        CatiaEntitySuffixPayload::Separator37
    ));

    let malformed_separator =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x84, 0x93, 0x81, 0xa1, 0x37, 0x81, 0x49, 0,
        ]));
    assert_eq!(malformed_separator.entity_records[0].suffix_value, None);

    let atom = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&[
                0x81, 0x92, 0x81, 0xb3, 0x83, 0x81, 0x49,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode generic atom entity suffix");
    assert_eq!(
        atom.report()
            .coverage_count(crate::coverage::DECODED_ATOM_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );
    let atom =
        crate::native::CatiaNative::load(atom.ir().native.namespace("catia").expect("namespace"))
            .expect("load generic atom suffix");
    assert!(matches!(
        atom.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete atom suffix")
            .payload,
        CatiaEntitySuffixPayload::Atom { value: 3 }
    ));
    let mut malformed_atom = atom;
    malformed_atom.entity_records[0]
        .suffix_value
        .as_mut()
        .expect("complete atom suffix")
        .payload = CatiaEntitySuffixPayload::Atom { value: 4 };
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    malformed_atom
        .store(&mut namespace)
        .expect("store malformed atom suffix");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let truncated_compact_atom =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x81, 0x92, 0x81, 0xb3, 0xd1,
        ]));
    assert_eq!(truncated_compact_atom.entity_records[0].suffix_value, None);

    let schema_selected_atom = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&[
                0x81, 0x92, 0x82, 0x32, 4, 0, 0, 0, 0x81, 0x81, 0x49,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode schema-selected atom entity suffix");
    assert_eq!(
        schema_selected_atom.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_SELECTED_ATOM_ENTITY_SUFFIX_VALUE_COUNT
        ),
        1
    );
    assert_eq!(
        schema_selected_atom
            .report()
            .coverage_count(crate::coverage::DECODED_SCHEMA_SELECTED_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );
    let schema_selected_atom = crate::native::CatiaNative::load(
        schema_selected_atom
            .ir()
            .native
            .namespace("catia")
            .expect("namespace"),
    )
    .expect("load schema-selected atom suffix");
    assert!(matches!(
        schema_selected_atom.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete schema-selected atom suffix")
            .payload,
        CatiaEntitySuffixPayload::SchemaSelected {
            selector: 4,
            value: crate::native::CatiaEntitySuffixSelectedValue::Atom { value: 1 },
            ..
        }
    ));
    assert_eq!(
        schema_selected_atom.entity_records[0]
            .suffix_schema_selection
            .as_ref()
            .expect("resolved suffix selector"),
        &crate::native::CatiaEntitySuffixSchemaSelection {
            offset: 3,
            ordinal: 4,
            entry: schema_selected_atom.catalogs[0].entries[4].id.clone(),
            name: "Thickness".to_string(),
            value: crate::native::CatiaEntitySuffixSchemaValue::Atom { value: 1 },
        }
    );
    let mut stale_schema_selected_atom = schema_selected_atom.clone();
    if let CatiaEntitySuffixPayload::SchemaSelected {
        selector_offset, ..
    } = &mut stale_schema_selected_atom.entity_records[0]
        .suffix_value
        .as_mut()
        .expect("complete schema-selected atom suffix")
        .payload
    {
        *selector_offset = 0;
    } else {
        panic!("schema-selected atom payload");
    }
    stale_schema_selected_atom.entity_records[0]
        .suffix_schema_selection
        .as_mut()
        .expect("resolved suffix selector")
        .offset = 0;
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    stale_schema_selected_atom
        .store(&mut namespace)
        .expect("store stale suffix schema offsets");
    namespace.set_version(
        std::num::NonZeroU32::new(crate::native::CATIA_SUFFIX_SCHEMA_OFFSET_VERSION - 1).unwrap(),
    );
    let migrated =
        crate::native::CatiaNative::load(&namespace).expect("migrate suffix schema offsets");
    assert_eq!(
        migrated.entity_records[0].suffix_value,
        schema_selected_atom.entity_records[0].suffix_value
    );
    assert_eq!(
        migrated.entity_records[0].suffix_schema_selection,
        schema_selected_atom.entity_records[0].suffix_schema_selection
    );

    let mut malformed_schema_selected_atom = schema_selected_atom.clone();
    malformed_schema_selected_atom.entity_records[0]
        .suffix_schema_selection
        .as_mut()
        .expect("resolved suffix selector")
        .offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    malformed_schema_selected_atom
        .store(&mut namespace)
        .expect("store malformed schema-selected atom suffix");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let out_of_range_schema_selected_atom =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x81, 0x92, 0x82, 0x32, 0xcf, 0, 0, 0, 0x81, 0x81, 0x49,
        ]));
    assert!(out_of_range_schema_selected_atom.entity_records[0]
        .suffix_schema_selection
        .is_none());

    let selected_scalar_bits = 17.25_f64.to_bits();
    let mut selected_scalar_suffix = vec![0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0xe6];
    selected_scalar_suffix.extend_from_slice(&selected_scalar_bits.to_le_bytes());
    selected_scalar_suffix.extend_from_slice(&[0x81, 0x4a]);
    let selected_scalar = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(
                &selected_scalar_suffix,
            )),
            &DecodeOptions::default(),
        )
        .expect("decode schema-selected scalar suffix");
    assert_eq!(
        selected_scalar.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_SELECTED_EVALUATION_ENTITY_SUFFIX_VALUE_COUNT
        ),
        1
    );
    let selected_scalar = crate::native::CatiaNative::load(
        selected_scalar
            .ir()
            .native
            .namespace("catia")
            .expect("namespace"),
    )
    .expect("load schema-selected scalar suffix");
    assert!(matches!(
        selected_scalar.entity_records[0]
            .suffix_schema_selection
            .as_ref()
            .expect("resolved schema-selected scalar"),
        crate::native::CatiaEntitySuffixSchemaSelection {
            ordinal: 4,
            name,
            value: crate::native::CatiaEntitySuffixSchemaValue::Evaluation {
                opcode_offset: 8,
                evaluation: CatiaEntityEvaluation::Scalar { bits },
            },
            ..
        } if name == "Thickness" && *bits == selected_scalar_bits
    ));
    let mut malformed_selected_evaluation_offset = selected_scalar;
    let crate::native::CatiaEntitySuffixSchemaValue::Evaluation { opcode_offset, .. } =
        &mut malformed_selected_evaluation_offset.entity_records[0]
            .suffix_schema_selection
            .as_mut()
            .expect("resolved schema-selected scalar")
            .value
    else {
        panic!("schema-selected scalar evaluation");
    };
    *opcode_offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    malformed_selected_evaluation_offset
        .store(&mut namespace)
        .expect("store malformed selected evaluation offset");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let selected_unset =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0xe7,
        ]));
    assert!(matches!(
        selected_unset.entity_records[0]
            .suffix_schema_selection
            .as_ref()
            .expect("resolved schema-selected unset"),
        crate::native::CatiaEntitySuffixSchemaSelection {
            value: crate::native::CatiaEntitySuffixSchemaValue::Evaluation {
                evaluation: CatiaEntityEvaluation::Unset,
                ..
            },
            ..
        }
    ));

    let selected_control = CatiaCodec
        .decode(
            &mut Cursor::new(standard_catpart_with_parameter_value(&[
                0x84, 0x88, 0x81, 0x32, 4, 0, 0, 0, 0xe8, 0x81, 0x49,
            ])),
            &DecodeOptions::default(),
        )
        .expect("decode schema-selected control suffix");
    assert_eq!(
        selected_control.report().coverage_count(
            crate::coverage::DECODED_SCHEMA_SELECTED_CONTROL_ENTITY_SUFFIX_VALUE_COUNT
        ),
        1
    );
    assert_eq!(
        selected_control
            .report()
            .coverage_count(crate::coverage::DECODED_SCHEMA_SELECTED_ENTITY_SUFFIX_VALUE_COUNT),
        1
    );
    let selected_control = crate::native::CatiaNative::load(
        selected_control
            .ir()
            .native
            .namespace("catia")
            .expect("namespace"),
    )
    .expect("load schema-selected control suffix");
    assert!(matches!(
        selected_control.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete schema-selected control suffix")
            .payload,
        CatiaEntitySuffixPayload::SchemaSelected {
            selector: 4,
            value: crate::native::CatiaEntitySuffixSelectedValue::ControlE8,
            ..
        }
    ));
    assert!(matches!(
        selected_control.entity_records[0]
            .suffix_schema_selection
            .as_ref()
            .expect("resolved schema-selected control suffix"),
        crate::native::CatiaEntitySuffixSchemaSelection {
            ordinal: 4,
            name,
            value: crate::native::CatiaEntitySuffixSchemaValue::ControlE8,
            ..
        } if name == "Thickness"
    ));
    let mut malformed_selected_control = selected_control.clone();
    malformed_selected_control.entity_records[0]
        .suffix_schema_selection
        .as_mut()
        .expect("resolved schema-selected control suffix")
        .value = crate::native::CatiaEntitySuffixSchemaValue::Separator37;
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    malformed_selected_control
        .store(&mut namespace)
        .expect("store malformed schema-selected control suffix");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
    let malformed_selected_control =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x84, 0x88, 0x81, 0x32, 4, 0, 0, 0, 0xe8, 0x81, 0x49, 0x00,
        ]));
    assert_eq!(
        malformed_selected_control.entity_records[0].suffix_value,
        None
    );

    let selected_separator =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x82, 0x93, 0x81, 0x32, 4, 0, 0, 0, 0x37, 0x81, 0x52,
        ]));
    assert!(matches!(
        &selected_separator.entity_records[0]
            .suffix_schema_selection
            .as_ref()
            .expect("resolved schema-selected separator")
            .value,
        crate::native::CatiaEntitySuffixSchemaValue::Separator37
    ));

    let selected_schema =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x84, 0x93, 0x82, 0x32, 4, 0, 0, 0, 0x32, 5, 0, 0, 0, 0x81, 0x49,
        ]));
    assert!(matches!(
        selected_schema.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete nested suffix selector")
            .payload,
        CatiaEntitySuffixPayload::SchemaSelected {
            selector_offset: 3,
            value: crate::native::CatiaEntitySuffixSelectedValue::SchemaSelector {
                offset: 8,
                ordinal: 5,
            },
            ..
        }
    ));
    assert!(matches!(
        &selected_schema.entity_records[0]
            .suffix_schema_selection
            .as_ref()
            .expect("resolved nested suffix selector")
            .value,
        crate::native::CatiaEntitySuffixSchemaValue::SchemaSelector {
            ordinal: 5,
            ref name,
            ..
        } if name.as_deref() == Some("#1_ /2")
    ));
    let mut malformed_nested_offset = selected_schema;
    let crate::native::CatiaEntitySuffixSchemaValue::SchemaSelector { offset, .. } =
        &mut malformed_nested_offset.entity_records[0]
            .suffix_schema_selection
            .as_mut()
            .expect("resolved nested suffix selector")
            .value
    else {
        panic!("nested suffix schema selector");
    };
    *offset += 1;
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    malformed_nested_offset
        .store(&mut namespace)
        .expect("store malformed nested suffix offset");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut nonfinite_selected_scalar = vec![0x84, 0x88, 0x82, 0x32, 4, 0, 0, 0, 0xe6];
    nonfinite_selected_scalar.extend_from_slice(&f64::NAN.to_bits().to_le_bytes());
    let nonfinite_selected_scalar = crate::native::CatiaNative::decode(
        &standard_catpart_with_parameter_value(&nonfinite_selected_scalar),
    );
    assert_eq!(
        nonfinite_selected_scalar.entity_records[0].suffix_value,
        None
    );

    let malformed_schema_selected_atom =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
            0x81, 0x92, 0x82, 0x32, 0xcf, 0, 0, 0, 0x81, 0, 0, 0,
        ]));
    assert_eq!(
        malformed_schema_selected_atom.entity_records[0].suffix_value,
        None
    );

    let mut bare_scalar = vec![0x84, 0x96, 0x82, 0xb1, 0xe6];
    bare_scalar.extend_from_slice(&6.75_f64.to_bits().to_le_bytes());
    let bare_scalar =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&bare_scalar));
    assert_eq!(
        bare_scalar.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete bare scalar suffix")
            .trailer,
        CatiaEntitySuffixTrailer::Empty
    );

    let bare_unset = crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&[
        0x84, 0x96, 0x82, 0xb1, 0xe7,
    ]));
    assert!(matches!(
        bare_unset.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete bare unset suffix")
            .payload,
        CatiaEntitySuffixPayload::Evaluation {
            evaluation: CatiaEntityEvaluation::Unset,
            ..
        }
    ));

    let nested_bits = 11.725_f64.to_bits();
    let mut nested_scalar = vec![0x84, 0x88, 0x82, 0x32, 0xe6, 0, 0, 0, 0xe6];
    nested_scalar.extend_from_slice(&nested_bits.to_le_bytes());
    let nested_scalar =
        crate::native::CatiaNative::decode(&standard_catpart_with_parameter_value(&nested_scalar));
    let nested_value = nested_scalar.entity_records[0]
        .suffix_value
        .as_ref()
        .expect("complete zero-padded scalar suffix");
    assert_eq!(
        nested_value.payload,
        CatiaEntitySuffixPayload::Evaluation {
            opcode_offset: 8,
            evaluation: CatiaEntityEvaluation::Scalar { bits: nested_bits },
            encoding: CatiaEntityEvaluationEncoding::ZeroPaddedScalar,
        }
    );
    assert_eq!(nested_value.trailer, CatiaEntitySuffixTrailer::Empty);

    let mut nonfinite_nested = vec![0x84, 0x88, 0x82, 0x32, 0xe6, 0, 0, 0, 0xe6];
    nonfinite_nested.extend_from_slice(&f64::INFINITY.to_bits().to_le_bytes());
    let nonfinite_nested = crate::native::CatiaNative::decode(
        &standard_catpart_with_parameter_value(&nonfinite_nested),
    );
    assert_eq!(nonfinite_nested.entity_records[0].suffix_value, None);

    let mut zero_frame_scalar = vec![0x84, 0x96, 0x82, 0x55, 0xe6];
    zero_frame_scalar.extend_from_slice(&(-26.703_618_806_753_155_f64).to_bits().to_le_bytes());
    zero_frame_scalar.extend_from_slice(&[0xfe, 0xf6]);
    zero_frame_scalar.extend_from_slice(&[0; 16]);
    let zero_frame_scalar = crate::native::CatiaNative::decode(
        &standard_catpart_with_parameter_value(&zero_frame_scalar),
    );
    assert_eq!(
        zero_frame_scalar.entity_records[0]
            .suffix_value
            .as_ref()
            .expect("complete zero-frame scalar suffix")
            .trailer,
        CatiaEntitySuffixTrailer::FixedZeroFrame
    );

    let mut malformed_zero_frame = vec![0x84, 0x96, 0x82, 0x55, 0xe6];
    malformed_zero_frame.extend_from_slice(&1.0_f64.to_bits().to_le_bytes());
    malformed_zero_frame.extend_from_slice(&[0xfe, 0xf6]);
    malformed_zero_frame.extend_from_slice(&[0; 15]);
    malformed_zero_frame.push(1);
    let malformed_zero_frame = crate::native::CatiaNative::decode(
        &standard_catpart_with_parameter_value(&malformed_zero_frame),
    );
    assert_eq!(malformed_zero_frame.entity_records[0].suffix_value, None);

    let mut malformed_encoding = native.clone();
    malformed_encoding.entity_records[0]
        .suffix_value
        .as_mut()
        .expect("complete suffix value")
        .payload = CatiaEntitySuffixPayload::Evaluation {
        opcode_offset: 4,
        evaluation: CatiaEntityEvaluation::Scalar { bits },
        encoding: CatiaEntityEvaluationEncoding::ZeroPaddedScalar,
    };
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    malformed_encoding
        .store(&mut namespace)
        .expect("store malformed suffix encoding");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));

    let mut malformed = native;
    malformed.entity_records[0]
        .suffix_value
        .as_mut()
        .expect("complete suffix value")
        .trailer = CatiaEntitySuffixTrailer::Token814A;
    let mut namespace = cadmpeg_ir::NativeNamespace::new(std::num::NonZeroU32::MIN);
    malformed
        .store(&mut namespace)
        .expect("store malformed suffix value");
    assert!(matches!(
        crate::native::CatiaNative::load(&namespace),
        Err(cadmpeg_ir::NativeConvertError::InvalidOwner(_))
    ));
}
