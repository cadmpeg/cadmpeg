// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::pmdc::{PmDcListMetadata, PmDcReference, PmDcReferenceList};
use crate::record_identity::{Located, RecordTypeId};
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};

fn list(indices: &[u32]) -> PmDcReferenceList {
    PmDcReferenceList::new(
        2,
        (!indices.is_empty()).then_some(PmDcListMetadata::U32([2, 0])),
        indices
            .iter()
            .copied()
            .map(PmDcReference::from_packed)
            .collect(),
    )
    .expect("synthetic reference list")
}

fn ellipse(ends: &[u32], state: u8) -> PmDcSketchEntityKind {
    PmDcSketchEntityKind::Ellipse {
        points: list(ends),
        auxiliary: Vec::new(),
        center: PmDcReference::from_packed(1),
        major_direction: [FiniteReal::ONE, FiniteReal::ZERO],
        major_radius: PositiveReal::new(3.0).expect("major radius"),
        minor_radius: PositiveReal::new(2.0).expect("minor radius"),
        state,
    }
}

fn entity(kind: PmDcSketchEntityKind) -> PmDcSketchEntity {
    let mut payload: super::super::PmDcSketchEntityPayload =
        serde_json::from_value(serde_json::json!({
            "save_version_major": 22,
            "header": {
                "header_value": 0, "header_id": 0, "next": PmDcReference::from_packed(0),
                "flags": 0, "context": PmDcReference::from_packed(0), "source_index": 0
            },
            "entity_flags": 0, "sketch": PmDcReference::from_packed(3),
            "kind": {"form": "point", "position": [0.0, 0.0],
                "endpoint_of": list(&[]), "center_of": list(&[]),
                "state": null, "associations": null}
        }))
        .expect("synthetic entity");
    let record_type = RecordTypeId::from_bytes(kind.record_type_id());
    payload.kind = kind;
    Located::new(
        payload,
        record_type,
        cadmpeg_ir::identity_key!("generated"),
        0,
    )
}

#[test]
fn closed_ellipse_requires_state_one_and_two_unqualified_null_ends() {
    assert!(closed_ellipse(&entity(ellipse(&[0, 0], 1))));
    assert!(closed_ellipse(&entity(ellipse(&[0, 0, 2], 1))));
    assert!(!closed_ellipse(&entity(ellipse(&[0x8000_0000, 0], 1))));
    for ends in [&[][..], &[0][..], &[1, 2][..], &[0, 1][..]] {
        assert!(!closed_ellipse(&entity(ellipse(ends, 1))));
    }
    for state in [0, 2, u8::MAX] {
        assert!(!closed_ellipse(&entity(ellipse(&[0, 0], state))));
    }
}

#[test]
fn native_curve_losses_name_each_retained_helper_and_trim_reason() {
    let identity = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
        .map(|row| row.map(|value| FiniteReal::new(value).expect("matrix value")));
    for (kind, reason) in [
        (
            PmDcSketchEntityKind::Polygon {
                reference: PmDcReference::from_packed(1),
                value: 0,
            },
            "polygon control value",
        ),
        (
            PmDcSketchEntityKind::BlockInstance {
                definition: PmDcReference::from_packed(1),
                group: PmDcReference::from_packed(2),
                masks: [0, 0],
                placement: identity,
                value: 1,
            },
            "block instance control value",
        ),
        (
            PmDcSketchEntityKind::BlockMember {
                instances: list(&[1]),
                member: PmDcReference::from_packed(2),
            },
            "block member link",
        ),
        (ellipse(&[1, 2], 0), "directed trim rule"),
        (ellipse(&[0, 0], 2), "directed trim rule"),
    ] {
        let source = entity(kind);
        let geometry = super::super::project_geometry(&source, &std::collections::HashMap::new());
        assert!(matches!(
            geometry
                .as_ref()
                .map(cadmpeg_ir::sketches::SketchGeometry::definition),
            Some(cadmpeg_ir::sketches::SketchGeometryDefinition::Native { .. })
        ));
        let expected_code = if matches!(source.kind, PmDcSketchEntityKind::Ellipse { .. }) {
            crate::loss::InventorLossCode::SketchEllipseCurveNative.code()
        } else {
            crate::loss::InventorLossCode::SketchCurveNative.code()
        };
        let losses = native_losses(&[source.clone(), source]);
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].code.namespace(), "inventor");
        assert_eq!(losses[0].code.local_code(), expected_code);
        assert!(losses[0].message.contains("Retained 2"));
        assert!(losses[0].message.contains(reason));
    }
    assert!(native_losses(&[entity(ellipse(&[0, 0], 1))]).is_empty());
}

#[test]
fn ellipse_projection_checks_the_stored_state() {
    let center = entity(PmDcSketchEntityKind::Point {
        position: [FiniteReal::ZERO; 2],
        endpoint_of: list(&[]),
        center_of: list(&[]),
        tail: super::super::PointTail::Absent,
    });
    let sources = std::collections::HashMap::from([(("generated", 0), Some(&center))]);
    for state in [0, 1, 2, u8::MAX] {
        let mut source = entity(ellipse(&[0, 0], state));
        source.identity.record_ordinal = 4;
        let projected = super::super::project_geometry(&source, &sources);
        let projected = projected.expect("ellipse entity");
        if state == 1 {
            assert!(matches!(projected.definition(),
                cadmpeg_ir::sketches::SketchGeometryDefinition::Ellipse { bounds: None, radii, .. }
                if radii.major().get() == 30.0 && radii.minor().get() == 20.0));
        } else {
            assert!(matches!(
                projected.definition(),
                cadmpeg_ir::sketches::SketchGeometryDefinition::Native { .. }
            ));
        }
    }
}

#[test]
fn untyped_and_absent_sketch_members_have_pinned_losses() {
    let unresolved = HashSet::from([("generated", 1), ("generated", 2), ("generated", 3)]);
    let losses = member_loss_counts(
        &unresolved,
        [
            ("generated", 1, [7; 16]),
            ("generated", 2, [7; 16]),
            ("generated", 4, [8; 16]),
            ("other", 3, [9; 16]),
        ],
    );
    assert_eq!(losses.len(), 2);
    for loss in &losses {
        assert_eq!(loss.code.namespace(), "inventor");
        assert_eq!(
            loss.code.local_code(),
            crate::loss::InventorLossCode::SketchMemberNative.code()
        );
    }
    assert!(losses[0].message.contains("Retained 2"));
    assert!(losses[0]
        .message
        .contains("07070707070707070707070707070707"));
    assert!(losses[0]
        .message
        .contains("byte grammar and curve operands"));
    assert!(losses[1].message.contains("Retained 1"));
    assert!(losses[1].message.contains("source record is absent"));
    assert!(member_loss_counts(&HashSet::new(), [("generated", 1, [7; 16])]).is_empty());
}

#[test]
fn midpoint_controls_require_a_null_parameter_and_zero_suffix() {
    let sources = crate::test_support::sketch_curves::line_midpoint_entities();
    let index = cadmpeg_core::decode::unique_index(sources.iter().map(|source| {
        (
            (
                source.identity.segment_token.as_str(),
                source.identity.record_ordinal,
            ),
            source,
        )
    }));
    let sketch =
        cadmpeg_ir::sketches::SketchId::mint("inventor:design:sketch#generated-2").expect("sketch");
    let entity = |source: &PmDcSketchEntity| {
        SketchEntity::new(
            super::super::entity_id(source).expect("entity identity"),
            sketch.clone(),
            super::super::project_geometry(source, &index).expect("source geometry"),
        )
    };
    let point = entity(&sources[0]);
    let curve = entity(&sources[2]);
    for (parameter, value, admitted) in [
        (0, Some(0), true),
        (4, Some(0), false),
        (0, Some(1), false),
        (0, None, false),
    ] {
        let mut wire = serde_json::to_value(crate::test_support::sketch_curves::spline_midpoint())
            .expect("constraint wire");
        wire["header"]["parameter"] = serde_json::json!(PmDcReference::from_packed(parameter));
        wire["kind"]["value"] = serde_json::json!(value);
        let constraint: super::super::PmDcSketchConstraint =
            serde_json::from_value(wire).expect("synthetic constraint");
        assert_eq!(midpoint_supported(&constraint, &curve.geometry), admitted);
        let definition = midpoint_definition(
            &constraint,
            PmDcReference::from_packed(8),
            PmDcReference::from_packed(4),
            &point,
            &curve,
        );
        assert_eq!(
            matches!(definition, SketchConstraintDefinitionInput::Midpoint { .. }),
            admitted
        );
        let inventory = SketchInventory {
            sketches: Vec::new(),
            entities: sources.clone(),
            transforms: Vec::new(),
            directions: Vec::new(),
            constraints: vec![constraint],
            issues: Vec::new(),
        };
        let loss = relation_loss(&inventory);
        assert_eq!(loss.is_none(), admitted);
        if let Some(loss) = loss {
            assert_eq!(
                loss.code.local_code(),
                InventorLossCode::SketchMidpointCurveNative.code()
            );
            assert!(loss.message.contains("control values"));
        }
    }
}
