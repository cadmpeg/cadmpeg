use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::text::NonBlankString;

use crate::geometry::pcurve::{PcurveNurbs, PcurveNurbsPoles, WeightedPole2};
use crate::math::Point2;
use crate::scalar::{Angle, Length, NonZeroReal};
use crate::sketches::{
    EllipseRadii, SketchFontWeight, SketchGeometry, SketchGeometryDefinition,
    SketchTextHorizontalAlignment, SketchTextVerticalAlignment, TextPlacement,
};

fn field_cost(ctx: &DecodeContext<'_>, geometry: &SketchGeometry) -> u64 {
    DecodeCost::decode_cost(geometry, ctx, "IR sketch geometry field cost")
        .expect("sketch geometry fields fit the cost budget")
}

#[test]
fn sketch_geometry_cost_counts_active_fields_and_tags() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("root context");

    let point = SketchGeometry::try_from(SketchGeometryDefinition::Point {
        position: Point2::new(1.0, 2.0),
    })
    .expect("finite point");
    assert_eq!(field_cost(&ctx, &point), 17);

    let line = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(1.0, 2.0),
        end: Point2::new(3.0, 4.0),
    })
    .expect("finite line");
    assert_eq!(field_cost(&ctx, &line), 33);

    let reference_line = SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
        origin: Point2::new(1.0, 2.0),
        direction: Point2::new(3.0, 4.0),
    })
    .expect("nonzero reference line");
    assert_eq!(field_cost(&ctx, &reference_line), 33);

    let circle = SketchGeometry::try_from(SketchGeometryDefinition::Circle {
        center: Point2::new(1.0, 2.0),
        radius: Length::new(3.0).expect("finite radius"),
    })
    .expect("positive circle radius");
    assert_eq!(field_cost(&ctx, &circle), 25);

    let arc = SketchGeometry::try_from(SketchGeometryDefinition::Arc {
        center: Point2::new(1.0, 2.0),
        radius: Length::new(3.0).expect("finite radius"),
        start_angle: Angle::new(0.25).expect("finite start angle"),
        end_angle: Angle::new(0.75).expect("finite end angle"),
    })
    .expect("positive arc radius");
    assert_eq!(field_cost(&ctx, &arc), 41);

    let ellipse = SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
        center: Point2::new(1.0, 2.0),
        major_angle: Angle::new(0.25).expect("finite major angle"),
        radii: EllipseRadii {
            major_radius: Length::new(4.0).expect("finite major radius"),
            minor_radius: Length::new(2.0).expect("finite minor radius"),
        },
        bounds: Some([
            Angle::new(0.0).expect("finite first bound"),
            Angle::new(1.0).expect("finite second bound"),
        ]),
    })
    .expect("ordered ellipse radii");
    assert_eq!(field_cost(&ctx, &ellipse), 58);

    let hyperbola = SketchGeometry::try_from(SketchGeometryDefinition::Hyperbola {
        center: Point2::new(1.0, 2.0),
        major_angle: Angle::new(0.25).expect("finite major angle"),
        major_radius: Length::new(4.0).expect("finite major radius"),
        minor_radius: Length::new(2.0).expect("finite minor radius"),
        bounds: Some([-1.0, 1.0]),
    })
    .expect("finite hyperbola bounds");
    assert_eq!(field_cost(&ctx, &hyperbola), 58);

    let parabola = SketchGeometry::try_from(SketchGeometryDefinition::Parabola {
        vertex: Point2::new(1.0, 2.0),
        axis_angle: Angle::new(0.25).expect("finite axis angle"),
        focal_length: Length::new(3.0).expect("finite focal length"),
        bounds: Some([-2.0, 2.0]),
    })
    .expect("finite parabola bounds");
    assert_eq!(field_cost(&ctx, &parabola), 50);

    let text = SketchGeometry::try_from(SketchGeometryDefinition::Text {
        text: NonBlankString::try_from("label").expect("text"),
        font_family: NonBlankString::try_from("font").expect("font family"),
        font_weight: SketchFontWeight::Medium,
        height: Length::new(2.0).expect("finite height"),
        width_factor: Some(1.5),
        placement: Some(TextPlacement {
            anchor: Point2::new(2.0, 3.0),
            rotation: Angle::new(0.25).expect("finite text rotation"),
        }),
        horizontal_alignment: Some(SketchTextHorizontalAlignment::Native(7)),
        vertical_alignment: Some(SketchTextVerticalAlignment::Middle),
    })
    .expect("text geometry");
    assert_eq!(field_cost(&ctx, &text), 61);

    let text_without_optional_fields = SketchGeometry::try_from(SketchGeometryDefinition::Text {
        text: NonBlankString::try_from("x").expect("text"),
        font_family: NonBlankString::try_from("f").expect("font family"),
        font_weight: SketchFontWeight::Regular,
        height: Length::new(1.0).expect("finite height"),
        width_factor: None,
        placement: None,
        horizontal_alignment: None,
        vertical_alignment: None,
    })
    .expect("text geometry");
    assert_eq!(field_cost(&ctx, &text_without_optional_fields), 16);

    let external = SketchGeometry::try_from(SketchGeometryDefinition::ExternalReference {
        document: Some(String::from("doc")),
        object: NonBlankString::try_from("object").expect("object identity"),
        subelements: vec![String::from("edge"), String::from("vertex")],
    })
    .expect("external reference");
    assert_eq!(field_cost(&ctx, &external), 21);

    let native = SketchGeometry::native(NonBlankString::try_from("native").expect("native kind"));
    assert_eq!(field_cost(&ctx, &native), 7);

    let polynomial = PcurveNurbs::new(
        &ctx,
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        PcurveNurbsPoles::Polynomial {
            points: vec![Point2::new(1.0, 2.0), Point2::new(3.0, 4.0)],
        },
        false,
    )
    .expect("polynomial pcurve admission")
    .expect("valid polynomial pcurve");
    assert_eq!(field_cost(&ctx, &SketchGeometry::nurbs(polynomial)), 71);

    let rational = PcurveNurbs::new(
        &ctx,
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        PcurveNurbsPoles::Rational {
            points: vec![
                WeightedPole2 {
                    point: Point2::new(1.0, 2.0),
                    weight: NonZeroReal::new(1.0).expect("nonzero weight"),
                },
                WeightedPole2 {
                    point: Point2::new(3.0, 4.0),
                    weight: NonZeroReal::new(2.0).expect("nonzero weight"),
                },
            ],
        },
        true,
    )
    .expect("rational pcurve admission")
    .expect("valid rational pcurve");
    assert_eq!(field_cost(&ctx, &SketchGeometry::nurbs(rational)), 87);
}

#[test]
fn sketch_geometry_equality_refuses_before_comparing_owned_text() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root context");
    let geometry = SketchGeometry::native(
        NonBlankString::try_from("native-geometry").expect("nonblank geometry kind"),
    );

    let error = ctx
        .equal(&geometry, &geometry, "IR sketch geometry equality")
        .expect_err("the owned text exceeds the work budget");
    let cadmpeg_core::CodecError::ResourceLimit(resource) = error else {
        panic!("expected a work refusal");
    };
    assert_eq!(resource.dimension, ResourceDimension::WorkUnits);
    assert_eq!(resource.operation, "IR sketch geometry equality");
}
