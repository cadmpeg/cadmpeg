// SPDX-License-Identifier: Apache-2.0
use crate::design::dimensions::{indirect_angular_lines, presentation_dimension_definition};
use crate::design::test_support::parameter_record;
use cadmpeg_core::decode::{DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{
    SketchConstraintDefinitionInput, SketchEntity, SketchEntityId, SketchGeometry,
    SketchGeometryDefinition, SketchId,
};
use std::collections::BTreeMap;

fn entity(name: &str, geometry: SketchGeometryDefinition) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:selector#{name}")).unwrap(),
        SketchId::mint("synthetic:test:sketch#selector").unwrap(),
        SketchGeometry::try_from(geometry).unwrap(),
    )
}

fn line(name: &str, start: (f64, f64), end: (f64, f64)) -> SketchEntity {
    entity(
        name,
        SketchGeometryDefinition::Line {
            start: Point2::new(start.0, start.1),
            end: Point2::new(end.0, end.1),
        },
    )
}

fn presentation(records: &[u32]) -> crate::records::dimensions::DesignDimensionPresentationFrame {
    crate::records::dimensions::DesignDimensionPresentationFrame {
        id: "native:presentation#0".into(),
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("314".to_owned()).unwrap(),
        record_index: 0,
        frame_length: 0,
        operands: records
            .iter()
            .map(
                |&record| crate::records::dimensions::DesignDimensionPresentationOperand {
                    geometry_record_index: std::num::NonZeroU32::new(record).unwrap(),
                    geometry_reference_offset: 0,
                    role: 0,
                    role_offset: 0,
                },
            )
            .collect(),
        presentation_bytes: Vec::new(),
        presentation_byte_offset: 0,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("281".to_owned())
            .unwrap(),
        paired_byte_offset: 0,
        owner_reference: 0,
        owner_reference_offset: 0,
        governing_owner_record_index: 0,
        governing_parameter_record_index: 0,
        governing_companion_record_index: 0,
    }
}

fn parameter() -> crate::records::parameters::DesignParameter {
    crate::design::decode::parameters::parse_design_parameter_record(&parameter_record(
        Some(1),
        "2 mm",
        "Linear Dimension-2",
        Some("mm"),
        "d1",
        0.2,
    ))
    .unwrap()
}

fn parameter_id() -> ParameterId {
    ParameterId::mint("synthetic:test:parameter#selector").unwrap()
}

fn rejected_presentation(records: &[u32], entities: &[SketchEntity], zero_work: bool) {
    let frame = presentation(records);
    let parameter = parameter();
    let parameter_id = parameter_id();
    let projected = entities
        .iter()
        .enumerate()
        .map(|(index, value)| (("native", u32::try_from(index + 1).unwrap()), value))
        .collect();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    if zero_work {
        policy.limits.max_work_units = 0;
    }
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(presentation_dimension_definition(
            ctx,
            "native",
            &frame,
            &projected,
            &parameter,
            &parameter_id,
            0.0
        )
        .transpose()
        .unwrap()
        .is_none());
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn presentation_empty_operands_do_no_work() {
    rejected_presentation(&[], &[], true);
}

#[test]
fn presentation_excess_operands_do_no_work() {
    let entities = [line("first", (0.0, 0.0), (2.0, 0.0))];
    rejected_presentation(&[1, 1, 1], &entities, true);
}

#[test]
fn presentation_missing_record_copies_no_ids() {
    rejected_presentation(&[1], &[], false);
}

#[test]
fn presentation_mismatched_sketch_copies_no_ids() {
    let first = line("first", (0.0, 0.0), (2.0, 0.0));
    let mut second = line("second", (0.0, 1.0), (2.0, 1.0));
    second.sketch = SketchId::mint("synthetic:test:sketch#other").unwrap();
    rejected_presentation(&[1, 2], &[first, second], false);
}

#[test]
fn presentation_rejected_measurement_copies_no_ids() {
    rejected_presentation(&[1], &[line("first", (0.0, 0.0), (3.0, 0.0))], false);
}

#[test]
fn presentation_selected_line_keeps_only_output_ids() {
    let first = line("first", (0.0, 0.0), (2.0, 0.0));
    let frame = presentation(&[1]);
    let parameter = parameter();
    let parameter_id = parameter_id();
    let projected = BTreeMap::from([(("native", 1), &first)]);
    let mut policy = DecodePolicy::service();
    let bytes = u64::try_from(2 * first.id().as_str().len() + parameter_id.as_str().len()).unwrap();
    policy.limits.max_retained_bytes = bytes;
    policy.limits.max_materialized_bytes = bytes;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let definition = presentation_dimension_definition(
            ctx,
            "native",
            &frame,
            &projected,
            &parameter,
            &parameter_id,
            0.0,
        )
        .transpose()
        .unwrap()
        .unwrap();
        assert_eq!(
            definition,
            SketchConstraintDefinitionInput::DistanceLoci {
                first: cadmpeg_ir::sketches::SketchLocus::Start(first.id().clone()),
                second: cadmpeg_ir::sketches::SketchLocus::End(first.id().clone()),
                parameter: parameter_id.clone(),
            }
        );
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn presentation_sketch_equality_refuses_work_limit() {
    let first = line("first", (0.0, 0.0), (2.0, 0.0));
    let second = line("second", (0.0, 1.0), (2.0, 1.0));
    let frame = presentation(&[1, 2]);
    let parameter = parameter();
    let parameter_id = parameter_id();
    let projected = BTreeMap::from([(("native", 1), &first), (("native", 2), &second)]);
    super::assert_dimension_refusal(
        "f3d presentation sketch equality",
        ResourceDimension::WorkUnits,
        |ctx| {
            presentation_dimension_definition(
                ctx,
                "native",
                &frame,
                &projected,
                &parameter,
                &parameter_id,
                0.0,
            )
            .transpose()
            .map(|_| ())
        },
    );
}

struct AngularFixture {
    point: SketchEntity,
    explicit: SketchEntity,
    first: SketchEntity,
    second: SketchEntity,
    tail: SketchEntity,
}

impl AngularFixture {
    fn new() -> Self {
        Self {
            point: entity(
                "point",
                SketchGeometryDefinition::Point {
                    position: Point2::new(0.0, 0.0),
                },
            ),
            explicit: line("explicit", (2.0, -2.0), (2.0, 2.0)),
            first: line("first", (0.0, 0.0), (2.0, 2.0)),
            second: line("second", (0.0, 0.0), (4.0, 4.0)),
            tail: line("tail", (0.0, 0.0), (6.0, 6.0)),
        }
    }

    fn projected(&self, tail: bool) -> BTreeMap<(&str, u32), &SketchEntity> {
        let mut map = BTreeMap::from([(("native", 1), &self.first), (("native", 2), &self.second)]);
        if tail {
            map.insert(("native", 3), &self.tail);
        }
        map
    }

    fn ambiguous(
        &self,
        ctx: &DecodeContext<'_>,
        tail: bool,
    ) -> Result<(), cadmpeg_core::CodecError> {
        assert!(indirect_angular_lines(
            ctx,
            "native",
            &[&self.point, &self.explicit],
            std::f64::consts::FRAC_PI_4,
            &self.projected(tail)
        )?
        .is_none());
        ctx.equal("done", "done", "selector end probe")?;
        Ok(())
    }
}

#[test]
fn indirect_angular_ambiguity_stops_before_the_third_candidate() {
    let fixture = AngularFixture::new();
    let refusal = |tail| {
        crate::test_support::resource_refusal_at(
            ResourceDimension::WorkUnits,
            "selector end probe",
            0,
            |ctx| fixture.ambiguous(ctx, tail),
        )
    };
    let cadmpeg_core::CodecError::ResourceLimit(first) = refusal(false) else {
        panic!("expected end probe refusal");
    };
    let cadmpeg_core::CodecError::ResourceLimit(second) = refusal(true) else {
        panic!("expected end probe refusal");
    };
    assert_eq!(first.used, second.used);
    assert_eq!(first.additional, second.additional);
}

#[test]
fn indirect_angular_ambiguity_copies_no_ids() {
    let fixture = AngularFixture::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| fixture.ambiguous(ctx, true)).unwrap();
}

#[test]
fn indirect_angular_duplicate_identity_keeps_the_first_candidate() {
    let fixture = AngularFixture::new();
    let duplicate = line("first", (0.0, 0.0), (4.0, 4.0));
    let projected = BTreeMap::from([(("native", 1), &fixture.first), (("native", 2), &duplicate)]);
    let selected = crate::test_support::with_decode_context(|ctx| {
        indirect_angular_lines(
            ctx,
            "native",
            &[&fixture.point, &fixture.explicit],
            std::f64::consts::FRAC_PI_4,
            &projected,
        )
    })
    .unwrap()
    .unwrap();
    assert_eq!(
        selected,
        (fixture.first.id().clone(), fixture.explicit.id().clone())
    );
}

#[test]
fn indirect_angular_candidate_identity_refuses_work_limit() {
    let fixture = AngularFixture::new();
    super::assert_dimension_refusal(
        "f3d indirect angular candidate equality",
        ResourceDimension::WorkUnits,
        |ctx| {
            indirect_angular_lines(
                ctx,
                "native",
                &[&fixture.point, &fixture.explicit],
                std::f64::consts::FRAC_PI_4,
                &fixture.projected(false),
            )
            .map(|_| ())
        },
    );
}

#[test]
fn indirect_angular_filtered_records_refuse_original_source_work() {
    let fixture = AngularFixture::new();
    let projected = BTreeMap::from([(("other", 1), &fixture.first)]);
    super::assert_dimension_refusal(
        "f3d indirect angular candidate source",
        ResourceDimension::WorkUnits,
        |ctx| {
            indirect_angular_lines(
                ctx,
                "native",
                &[&fixture.point, &fixture.explicit],
                std::f64::consts::FRAC_PI_4,
                &projected,
            )
            .map(|_| ())
        },
    );
}

#[test]
fn indirect_angular_wrong_cardinality_does_no_work() {
    let fixture = AngularFixture::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert!(indirect_angular_lines(
            ctx,
            "native",
            &[&fixture.point],
            std::f64::consts::FRAC_PI_4,
            &fixture.projected(true)
        )
        .unwrap()
        .is_none());
        assert!(ctx.resource_refusal().is_none());
    });
}
