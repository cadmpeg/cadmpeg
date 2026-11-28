// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeMap;
use std::sync::OnceLock;

use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::design::options::{BodyForm, Thickness};
use cadmpeg_ir::report::loss::LossNote;
use serde_json::Value;

use super::Source;
use crate::loss::OnshapeLossCode;
use crate::reader::{expression, features, query};

pub(super) const TABLE: &str = include_str!("parameter-map.tsv");

fn destinations(kind: &str) -> Option<&'static BTreeMap<&'static str, &'static str>> {
    type Table = BTreeMap<&'static str, BTreeMap<&'static str, &'static str>>;
    static DESTINATIONS: OnceLock<Table> = OnceLock::new();
    DESTINATIONS
        .get_or_init(|| {
            let mut result = Table::new();
            for line in TABLE.lines().filter(|line| !line.starts_with('#')) {
                let mut columns = line.split('\t');
                if let (Some(kind), Some(key), Some(_), Some(destination)) = (
                    columns.next(),
                    columns.next(),
                    columns.next(),
                    columns.next(),
                ) {
                    result.entry(kind).or_default().insert(key, destination);
                }
            }
            result
        })
        .get(kind)
}

pub(super) fn loss_message(key: &str, subject: &str) -> String {
    format!("parameter {key} has no projected design field; subject: {subject}")
}

pub(super) fn report(
    source: &Source<'_>,
    operation: &FeatureOperation,
    variable: bool,
    subject: &str,
    notes: &mut Vec<LossNote>,
) {
    let Some(kind) = source.kind() else { return };
    let Some(destinations) = destinations(kind) else {
        return;
    };
    let Some(rows) = source.record.get("parameters").and_then(Value::as_array) else {
        return;
    };
    let wire = json::to_value(operation).ok();
    for row in rows {
        let Some(key) = row.get("parameterId").and_then(Value::as_str) else {
            continue;
        };
        if features::is_custom(source.record) || dormant(source, key, row) {
            continue;
        }
        let kept = match destinations.get(key).copied() {
            Some("inactive") => true,
            Some("parameter") => variable,
            Some("sketch.placement") => matches!(operation, FeatureOperation::Sketch(_)),
            Some(paths) => wire.as_ref().is_some_and(|op| {
                paths
                    .split('|')
                    .any(|path| path.starts_with('/') && op.pointer(path).is_some())
            }),
            None => false,
        };
        let message = if kept {
            residual(kind, key, row, operation)
                .map(|reason| format!("parameter {key} {reason}; subject: {subject}"))
        } else if key == "startStyle"
            && row["value"] == "SKETCH"
            && matches!(operation, FeatureOperation::Hole(_))
        {
            Some(format!(
                "parameter startStyle uses SKETCH as the hole depth reference; subject: {subject}"
            ))
        } else {
            Some(loss_message(key, subject))
        };
        if let Some(message) = message {
            notes.push(OnshapeLossCode::DesignParameterUnprojected.note(message));
        }
    }
}

fn residual(
    kind: &str,
    key: &str,
    row: &Value,
    operation: &FeatureOperation,
) -> Option<&'static str> {
    match (kind, key) {
        ("extrude", "thickness1" | "thickness2")
            if row["expression"]
                .as_str()
                .is_some_and(|text| !expression::literal(text)) =>
        {
            let FeatureOperation::Extrude(extrude) = operation else {
                return None;
            };
            let Some(BodyForm::Thin(wall)) = &extrude.output.body else {
                return None;
            };
            matches!(wall.thickness, Thickness::Symmetric(Dimension::Literal(_)))
                .then_some("contributes to the evaluated symmetric wall without a formula binding")
        }
        ("chamfer", "chamferMethod") if row["value"] == "APEX_RANGE" => {
            Some("keeps APEX_RANGE as a native measure with no neutral equivalent")
        }
        ("extrude" | "revolve" | "sweep", "booleanScope")
        | (
            "extrude",
            "booleanSurfaceScope"
            | "endBoundEntityFace"
            | "endBoundEntityBody"
            | "startOffsetEntity",
        )
        | ("chamfer", "entities" | "directionOverrides")
        | ("fillet", "entities")
        | ("hole", "scope" | "startBoundEntity")
        | ("shell", "entities" | "parts")
        | ("sweep", "lockFaces" | "lockDirectionQuery") => query::has_unresolved_ids(row)
            .then_some("contains a query with no readable deterministic ids"),
        _ => None,
    }
}

fn has_query(row: &Value) -> bool {
    row.get("queries")
        .and_then(Value::as_array)
        .is_some_and(|queries| !queries.is_empty())
}

pub(super) fn dormant(source: &Source<'_>, key: &str, row: &Value) -> bool {
    let flag = |key| source.flag(key) == Ok(true);
    let tag = |key| source.tag(key);
    let symmetric = || source.symmetric_extrude() == Ok(true);
    let second = || !symmetric() && flag("hasSecondDirection");
    let false_value = row.get("value").and_then(Value::as_bool) == Some(false);
    match (source.kind(), key) {
        (Some("extrude"), "thickness") => tag("bodyType") != Some("THIN") || !flag("midplane"),
        (Some("extrude"), "thickness1" | "thickness2") => {
            tag("bodyType") != Some("THIN") || flag("midplane")
        }
        (Some("extrude"), "secondDirectionDepth") => {
            !second() || tag("secondDirectionBound") != Some("BLIND")
        }
        (Some("extrude"), "startOffsetDistance") => {
            !flag("startOffset") || tag("startOffsetBound") != Some("BLIND")
        }
        (Some("extrude"), "startOffsetEntity") => {
            !flag("startOffset") || tag("startOffsetBound") != Some("ENTITY")
        }
        (Some("fillet"), "radius") => tag("blendControlType") == Some("WIDTH"),
        (Some("fillet"), "width") => tag("blendControlType") != Some("WIDTH"),
        (Some("hole"), "cBoreDiameter" | "cBoreDepth") => tag("style") != Some("C_BORE"),
        (Some("hole"), "cSinkDiameter" | "cSinkAngle") => tag("style") != Some("C_SINK"),
        (Some("hole"), "oppositeDirection") => false_value,
        (Some("hole"), "majorDiameter" | "tapDrillDiameter") => {
            !features::tapped_standard(source.record)
        }
        (Some("extrude" | "sweep" | "revolve"), "booleanScope") => {
            source.default_scope("defaultScope") == Ok(true)
                || tag("operationType") == Some("NEW")
                || tag("bodyType") == Some("SURFACE")
        }
        (Some("extrude" | "sweep" | "revolve"), "defaultScope") => {
            tag("operationType") == Some("NEW") || tag("bodyType") == Some("SURFACE")
        }
        (Some("extrude"), "booleanSurfaceScope") => {
            source.default_scope("defaultSurfaceScope") == Ok(true)
                || tag("surfaceOperationType") != Some("ADD")
                || tag("bodyType") != Some("SURFACE")
        }
        (Some("extrude"), "defaultSurfaceScope") => {
            tag("surfaceOperationType") != Some("ADD") || tag("bodyType") != Some("SURFACE")
        }
        (Some("extrude"), "depth") => tag("endBound") != Some("BLIND"),
        (Some("extrude"), "width" | "wallShape" | "midplane" | "flipWall") => {
            tag("bodyType") != Some("THIN")
        }
        (Some("extrude"), "surfaceEntities" | "surfaceOperationType") => {
            tag("bodyType") != Some("SURFACE")
        }
        (Some("extrude"), "entities") => matches!(tag("bodyType"), Some("THIN" | "SURFACE")),
        (Some("extrude"), "endBoundEntityFace") => tag("endBound") != Some("UP_TO_SURFACE"),
        (Some("extrude"), "endBoundEntityBody") => tag("endBound") != Some("UP_TO_BODY"),
        (Some("extrude"), "endBoundEntityVertex") => tag("endBound") != Some("UP_TO_VERTEX"),
        (Some("extrude"), "hasOffset" | "hasDraft" | "hasDirection") => false_value,
        (Some("extrude"), "hasSecondDirection") => symmetric(),
        (Some("extrude"), "hasSecondDirectionDraft") => false_value || !second(),
        (Some("extrude"), "offsetDistance" | "offsetOppositeDirection") => {
            !flag("hasOffset") || matches!(tag("endBound"), Some("BLIND" | "THROUGH_ALL"))
        }
        (Some("extrude"), "secondDirectionBound" | "secondDirectionOppositeDirection") => !second(),
        (Some("extrude"), "draftAngle" | "draftPullDirection") => !flag("hasDraft"),
        (Some("extrude"), "secondDirectionDraftAngle" | "secondDirectionDraftPullDirection") => {
            !second() || !flag("hasSecondDirectionDraft")
        }
        (Some("extrude"), "startOffsetBound" | "startOffsetOppositeDirection") => {
            !flag("startOffset")
        }
        (Some("revolve"), "angle") => flag("fullRevolve") || tag("revolveType") == Some("FULL"),
        (Some("revolve"), "angleBack" | "startBound") => !flag("hasStartBound"),
        (Some("fillet"), "otherRadius" | "flipAsymmetric") => {
            !flag("isAsymmetric") || tag("blendControlType") == Some("WIDTH")
        }
        (Some("fillet"), "defaultsChanged") => true,
        (Some("chamfer"), "width1" | "width2") => tag("chamferType") != Some("TWO_OFFSETS"),
        (Some("chamfer"), "width") => tag("chamferType") == Some("TWO_OFFSETS"),
        (Some("chamfer"), "angle") => {
            !matches!(tag("chamferType"), Some("OFFSET_ANGLE" | "DISTANCE_ANGLE"))
        }
        (Some("chamfer"), "oppositeDirection" | "directionOverrides") => !matches!(
            tag("chamferType"),
            Some("TWO_OFFSETS" | "OFFSET_ANGLE" | "DISTANCE_ANGLE")
        ),
        (Some("booleanBodies"), "targets" | "offset") => {
            tag("operationType") != Some("SUBTRACTION") || key == "offset" && false_value
        }
        (
            Some("booleanBodies"),
            "offsetAll" | "entitiesToOffset" | "offsetDistance" | "oppositeDirection" | "reFillet",
        ) => tag("operationType") != Some("SUBTRACTION") || !flag("offset"),
        (Some("hole"), "holeDepth" | "tipAngleStyle") => tag("endStyle") != Some("BLIND"),
        (Some("hole"), "tipAngle") => {
            tag("endStyle") != Some("BLIND") || features::tip_angle_preset(source.record).is_some()
        }
        (Some("hole"), "depthToTip" | "useTipDepth") => {
            tag("endStyle") != Some("BLIND") || tag("tipAngleStyle") == Some("FLAT")
        }
        (Some("hole"), "startBoundEntity") => tag("startStyle") != Some("PLANE"),
        (Some("hole"), "startStyle") => tag("startStyle") == Some("PART"),
        (Some("hole"), "isTappedThrough") => {
            !features::tapped_standard(source.record) || !features::through_hole(source.record)
        }
        (Some("hole"), "tappedDepth") => {
            !features::tapped_standard(source.record)
                || features::tap_through(source.record) == Ok(true)
        }
        (Some("hole"), "scope" | "initEntities") => !has_query(row),
        (Some("hole"), "holeDiameter") => {
            features::parameter(source.record, "tapDrillDiameter").is_ok()
                && features::tapped_standard(source.record)
        }
        (Some("shell"), "parts") => !flag("isHollow"),
        (Some("shell"), "entities") => flag("isHollow"),
        (Some("sweep"), "hasScale") => false_value,
        (Some("sweep"), "scaleFactor") => !flag("hasScale"),
        (Some("sweep"), "lockFaces") => tag("profileControl") != Some("LOCK_FACES"),
        (Some("sweep"), "lockDirectionQuery") => tag("profileControl") != Some("LOCK_DIRECTION"),
        (Some("sweep"), "hasTwist" | "twistType" | "turns" | "angle" | "pitch" | "ccw") => {
            !flag("hasTwist")
                || matches!(tag("profileControl"), Some("LOCK_FACES" | "LOCK_DIRECTION"))
                || matches!(key, "turns" | "angle" | "pitch")
                    && key
                        != match tag("twistType") {
                            Some("ANGLE") => "angle",
                            Some("PITCH") => "pitch",
                            _ => "turns",
                        }
        }
        (Some("sweep"), "pathOppositeDirection" | "extendToFullPath") => false_value,
        (
            Some("sweep"),
            "wallShape" | "midplane" | "thickness" | "thickness1" | "thickness2" | "flipWall"
            | "trimEnds",
        ) => tag("bodyType") != Some("THIN"),
        (
            Some("sweep"),
            "surfaceProfiles"
            | "surfaceOperationType"
            | "defaultSurfaceScope"
            | "booleanSurfaceScope",
        ) => tag("bodyType") != Some("SURFACE"),
        (Some("assignVariable"), "initEntities") => !has_query(row),
        (Some("assignVariable"), "lengthValue") => tag("variableType") != Some("LENGTH"),
        (Some("assignVariable"), "angleValue") => tag("variableType") != Some("ANGLE"),
        (Some("assignVariable"), "numberValue") => tag("variableType") != Some("NUMBER"),
        (Some("importDerived"), "location" | "parts") => !has_query(row),
        (
            Some("importDerived"),
            "includeProperties" | "includeMateConnectors" | "preserveActiveSheetMetal",
        ) => false_value,
        (Some("importDerived"), "placement") => tag("placement") == Some("AT_ORIGIN"),
        (
            Some("importDerived"),
            "mateConnectorIndex" | "mateConnectorId" | "mateConnectorIndexInFeature",
        ) => tag("placement") != Some("AT_MATE_CONNECTOR"),
        _ => false,
    }
}

#[cfg(test)]
mod tests;
