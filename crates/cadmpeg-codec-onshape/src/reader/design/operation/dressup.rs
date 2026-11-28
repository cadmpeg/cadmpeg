// SPDX-License-Identifier: Apache-2.0
//! Fillet, chamfer, shell, and hole operations (§6.4).

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::ops::dressup::{
    BlendControls, Bore, Chamfer, ChamferMeasure, ChamferSet, ChamferSize, Cone, DepthTo, Fillet,
    FilletProfile, FilletSection, FilletSet, FilletSize, Hole, HoleBottom, HoleHead, HolePositions,
    HoleShape, ShellFeature, StandardSize, ThreadLength, ThreadSpec,
};
use cadmpeg_ir::design::ops::shared::Limit;
use cadmpeg_ir::design::options::{Hand, NativeOption, NativeOptions, Thickness, WallSide};
use cadmpeg_ir::features as v10;
use cadmpeg_ir::scalar::{InteriorAngle, Length, PositiveLength};

use super::picks::{self, Mapping};
use super::{bound, signed, Context};
use crate::reader::design::Source;
use crate::reader::{features, Mapping as SourceMapping};
use serde_json::Value;

/// The assumption a chamfer with two setbacks states: the first lies on
/// the face whose loop runs along the edge's own direction (§6.4), swapped
/// by the stored `oppositeDirection`.
pub(super) const CHAMFER_SIDE: &str =
    "the first chamfer setback is read on the face whose loop runs along the edge, swapped by oppositeDirection (ONSHAPE-013)";

fn controls() -> BlendControls {
    BlendControls {
        corner_style: None,
        corners: Vec::new(),
        limits: Vec::new(),
        keep: None,
        transitions: Vec::new(),
        overflow: None,
        keep_features: None,
        round_corners: None,
    }
}

pub(super) fn fillet(
    model: &v10::Feature,
    source: &Source<'_>,
    context: &Context<'_, '_>,
) -> SourceMapping<FeatureOperation> {
    if source.tag("filletType").is_some_and(|kind| kind != "EDGE")
        || source.tag("crossSection") != Some("CIRCULAR")
    {
        return Err("filletType or crossSection is outside the circular edge form".into());
    }
    source.usage.ignore_false(&[
        "isVariable",
        "isPartial",
        "allowEdgeOverflow",
        "smoothCorners",
        "smoothTransition",
    ]);
    source
        .usage
        .account(&["defaultsChanged", "nonCircularRadius", "rho", "magnitude"]);
    if source.peek_flag("isVariable") == Ok(false) {
        source.usage.account(&[
            "vertexSettings",
            "pointOnEdgeSettings",
            "pointSettings.edge",
            "pointSettings.pointRadius",
            "smoothTransition",
        ]);
    }
    if source.peek_flag("isPartial") == Ok(false) {
        source.usage.account(&[
            "startPartialType",
            "startPartialEntity",
            "startPartialOffset",
            "partialFirstEdgeTotalParameter",
            "partialOppositeParameter",
            "secondBound",
            "endPartialType",
            "endPartialEntity",
            "endPartialOffset",
            "partialSecondEdgeTotalParameter",
        ]);
    }
    if source.peek_flag("allowEdgeOverflow") == Ok(false) {
        source.usage.account(&["keepEdges"]);
    }
    if source.peek_flag("smoothCorners") == Ok(false) {
        source.usage.account(&["smoothCornerExceptions"]);
    }
    let input = context.input(model);
    let ids = picks::ids(source, "entities")?;
    let seeds = picks::edge_picks(&ids, input)?;
    if seeds.is_empty() {
        return Err("entities names no edges".into());
    }
    let length = |key, role| {
        signed(
            source,
            key,
            context.owned(model, &[role]),
            || source.quantity_value(key, PositiveLength::new),
            false,
        )
    };
    let blend = source.peek_tag("blendControlType");
    let size = if blend == Some("WIDTH") {
        source.usage.account(&[
            "blendControlType",
            "radius",
            "isAsymmetric",
            "otherRadius",
            "flipAsymmetric",
        ]);
        FilletSize::Chord(length("width", "Width")?)
    } else {
        source.usage.account(&["width"]);
        if blend == Some("RADIUS") {
            source.usage.account(&["blendControlType"]);
        }
        let radius = length("radius", "Radius")?;
        if source.flag("isAsymmetric")? {
            let other = length("otherRadius", "OtherRadius")?;
            let (first, second) = if source.flag("flipAsymmetric")? {
                (other, radius)
            } else {
                (radius, other)
            };
            FilletSize::Offsets { first, second }
        } else {
            source.usage.account(&["otherRadius", "flipAsymmetric"]);
            FilletSize::Radius(radius)
        }
    };
    let tangent = source.flag("tangentPropagation")?;
    let edges = if features::parameter(source.record, "tangentPropagation").is_ok() {
        picks::blend(seeds, tangent, Vec::new())?
    } else {
        picks::selection(seeds)?
    };
    Ok(FeatureOperation::Fillet(Box::new(Fillet {
        sets: vec![FilletSet::Edges {
            edges,
            section: FilletSection {
                size: Some(size),
                profile: Some(FilletProfile::Circular),
                weight: None,
            },
            spine: None,
            hold: [None, None],
        }],
        controls: controls(),
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    })))
}

pub(super) fn chamfer(
    model: &v10::Feature,
    source: &Source<'_>,
    context: &Context<'_, '_>,
) -> SourceMapping<(FeatureOperation, bool)> {
    let length = |key, role| {
        signed(
            source,
            key,
            context.owned(model, &[role]),
            || source.quantity_value(key, PositiveLength::new),
            false,
        )
    };
    let size = match source.tag("chamferType") {
        None | Some("EQUAL_OFFSETS") => {
            source.usage.account(&[
                "width1",
                "width2",
                "angle",
                "oppositeDirection",
                "directionOverrides",
            ]);
            ChamferSize::Distance(length("width", "Distance")?)
        }
        Some("TWO_OFFSETS") => {
            source.usage.account(&["width", "angle"]);
            ChamferSize::Distances {
                first: length("width1", "FirstDistance")?,
                second: length("width2", "SecondDistance")?,
            }
        }
        Some("OFFSET_ANGLE" | "DISTANCE_ANGLE") => {
            source.usage.account(&["width1", "width2"]);
            ChamferSize::DistanceAngle {
                distance: length("width", "Distance")?,
                angle: bound(
                    context.owned(model, &["Angle"]),
                    source.quantity("angle", InteriorAngle::new)?,
                ),
            }
        }
        _ => {
            return Err(
                "chamferType is outside the equal, two-offset and offset-angle forms".into(),
            )
        }
    };
    let sided = !matches!(size, ChamferSize::Distance(_));
    let measure = match source.tag("chamferMethod") {
        None | Some("FACE_OFFSET") => ChamferMeasure::OffsetFaces,
        Some(other) => {
            let reason = if other == "APEX_RANGE" {
                "keeps APEX_RANGE as a native measure with no neutral equivalent"
            } else {
                "keeps a native measure with no neutral equivalent"
            };
            source.usage.partial("chamferMethod", reason);
            ChamferMeasure::Native(text(other)?)
        }
    };
    let input = context.input(model);
    let ids = picks::ids(source, "entities")?;
    if ids.is_empty() {
        return Err("entities names no edges".into());
    }
    let tangent = source.flag("tangentPropagation")?;
    let flip = sided && source.flag("oppositeDirection")?;
    let overridden = if sided && features::parameter(source.record, "directionOverrides").is_ok() {
        picks::ids(source, "directionOverrides")?
    } else {
        Vec::new()
    };
    let base: Vec<_> = ids
        .iter()
        .copied()
        .filter(|id| tangent || !overridden.contains(id))
        .collect();
    let overrides = picks::edge_picks(&overridden, input)?;
    let set = |edges, flip| ChamferSet {
        edges,
        size: Some(size.clone()),
        reference: None,
        flip: Some(Dimension::Literal(flip)),
        measure: Some(measure.clone()),
    };
    let mut sets = Vec::new();
    if !base.is_empty() {
        let base = picks::edge_picks(&base, input)?;
        let edges = if tangent || features::parameter(source.record, "tangentPropagation").is_ok() {
            picks::blend(
                base,
                tangent,
                if tangent {
                    overrides.clone()
                } else {
                    Vec::new()
                },
            )?
        } else {
            picks::selection(base)?
        };
        sets.push(set(edges, flip));
    }
    if !overrides.is_empty() {
        sets.push(set(picks::blend(overrides, false, Vec::new())?, !flip));
    }
    let mut native_options = NativeOptions::new();
    if matches!(measure, ChamferMeasure::OffsetFaces) && ids.len() > 1 {
        option(
            &mut native_options,
            "chamfer_seed_order",
            NativeOption::OrderedSelection(picks::selection(picks::edge_picks(&ids, input)?)?),
        )?;
    }
    Ok((
        FeatureOperation::Chamfer(Box::new(Chamfer {
            sets,
            controls: controls(),
            native_options,
            source_settings: NativeOptions::new(),
        })),
        sided,
    ))
}

pub(super) fn shell(
    model: &v10::Feature,
    source: &Source<'_>,
    context: &Context<'_, '_>,
) -> SourceMapping<FeatureOperation> {
    if !features::shell_record_valid(source.record) {
        return Err("a parameter is outside the shell definition".into());
    }
    let hollow = source.flag("isHollow")?;
    let value = bound(
        context.owned(model, &["Thickness"]),
        source.quantity("thickness", PositiveLength::new)?,
    );
    let side = if source.flag("oppositeDirection")? {
        WallSide::Outside
    } else {
        WallSide::Inside
    };
    let (bodies, removed) = if hollow {
        source.usage.account(&["entities"]);
        (
            Some(picks::selected(source, "parts")?),
            picks::selection(Vec::new())?,
        )
    } else {
        source.usage.account(&["parts"]);
        (None, picks::selected(source, "entities")?)
    };
    Ok(FeatureOperation::Shell(Box::new(ShellFeature {
        bodies,
        removed,
        thickness: Some(Thickness::OneSide {
            value,
            side: Some(side),
        }),
        faces: Vec::new(),
        join: None,
        face_meeting: None,
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    })))
}

pub(super) fn hole(
    model: &v10::Feature,
    source: &Source<'_>,
    context: &Context<'_, '_>,
) -> SourceMapping<FeatureOperation> {
    let source_standard = features::parameter(source.record, "standardTappedOrClearance")
        .ok()
        .and_then(|parameter| parameter.get("value"))
        .filter(|value| !value.is_null());
    let tapped = source_standard.is_some_and(|standard| standard["type"] == "Tapped");
    if source.flag("isTapped")? && !tapped {
        return Err("isTapped selects a thread with no readable standard".into());
    }
    if source.flag("isV2")? || source.peek_tag("holeVersion") == Some("V3") {
        return Err(
            "the versioned hole dimensions and standard table are not supported yet".into(),
        );
    }
    source.usage.ignore_false(&[
        "isMultiple",
        "hasClearance",
        "offset",
        "showTappedDepth",
        "showThreadClass",
        "tapClearance",
        "toleranceMatching",
        "oppositeDirection",
    ]);
    source
        .usage
        .ignore_value(&["holeVersion"], &Value::from("LEGACY"));
    source
        .usage
        .ignore_value(&["threadStandard"], &Value::from("UNSET"));
    source
        .usage
        .ignore_value(&["featureName"], &Value::from(""));
    source
        .usage
        .account(&["standardBlindInLast", "styleV2", "endStyleV2"]);
    source
        .usage
        .ignore_value(&["standardTappedOrClearance"], &Value::Null);
    source.usage.ignore_empty_queries(&["initEntities"]);
    if source.peek_flag("offset") == Ok(false) {
        source
            .usage
            .account(&["offsetDistance", "oppositeOffsetDirection"]);
    }
    let points = super::source_profile(
        source,
        if features::parameter(source.record, "locations").is_ok() {
            "locations"
        } else {
            "entities"
        },
        context,
    )?;
    let start = match source.tag("startStyle") {
        None | Some("PART") => {
            source.usage.account(&["startBoundEntity"]);
            None
        }
        Some("SKETCH") => {
            source.usage.account(&["startBoundEntity"]);
            source
                .usage
                .partial("startStyle", "uses SKETCH as the hole depth reference");
            None
        }
        Some("PLANE") => Some(picks::selected(source, "startBoundEntity")?),
        _ => return Err("startStyle does not name a supported hole start".into()),
    };
    let owned = |role| context.owned(model, &[role]);
    let length = |key, role| {
        signed(
            source,
            key,
            owned(role),
            || source.quantity_value(key, PositiveLength::new),
            false,
        )
    };
    let style = source.tag("style");
    if style != Some("C_BORE") {
        source.usage.account(&["cBoreDiameter", "cBoreDepth"]);
    }
    if style != Some("C_SINK") {
        source.usage.account(&["cSinkDiameter", "cSinkAngle"]);
    }
    let head = match style {
        Some("SIMPLE") => HoleHead::Plain,
        Some("C_BORE") => HoleHead::Counterbore {
            diameter: Some(length("cBoreDiameter", "CounterboreDiameter")?),
            depth: Some(length("cBoreDepth", "CounterboreDepth")?),
        },
        Some("C_SINK") => HoleHead::Countersink(Cone {
            diameter: Some(length("cSinkDiameter", "CountersinkDiameter")?),
            depth: None,
            angle: Some(bound(
                owned("CountersinkAngle"),
                source.quantity("cSinkAngle", InteriorAngle::new)?,
            )),
        }),
        _ => return Err("style does not name a simple, counterbored or countersunk hole".into()),
    };
    let (extent, bottom) = match source.tag("endStyle") {
        Some("THROUGH" | "THROUGH_ALL") => {
            source.usage.account(&[
                "holeDepth",
                "tipAngleStyle",
                "tipAngle",
                "depthToTip",
                "useTipDepth",
            ]);
            (Limit::ThroughAll, None)
        }
        Some("BLIND") => {
            let preset = features::tip_angle_preset(source.record);
            let tip = match preset {
                Some(tip) => {
                    source.usage.account(&["tipAngleStyle", "tipAngle"]);
                    tip
                }
                None => {
                    source
                        .usage
                        .ignore_value(&["tipAngleStyle"], &Value::from("CUSTOM"));
                    source.quantity("tipAngle", Some)?
                }
            };
            let bottom = if (tip - std::f64::consts::PI).abs() < features::DRILL_TIP_ANGLE_TOLERANCE
            {
                source.usage.account(&["depthToTip", "useTipDepth"]);
                HoleBottom::Flat
            } else {
                HoleBottom::DrillPoint {
                    angle: Some(bound(
                        preset.is_none().then(|| owned("TipAngle")).flatten(),
                        InteriorAngle::new(tip)
                            .ok_or("tipAngle is outside the drill-point range")?,
                    )),
                    depth_to: Some(
                        if source.flag("depthToTip")? || source.flag("useTipDepth")? {
                            source.usage.account(&["useTipDepth"]);
                            DepthTo::Tip
                        } else {
                            DepthTo::Shoulder
                        },
                    ),
                }
            };
            (
                Limit::Amount(bound(
                    owned("Depth"),
                    source.quantity("holeDepth", Length::new)?,
                )),
                Some(bottom),
            )
        }
        _ => return Err("endStyle is outside the blind and through hole forms".into()),
    };
    let targets = if features::parameter(source.record, "scope").is_ok() {
        let ids = picks::ids(source, "scope")?;
        if ids.is_empty() {
            None
        } else {
            Some(cadmpeg_ir::design::options::Targets::Selected(
                picks::named_ids(ids)?,
            ))
        }
    } else {
        None
    };
    let mut native_options = NativeOptions::new();
    let (standard, thread) = match source_standard {
        None => (None, None),
        Some(table) => {
            let dimensions = standard(source, table, model, context, &mut native_options)?;
            source.usage.account(&["standardTappedOrClearance"]);
            dimensions
        }
    };
    if !tapped {
        source.usage.account(&[
            "majorDiameter",
            "tapDrillDiameter",
            "tappedDepth",
            "isTappedThrough",
            "tappedAngle",
            "tapClearance",
        ]);
    }
    let diameter_key = if tapped && features::parameter(source.record, "tapDrillDiameter").is_ok() {
        source.usage.account(&["holeDiameter"]);
        "tapDrillDiameter"
    } else {
        "holeDiameter"
    };
    Ok(FeatureOperation::Hole(Box::new(Hole {
        positions: HolePositions::Points {
            points,
            centers: None,
        },
        start,
        direction: None,
        shape: HoleShape::Bore(Bore {
            diameter: Some(length(diameter_key, "Diameter")?),
            head: Some(head),
            taper: None,
            bottom,
            near_chamfer: None,
            neck_chamfer: None,
            far_chamfer: None,
        }),
        extent: Some(extent),
        standard,
        thread,
        targets,
        series: Vec::new(),
        native_options,
        source_settings: NativeOptions::new(),
    })))
}

fn text(value: &str) -> Mapping<NonBlankString> {
    NonBlankString::new(value.trim()).ok_or("a source text value is blank")
}

fn option(options: &mut NativeOptions, key: &str, value: NativeOption) -> Mapping<()> {
    let name = cadmpeg_ir::design::names::CodecName::new(
        &cadmpeg_ir::codec_id!(crate::dialect::FORMAT),
        key,
    )
    .ok_or("a native option name is invalid")?;
    options.insert(name, value);
    Ok(())
}

fn standard(
    source: &Source<'_>,
    table: &Value,
    model: &v10::Feature,
    context: &Context<'_, '_>,
    options: &mut NativeOptions,
) -> SourceMapping<(Option<StandardSize>, Option<ThreadSpec>)> {
    let get = |key| table.get(key).and_then(Value::as_str);
    let flag = |key| {
        table
            .get(key)
            .and_then(Value::as_bool)
            .ok_or_else(|| format!("the standard table does not state {key}"))
    };
    let kind = get("type").ok_or("the hole standard has no type")?;
    let standard = StandardSize {
        standard: text(get("standard").ok_or("the hole standard has no name")?)?,
        kind: None,
        size: get("size").map(text).transpose()?.map(Dimension::Literal),
        class: get(if kind == "Tapped" { "class" } else { "fit" })
            .map(text)
            .transpose()?,
    };
    let hand = match get("hand") {
        Some("left") => Hand::Left,
        Some("right") => Hand::Right,
        _ => return Err("the hole standard has no resolved hand".into()),
    };
    let modeled = Dimension::Literal(flag("modeled")?);
    let cosmetic = Dimension::Literal(flag("cosmetic")?);
    match kind {
        "Clearance" => {
            option(
                options,
                "hole_hand",
                NativeOption::Text(Dimension::Literal(text(
                    get("hand").ok_or("the clearance table has no hand")?,
                )?)),
            )?;
            option(options, "hole_modeled", NativeOption::Bool(modeled))?;
            option(options, "hole_cosmetic", NativeOption::Bool(cosmetic))?;
            Ok((Some(standard), None))
        }
        "Tapped" => {
            let owned = |role| context.owned(model, &[role]);
            let length = |key, role| {
                signed(
                    source,
                    key,
                    owned(role),
                    || source.quantity_value(key, PositiveLength::new),
                    false,
                )
            };
            option(options, "thread_cosmetic", NativeOption::Bool(cosmetic))?;
            let pitch = get("pitch")
                .and_then(features::pitch_mm)
                .and_then(PositiveLength::new)
                .map(Dimension::Literal);
            let major = features::parameter(source.record, "majorDiameter")
                .is_ok()
                .then(|| length("majorDiameter", "MajorDiameter"))
                .transpose()?;
            let through = features::through_hole(source.record);
            if !through {
                source.usage.account(&["isTappedThrough"]);
            }
            let depth = if through && source.flag("isTappedThrough")? {
                source.usage.account(&["tappedDepth"]);
                ThreadLength::Full
            } else {
                ThreadLength::Blind(length("tappedDepth", "TappedDepth")?)
            };
            Ok((
                None,
                Some(ThreadSpec {
                    standard: Some(standard),
                    major_diameter: major,
                    minor_diameter: None,
                    pitch_diameter: None,
                    pitch,
                    hand: Some(hand),
                    length: Some(depth),
                    modeled: Some(modeled),
                }),
            ))
        }
        _ => Err("the hole standard is outside the clearance and tapped forms".into()),
    }
}
