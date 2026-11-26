// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::f64::consts::FRAC_PI_2;

use crate::design::construction::{
    kinds, AxisValue, Construction, ConstructionKind, FrameValue, PlaneValue,
};
use crate::design::container::Container;
use crate::design::dimension::Dimension;
use crate::design::feature::FeatureOperation;
use crate::design::ops::assembly::{
    MirrorComponents, MirrorOrientation, MirrorPivot, MirrorResult, MirrorReversal, Place,
    PlacementBase,
};
use crate::design::ops::body::{TransformBodies, TransformStep, Transformation};
use crate::design::selection::{
    Anchor, Members, OccurrencePath, PathStep, PickContext, PickTarget, Picked, Picks, Selection,
    StepRef,
};
use crate::document::CadIr;
use crate::features::geometry::FinitePoint3;
use crate::math::{Point3, Vector3};
use crate::products::Occurrence;
use crate::report::Severity;
use crate::scalar::{Angle, Length, PositiveLength};
use crate::transform::Transform;
use crate::units::UnitVector3;
use crate::validate::design::fixture::{design_findings, document, id};

/// The linear tolerance in force in these tests, exact in binary64.
const LINEAR: f64 = 1.0 / 1024.0;

fn fixture() -> CadIr {
    let mut ir = document();
    ir.tolerances.linear = PositiveLength::new(LINEAR).unwrap();
    ir
}

fn warnings(ir: &CadIr, fragment: &str) -> usize {
    design_findings(ir)
        .iter()
        .filter(|finding| {
            finding.severity == Severity::Warning && finding.message.contains(fragment)
        })
        .count()
}

fn resolved<K: ConstructionKind>(value: Option<K::Value>) -> Construction<K> {
    Construction {
        inputs: Vec::new(),
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved: value,
        resolved_in: BTreeMap::new(),
    }
}

fn point(x: f64, y: f64, z: f64) -> FinitePoint3 {
    FinitePoint3::new(Point3::new(x, y, z)).unwrap()
}

fn unit(x: f64, y: f64, z: f64) -> UnitVector3 {
    UnitVector3::new(Vector3::new(x, y, z)).unwrap()
}

fn affine(rows: [[f64; 4]; 3]) -> Transform {
    Transform::affine(rows).unwrap()
}

fn translation(x: f64, y: f64, z: f64) -> Transform {
    affine([[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, y], [0.0, 0.0, 1.0, z]])
}

fn moving(steps: Vec<TransformStep>, cached: Option<Transform>) -> CadIr {
    let mut ir = fixture();
    ir.design.features[0].definition.operation =
        FeatureOperation::TransformBodies(Box::new(TransformBodies {
            bodies: Selection {
                at: Anchor::Input,
                picks: Picks::Unread,
                native: None,
            },
            transformation: Transformation {
                steps,
                resolved: cached,
            },
            mode: None,
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }));
    ir
}

const STEPS: &str = "is not its steps composed";

#[test]
fn a_transformation_resolved_is_its_steps_composed() {
    let translate = |direction: Option<UnitVector3>| TransformStep::Translate {
        direction: resolved::<kinds::Direction>(direction),
        distance: Dimension::Literal(Length::new(5.0).unwrap()),
    };
    let x = Some(unit(1.0, 0.0, 0.0));
    let along = |offset: f64| Some(translation(5.0 + offset, 0.0, 0.0));
    assert_eq!(warnings(&moving(vec![translate(x)], along(0.0)), STEPS), 0);
    assert_eq!(
        warnings(&moving(vec![translate(x)], along(LINEAR)), STEPS),
        0
    );
    assert_eq!(
        warnings(&moving(vec![translate(x)], along(2.0 * LINEAR)), STEPS),
        1
    );
    assert_eq!(
        warnings(&moving(vec![translate(None)], along(1.0)), STEPS),
        0
    );
    assert_eq!(warnings(&moving(vec![translate(x)], None), STEPS), 0);

    let rotate = TransformStep::Rotate {
        axis: resolved::<kinds::Axis>(Some(AxisValue {
            origin: point(1.0, 0.0, 0.0),
            direction: unit(0.0, 0.0, 1.0),
        })),
        angle: Dimension::Literal(Angle::new(FRAC_PI_2).unwrap()),
    };
    let quarter = affine([
        [0.0, -1.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, -1.0],
        [0.0, 0.0, 1.0, 0.0],
    ]);
    let fixed = TransformStep::Fixed(translation(3.0, 0.0, 0.0));
    let both = affine([
        [0.0, -1.0, 0.0, 4.0],
        [1.0, 0.0, 0.0, -1.0],
        [0.0, 0.0, 1.0, 0.0],
    ]);
    assert_eq!(
        warnings(&moving(vec![rotate.clone()], Some(quarter)), STEPS),
        0
    );
    assert_eq!(
        warnings(
            &moving(vec![rotate.clone(), fixed.clone()], Some(both)),
            STEPS
        ),
        0
    );
    assert_eq!(warnings(&moving(vec![fixed, rotate], Some(both)), STEPS), 1);
}

fn frame_at(z: f64) -> FrameValue {
    FrameValue {
        origin: point(0.0, 0.0, z),
        x_axis: unit(1.0, 0.0, 0.0),
        y_axis: unit(0.0, 1.0, 0.0),
        z_axis: unit(0.0, 0.0, 1.0),
    }
}

fn placed(base: PlacementBase, transform: Transform) -> CadIr {
    let mut ir = fixture();
    let part = ir.model.product_definitions[0].id.clone();
    ir.model.occurrences.push(Occurrence::new(
        id("occurrence", "pin"),
        part,
        Some(transform),
    ));
    ir.design.features[0].definition.operation = FeatureOperation::Place(Box::new(Place {
        occurrence: id("occurrence", "pin"),
        placement: Some(resolved::<kinds::Frame>(Some(frame_at(2.0)))),
        scale: None,
        base: Some(base),
        constraints: Vec::new(),
        packaged: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
    }));
    ir
}

const PLACE: &str = "is not its Place.placement's resolved";

#[test]
fn a_place_placement_resolved_is_its_occurrence_transform() {
    let parent = PlacementBase::Parent;
    assert_eq!(
        warnings(&placed(parent, translation(0.0, 0.0, 2.0)), PLACE),
        0
    );
    assert_eq!(
        warnings(&placed(parent, translation(0.0, 0.0, 2.0 + LINEAR)), PLACE),
        0
    );
    assert_eq!(
        warnings(
            &placed(parent, translation(0.0, 0.0, 2.0 + 2.0 * LINEAR)),
            PLACE
        ),
        1
    );
    let own = PlacementBase::OwnOccurrence;
    assert_eq!(warnings(&placed(own, translation(0.0, 0.0, 9.0)), PLACE), 0);
}

fn mirrored(orientation: Option<MirrorOrientation>, result: Transform) -> CadIr {
    let mut ir = fixture();
    let part = ir.model.product_definitions[0].id.clone();
    ir.model.occurrences.push(Occurrence::new(
        id("occurrence", "seed"),
        part.clone(),
        Some(translation(0.0, 3.0, 0.0)),
    ));
    ir.model.occurrences.push(Occurrence::new(
        id("occurrence", "image"),
        part,
        Some(result),
    ));
    let seed = Picked {
        target: PickTarget::Component,
        context: Some(PickContext::Occurrence(OccurrencePath(vec![PathStep {
            occurrence: StepRef::Occurrence(id("occurrence", "seed")),
            configuration: None,
        }]))),
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    };
    ir.design.features[0].definition.operation =
        FeatureOperation::MirrorComponents(Box::new(MirrorComponents {
            seeds: Selection {
                at: Anchor::Input,
                picks: Picks::Named(Members::new(vec![seed]).unwrap()),
                native: None,
            },
            plane: resolved::<kinds::Plane>(Some(PlaneValue {
                origin: point(1.0, 0.0, 0.0),
                normal: unit(1.0, 0.0, 0.0),
                x_axis: unit(0.0, 1.0, 0.0),
            })),
            results: vec![MirrorResult {
                occurrence: id("occurrence", "image"),
                orientation,
            }],
            native_options: BTreeMap::new(),
            source_settings: BTreeMap::new(),
        }));
    ir
}

const MIRROR: &str = "is not its seed's placement reflected";

#[test]
fn a_mirror_result_is_its_seed_reflected_across_the_plane() {
    let reflected = affine([
        [-1.0, 0.0, 0.0, 2.0],
        [0.0, 1.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 0.0],
    ]);
    assert_eq!(warnings(&mirrored(None, reflected), MIRROR), 0);
    assert_eq!(
        warnings(&mirrored(None, translation(2.0, 3.0, 0.0)), MIRROR),
        1
    );
    let copy = Some(MirrorOrientation::SeedAxes {
        reversed: MirrorReversal::X,
        about: Some(MirrorPivot::Origin),
    });
    assert_eq!(
        warnings(&mirrored(copy, translation(2.0, 3.0, 0.0)), MIRROR),
        0
    );
    let unstated = Some(MirrorOrientation::SeedAxes {
        reversed: MirrorReversal::X,
        about: None,
    });
    assert_eq!(
        warnings(&mirrored(unstated, translation(9.0, 3.0, 0.0)), MIRROR),
        0
    );
}

fn contained(body: Option<Transform>) -> CadIr {
    let mut ir = fixture();
    let part = ir.model.product_definitions[0].id.clone();
    for feature in &mut ir.design.features {
        feature.component = Some(part.clone());
    }
    if let Container::Ordered { placement, .. } = &mut ir.design.containers[0] {
        *placement = Some(Box::new(resolved::<kinds::Frame>(Some(frame_at(5.0)))));
    }
    ir.geometry.bodies[0].transform = body;
    ir
}

const CONTAINER: &str = "does not have the container's placement as its transform";

#[test]
fn a_container_placement_is_its_bodies_transform() {
    assert_eq!(
        warnings(&contained(Some(translation(0.0, 0.0, 5.0))), CONTAINER),
        0
    );
    assert_eq!(
        warnings(
            &contained(Some(translation(0.0, 0.0, 5.0 + LINEAR))),
            CONTAINER
        ),
        0
    );
    assert_eq!(
        warnings(&contained(Some(translation(0.0, 0.0, 6.0))), CONTAINER),
        1
    );
    assert_eq!(warnings(&contained(None), CONTAINER), 0);
}
