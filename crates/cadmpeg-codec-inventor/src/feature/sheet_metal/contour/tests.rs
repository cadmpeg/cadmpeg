// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::feature::{PmDcFeatureEnumFamily as EnumFamily, PmDcFeaturePropertyKind as Kind};
use crate::test_support::sheet_metal::{feature, reference, ContourFixture};
use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchEntityId, SketchId, SketchPlacement, SketchProfiles};

fn context_run(test: impl FnOnce(&DecodeContext<'_>)) {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    test(&ctx);
}

fn sketch(name: &str) -> Sketch {
    Sketch {
        id: SketchId::mint(format!("test:design:sketch#{name}")).expect("id"),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("plane"),
        profiles: SketchProfiles::default(),
        native_ref: None,
    }
}

fn curves(names: &[&str]) -> Vec<SketchEntityId> {
    names
        .iter()
        .map(|name| SketchEntityId::mint(format!("test:design:sketch-entity#{name}")).expect("id"))
        .collect()
}

fn open_construction(fixture: &ContourFixture) -> SheetMetalContourFlangeConstruction {
    let sketch = sketch("open");
    let profile = PlanarProfileRef::sketch_entities(sketch.id.clone(), curves(&["floor", "wall"]))
        .expect("profile");
    fixture
        .operands()
        .expect("contour operands")
        .construction(profile)
        .expect("construction")
}

#[test]
fn contour_operands_read_travel_distance_thickness_and_radius() {
    let fixture = ContourFixture::new();
    let values = fixture.operands().expect("contour operands");
    assert_eq!(values.direction, UnitVector3::Z_AXIS);
    assert_eq!(values.distance.get(), 29.0);
    assert_eq!(values.thickness.get(), 7.0);
    assert_eq!(values.inside_radius.get(), 11.0);
    let construction = open_construction(&fixture);
    assert_eq!(
        construction.profile,
        Some(
            PlanarProfileRef::sketch_entities(
                sketch("open").id.clone(),
                curves(&["floor", "wall"])
            )
            .expect("profile")
        )
    );
    assert_eq!(
        construction.direction,
        Some(FeatureDirection3::from(UnitVector3::Z_AXIS))
    );
    let Some(ExtrudeExtent::OneSided {
        side:
            ExtrudeSide {
                termination: LinearTermination::Blind { length },
                draft: None,
            },
    }) = construction.extent
    else {
        panic!("one-sided blind extent");
    };
    assert_eq!(length.get(), 29.0);
    assert_eq!(construction.thickness.map(PositiveLength::get), Some(7.0));
    assert_eq!(
        construction.inside_radius.map(NonNegativeLength::get),
        Some(11.0)
    );
    assert_eq!(
        construction.side,
        Some(SheetMetalContourThicknessSide::Right)
    );
    assert_eq!(construction.neutral_factor, None);
}

#[test]
fn contour_other_plate_side_keeps_thickness_side_unresolved() {
    let mut fixture = ContourFixture::new();
    fixture.edit(3, |kind| {
        *kind = Kind::Enumeration {
            family: EnumFamily::PlateSide,
            type_value: 2,
            value: 1,
        };
    });
    assert_eq!(thickness_side(fixture.property(reference(3))), None);
    assert_eq!(fixture.operands(), Err(ContourRefusal::Side));
}

#[test]
fn contour_travel_against_the_profile_normal_keeps_its_sign() {
    let fixture = ContourFixture::new();
    let values = operands(
        fixture.children(),
        Some(&fixture.style),
        |r| fixture.property(r),
        ContourFixture::length,
        |r| ContourFixture::direction(r).map(UnitVector3::reversed),
        ContourFixture::stored,
        UnitVector3::Z_AXIS,
    )
    .expect("reversed travel");
    assert_eq!(values.direction, UnitVector3::Z_AXIS.reversed());
}

fn refusal(edit: impl FnOnce(&mut ContourFixture)) -> ContourRefusal {
    let mut fixture = ContourFixture::new();
    edit(&mut fixture);
    fixture.operands().expect_err("refused contour")
}

fn boolean(value: bool) -> Kind {
    Kind::Boolean {
        name: String::new(),
        name_value: 0,
        value,
    }
}

fn not_boolean() -> Kind {
    Kind::SheetMetal {
        definition: SheetMetalProperty::PlateControl { value: 1 },
    }
}

fn edit_attachment(fixture: &mut ContourFixture, edit: impl FnOnce(&mut SheetMetalProperty)) {
    fixture.edit(5, |kind| {
        let Kind::SheetMetal { definition } = kind else {
            panic!("contour attachment");
        };
        edit(definition);
    });
}

/// The operands of `fixture` with replaced length and direction readers.
fn operands_with(
    fixture: &ContourFixture,
    length: impl Fn(PmDcReference) -> Option<Length>,
    direction: impl Fn(PmDcReference) -> Option<UnitVector3>,
) -> Result<ContourOperands, ContourRefusal> {
    operands(
        fixture.children(),
        Some(&fixture.style),
        |r| fixture.property(r),
        length,
        direction,
        ContourFixture::stored,
        UnitVector3::Z_AXIS,
    )
}

#[test]
fn contour_plate_records_name_the_first_refused_check() {
    let fixture = ContourFixture::new();
    let missing_style = operands(
        fixture.children(),
        None,
        |r| fixture.property(r),
        ContourFixture::length,
        ContourFixture::direction,
        ContourFixture::stored,
        UnitVector3::Z_AXIS,
    );
    assert_eq!(missing_style, Err(ContourRefusal::Style));
    assert_eq!(
        refusal(|f| f.plate = feature(100, &[1, 2, 3, 50, 5, 0, 0, 6, 7, 8, 9, 10, 0, 12, 0])),
        ContourRefusal::PropertyCount
    );
    assert_eq!(
        refusal(|f| {
            f.plate = feature(
                100,
                &[1, 2, 3, 50, 5, 0, 0, 6, 7, 8, 9, 10, 51, 12, 0, 13, 14],
            );
        }),
        ContourRefusal::ReservedSlot
    );
    assert_eq!(
        refusal(|f| f.edit(14, |kind| {
            *kind = Kind::Enumeration {
                family: EnumFamily::PartOperation,
                type_value: 5,
                value: 3,
            };
        })),
        ContourRefusal::PartOperation
    );
    assert_eq!(
        refusal(|f| f.edit(12, |kind| {
            *kind = Kind::SheetMetal {
                definition: SheetMetalProperty::PlateControl { value: 1 },
            };
        })),
        ContourRefusal::PlateControl
    );
    assert_eq!(
        refusal(|f| f.edit(3, |kind| *kind = boolean(false))),
        ContourRefusal::Side
    );
    let in_plane = operands_with(&fixture, ContourFixture::length, |r| {
        ContourFixture::direction(r).map(|_| UnitVector3::X_AXIS)
    });
    assert_eq!(in_plane, Err(ContourRefusal::TravelDirection));
    let no_direction = operands_with(&fixture, ContourFixture::length, |_| None);
    assert_eq!(no_direction, Err(ContourRefusal::Direction));
    assert_eq!(
        refusal(|f| f.edit(29, |kind| *kind = not_boolean())),
        ContourRefusal::Relief
    );
    let mut false_control = ContourFixture::new();
    false_control.edit(29, |kind| *kind = boolean(false));
    assert!(false_control.operands().is_ok());
    assert_eq!(
        refusal(|f| f.edit(35, |kind| *kind = not_boolean())),
        ContourRefusal::Transition
    );
    let mut false_control = ContourFixture::new();
    false_control.edit(35, |kind| *kind = boolean(false));
    assert!(false_control.operands().is_ok());
    for (missing, expected) in [
        (50, ContourRefusal::Distance),
        (4, ContourRefusal::Thickness),
        (25, ContourRefusal::Radius),
    ] {
        let result = operands_with(
            &fixture,
            |r| {
                (r.index() != missing)
                    .then(|| ContourFixture::length(r))
                    .flatten()
            },
            ContourFixture::direction,
        );
        assert_eq!(result, Err(expected));
    }
}

#[test]
fn contour_attachment_records_name_the_first_refused_check() {
    assert_eq!(
        refusal(|f| f.edit(5, |kind| {
            *kind = Kind::SheetMetal {
                definition: SheetMetalProperty::PlateControl { value: 0 },
            };
        })),
        ContourRefusal::AttachmentControl
    );
    assert_eq!(
        refusal(|f| f.edit(23, |kind| *kind = boolean(true))),
        ContourRefusal::AttachmentBoolean
    );
    assert_eq!(
        refusal(|f| f.edit(22, |kind| {
            *kind = Kind::SheetMetal {
                definition: SheetMetalProperty::Enumeration {
                    type_value: 9,
                    value: 1,
                },
            };
        })),
        ContourRefusal::AttachmentMode
    );
    assert_eq!(
        refusal(|f| edit_attachment(f, |definition| {
            if let SheetMetalProperty::ContourAttachment { value, .. } = definition {
                *value = 1;
            }
        })),
        ContourRefusal::AttachmentStates
    );
    assert_eq!(
        refusal(|f| f.edit(51, |kind| {
            if let Kind::SheetMetal {
                definition: SheetMetalProperty::AttachmentState { radius, .. },
            } = kind
            {
                *radius = reference(26);
            }
        })),
        ContourRefusal::AttachmentExtension
    );
    for slot in [0, 1] {
        assert_eq!(
            refusal(|f| edit_attachment(f, |definition| {
                if let SheetMetalProperty::ContourAttachment { directions, .. } = definition {
                    directions[slot] = reference(33);
                }
            })),
            ContourRefusal::AttachmentDirection
        );
    }
}

#[test]
fn contour_bend_and_corner_must_repeat_the_plate_operands() {
    for bend in [
        [6, 25, 50, 5, 9, 0, 7, 10, 44, 13, 45],
        [6, 26, 4, 5, 9, 0, 7, 10, 44, 13, 45],
        [6, 25, 4, 5, 9, 46, 7, 10, 44, 13, 45],
    ] {
        assert_eq!(
            refusal(|f| f.bend = feature(200, &bend)),
            ContourRefusal::Bend
        );
    }
    assert_eq!(
        refusal(|f| f.corner = feature(300, &[8, 0, 52, 0, 0, 0, 13])),
        ContourRefusal::Corner
    );
    assert_eq!(
        refusal(|f| f.corner = feature(300, &[8, 0, 52, 0, 1, 0])),
        ContourRefusal::Corner
    );
}

#[test]
fn contour_refusal_details_are_pinned() {
    let details: Vec<_> = [
        ContourRefusal::Members,
        ContourRefusal::ProfileSketch,
        ContourRefusal::Style,
        ContourRefusal::PropertyCount,
        ContourRefusal::StyleVersion,
        ContourRefusal::ReservedSlot,
        ContourRefusal::EdgeAttached(2),
        ContourRefusal::PartOperation,
        ContourRefusal::ProfileSelection,
        ContourRefusal::EdgeCollection,
        ContourRefusal::FeatureDimensions,
        ContourRefusal::CornerControl,
        ContourRefusal::PlateControl,
        ContourRefusal::AttachmentControl,
        ContourRefusal::AttachmentBoolean,
        ContourRefusal::AttachmentMode,
        ContourRefusal::AttachmentStates,
        ContourRefusal::AttachmentExtension,
        ContourRefusal::AttachmentDirection,
        ContourRefusal::Side,
        ContourRefusal::Direction,
        ContourRefusal::TravelDirection,
        ContourRefusal::Relief,
        ContourRefusal::Transition,
        ContourRefusal::Length,
        ContourRefusal::Distance,
        ContourRefusal::Thickness,
        ContourRefusal::Radius,
        ContourRefusal::Bend,
        ContourRefusal::Corner,
        ContourRefusal::Profile,
        ContourRefusal::Result,
    ]
    .into_iter()
    .map(ContourRefusal::detail)
    .collect();
    assert_eq!(
        details,
        [
            "members",
            "profile-sketch",
            "style",
            "property-count",
            "style-version",
            "reserved-slot",
            "edge-attached",
            "part-operation",
            "profile-selection",
            "edge-collection",
            "feature-dimensions",
            "corner-control",
            "plate-control",
            "attachment-control",
            "attachment-boolean",
            "attachment-mode",
            "attachment-states",
            "attachment-extension",
            "attachment-direction",
            "side",
            "direction",
            "travel-direction",
            "relief",
            "transition",
            "length",
            "distance",
            "thickness",
            "radius",
            "bend",
            "corner",
            "profile",
            "result",
        ]
    );
}

fn native(id: &str, native_ref: &str) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint(format!("test:model:feature#{id}")).expect("feature identity"),
        ordinal: 1,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Native {
                kind: cadmpeg_ir::features::NativeFeatureKind::Other("Plate".into()),
                parameters: BTreeMap::new(),
            },
        )),
        native_ref: Some(native_ref.to_owned()),
    }
}

#[test]
fn refused_contours_report_one_loss_for_each_native_plate() {
    let mut refusals = Refusals::default();
    refusals.insert("plate".to_owned(), ContourRefusal::Bend);
    refusals.insert("plate".to_owned(), ContourRefusal::Corner);
    let losses = refusals.losses(&[native("a", "plate"), native("b", "other")]);
    assert_eq!(losses.len(), 1);
    let text = cadmpeg_core::serde_erase::json::to_string(&losses).expect("losses");
    assert!(text.contains("feature.sheet-metal-contour-flange-refused"));
    assert!(text.contains("contour-flange check is bend."));
}

fn flange(type_id: [u8; 16]) -> Flange {
    let mut bytes = crate::test_support::test_fixtures::content_at(13, 29, &[]);
    bytes.extend_from_slice(&(-1_i32).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&0x3000_u16.to_le_bytes());
    for value in [3_u32, 3, 16, 101, 201, 301, 0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let payload = crate::test_support::test_fixtures::parse(&bytes, |ctx, view| {
        super::super::flange::parse(ctx, view, 29)
    })
    .expect("flange");
    crate::record_identity::Located::new(
        payload,
        crate::record_identity::RecordTypeId::from_bytes(type_id),
        cadmpeg_ir::ids::IdentityKey::try_new("fixture").expect("token"),
        12,
    )
}

#[test]
fn contour_children_are_the_children_of_a_contour_parent() {
    let fixture = ContourFixture::new();
    let contour = [flange(super::super::flange::CONTOUR_TYPE)];
    let edge = [flange(super::super::flange::TYPE)];
    for child in [&fixture.plate, &fixture.bend, &fixture.corner] {
        assert!(is_child(&contour, child));
        assert!(!is_child(&edge, child));
    }
    assert!(!is_child(&contour, &feature(400, &[])));
}

#[test]
fn contour_without_labels_stays_native_with_the_member_refusal() {
    let fixture = ContourFixture::new();
    let inventory = FeatureInventory {
        features: vec![
            fixture.plate.clone(),
            fixture.bend.clone(),
            fixture.corner.clone(),
        ],
        sheet_metal_flanges: vec![
            flange(super::super::flange::CONTOUR_TYPE),
            flange(super::super::flange::TYPE),
        ],
        ..FeatureInventory::default()
    };
    let index = crate::feature::tests::test_projection_index(
        &fixture.properties,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    context_run(|ctx| {
        let mut outcomes = project_all(ctx, &inventory, &index, |_, _| None).expect("project");
        assert!(outcomes.consumed().is_empty());
        assert!(matches!(
            outcomes.take("fixture", 100),
            Some(Err(ContourRefusal::Members))
        ));
        assert!(outcomes.take("fixture", 100).is_none());
    });
}

#[test]
fn the_contour_result_is_the_new_body_collection_of_the_plate() {
    let fixture = ContourFixture::new();
    let index = crate::feature::tests::test_projection_index(
        &fixture.properties,
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
    );
    context_run(|ctx| {
        let (id, result) = crate::feature::plate_result(ctx, &fixture.plate, &index)
            .expect("new body")
            .expect("charge");
        assert_eq!(result.output_of, id);
        assert_eq!(
            result.native_ref,
            Some(fixture.property(reference(13)).expect("collection").id())
        );
    });
}

#[test]
fn projected_contours_report_retained_controls_and_style_forms() {
    let fixture = ContourFixture::new();
    let mut feature = native("contour", "plate");
    feature.evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::SheetMetalContourFlange {
            construction: open_construction(&fixture),
        },
    ));
    let losses = super::super::retained_control_losses([&feature]);
    assert_eq!(losses.len(), 2);
    let text = cadmpeg_core::serde_erase::json::to_string(&losses).expect("losses");
    assert!(text.contains("feature.sheet-metal-contour-controls-retained"));
    assert!(text.contains("neutral factor stays unresolved"));
    assert!(text.contains("feature.sheet-metal-style-forms-native"));
}

#[test]
fn the_travel_parameter_is_a_length_or_internal_centimetres() {
    use cadmpeg_ir::features::parameters::ParameterValue;
    let length = |value: f64| Length::new(value).expect("length");
    assert_eq!(
        travel_length(&ParameterValue::Length(length(37.0))),
        Some(length(37.0))
    );
    assert_eq!(
        travel_length(&ParameterValue::Real(
            cadmpeg_ir::scalar::FiniteReal::new(3.7).expect("real")
        )),
        Some(length(37.0))
    );
    assert_eq!(travel_length(&ParameterValue::Integer(4)), None);
    let fixture = ContourFixture::new();
    let real = ParameterValue::Real(cadmpeg_ir::scalar::FiniteReal::new(3.0).expect("real"));
    let lengths = plate_lengths(
        &fixture.plate,
        |r| (r.index() == 50).then_some(&real),
        ContourFixture::length,
    );
    assert_eq!(lengths(reference(50)).map(Length::get), Some(30.0));
    assert_eq!(lengths(reference(4)), ContourFixture::length(reference(4)));
}

mod attachments;
