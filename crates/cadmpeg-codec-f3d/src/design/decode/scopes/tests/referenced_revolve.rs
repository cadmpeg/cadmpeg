// SPDX-License-Identifier: Apache-2.0

use super::super::parameter_scope::parse_parameter_scope_with_type_guid;
use super::super::path_feature::{
    exact_path_feature_construction, referenced_revolve_reference_loss,
};
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::path_features::{
    DesignPathFeatureConstruction, DesignRevolveConstruction,
};
use crate::records::parameters::DesignParameterOwner;
use crate::records::references::DesignClassTag;
use crate::test_support::{indexed_header, lp_utf16};
use cadmpeg_core::serde_erase::json;

const REVOLVE_TYPE_GUID: &str = "E3849A15-2FC6-42A0-AF3A-2F1D7273B406";
const SCOPE: u32 = 70;
const ANGLE: u32 = 95;
const STREAM: &str = "f3d:Design/BulkStream.dat";

#[derive(Clone)]
struct Fixture {
    unmarked: bool,
    references: usize,
    operation: u32,
    zero_run: u8,
    marker: u8,
    zero_value: u32,
    extent_kind: u32,
    direction_kind: u8,
    project_axis: u8,
    solid: u8,
    zero_flags: [u8; 3],
    angle_marker: u8,
    angle: f64,
    label: Option<&'static str>,
    second_reference: Option<u64>,
    payload_length: Option<usize>,
}

impl Fixture {
    fn marked() -> Self {
        Self {
            unmarked: false,
            references: 6,
            operation: 4,
            zero_run: 0,
            marker: 1,
            zero_value: 0,
            extent_kind: 2,
            direction_kind: 0,
            project_axis: 0,
            solid: 1,
            zero_flags: [0; 3],
            angle_marker: 1,
            angle: 1.5,
            label: None,
            second_reference: None,
            payload_length: None,
        }
    }

    fn unmarked() -> Self {
        Self {
            unmarked: true,
            references: 8,
            operation: 1,
            ..Self::marked()
        }
    }

    fn bytes(&self) -> Vec<u8> {
        fn marked(bytes: &mut Vec<u8>, marker: u8, record: u32) {
            bytes.push(marker);
            bytes.extend_from_slice(&record.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"331", SCOPE);
        if self.unmarked {
            bytes.push(self.zero_run);
            bytes.extend_from_slice(&[0; 9]);
        } else {
            bytes.push(self.zero_run);
            bytes.extend_from_slice(&[0; 8]);
            bytes.push(self.marker);
            bytes.extend_from_slice(&self.zero_value.to_le_bytes());
        }
        bytes.extend_from_slice(&self.operation.to_le_bytes());
        bytes.extend_from_slice(&self.extent_kind.to_le_bytes());
        bytes.push(self.direction_kind);
        bytes.push(self.project_axis);
        bytes.push(self.solid);
        bytes.extend_from_slice(&self.zero_flags);
        marked(&mut bytes, self.angle_marker, ANGLE);
        let payload =
            self.payload_length
                .unwrap_or(if self.unmarked && self.second_reference.is_some() {
                    379
                } else if self.unmarked {
                    369
                } else if self.second_reference.is_some() {
                    361
                } else if self.references >= 8 {
                    373 + 11 * (self.references - 8)
                } else {
                    351
                });
        let table = [79_u32, 80, 81, 82, 91, 92, 93, 94, ANGLE, 96];
        let table = &table[table.len() - self.references..];
        let tail_extra = if self.second_reference.is_some() {
            10
        } else {
            0
        };
        let padding = payload - bytes.len() - 90 - tail_extra - 11 * table.len();
        bytes.extend(std::iter::repeat_n(0, padding));
        bytes.extend_from_slice(&u32::try_from(table.len()).unwrap().to_le_bytes());
        for &record in table {
            marked(&mut bytes, 1, record);
        }
        bytes.extend_from_slice(&17_u32.to_le_bytes());
        lp_utf16(&mut bytes, "Révolution");
        if let Some(label) = self.label {
            bytes.extend_from_slice(&2u32.to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes());
            lp_utf16(&mut bytes, label);
            bytes.extend_from_slice(&[0; 7]);
            bytes.extend_from_slice(&[1, 15]);
            bytes.extend_from_slice(&1u64.to_le_bytes());
            bytes.extend_from_slice(&[0; 2]);
            bytes.extend_from_slice(&9u32.to_le_bytes());
            bytes.extend_from_slice(&252u32.to_le_bytes());
            bytes.extend_from_slice(&0.25f64.to_le_bytes());
            bytes.extend_from_slice(&252u32.to_le_bytes());
            bytes.extend_from_slice(&[1, 14]);
            bytes.extend_from_slice(&1u64.to_le_bytes());
            bytes.extend_from_slice(&[0, 1, 0, 0, 1, 13]);
            bytes.extend_from_slice(&1u64.to_le_bytes());
            bytes.extend_from_slice(&[0; 3]);
        } else {
            let mut tail = vec![0; 78 + tail_extra];
            tail[0..4].copy_from_slice(&2_u32.to_le_bytes());
            let previous_state = if let Some(target) = self.second_reference {
                tail[30] = 1;
                tail[31..39].copy_from_slice(&target.to_le_bytes());
                41
            } else {
                31
            };
            tail[previous_state..previous_state + 4].copy_from_slice(&16_u32.to_le_bytes());
            bytes.extend_from_slice(&tail);
        }
        indexed_header(&mut bytes, *b"258", SCOPE);
        bytes
    }

    fn decode(&self) -> Option<DesignRevolveConstruction> {
        self.decode_with_loss().0
    }

    fn decode_with_loss(
        &self,
    ) -> (
        Option<DesignRevolveConstruction>,
        Option<cadmpeg_ir::report::loss::LossNote>,
    ) {
        self.decode_bytes(&self.bytes())
    }

    fn decode_bytes(
        &self,
        bytes: &[u8],
    ) -> (
        Option<DesignRevolveConstruction>,
        Option<cadmpeg_ir::report::loss::LossNote>,
    ) {
        let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
        let class_tag = DesignClassTag::try_from("331".to_owned()).unwrap();
        let mut scope = parse_parameter_scope_with_type_guid(
            bytes,
            &records,
            SCOPE,
            &class_tag,
            0,
            Some(REVOLVE_TYPE_GUID),
        )
        .unwrap()
        .expect("Revolve scope");
        scope.id = format!("{STREAM}:design-parameter-scope#0");
        let owner: DesignParameterOwner = json::from_value(cadmpeg_core::json!({
            "id": format!("{STREAM}:design-parameter-owner#{ANGLE}"),
            "byte_offset": 2000,
            "frame_length": 104,
            "class_tag": "351",
            "record_index": ANGLE,
            "scope_record_index": SCOPE,
            "local_ordinal": 0,
            "evaluated_value": self.angle,
            "evaluated_value_offset": 2040,
            "parameter_record_index": ANGLE + 1,
            "owned_ordinal": 1,
            "variant": 0,
            "companion_record_index": ANGLE + 2,
        }))
        .unwrap();
        let construction = match exact_path_feature_construction(bytes, &records, &scope, &[owner])
        {
            Some(DesignPathFeatureConstruction::Revolve(construction)) => Some(construction),
            _ => None,
        };
        let loss = construction
            .as_ref()
            .and_then(|value| referenced_revolve_reference_loss(bytes, &scope, value));
        (construction, loss)
    }
}

#[test]
fn the_marked_form_reads_its_solid_flag() {
    let solid = Fixture::marked().decode().expect("solid revolve");
    assert_eq!(solid.solid, Some(true));
    assert_eq!(solid.operation, DesignExtrudeOperation::NewBody);
    assert_eq!(
        solid.travel,
        crate::records::feature::path_features::DesignRevolveTravel::Angle(
            cadmpeg_ir::scalar::PositiveAngle::new(1.5).unwrap()
        )
    );
    assert_eq!(solid.angle_record_index, ANGLE);
    assert!(!solid.reversed);
    let mut surface = Fixture::marked();
    surface.solid = 0;
    assert_eq!(surface.decode().unwrap().solid, Some(false));
    for project_axis in [0, 1] {
        let mut fixture = surface.clone();
        fixture.project_axis = project_axis;
        assert_eq!(
            fixture.decode().map(|c| c.solid),
            Some(Some(false)),
            "{project_axis}"
        );
    }
    let mut invalid = Fixture::marked();
    invalid.solid = 2;
    assert!(invalid.decode().is_none());
}

#[test]
fn the_unmarked_form_has_eight_references_and_its_operation_at_offset_21() {
    for (operation, expected) in [
        (1, DesignExtrudeOperation::Join),
        (2, DesignExtrudeOperation::Cut),
        (3, DesignExtrudeOperation::Intersect),
        (4, DesignExtrudeOperation::NewBody),
    ] {
        let mut fixture = Fixture::unmarked();
        fixture.operation = operation;
        let construction = fixture.decode().expect("unmarked revolve");
        assert_eq!(construction.operation, expected);
        assert_eq!(construction.solid, Some(true));
        assert_eq!(construction.angle_record_index, ANGLE);
        let mut surface = fixture.clone();
        surface.solid = 0;
        assert_eq!(surface.decode().unwrap().solid, Some(false));
    }
    let mut six = Fixture::unmarked();
    six.references = 6;
    assert!(six.decode().is_none());
    let mut eight = Fixture::marked();
    eight.references = 8;
    assert!(eight.decode().is_none());
}

#[test]
fn a_nine_reference_boolean_marked_frame_adds_one_table_reference() {
    let mut nine = Fixture::marked();
    nine.references = 9;
    nine.operation = 2;
    let construction = nine.decode().expect("nine-reference cut revolve");
    assert_eq!(construction.operation, DesignExtrudeOperation::Cut);
    assert_eq!(construction.solid, Some(true));
    assert_eq!(construction.angle_record_index, ANGLE);
    let mut new_body = nine.clone();
    new_body.operation = 4;
    assert!(new_body.decode().is_none());
    let mut short = nine.clone();
    short.payload_length = Some(373);
    assert!(short.decode().is_none());
    let mut ten = nine.clone();
    ten.references = 10;
    assert!(ten.decode().is_none());
}

#[test]
fn both_forms_refuse_other_prologue_values() {
    for base in [Fixture::marked(), Fixture::unmarked()] {
        let mut cases = Vec::new();
        for (index, byte) in [(0, 1), (1, 1), (2, 1)] {
            let mut fixture = base.clone();
            fixture.zero_flags[index] = byte;
            cases.push(("zero flag", fixture));
        }
        let mut fixture = base.clone();
        fixture.project_axis = 2;
        cases.push(("project axis", fixture));
        let mut fixture = base.clone();
        fixture.extent_kind = 1;
        cases.push(("extent kind", fixture));
        let mut fixture = base.clone();
        fixture.direction_kind = 1;
        cases.push(("direction kind", fixture));
        if base.unmarked {
            let mut fixture = base.clone();
            fixture.zero_run = 1;
            cases.push(("zero run", fixture));
        } else {
            for marker in [0, 2] {
                let mut fixture = base.clone();
                fixture.marker = marker;
                cases.push(("marker", fixture));
            }
            let mut fixture = base.clone();
            fixture.zero_value = 1;
            cases.push(("zero value", fixture));
        }
        let mut fixture = base.clone();
        fixture.angle_marker = 0;
        cases.push(("angle marker", fixture));
        for (name, fixture) in cases {
            assert!(
                fixture.decode().is_none(),
                "{name} unmarked={}",
                base.unmarked
            );
        }
    }
}

#[test]
fn a_negative_angle_states_its_magnitude_and_a_reversed_turn() {
    for base in [Fixture::marked(), Fixture::unmarked()] {
        let mut negative = base.clone();
        negative.angle = -1.5;
        let construction = negative.decode().expect("negative angle");
        assert_eq!(
            construction.travel,
            crate::records::feature::path_features::DesignRevolveTravel::Angle(
                cadmpeg_ir::scalar::PositiveAngle::new(1.5).unwrap()
            )
        );
        assert!(construction.reversed);
        let mut zero = base.clone();
        zero.angle = 0.0;
        assert!(zero.decode().is_none());
    }
}

#[test]
fn marked_form_matches_operation_to_frame_and_reference_count() {
    let mut labelled = Fixture::marked();
    labelled.label = Some("Angle 😀");
    assert_eq!(
        labelled.decode().unwrap().operation,
        DesignExtrudeOperation::NewBody
    );

    let mut boolean = Fixture::marked();
    boolean.references = 8;
    for (operation, expected) in [
        (1, DesignExtrudeOperation::Join),
        (2, DesignExtrudeOperation::Cut),
        (3, DesignExtrudeOperation::Intersect),
    ] {
        boolean.operation = operation;
        assert_eq!(boolean.decode().unwrap().operation, expected);
    }
    boolean.operation = 4;
    assert!(boolean.decode().is_none());

    let mut wrong_operation = Fixture::marked();
    wrong_operation.operation = 1;
    assert!(wrong_operation.decode().is_none());
    wrong_operation.project_axis = 1;
    wrong_operation.solid = 0;
    assert!(wrong_operation.decode().is_none());
}

#[test]
fn direction_kind_one_reads_only_for_a_full_turn() {
    let mut full_turn = Fixture::marked();
    full_turn.angle = std::f64::consts::TAU;
    let expected = full_turn.decode().expect("full turn");
    full_turn.direction_kind = 1;
    assert_eq!(full_turn.decode(), Some(expected));
    let mut partial = Fixture::marked();
    partial.direction_kind = 1;
    assert_eq!(partial.decode(), None);
    full_turn.direction_kind = 2;
    assert_eq!(full_turn.decode(), None);
}

#[test]
fn a_361_byte_frame_carries_a_second_tail_reference() {
    let fixture = Fixture {
        second_reference: Some(56),
        ..Fixture::marked()
    };
    let (construction, loss) = fixture.decode_with_loss();
    let construction = construction.expect("361-byte referenced-angle Revolve");
    assert_eq!(construction.operation, DesignExtrudeOperation::NewBody);
    assert_eq!(construction.angle_record_index, ANGLE);
    assert!(loss
        .expect("second tail reference loss")
        .message
        .contains("target 56"));
    assert_eq!(Fixture::marked().decode_with_loss().1, None);
    let boolean = Fixture {
        references: 8,
        operation: 1,
        ..fixture
    };
    assert_eq!(boolean.decode(), None);
}

#[test]
fn an_unmarked_379_byte_frame_preserves_the_angle_and_second_tail_reference() {
    for (operation, expected) in [
        (1, DesignExtrudeOperation::Join),
        (2, DesignExtrudeOperation::Cut),
        (3, DesignExtrudeOperation::Intersect),
        (4, DesignExtrudeOperation::NewBody),
    ] {
        let fixture = Fixture {
            operation,
            second_reference: Some(60),
            ..Fixture::unmarked()
        };
        let (construction, loss) = fixture.decode_with_loss();
        let construction = construction.expect("unmarked referenced-angle Revolve");
        assert_eq!(construction.operation, expected);
        assert_eq!(construction.angle_record_index, ANGLE);
        assert_eq!(construction.solid, Some(true));
        assert_eq!(
            construction.travel,
            crate::records::feature::path_features::DesignRevolveTravel::Angle(
                cadmpeg_ir::scalar::PositiveAngle::new(1.5).unwrap()
            )
        );
        assert!(!construction.reversed);
        let loss = loss.expect("second tail reference loss");
        assert_eq!(
            loss.code,
            crate::loss::F3dLossCode::FeatureRevolveLayoutAssumed.kind()
        );
        assert!(loss.message.contains("target 60"));
        assert!(loss.message.contains("379-byte"));
        let negative = Fixture {
            angle: -1.5,
            ..fixture.clone()
        };
        assert!(negative.decode().unwrap().reversed);
        for invalid in [
            Fixture {
                references: 6,
                ..fixture.clone()
            },
            Fixture {
                direction_kind: 1,
                ..fixture.clone()
            },
            Fixture {
                angle_marker: 0,
                ..fixture.clone()
            },
            Fixture {
                solid: 2,
                ..fixture
            },
        ] {
            assert!(invalid.decode().is_none());
        }
    }
    assert_eq!(Fixture::unmarked().decode_with_loss().1, None);
}

#[test]
fn a_379_byte_frame_requires_the_88_byte_second_reference_tail() {
    let fixture = Fixture {
        payload_length: Some(379),
        ..Fixture::unmarked()
    };
    assert_eq!(fixture.decode_with_loss(), (None, None));
    let mut short_tail = fixture.bytes();
    let tail = short_tail.len() - 11 - 78;
    short_tail[tail + 20] = 1;
    short_tail[tail + 21..tail + 29].copy_from_slice(&60_u64.to_le_bytes());
    assert_eq!(fixture.decode_bytes(&short_tail), (None, None));
    let fixture = Fixture {
        second_reference: Some(60),
        ..fixture
    };
    assert!(fixture.decode().is_some());
    let bytes = fixture.bytes();
    let tail = bytes.len() - 11 - 88;
    for (offset, value) in [(30, 0), (30, 2), (39, 1), (40, 1)] {
        let mut invalid = bytes.clone();
        invalid[tail + offset] = value;
        assert_eq!(fixture.decode_bytes(&invalid), (None, None), "{offset}");
    }
}

#[test]
fn second_tail_references_refuse_ordered_scope_members() {
    for base in [Fixture::marked(), Fixture::unmarked()] {
        for target in [u64::from(ANGLE), 96] {
            let fixture = Fixture {
                second_reference: Some(target),
                ..base.clone()
            };
            assert_eq!(fixture.decode_with_loss(), (None, None));
        }
        let fixture = Fixture {
            second_reference: Some(u64::from(u32::MAX) + u64::from(ANGLE) + 1),
            ..base
        };
        assert!(fixture.decode_with_loss().1.is_some());
    }
}

#[test]
fn marker_one_assumed_lengths_use_the_matched_layout_for_loss() {
    for length in [361, 379] {
        let fixture = Fixture {
            payload_length: Some(length),
            project_axis: 1,
            solid: 0,
            angle_marker: 0,
            second_reference: Some(60),
            ..Fixture::marked()
        };
        let (construction, loss) = fixture.decode_with_loss();
        let construction = construction.expect("marker-one assumed-length Revolve");
        assert_eq!(construction.operation_offset, 25);
        assert_eq!(construction.solid, None);
        assert_eq!(construction.operation, DesignExtrudeOperation::NewBody);
        assert_eq!(loss, None);
    }
}
