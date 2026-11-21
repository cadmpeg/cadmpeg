// SPDX-License-Identifier: Apache-2.0
//! Sketches on planes parallel to an origin plane, written on Fusion offset
//! construction planes (f3d.md §3.1, "Records of a construction-plane
//! item"): what the written file decodes back to, and the records of the
//! Design segment.

use cadmpeg_ir::design::construction::{kinds, Construction, ConstructionInput, FrameValue};
use cadmpeg_ir::design::container::Container;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::datum::{Datum, DatumElement};
use cadmpeg_ir::design::ops::sketch::{DefiningRole, DistanceAxis, PlanarCurve, SketchRelation};
use cadmpeg_ir::design::options::NativeOptions;
use cadmpeg_ir::design::refs::Member;
use cadmpeg_ir::design::selection::{PickTarget, PrincipalEntity};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, ParameterValue};
use cadmpeg_ir::scalar::Length;

use super::roster::{segment, Segment};
use super::sketch::{
    decoded_points, feature, feature_id, fixed_clock_tables, frame, near, picked, point2,
    principal, sketch_id, unread, world, Source,
};
use super::{loss, scoped_cube, u32_at, write, write_bytes};
use crate::loss::F3dLossCode;

const TOLERANCE: f64 = 1e-9;
const DESIGN: &str = "FusionDesignSegmentType1";

fn substituted() -> String {
    F3dLossCode::WriterSketchSupportSubstituted
        .kind()
        .to_string()
}

/// `scoped_cube` with `sources` and `datums` before its features in the
/// timeline, the datums first.
fn with(datums: Vec<cadmpeg_ir::design::feature::Feature>, sources: Vec<Source>) -> CadIr {
    let mut ir = scoped_cube();
    let mut timeline = datums
        .iter()
        .map(|datum| Member::Feature(datum.id.clone()))
        .collect::<Vec<_>>();
    ir.design.features.extend(datums);
    for source in sources {
        timeline.push(Member::Feature(source.add_to(&mut ir)));
    }
    let Some(Container::Ordered { members, .. }) = ir.design.containers.first_mut() else {
        panic!("the cube's timeline");
    };
    timeline.append(members);
    *members = timeline;
    ir
}

/// A rectangle of four lines from `(2, 3)`, 20 mm by 10 mm, on `frame`,
/// whose plane `input` picks; with horizontal and vertical relations and,
/// where `dimension`, a literal 10 mm distance between `l1` and `l3`.
fn rectangle(
    name: &'static str,
    input: ConstructionInput,
    frame: FrameValue,
    dimension: bool,
) -> Source {
    let mut source = Source::new(name, input, frame);
    let corners = [(2.0, 3.0), (22.0, 3.0), (22.0, 13.0), (2.0, 13.0)];
    for (index, (u, v)) in corners.iter().enumerate() {
        let start_of = format!("l{}", index + 1);
        let end_of = format!("l{}", (index + 3) % 4 + 1);
        source.entity(
            &format!("p{}", index + 1),
            PlanarCurve::Point(point2(*u, *v)),
            &[
                (start_of.as_str(), DefiningRole::Start),
                (end_of.as_str(), DefiningRole::End),
            ],
        );
    }
    for (index, start) in corners.iter().enumerate() {
        let end = corners[(index + 1) % 4];
        source.entity(
            &format!("l{}", index + 1),
            PlanarCurve::Line {
                start: point2(start.0, start.1),
                end: point2(end.0, end.1),
            },
            &[],
        );
    }
    let (l1, l2, l3) = (
        source.operand("l1"),
        source.operand("l2"),
        source.operand("l3"),
    );
    source
        .constraint(
            "h1",
            SketchRelation::Horizontal {
                a: l1.clone(),
                b: None,
            },
        )
        .constraint("v2", SketchRelation::Vertical { a: l2, b: None });
    if dimension {
        source.constraint(
            "gap",
            SketchRelation::Distance {
                a: l1,
                b: l3,
                along: Some(DistanceAxis::Aligned),
                doubled: None,
                value: Dimension::Literal(Length::new(10.0).expect("a length")),
            },
        );
    }
    source
}

/// The decoded datum planes in timeline order, each as its origin and
/// normal in mm, or its offset distance in mm.
fn datum_planes(decoded: &CadIr) -> Vec<(&cadmpeg_ir::features::Feature, Plane)> {
    let mut features = decoded.model.features.iter().collect::<Vec<_>>();
    features.sort_by_key(|feature| feature.ordinal);
    features
        .into_iter()
        .filter_map(|feature| match feature.evaluation.definition() {
            FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::DatumPlane {
                frame,
            }) => {
                let (o, n) = (frame.origin().get(), frame.normal().get());
                Some((feature, Plane::Frame([o.x, o.y, o.z], [n.x, n.y, n.z])))
            }
            FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::DatumOffsetPlane { distance, .. },
            ) => Some((feature, Plane::Offset(distance.get()))),
            _ => None,
        })
        .collect()
}

#[derive(Debug)]
enum Plane {
    Frame([f64; 3], [f64; 3]),
    Offset(f64),
}

impl Plane {
    /// Whether the plane is the plane at `offset` mm along `normal`.
    fn is(&self, normal: [f64; 3], offset: f64) -> bool {
        match self {
            Self::Frame(origin, n) => {
                near(*n, normal)
                    && (origin[0] * normal[0] + origin[1] * normal[1] + origin[2] * normal[2]
                        - offset)
                        .abs()
                        <= TOLERANCE
            }
            Self::Offset(distance) => (distance - offset).abs() <= TOLERANCE,
        }
    }
}

/// Assert that every source point and line end of `name` on `frame`
/// decodes at its model-space position.
fn assert_points(ir: &CadIr, decoded: &CadIr, name: &str, frame: &FrameValue) {
    let points = decoded_points(decoded);
    for entity in ir
        .design
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == sketch_id(name))
    {
        let expected = match &entity.definition {
            cadmpeg_ir::design::ops::sketch::EntityDefinition::Free(PlanarCurve::Point(p)) => {
                vec![p.get()]
            }
            cadmpeg_ir::design::ops::sketch::EntityDefinition::Free(PlanarCurve::Line {
                start,
                end,
            }) => vec![start.get(), end.get()],
            _ => Vec::new(),
        };
        for p in expected {
            let at = world(frame, p.u, p.v);
            assert!(
                points.iter().any(|point| near(*point, at)),
                "{name}: {at:?} among {points:?}"
            );
        }
    }
}

/// The length in mm and the owner of the decoded parameter `name`.
fn decoded_parameter(decoded: &CadIr, name: &str) -> (f64, Option<FeatureId>) {
    let parameter = decoded
        .model
        .parameters
        .iter()
        .find(|parameter| parameter.name == name)
        .unwrap_or_else(|| panic!("{name}: {:#?}", decoded.model.parameters));
    let Some(ParameterValue::Length(length)) = &parameter.value else {
        panic!("{name}: {:?}", parameter.value);
    };
    (length.get(), parameter.owner.clone())
}

#[test]
fn a_sketch_parallel_to_an_origin_plane_is_written_on_an_offset_construction_plane() {
    let top = frame([0.0, 0.0, 10.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let ir = with(
        Vec::new(),
        vec![rectangle("top", unread(), top.clone(), true)],
    );
    let (report, decoded) = write(&ir);
    assert_eq!(decoded.model.sketches.len(), 1, "{:#?}", report.losses);
    assert_points(&ir, &decoded, "top", &top);
    let planes = datum_planes(&decoded);
    let [(plane, geometry)] = planes.as_slice() else {
        panic!("one decoded construction plane: {planes:#?}");
    };
    assert!(geometry.is([0.0, 0.0, 1.0], 10.0), "{geometry:?}");
    let (along, owner) = decoded_parameter(&decoded, "d1");
    assert!((along - 10.0).abs() <= TOLERANCE, "{along}");
    assert_eq!(owner.as_ref(), Some(&plane.id));
    let (extra, owner) = decoded_parameter(&decoded, "d2");
    assert!(extra.abs() <= TOLERANCE, "{extra}");
    assert_eq!(owner.as_ref(), Some(&plane.id));
    let sketch = decoded
        .model
        .features
        .iter()
        .find(|feature| {
            matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Sketch { .. })
            )
        })
        .expect("the decoded sketch");
    let (gap, owner) = decoded_parameter(&decoded, "d3");
    assert!((gap - 10.0).abs() <= TOLERANCE, "{gap}");
    assert_eq!(owner.as_ref(), Some(&sketch.id));
    let notes = loss(&report, &substituted());
    let [note] = notes.as_slice() else {
        panic!("one substitution note: {:#?}", report.losses);
    };
    assert!(
        note.contains(sketch_id("top").as_str())
            && note.contains("offset 10 mm from XY")
            && note.contains("d1"),
        "{note}"
    );
}

#[test]
fn sketches_share_a_plane_only_where_the_source_names_the_same_support() {
    let datum = feature(
        "plane1",
        FeatureOperation::Datum(Box::new(Datum {
            element: DatumElement::Plane(Construction::<kinds::Plane> {
                inputs: vec![unread()],
                ..Construction::default()
            }),
            link: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        })),
    );
    let on_datum = || {
        picked(PickTarget::Datum {
            feature: feature_id("plane1"),
            part: None,
        })
    };
    let at = |z: f64| frame([0.0, 0.0, z], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let ir = with(
        vec![datum],
        vec![
            rectangle("a", on_datum(), at(10.0), false),
            rectangle("b", on_datum(), at(10.0), false),
            rectangle("c", unread(), at(10.0), false),
            rectangle("d", unread(), at(10.0), false),
            rectangle("e", unread(), at(25.0), false),
        ],
    );
    let (report, decoded) = write(&ir);
    assert_eq!(decoded.model.sketches.len(), 5, "{:#?}", report.losses);
    let planes = datum_planes(&decoded);
    assert_eq!(planes.len(), 4, "{planes:#?}");
    let offsets = [10.0, 10.0, 10.0, 25.0];
    for ((_, plane), offset) in planes.iter().zip(offsets) {
        assert!(plane.is([0.0, 0.0, 1.0], offset), "{plane:?}");
    }
    let notes = loss(&report, &substituted());
    assert_eq!(notes.len(), 4, "{notes:#?}");
    assert!(
        notes.iter().any(|note| note.contains(&format!(
            "sketches {} {} on construction plane 1",
            sketch_id("a"),
            sketch_id("b")
        )) && note.contains(feature_id("plane1").as_str())),
        "{notes:#?}"
    );
    let mut features = decoded.model.features.iter().collect::<Vec<_>>();
    features.sort_by_key(|feature| feature.ordinal);
    let order = features
        .iter()
        .filter_map(|feature| match feature.evaluation.definition() {
            FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::Sketch {
                ..
            }) => Some("sketch"),
            FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::DatumPlane { .. }
                | cadmpeg_ir::features::FeatureOperation::DatumOffsetPlane { .. },
            ) => Some("plane"),
            FeatureDefinition::Operation(cadmpeg_ir::features::FeatureOperation::BaseFeature {
                ..
            }) => Some("base"),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        order,
        [
            "plane", "sketch", "sketch", "plane", "sketch", "plane", "sketch", "plane", "sketch",
            "base"
        ]
    );
}

#[test]
fn each_origin_plane_family_offsets_along_its_normal() {
    let front = frame([0.0, 7.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]);
    let side = frame([-3.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let ir = with(
        Vec::new(),
        vec![
            rectangle("front", unread(), front.clone(), false),
            rectangle("side", unread(), side.clone(), false),
        ],
    );
    let (report, decoded) = write(&ir);
    assert_eq!(decoded.model.sketches.len(), 2, "{:#?}", report.losses);
    assert_points(&ir, &decoded, "front", &front);
    assert_points(&ir, &decoded, "side", &side);
    let planes = datum_planes(&decoded);
    let [(_, xz), (_, yz)] = planes.as_slice() else {
        panic!("two construction planes: {planes:#?}");
    };
    assert!(xz.is([0.0, 1.0, 0.0], 7.0), "{xz:?}");
    assert!(yz.is([1.0, 0.0, 0.0], -3.0), "{yz:?}");
    let orientations = decoded
        .model
        .sketch_constraints
        .iter()
        .map(|constraint| format!("{:?}", constraint.definition.kind()))
        .collect::<Vec<_>>();
    assert_eq!(
        orientations
            .iter()
            .filter(|kind| kind.starts_with("Horizontal") || kind.starts_with("Vertical"))
            .count(),
        4,
        "{orientations:?}"
    );
}

#[test]
fn a_sketch_facing_away_from_its_plane_is_written_by_reflection() {
    let below = frame([0.0, 0.0, -5.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]);
    let ir = with(
        Vec::new(),
        vec![rectangle("below", unread(), below.clone(), false)],
    );
    let (report, decoded) = write(&ir);
    assert_eq!(decoded.model.sketches.len(), 1, "{:#?}", report.losses);
    assert_points(&ir, &decoded, "below", &below);
    let planes = datum_planes(&decoded);
    let [(_, plane)] = planes.as_slice() else {
        panic!("one construction plane: {planes:#?}");
    };
    assert!(plane.is([0.0, 0.0, 1.0], -5.0), "{plane:?}");
}

#[test]
fn a_failure_in_the_plane_records_keeps_the_origin_plane_sketches() {
    super::super::FAIL_NEXT_PLANES.with(|fail| fail.set(true));
    let ir = with(
        Vec::new(),
        vec![
            rectangle(
                "front",
                picked(principal(PrincipalEntity::XyPlane)),
                frame([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
                false,
            ),
            rectangle(
                "top",
                unread(),
                frame([0.0, 0.0, 10.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
                false,
            ),
        ],
    );
    let (report, decoded) = write(&ir);
    assert_eq!(decoded.model.sketches.len(), 1, "{:#?}", report.losses);
    assert!(datum_planes(&decoded).is_empty());
    assert!(
        !report
            .losses
            .iter()
            .any(|loss| loss.message.contains("stores no design")
                || loss.message.contains("writes no design")),
        "{:#?}",
        report.losses
    );
    let missing = loss(&report, "export/evaluation.missing").join("\n");
    assert!(
        missing.contains(&format!(
            "{} (its sketch is dropped because the writer failed with the design's construction planes (not implemented yet: a test failure of the plane records))",
            feature_id("top")
        )),
        "{missing}"
    );
    assert!(loss(&report, &substituted()).is_empty());
}

/// The records of `design` whose type GUID starts with `prefix`.
fn records_of(design: &Segment, prefix: &str) -> Vec<u64> {
    design
        .primary
        .iter()
        .map(|(index, _)| *index)
        .filter(|index| design.type_of(*index).0.starts_with(prefix))
        .collect()
}

#[test]
fn the_records_of_a_workplane_item_have_the_stated_sizes() {
    let top = frame([0.0, 0.0, 10.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let ir = with(Vec::new(), vec![rectangle("top", unread(), top, false)]);
    let (_, _, bytes) = write_bytes(&ir);
    let design = segment(&bytes, DESIGN);
    let [scope] = records_of(&design, "D869265F").as_slice()[..] else {
        panic!("one WorkPlane scope");
    };
    assert_eq!(design.type_of(scope).1, 3);
    assert_eq!(design.record(scope).len(), 322);
    let entity = scope - 1;
    assert_eq!(
        design.type_of(entity).0,
        "8D424E29-DE23-4B1F-A3AD-84C52A497D1A"
    );
    let name = format!("0_{entity}");
    assert_eq!(design.record(entity).len(), 268 + 2 * name.len());
    assert_eq!(design.record(scope + 4).len(), 312, "placement path");
    assert_eq!(u32_at(design.record(scope + 4), 219), 0, "path word");
    assert_eq!(design.record(scope + 8).len(), 392, "placement");
    for owner in [scope + 2, scope + 15] {
        assert_eq!(design.record(owner).len(), 134, "owner {owner}");
    }
    let collection = design.record(11);
    assert_eq!(u32_at(collection, 21), 1, "entity count");
    assert_eq!(super::u64_at(collection, 26), entity);
    assert_eq!(u32_at(collection, 36), 0, "WorkAxis counter");
    assert_eq!(u32_at(collection, 40), 1, "WorkPlane counter");
    assert_eq!(&collection[52..54], [1, 1], "the bytes after the zero word");
}

/// The construction plane's `AlongDistance` and `ExtraOffset` parameters
/// are written in the form of the parameter type's version, as the
/// Extrude's are.
#[test]
fn the_workplane_parameters_close_with_the_trailer_of_their_type_version() {
    let top = frame([0.0, 0.0, 10.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let ir = with(Vec::new(), vec![rectangle("top", unread(), top, true)]);
    let (_, _, bytes) = write_bytes(&ir);
    let written = super::extrude::parameter_records(&bytes);
    for kind in ["AlongDistance", "ExtraOffset"] {
        assert!(
            written
                .iter()
                .any(|(written, comment)| written == kind && comment.is_empty()),
            "{kind} in {written:?}"
        );
    }
}

#[test]
fn the_fixed_clock_note_names_the_workplane_timestamp_tables() {
    let top = frame([0.0, 0.0, 10.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let ir = with(Vec::new(), vec![rectangle("top", unread(), top, false)]);
    let (report, _, bytes) = write_bytes(&ir);
    let design = segment(&bytes, DESIGN);
    let [scope] = records_of(&design, "D869265F").as_slice()[..] else {
        panic!("one WorkPlane scope");
    };
    let tables = fixed_clock_tables(&report, &bytes);
    for (table, role) in [
        (scope + 3, "AlongDistance"),
        (scope + 5, "placement path"),
        (scope + 9, "placement"),
        (scope + 11, "scope"),
        (scope + 16, "ExtraOffset"),
    ] {
        assert!(tables.contains(&table), "{role} table {table}: {tables:?}");
    }
}

#[test]
fn the_context_value_of_a_design_with_planes_and_no_dimension_is_named() {
    let top = frame([0.0, 0.0, 10.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let ir = with(Vec::new(), vec![rectangle("top", unread(), top, false)]);
    let (report, _, _) = write_bytes(&ir);
    let notes = loss(&report, "export/record.residual-unavailable")
        .into_iter()
        .filter(|message| message.contains("record 198"))
        .collect::<Vec<_>>();
    let [note] = notes.as_slice() else {
        panic!("one context-value note: {:#?}", report.losses);
    };
    for named in ["WorkPlane items", "context value 2", "DR-882"] {
        assert!(note.contains(named), "{named}: {note}");
    }
}

#[test]
fn the_workplane_note_names_the_parameter_numbering() {
    let top = frame([0.0, 0.0, 10.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    let ir = with(Vec::new(), vec![rectangle("top", unread(), top, false)]);
    let (report, _, _) = write_bytes(&ir);
    let notes = loss(&report, "export/record.residual-unavailable")
        .into_iter()
        .filter(|message| message.contains("writes the WorkPlane items"))
        .collect::<Vec<_>>();
    let [note] = notes.as_slice() else {
        panic!("one WorkPlane note: {:#?}", report.losses);
    };
    for named in [
        "one counter from 1 over the WorkPlane and Sketch items in timeline order",
        "AlongDistance parameter takes the ordinal after the parameters of the WorkPlane and Sketch items before it",
        "ExtraOffset parameter's ordinal plus one",
        "an Extrude item's two parameters take the last two ordinals",
    ] {
        assert!(note.contains(named), "{named}: {note}");
    }
}
