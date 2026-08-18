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

const TYPE212_NOTE_PARAMETERS: &str = "212,1,1,1,1,1,1.5707963267948966,0,0,0,0,0,0,1HA;";

fn type100_boundary_entity(label: &str) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type: 100,
        form: 0,
        label: label.into(),
        status: "00000000",
        parameters: "100,0,0,0,1,0,1,0;".into(),
    }
}

fn type212_note_entity(label: &str) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type: 212,
        form: 0,
        label: label.into(),
        status: "00010100",
        parameters: TYPE212_NOTE_PARAMETERS.into(),
    }
}

fn type230_sectioned_area_entity(label: &str, parameters: &str) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type: 230,
        form: 0,
        label: label.into(),
        status: "00000100",
        parameters: parameters.into(),
    }
}

fn type132_connect_point_entity(label: &str) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type: 132,
        form: 0,
        label: label.into(),
        status: "00000400",
        parameters: "132,0,0,0,0,101,1,2HP1,0,3HPIN,0,1,1,0,0;".into(),
    }
}

fn type322_form0_entity(label: &str, parameters: &str) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type: 322,
        form: 0,
        label: label.into(),
        status: "00000000",
        parameters: parameters.into(),
    }
}

fn type322_entity(form: i64, label: &str, parameters: &str) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type: 322,
        form,
        label: label.into(),
        status: "00000000",
        parameters: parameters.into(),
    }
}

fn type422_entity(form: i64, label: &str, parameters: &str) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type: 422,
        form,
        label: label.into(),
        status: "00000000",
        parameters: parameters.into(),
    }
}

fn type312_template_entity(label: &str) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type: 312,
        form: 0,
        label: label.into(),
        status: "00000200",
        parameters: "312,0,0;".into(),
    }
}

fn type302_form5001_entity(label: &str, parameters: &str) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type: 302,
        form: 5001,
        label: label.into(),
        status: "00000200",
        parameters: parameters.into(),
    }
}

fn type406_form14_entity(label: &str, parameters: &str) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type: 406,
        form: 14,
        label: label.into(),
        status: "00000000",
        parameters: parameters.into(),
    }
}

fn type316_entity(label: &str, parameters: &str) -> OwnedTestEntity {
    OwnedTestEntity {
        entity_type: 316,
        form: 0,
        label: label.into(),
        status: "00000200",
        parameters: parameters.into(),
    }
}

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
fn type308_count_driven_boundary_follows_member_list() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 212,
                    form: 0,
                    label: "TARGET".into(),
                    status: "00010100",
                    parameters: TYPE212_NOTE_PARAMETERS.into(),
                },
                OwnedTestEntity {
                    entity_type: 212,
                    form: 0,
                    label: "MEMBER3".into(),
                    status: "00010100",
                    parameters: TYPE212_NOTE_PARAMETERS.into(),
                },
                OwnedTestEntity {
                    entity_type: 212,
                    form: 0,
                    label: "MEMBER5".into(),
                    status: "00010100",
                    parameters: TYPE212_NOTE_PARAMETERS.into(),
                },
                OwnedTestEntity {
                    entity_type: 308,
                    form: 0,
                    label: "SUBFIG".into(),
                    status: "00000200",
                    parameters: "308,0,8HDEFNAME1,2,3,1,1,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let subfigure = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#7")
        .unwrap();
    assert_eq!(
        subfigure.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#1"])
    );
    let fields = subfigure.fields();
    let references = fields["references"].as_array().unwrap();
    let mut parameter_indices = references
        .iter()
        .map(|reference| reference["parameter_index"].as_u64().unwrap())
        .collect::<Vec<_>>();
    parameter_indices.sort_unstable();
    assert_eq!(parameter_indices, vec![4, 5, 7]);
    assert_eq!(
        references
            .iter()
            .find(|reference| reference["parameter_index"] == 7)
            .unwrap()["raw_pointer"],
        1
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn type308_negative_count_suppresses_generic_suffix_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 212,
                    form: 0,
                    label: "TARGET".into(),
                    status: "00010100",
                    parameters: TYPE212_NOTE_PARAMETERS.into(),
                },
                OwnedTestEntity {
                    entity_type: 308,
                    form: 0,
                    label: "BADCOUNT".into(),
                    status: "00000200",
                    parameters: "308,0,8HDEFNAME1,-1,3,1,1,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let subfigure = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#3")
        .unwrap();
    assert!(subfigure.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(subfigure.fields()["references"]
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
fn type308_wrong_typed_member_keeps_count_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 212,
                    form: 0,
                    label: "TARGET".into(),
                    status: "00010100",
                    parameters: TYPE212_NOTE_PARAMETERS.into(),
                },
                OwnedTestEntity {
                    entity_type: 308,
                    form: 0,
                    label: "BADMEM".into(),
                    status: "00000200",
                    parameters: "308,0,8HDEFNAME1,2,2,1,1,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let subfigure = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#3")
        .unwrap();
    assert_eq!(
        subfigure.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#1"])
    );
    assert_eq!(
        subfigure.fields()["references"]
            .as_array()
            .unwrap()
            .iter()
            .find(|reference| reference["parameter_index"] == 7)
            .unwrap()["raw_pointer"],
        1
    );
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
fn type308_zero_count_keeps_the_count_defined_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                OwnedTestEntity {
                    entity_type: 212,
                    form: 0,
                    label: "TARGET".into(),
                    status: "00010100",
                    parameters: TYPE212_NOTE_PARAMETERS.into(),
                },
                OwnedTestEntity {
                    entity_type: 308,
                    form: 0,
                    label: "EMPTY".into(),
                    status: "00000200",
                    parameters: "308,0,8HDEFNAME1,0,1,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let subfigure = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#3")
        .unwrap();
    assert_eq!(
        subfigure.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#1"])
    );
    assert_eq!(subfigure.fields()["references"][0]["parameter_index"], 5);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn type230_count_driven_boundary_follows_island_list() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type100_boundary_entity("BOUNDARY"),
                type100_boundary_entity("ISLAND"),
                type212_note_entity("TARGET"),
                type230_sectioned_area_entity(
                    "SECTION",
                    "230,1,2,0,0,0,1,0.7853981633974483,1,3,1,5,0;",
                ),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let section = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#7")
        .unwrap();
    assert_eq!(
        section.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#5"])
    );
    let mut parameter_indices = section.fields()["references"]
        .as_array()
        .unwrap()
        .iter()
        .map(|reference| reference["parameter_index"].as_u64().unwrap())
        .collect::<Vec<_>>();
    parameter_indices.sort_unstable();
    assert_eq!(parameter_indices, vec![1, 9, 11]);
    assert_eq!(
        section.fields()["references"]
            .as_array()
            .unwrap()
            .iter()
            .find(|reference| reference["parameter_index"] == 11)
            .unwrap()["raw_pointer"],
        5
    );
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn type230_zero_count_keeps_the_count_defined_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type100_boundary_entity("BOUNDARY"),
                type212_note_entity("TARGET"),
                type230_sectioned_area_entity(
                    "EMPTY",
                    "230,1,2,0,0,0,1,0.7853981633974483,0,1,3,0;",
                ),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let section = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#5")
        .unwrap();
    assert_eq!(
        section.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#3"])
    );
    let fields = section.fields();
    let association_reference = fields["references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|reference| reference["parameter_index"] == 10)
        .unwrap();
    assert_eq!(association_reference["raw_pointer"], 3);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn type230_negative_count_suppresses_generic_suffix_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type100_boundary_entity("BOUNDARY"),
                type212_note_entity("TARGET"),
                type230_sectioned_area_entity(
                    "BADCOUNT",
                    "230,1,2,0,0,0,1,0.7853981633974483,-1,1,3,0;",
                ),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let section = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#5")
        .unwrap();
    assert!(section.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!section.fields()["references"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reference| reference["parameter_index"] == 9));
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
fn type230_wrong_typed_island_keeps_count_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type100_boundary_entity("BOUNDARY"),
                type212_note_entity("TARGET"),
                type230_sectioned_area_entity(
                    "BADISLND",
                    "230,1,2,0,0,0,1,0.7853981633974483,1,3,1,3,0;",
                ),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let section = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#5")
        .unwrap();
    assert_eq!(
        section.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#3"])
    );
    assert_eq!(
        section.fields()["references"]
            .as_array()
            .unwrap()
            .iter()
            .find(|reference| reference["parameter_index"] == 9)
            .unwrap()["resolution"],
        "wrong_type"
    );
    assert_eq!(
        section.fields()["references"]
            .as_array()
            .unwrap()
            .iter()
            .find(|reference| reference["parameter_index"] == 11)
            .unwrap()["raw_pointer"],
        3
    );
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
fn type230_truncated_island_suppresses_generic_suffix_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type100_boundary_entity("BOUNDARY"),
                type100_boundary_entity("ISLAND"),
                type212_note_entity("TARGET"),
                type230_sectioned_area_entity(
                    "TRUNCATE",
                    "230,1,2,0,0,0,1,0.7853981633974483,2,3;",
                ),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let section = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#7")
        .unwrap();
    assert!(section.fields()["association_links"]
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
fn type320_count_driven_boundary_follows_member_and_connect_lists() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type212_note_entity("CHILD1"),
                type212_note_entity("CHILD2"),
                type132_connect_point_entity("CONNECT"),
                type212_note_entity("TARGET"),
                OwnedTestEntity {
                    entity_type: 320,
                    form: 0,
                    label: "NETWORK".into(),
                    status: "00000200",
                    parameters: "320,0,3HNET,2,1,3,1,2HPR,0,1,5,1,7,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let network = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#9")
        .unwrap();
    assert_eq!(
        network.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#7"])
    );
    let fields = network.fields();
    let references = fields["references"].as_array().unwrap();
    for (parameter_index, raw_pointer) in [(4, 1), (5, 3), (10, 5), (12, 7)] {
        let reference = references
            .iter()
            .find(|reference| reference["parameter_index"] == parameter_index)
            .unwrap();
        assert_eq!(reference["raw_pointer"], raw_pointer);
    }
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn type320_zero_counts_keep_the_count_defined_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type212_note_entity("TARGET"),
                OwnedTestEntity {
                    entity_type: 320,
                    form: 0,
                    label: "EMPTY".into(),
                    status: "00000200",
                    parameters: "320,0,5HEMPTY,0,1,2HPR,0,0,1,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let network = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#3")
        .unwrap();
    assert_eq!(
        network.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#1"])
    );
    let fields = network.fields();
    let association = fields["references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|reference| reference["parameter_index"] == 9)
        .unwrap();
    assert_eq!(association["raw_pointer"], 1);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn type320_wrong_connect_point_keeps_count_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type212_note_entity("TARGET"),
                type100_boundary_entity("CHILD"),
                OwnedTestEntity {
                    entity_type: 320,
                    form: 0,
                    label: "WRONG".into(),
                    status: "00000200",
                    parameters: "320,0,5HWRONG,1,3,1,2HPR,0,1,1,1,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let network = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#5")
        .unwrap();
    assert_eq!(
        network.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#1"])
    );
    let fields = network.fields();
    let references = fields["references"].as_array().unwrap();
    assert_eq!(
        references
            .iter()
            .find(|reference| reference["parameter_index"] == 9)
            .unwrap()["resolution"],
        "wrong_type"
    );
    assert_eq!(
        references
            .iter()
            .find(|reference| reference["parameter_index"] == 11)
            .unwrap()["raw_pointer"],
        1
    );
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
fn type320_negative_member_count_suppresses_generic_suffix_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type212_note_entity("TARGET"),
                OwnedTestEntity {
                    entity_type: 320,
                    form: 0,
                    label: "BADCOUNT".into(),
                    status: "00000200",
                    parameters: "320,0,8HBADCOUNT,-1,1,2HPR,0,0,1,1,1,0;".into(),
                },
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let network = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#3")
        .unwrap();
    assert!(network.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!network.fields()["references"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reference| reference["parameter_index"] == 10));
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
fn type320_truncated_member_list_suppresses_generic_suffix_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 320,
                form: 0,
                label: "TRUNCMEM".into(),
                status: "00000200",
                parameters: "320,0,8HTRUNCMEM,2,1,1,2HPR,0,0;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let network = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert!(network.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind()));
}

#[test]
fn type320_truncated_connect_point_list_suppresses_generic_suffix_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[OwnedTestEntity {
                entity_type: 320,
                form: 0,
                label: "TRUNCCP".into(),
                status: "00000200",
                parameters: "320,0,7HTRUNCCP,0,1,2HPR,0,2,0;".into(),
            }])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let network = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert!(network.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind()));
}

#[test]
fn type322_form0_count_driven_boundary_follows_descriptor_list() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type322_form0_entity("DEFINITN", "322,3HDEF,1,2,1,3,,20,1,1,1,3,0;"),
                type212_note_entity("TARGET"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let definition = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert_eq!(
        definition.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#3"])
    );
    let fields = definition.fields();
    let reference = fields["references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|reference| reference["parameter_index"] == 11)
        .unwrap();
    assert_eq!(reference["raw_pointer"], 3);
    assert!(result.report().losses.is_empty());
}

#[test]
fn type322_form0_zero_count_suppresses_generic_suffix_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type322_form0_entity("ZERO", "322,3HDEF,1,0,1,1,0;"),
                type212_note_entity("TARGET"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let definition = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert!(definition.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind()));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn type322_form0_negative_count_suppresses_generic_suffix_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type322_form0_entity("NEGATIVE", "322,3HDEF,1,-1,1,1,0;"),
                type212_note_entity("TARGET"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let definition = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert!(definition.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind()));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn type322_form0_wrong_typed_descriptor_keeps_count_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type322_form0_entity("WRONGVAL", "322,3HDEF,1,2,1,1,1,20,2.5,1,1,3,0;"),
                type212_note_entity("TARGET"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let definition = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert_eq!(
        definition.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#3"])
    );
    let fields = definition.fields();
    let reference = fields["references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|reference| reference["parameter_index"] == 11)
        .unwrap();
    assert_eq!(reference["raw_pointer"], 3);
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
fn type322_form0_truncated_descriptor_list_suppresses_generic_suffix_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type322_form0_entity("TRUNCATE", "322,3HDEF,1,2,1,1;"),
                type212_note_entity("TARGET"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let definition = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert!(definition.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind()));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn type322_form1_count_driven_boundary_follows_value_widths() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type322_entity(1, "ATTRDEFN", "322,3HDEF,1,2,1,3,,2HAA,20,1,2,11,12,1,3,0;"),
                type212_note_entity("TARGET"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let definition = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert_eq!(
        definition.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#3"])
    );
    let fields = definition.fields();
    let reference = fields["references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|reference| reference["parameter_index"] == 14)
        .unwrap();
    assert_eq!(reference["raw_pointer"], 3);
    let attribute_fields = native.arenas["attribute_table_definitions"][0].fields();
    let attributes = attribute_fields["attributes"].as_array().unwrap();
    assert_eq!(attributes[0]["values"].as_array().unwrap().len(), 1);
    assert_eq!(attributes[1]["values"].as_array().unwrap().len(), 2);
    assert!(result.report().losses.is_empty());
}

#[test]
fn type322_form2_count_driven_boundary_follows_value_pointer_pairs() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type322_entity(
                    2,
                    "ATTRDEFN",
                    "322,3HDEF,1,2,1,3,,2HAA,,20,1,2,11,3,12,3,1,5,0;",
                ),
                type312_template_entity("TEMPLATE"),
                type212_note_entity("TARGET"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let definition = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert_eq!(
        definition.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#5"])
    );
    let definition_fields = definition.fields();
    let references = definition_fields["references"].as_array().unwrap();
    for parameter_index in [13, 15, 17] {
        assert!(references
            .iter()
            .any(|reference| reference["parameter_index"] == parameter_index));
    }
    let attribute_fields = native.arenas["attribute_table_definitions"][0].fields();
    let attributes = attribute_fields["attributes"].as_array().unwrap();
    assert_eq!(
        attributes[0]["values"][0]["display_template"],
        serde_json::Value::Null
    );
    assert_eq!(
        attributes[1]["values"][0]["display_template"],
        serde_json::json!("iges:entity:directory#3")
    );
    assert_eq!(
        attributes[1]["values"][1]["display_template"],
        serde_json::json!("iges:entity:directory#3")
    );
    assert!(result.report().losses.is_empty());
}

#[test]
fn type322_forms12_zero_value_counts_keep_the_count_defined_boundary() {
    for (form, label) in [(1, "ZEROFORM"), (2, "ZEROFORM")] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file(&[
                    type322_entity(form, label, "322,3HDEF,1,2,1,3,0,20,1,0,1,3,0;"),
                    type212_note_entity("TARGET"),
                ])),
                &DecodeOptions::default(),
            )
            .unwrap();
        let native = result.ir().native.namespace("iges").unwrap();
        let definition = native.arenas["entities"]
            .iter()
            .find(|entity| entity.id() == "iges:entity:directory#1")
            .unwrap();
        assert_eq!(
            definition.fields()["association_links"],
            serde_json::json!(["iges:entity:directory#3"])
        );
        assert!(definition.fields()["references"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reference| reference["parameter_index"] == 11));
        assert!(result.report().losses.is_empty());
    }
}

#[test]
fn type322_form1_wrong_typed_value_keeps_count_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type322_entity(1, "WRONGVAL", "322,3HDEF,1,1,1,3,1,20,1,3,0;"),
                type212_note_entity("TARGET"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let definition = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert_eq!(
        definition.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#3"])
    );
    assert!(definition.fields()["references"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reference| reference["parameter_index"] == 9));
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
fn type322_form2_wrong_typed_value_keeps_count_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type322_entity(
                    2,
                    "WRONGVAL",
                    "322,3HDEF,1,2,1,3,,20,,20,1,2,11,3,12,3,1,5,0;",
                ),
                type312_template_entity("TEMPLATE"),
                type212_note_entity("TARGET"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let definition = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert_eq!(
        definition.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#5"])
    );
    assert!(definition.fields()["references"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reference| reference["parameter_index"] == 17));
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
fn type322_form2_wrong_typed_pointer_keeps_value_pair_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type322_entity(2, "WRONGPTR", "322,3HDEF,1,1,1,3,1,2HAA,5,1,5,0;"),
                type312_template_entity("TEMPLATE"),
                type212_note_entity("TARGET"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let definition = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert_eq!(
        definition.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#5"])
    );
    let definition_fields = definition.fields();
    let references = definition_fields["references"].as_array().unwrap();
    assert!(references
        .iter()
        .any(|reference| reference["parameter_index"] == 8
            && reference["resolution"] == "wrong_type"));
    assert!(references.iter().any(
        |reference| reference["parameter_index"] == 10 && reference["resolution"] == "resolved"
    ));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::PointerUnresolved.kind()));
}

#[test]
fn type322_forms12_invalid_value_width_suppresses_generic_suffix_candidate() {
    let cases = [
        (1, "322,3HDEF,1,0,1,3,0;"),
        (1, "322,3HDEF,1,1,1,3,-1,1,3,0;"),
        (1, "322,3HDEF,1,1,1,3,1.5,1,3,0;"),
        (1, "322,3HDEF,1,1,1,3,1;"),
        (2, "322,3HDEF,1,0,1,3,0;"),
        (2, "322,3HDEF,1,1,1,3,-1,1,3,0;"),
        (2, "322,3HDEF,1,1,1,3,1.5,1,3,0;"),
        (2, "322,3HDEF,1,1,1,3,1,2HAA;"),
    ];
    for (form, parameters) in cases {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file(&[
                    type322_entity(form, "MALFORM", parameters),
                    type212_note_entity("TARGET"),
                ])),
                &DecodeOptions::default(),
            )
            .unwrap();
        let native = result.ir().native.namespace("iges").unwrap();
        let definition = native.arenas["entities"]
            .iter()
            .find(|entity| entity.id() == "iges:entity:directory#1")
            .unwrap();
        assert!(definition.fields()["association_links"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(!result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind()));
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
    }
}

#[test]
fn type422_forms_use_referenced_definition_width_for_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_structures(
                &[
                    type322_form0_entity("ATTRDEF", "322,3HDEF,1,2,10,1,2,11,1,1;"),
                    type422_entity(0, "ATTRONE", "422,42,43,44,1,7,0;"),
                    type422_entity(1, "ATTRTAB", "422,2,42,43,44,45,46,47,1,7,0;"),
                    type212_note_entity("TARGET"),
                ],
                &[(3, -1), (5, -1)],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    for (sequence, parameter_index, row_count) in [(3, 5, 1), (5, 9, 2)] {
        let entity = native.arenas["entities"]
            .iter()
            .find(|entity| entity.id() == format!("iges:entity:directory#{sequence}"))
            .unwrap();
        assert_eq!(
            entity.fields()["association_links"],
            serde_json::json!(["iges:entity:directory#7"])
        );
        assert!(entity.fields()["references"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reference| reference["parameter_index"] == parameter_index));
        let instance = native.arenas["attribute_table_instances"]
            .iter()
            .find(|instance| {
                instance.fields()["source_entity"] == format!("iges:entity:directory#{sequence}")
            })
            .unwrap();
        assert_eq!(
            instance.fields()["rows"].as_array().unwrap().len(),
            row_count
        );
    }
    assert!(result.report().losses.is_empty());
}

#[test]
fn type422_wrong_typed_values_keep_definition_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_structures(
                &[
                    type322_form0_entity("ATTRDEF", "322,3HDEF,1,2,10,1,2,11,1,1;"),
                    type422_entity(0, "WRONGONE", "422,5Hwrong,43,44,1,7,0;"),
                    type422_entity(1, "WRONGTAB", "422,1,5Hwrong,43,44,1,7,0;"),
                    type212_note_entity("TARGET"),
                ],
                &[(3, -1), (5, -1)],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    for (sequence, parameter_index) in [(3, 5), (5, 6)] {
        let entity = native.arenas["entities"]
            .iter()
            .find(|entity| entity.id() == format!("iges:entity:directory#{sequence}"))
            .unwrap();
        assert_eq!(
            entity.fields()["association_links"],
            serde_json::json!(["iges:entity:directory#7"])
        );
        assert!(entity.fields()["references"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reference| reference["parameter_index"] == parameter_index));
    }
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == IgesLossCode::EntityNotProjected.kind())
            .count(),
        2
    );
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind()));
}

#[test]
fn type422_zero_definition_width_keeps_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_structures(
                &[
                    type322_form0_entity("ZERODEF", "322,3HDEF,1,2,10,1,0,11,1,0;"),
                    type422_entity(0, "ZEROONE", "422,1,7,0;"),
                    type422_entity(1, "ZEROTAB", "422,1,1,7,0;"),
                    type212_note_entity("TARGET"),
                ],
                &[(3, -1), (5, -1)],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    for (sequence, parameter_index) in [(3, 2), (5, 3)] {
        let entity = native.arenas["entities"]
            .iter()
            .find(|entity| entity.id() == format!("iges:entity:directory#{sequence}"))
            .unwrap();
        assert_eq!(
            entity.fields()["association_links"],
            serde_json::json!(["iges:entity:directory#7"])
        );
        assert!(entity.fields()["references"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reference| reference["parameter_index"] == parameter_index));
    }
    assert!(result.report().losses.is_empty());
}

#[test]
fn type422_invalid_context_suppresses_generic_suffix_candidate() {
    let assert_suppressed =
        |entities: Vec<OwnedTestEntity>, structures: &[(u32, i64)], sequence: u32| {
            let result = IgesCodec
                .decode(
                    &mut Cursor::new(owned_test_file_with_structures(&entities, structures)),
                    &DecodeOptions::default(),
                )
                .unwrap();
            let native = result.ir().native.namespace("iges").unwrap();
            let entity = native.arenas["entities"]
                .iter()
                .find(|entity| entity.id() == format!("iges:entity:directory#{sequence}"))
                .unwrap();
            assert!(entity.fields()["association_links"]
                .as_array()
                .unwrap()
                .is_empty());
            assert!(!entity.fields()["references"]
                .as_array()
                .unwrap()
                .iter()
                .any(|reference| reference["kind"] == "parameter"));
            assert!(!result
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind()));
        };

    assert_suppressed(
        vec![
            type422_entity(0, "NODEF", "422,1,3,0;"),
            type212_note_entity("TARGET"),
        ],
        &[(1, -99)],
        1,
    );
    assert_suppressed(
        vec![
            type322_entity(1, "WRNGFORM", "322,3HDEF,1,1,10,1,1,42;"),
            type422_entity(0, "WRONGREF", "422,1,3,0;"),
            type212_note_entity("TARGET"),
        ],
        &[(3, -1)],
        3,
    );
    assert_suppressed(
        vec![
            type322_form0_entity("BADAVC", "322,3HDEF,1,2,10,1,2.5,11,1,1;"),
            type422_entity(0, "BADDEF", "422,1,5,0;"),
            type212_note_entity("TARGET"),
        ],
        &[(3, -1)],
        3,
    );
    assert_suppressed(
        vec![
            type322_form0_entity("ATTRDEF", "322,3HDEF,1,2,10,1,2,11,1,1;"),
            type422_entity(1, "BADROW", "422,1,42,43,1,5,0;"),
            type212_note_entity("TARGET"),
        ],
        &[(3, -1)],
        3,
    );
    assert_suppressed(
        vec![
            type322_form0_entity("ATTRDEF", "322,3HDEF,1,2,10,1,2,11,1,1;"),
            type422_entity(1, "BADCOUNT", "422,1.5,1,5,0;"),
            type212_note_entity("TARGET"),
        ],
        &[(3, -1)],
        3,
    );
}

#[test]
fn type302_form5001_count_driven_boundary_follows_nested_class_widths() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type302_form5001_entity("ASSOCDEF", "302,2,1,1,2,1,2,2,2,1,3,1,3,0;"),
                type212_note_entity("TARGET"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let definition = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert_eq!(
        definition.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#3"])
    );
    let fields = definition.fields();
    let reference = fields["references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|reference| reference["parameter_index"] == 12)
        .unwrap();
    assert_eq!(reference["raw_pointer"], 3);
    assert!(result.report().losses.is_empty());
}

#[test]
fn type302_form5001_zero_class_count_suppresses_generic_suffix_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type302_form5001_entity("ZEROCLS", "302,0,1,1,1,3,1,3,0;"),
                type212_note_entity("TARGET"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let definition = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert!(definition.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind()));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn type302_form5001_negative_class_count_suppresses_generic_suffix_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type302_form5001_entity("NEGCLASS", "302,-1,1,1,1,3,1,3,0;"),
                type212_note_entity("TARGET"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let definition = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert!(definition.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind()));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn type302_form5001_zero_item_count_suppresses_generic_suffix_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type302_form5001_entity("ZEROITEM", "302,1,1,1,0,1,3,0;"),
                type212_note_entity("TARGET"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let definition = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert!(definition.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind()));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn type302_form5001_wrong_item_count_suppresses_generic_suffix_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type302_form5001_entity("WRONGCNT", "302,1,1,1,2.5,1,3,1,3,0;"),
                type212_note_entity("TARGET"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let definition = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert!(definition.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind()));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn type302_form5001_wrong_item_type_keeps_count_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type302_form5001_entity("WRONGITM", "302,1,1,1,2,4,1,1,3,0;"),
                type212_note_entity("TARGET"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let definition = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert_eq!(
        definition.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#3"])
    );
    let fields = definition.fields();
    let reference = fields["references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|reference| reference["parameter_index"] == 8)
        .unwrap();
    assert_eq!(reference["raw_pointer"], 3);
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
fn type302_form5001_truncated_class_suppresses_generic_suffix_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type302_form5001_entity("TRUNC302", "302,2,1,1,1,1;"),
                type212_note_entity("TARGET"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let definition = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert!(definition.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind()));
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
}

#[test]
fn type406_form14_count_driven_boundary_follows_string_list() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type212_note_entity("TARGET"),
                type406_form14_entity("POSITIVE", "406,2,4HMAIN,3HHOT,1,1,0;"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let property = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#3")
        .unwrap();
    assert_eq!(
        property.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#1"])
    );
    let fields = property.fields();
    let reference = fields["references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|reference| reference["parameter_index"] == 5)
        .unwrap();
    assert_eq!(reference["raw_pointer"], 1);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn type406_form14_zero_count_suppresses_generic_suffix_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type212_note_entity("TARGET"),
                type406_form14_entity("ZERO", "406,0,1,1,0;"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let property = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#3")
        .unwrap();
    assert!(property.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!property.fields()["references"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reference| reference["parameter_index"] == 3));
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
fn type406_form14_negative_count_suppresses_generic_suffix_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type212_note_entity("TARGET"),
                type406_form14_entity("NEGATIVE", "406,-1,1,1,0;"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let property = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#3")
        .unwrap();
    assert!(property.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!property.fields()["references"]
        .as_array()
        .unwrap()
        .iter()
        .any(|reference| reference["parameter_index"] == 3));
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
fn type406_form14_wrong_typed_value_keeps_count_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type212_note_entity("TARGET"),
                type406_form14_entity("WRONGVAL", "406,2,4HMAIN,1,1,1,0;"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let property = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#3")
        .unwrap();
    assert_eq!(
        property.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#1"])
    );
    let fields = property.fields();
    let reference = fields["references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|reference| reference["parameter_index"] == 5)
        .unwrap();
    assert_eq!(reference["raw_pointer"], 1);
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
fn type406_form14_truncated_string_list_suppresses_generic_suffix_candidate() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[type406_form14_entity(
                "TRUNCATE",
                "406,2,4HMAIN;",
            )])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let property = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert!(property.fields()["association_links"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind()));
}

#[test]
fn type316_count_driven_boundary_follows_unit_triples() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type316_entity("UNITS", "316,2,6HLENGTH,1HM,1000,4HTIME,1HS,1,1,3,0;"),
                type212_note_entity("TARGET"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let entity = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert_eq!(
        entity.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#3"])
    );
    let fields = entity.fields();
    let reference = fields["references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|reference| reference["parameter_index"] == 9)
        .unwrap();
    assert_eq!(reference["raw_pointer"], 3);
    let units = &native.arenas["units_data"][0];
    assert_eq!(units.fields()["declared_count"], 2);
    assert_eq!(units.fields()["units"].as_array().unwrap().len(), 2);
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn type316_entity_boundary_beats_two_valid_generic_suffixes() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type316_entity("UNITS", "316,2,6HLENGTH,1HM,1000,4HTIME,1HS,4,3,3,5,7,0;"),
                type212_note_entity("TARGET1"),
                type212_note_entity("TARGET2"),
                type212_note_entity("TARGET3"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let entity = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert_eq!(
        entity.fields()["association_links"],
        serde_json::json!([
            "iges:entity:directory#3",
            "iges:entity:directory#5",
            "iges:entity:directory#7"
        ])
    );
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind() }));
    assert!(
        result.report().losses.is_empty(),
        "{:#?}",
        result.report().losses
    );
}

#[test]
fn type316_invalid_count_or_truncated_units_suppresses_generic_suffix_candidate() {
    for (label, parameters) in [
        ("ZERO", "316,0,3,3,5,7,0;"),
        ("NEGATIVE", "316,-1,3,3,5,7,0;"),
        ("WRONGCNT", "316,1.5,3,3,5,7,0;"),
        ("TRUNCATE", "316,2,6HLENGTH,1HM,1000,4HTIME,0;"),
    ] {
        let result = IgesCodec
            .decode(
                &mut Cursor::new(owned_test_file(&[
                    type316_entity(label, parameters),
                    type212_note_entity("TARGET1"),
                    type212_note_entity("TARGET2"),
                    type212_note_entity("TARGET3"),
                ])),
                &DecodeOptions::default(),
            )
            .unwrap();
        let native = result.ir().native.namespace("iges").unwrap();
        let entity = native.arenas["entities"]
            .iter()
            .find(|entity| entity.id() == "iges:entity:directory#1")
            .unwrap();
        assert!(
            entity.fields()["association_links"]
                .as_array()
                .unwrap()
                .is_empty(),
            "{label}: {:#?}",
            entity.fields()["association_links"]
        );
        assert!(!entity.fields()["references"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reference| reference["parameter_index"] == 3));
        assert!(result
            .report()
            .losses
            .iter()
            .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
        assert!(!result
            .report()
            .losses
            .iter()
            .any(|loss| { loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind() }));
    }
}

#[test]
fn type316_wrong_typed_value_keeps_count_boundary() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file(&[
                type316_entity("WRONGVAL", "316,1,6HLENGTH,2.5,1000,1,3,0;"),
                type212_note_entity("TARGET"),
            ])),
            &DecodeOptions::default(),
        )
        .unwrap();
    let native = result.ir().native.namespace("iges").unwrap();
    let entity = native.arenas["entities"]
        .iter()
        .find(|entity| entity.id() == "iges:entity:directory#1")
        .unwrap();
    assert_eq!(
        entity.fields()["association_links"],
        serde_json::json!(["iges:entity:directory#3"])
    );
    let fields = entity.fields();
    let reference = fields["references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|reference| reference["parameter_index"] == 6)
        .unwrap();
    assert_eq!(reference["raw_pointer"], 3);
    let units = &native.arenas["units_data"][0];
    assert_eq!(
        units.fields()["units"][0]["unit_value"],
        serde_json::Value::Null
    );
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == IgesLossCode::EntityNotProjected.kind()));
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| { loss.code == IgesLossCode::AmbiguousTrailingPointerGroups.kind() }));
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
