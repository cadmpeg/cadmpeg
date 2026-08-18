// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, Cursor, Read, Seek, SeekFrom};

use cadmpeg_core::decode::DecodeMode;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence, DecodeOptions, EncodeInput, Encoder};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, RegionId, ShellId,
    SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::WritePath;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, LoopBoundaryRole, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::CadIr;

use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::{IgesCodec, IgesEncoder, IgesVersion, IgesWriteOptions};

#[test]
fn blank_parameter_field_is_an_omitted_value() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 116,
                form: 0,
                label: "BLANK".into(),
                status: "00010000",
                parameters: "116,1,2,3,   ;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert_eq!(result.ir().model.points.len(), 1);
    let validation = cadmpeg_ir::validate_neutral(result.ir(), result.report().losses.clone());
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn even_parameter_back_pointer_is_rejected_without_guessing_its_owner() {
    let mut bytes = owned_test_file(&[
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "FIRST".into(),
            status: "00010000",
            parameters: "116,1,2,3,0;comment".into(),
        },
        OwnedTestEntity {
            entity_type: 116,
            form: 0,
            label: "SECOND".into(),
            status: "00010000",
            parameters: "116,4,5,6,0;".into(),
        },
    ]);
    let marker = bytes
        .windows(8)
        .position(|window| window == b"P      2")
        .expect("second Parameter Data card");
    let card_start = marker - 72;
    bytes[card_start + 64..card_start + 72].copy_from_slice(b"       2");

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap_err();
    assert!(error.to_string().contains(
        "Parameter Data card P2 back-pointer 2 is not an owning odd Directory Entry sequence"
    ));
}

#[test]
fn zero_parameter_back_pointer_is_not_bound_to_the_first_directory_entry() {
    let mut bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 116,
        form: 0,
        label: "POINT".into(),
        status: "00010000",
        parameters: "116,1,2,3,0;".into(),
    }]);
    let marker = bytes
        .windows(8)
        .position(|window| window == b"P      1")
        .expect("Parameter Data card");
    let card_start = marker - 72;
    bytes[card_start + 64..card_start + 72].copy_from_slice(b"       0");

    let error = IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap_err();
    assert!(error.to_string().contains(
        "Parameter Data card P1 back-pointer 0 is not an owning odd Directory Entry sequence"
    ));
}

#[test]
fn parameter_card_count_must_equal_the_owned_contiguous_range() {
    let entity = OwnedTestEntity {
        entity_type: 116,
        form: 0,
        label: "POINT".into(),
        status: "00010000",
        parameters: format!("116,1,2,3,0;{}", "comment".repeat(12)),
    };
    let canonical = owned_test_file(&[entity]);

    for (declared, expected) in [
        (1, "declares 1 Parameter Data cards but owns 2"),
        (3, "declares 3 Parameter Data cards but owns 2"),
    ] {
        let mut bytes = canonical.clone();
        let marker = bytes
            .windows(8)
            .position(|window| window == b"D      2")
            .expect("second Directory Entry card");
        let card_start = marker - 72;
        bytes[card_start + 24..card_start + 32]
            .copy_from_slice(format!("{declared:>8}").as_bytes());

        let error = IgesCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}

#[test]
fn type116_association_group_defaults_np_at_record_delimiter() {
    for parameters in ["116,1.25,2.5,3.75,0,1,1;", "116,1.25,2.5,3.75,,1,1;"] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file(&[
                    OwnedTestEntity {
                        entity_type: 402,
                        form: 1,
                        label: "GROUP".into(),
                        status: "00000200",
                        parameters: "402,1,3;".into(),
                    },
                    OwnedTestEntity {
                        entity_type: 116,
                        form: 0,
                        label: "POINT".into(),
                        status: "00000000",
                        parameters: parameters.into(),
                    },
                ])),
                &DecodeOptions::default(),
            )
            .unwrap();
        let native = result.ir().native.namespace("iges").unwrap();
        let groups = &native.arenas["groups"];
        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups[0].fields()["members"],
            serde_json::json!(["iges:entity:directory#3"])
        );
        let point = native.arenas["entities"]
            .iter()
            .find(|entity| entity.id() == "iges:entity:directory#3")
            .unwrap();
        assert_eq!(
            point.fields()["association_links"],
            serde_json::json!(["iges:entity:directory#1"])
        );
        assert_eq!(point.fields()["references"].as_array().unwrap().len(), 1);
        assert_eq!(point.fields()["references"][0]["parameter_index"], 6);
        assert_eq!(point.fields()["references"][0]["raw_pointer"], 1);
        assert!(
            result.report().losses.is_empty(),
            "{:#?}",
            result.report().losses
        );
    }
}

#[test]
fn type116_property_group_follows_explicit_or_omitted_display_pointer() {
    for parameters in ["116,1.25,2.5,3.75,0,0,1,3;", "116,1.25,2.5,3.75,,0,1,3;"] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file(&[
                    OwnedTestEntity {
                        entity_type: 116,
                        form: 0,
                        label: "POINT".into(),
                        status: "00000000",
                        parameters: parameters.into(),
                    },
                    OwnedTestEntity {
                        entity_type: 406,
                        form: 7,
                        label: "REFDES".into(),
                        status: "00010000",
                        parameters: "406,1,2HR1;".into(),
                    },
                ])),
                &DecodeOptions::default(),
            )
            .unwrap();
        let native = result.ir().native.namespace("iges").unwrap();
        let point = &native.arenas["entities"][0];
        assert_eq!(
            point.fields()["property_links"],
            serde_json::json!(["iges:entity:directory#3"])
        );
        let property = &native.arenas["product_properties"][0];
        assert_eq!(
            property.fields()["owners"],
            serde_json::json!(["iges:entity:directory#1"])
        );
        assert!(
            result.report().losses.is_empty(),
            "{:#?}",
            result.report().losses
        );
    }
}

#[test]
fn type102_count_driven_boundary_follows_constituent_list() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "GROUP".into(),
                    status: "00000000",
                    parameters: "402,1,7;".into(),
                },
                OwnedTestEntity {
                    entity_type: 110,
                    form: 0,
                    label: "CHILD1".into(),
                    status: "00010000",
                    parameters: "110,0,0,0,1,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 110,
                    form: 0,
                    label: "CHILD2".into(),
                    status: "00010000",
                    parameters: "110,1,0,0,2,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 102,
                    form: 0,
                    label: "COMPOSIT".into(),
                    status: "00000000",
                    parameters: "102,2,3,5,1,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let composite = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#7")
        .unwrap();
    assert_eq!(
        composite.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#1"])
    );
    assert_eq!(composite.fields()["references"][0]["parameter_index"], 5);
    assert_eq!(composite.fields()["references"][0]["raw_pointer"], 1);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn type102_wrong_typed_constituent_keeps_count_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "GROUP".into(),
                    status: "00000000",
                    parameters: "402,1,5;".into(),
                },
                OwnedTestEntity {
                    entity_type: 110,
                    form: 0,
                    label: "CHILD".into(),
                    status: "00010000",
                    parameters: "110,0,0,0,1,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 102,
                    form: 0,
                    label: "BADTYPE".into(),
                    status: "00000000",
                    parameters: "102,1,3.5,1,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let composite = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#5")
        .unwrap();
    assert_eq!(
        composite.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#1"])
    );
    assert_eq!(composite.fields()["references"][0]["parameter_index"], 4);
    assert_eq!(composite.fields()["references"][0]["raw_pointer"], 1);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn type102_invalid_count_suppresses_generic_suffix_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "GROUP".into(),
                    status: "00000000",
                    parameters: "402,1,5;".into(),
                },
                OwnedTestEntity {
                    entity_type: 110,
                    form: 0,
                    label: "CHILD".into(),
                    status: "00010000",
                    parameters: "110,0,0,0,1,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 102,
                    form: 0,
                    label: "BAD".into(),
                    status: "00000000",
                    parameters: "102,0,3,1,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let composite = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#5")
        .unwrap();
    assert!(composite.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(composite.fields()["references"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn type106_ip_width_defines_boundary_for_forms_1_2_and_3() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "GROUP".into(),
                    status: "00000000",
                    parameters: "402,3,3,5,7;".into(),
                },
                OwnedTestEntity {
                    entity_type: 106,
                    form: 1,
                    label: "FORM1".into(),
                    status: "00000000",
                    parameters: "106,1,2,6,1,1,1,1,1,1,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 106,
                    form: 2,
                    label: "FORM2".into(),
                    status: "00000000",
                    parameters: "106,2,2,7,1,1,1,1,1,1,1,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 106,
                    form: 3,
                    label: "FORM3".into(),
                    status: "00000000",
                    parameters: "106,3,2,13,1,1,1,1,1,1,1,1,1,1,1,1,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    for (directory_sequence, parameter_index) in [(3, 9), (5, 10), (7, 16)] {
        let copious = native.arenas["entities"]
            .iter()
            .find(|entity| entity.id() == format!("iges:entity:directory#{directory_sequence}"))
            .unwrap();
        assert_eq!(
            copious.fields()["association_links"],
            serde_json::json!(["iges:entity:directory#1"])
        );
        assert_eq!(
            copious.fields()["references"][0]["parameter_index"],
            parameter_index
        );
        assert_eq!(copious.fields()["references"][0]["raw_pointer"], 1);
    }
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn type106_form_ip_mismatch_suppresses_generic_suffix_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "GROUP".into(),
                    status: "00000000",
                    parameters: "402,1,3;".into(),
                },
                OwnedTestEntity {
                    entity_type: 106,
                    form: 1,
                    label: "BADIP".into(),
                    status: "00000000",
                    parameters: "106,2,2,7,1,1,1,1,1,1,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let copious = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#3")
        .unwrap();
    assert!(copious.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(copious.fields()["references"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind()));
}

#[test]
fn type106_form63_rejects_nonplanar_ip_before_suffix_recovery() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "GROUP".into(),
                    status: "00000000",
                    parameters: "402,1,3;".into(),
                },
                OwnedTestEntity {
                    entity_type: 106,
                    form: 63,
                    label: "BAD63".into(),
                    status: "00000000",
                    parameters: "106,2,2,7,1,1,1,1,1,1,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let copious = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#3")
        .unwrap();
    assert!(copious.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(copious.fields()["references"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind()));
}

#[test]
fn type106_nonpositive_count_suppresses_generic_suffix_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "GROUP".into(),
                    status: "00000000",
                    parameters: "402,1,3;".into(),
                },
                OwnedTestEntity {
                    entity_type: 106,
                    form: 1,
                    label: "BADCOUNT".into(),
                    status: "00000000",
                    parameters: "106,1,0,1,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let copious = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#3")
        .unwrap();
    assert!(copious.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(copious.fields()["references"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind()));
}

#[test]
fn type402_group_forms_share_count_driven_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "TARGET".into(),
                    status: "00000000",
                    parameters: "402,1,5,3,3,7,9,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 402,
                    form: 1,
                    label: "FORM1".into(),
                    status: "00000000",
                    parameters: "402,1,1,2,1,3,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 116,
                    form: 0,
                    label: "POINT".into(),
                    status: "00010000",
                    parameters: "116,0,0,0,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "FORM7".into(),
                    status: "00000000",
                    parameters: "402,1,1,2,1,3,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 402,
                    form: 14,
                    label: "FORM14".into(),
                    status: "00000000",
                    parameters: "402,1,1,2,1,3,0;".into(),
                },
                OwnedTestEntity {
                    entity_type: 402,
                    form: 15,
                    label: "FORM15".into(),
                    status: "00000000",
                    parameters: "402,1,1,2,1,3,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    for directory_sequence in [3, 7, 9, 11] {
        let group = native.arenas["entities"]
            .iter()
            .find(|entity| entity.id() == format!("iges:entity:directory#{directory_sequence}"))
            .unwrap();
        let fields = group.fields();
        assert_eq!(
            fields["association_links"],
            serde_json::json!(["iges:entity:directory#1", "iges:entity:directory#3"])
        );
        let references = fields["references"].as_array().unwrap();
        assert_eq!(references.len(), 3);
        let mut parameter_indices = references
            .iter()
            .map(|reference| reference["parameter_index"].as_u64().unwrap())
            .collect::<Vec<_>>();
        parameter_indices.sort_unstable();
        assert_eq!(parameter_indices, vec![2, 4, 5]);
    }
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn type402_negative_count_suppresses_generic_suffix_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "TARGET".into(),
                    status: "00000000",
                    parameters: "402,1,3;".into(),
                },
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "BADCOUNT".into(),
                    status: "00000000",
                    parameters: "402,-1,1,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let group = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#3")
        .unwrap();
    assert!(group.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(group.fields()["references"].as_array().unwrap().is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind()));
}

#[test]
fn type402_wrong_typed_member_keeps_count_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "TARGET".into(),
                    status: "00000000",
                    parameters: "402,1,3;".into(),
                },
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "BADMEM".into(),
                    status: "00000000",
                    parameters: "402,1,2,1,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let group = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#3")
        .unwrap();
    assert_eq!(
        group.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#1"])
    );
    assert_eq!(group.fields()["references"][0]["parameter_index"], 4);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind()));
}

#[test]
fn type402_zero_count_keeps_the_count_defined_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "TARGET".into(),
                    status: "00000000",
                    parameters: "402,1,3;".into(),
                },
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "EMPTY".into(),
                    status: "00000000",
                    parameters: "402,0,1,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let group = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#3")
        .unwrap();
    assert_eq!(
        group.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#1"])
    );
    assert_eq!(group.fields()["references"][0]["parameter_index"], 3);
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind()));
}

#[test]
fn entity_table_boundary_beats_pointer_shaped_line_coordinates() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(ambiguous_trailing_pointer_boundary_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let line = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#5")
        .unwrap();

    assert_eq!(
        line.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#3"])
    );
    assert_eq!(line.fields()["references"].as_array().unwrap().len(), 1);
    assert_eq!(line.fields()["references"][0]["parameter_index"], 8);
    assert_eq!(line.fields()["references"][0]["raw_pointer"], 3);
    assert!(!result.report().losses.iter().any(|loss| {
        loss.message
            .contains("fully valid trailing pointer-group boundaries")
    }));
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn unknown_entity_with_ambiguous_suffix_is_not_guessed() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "GROUP-A".into(),
                    status: "00000000",
                    parameters: "402,1,5;".into(),
                },
                OwnedTestEntity {
                    entity_type: 402,
                    form: 7,
                    label: "GROUP-B".into(),
                    status: "00000000",
                    parameters: "402,1,5;".into(),
                },
                OwnedTestEntity {
                    entity_type: 999,
                    form: 0,
                    label: "UNKNOWN".into(),
                    status: "00000000",
                    parameters: "999,7,3,3,1,3,3,1,3,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let unknown = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#5")
        .unwrap();

    assert!(unknown.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| {
            loss.message
                .contains("fully valid trailing pointer-group boundaries")
        })
        .unwrap();
    assert_eq!(
        loss.provenance.as_ref().unwrap().tag.as_deref(),
        Some("directory_entry:D5")
    );
}

#[test]
fn exact_entity_boundary_retains_an_unresolved_pointer() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 110,
                form: 0,
                label: "DANGLING".into(),
                status: "00000000",
                parameters: "110,0,0,0,1,0,0,1,99,0;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let line = &native.arenas["entities"][0];

    assert!(line.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    let reference = &line.fields()["references"][0];
    assert_eq!(reference["parameter_index"], 8);
    assert_eq!(reference["raw_pointer"], 99);
    assert_eq!(reference["resolution"], "dangling");
    let loss = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.message.contains("pointer 99"))
        .unwrap();
    assert_eq!(
        loss.provenance.as_ref().unwrap().tag.as_deref(),
        Some("D1:parameter[8]")
    );
}
