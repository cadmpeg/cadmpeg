// SPDX-License-Identifier: Apache-2.0
//! The sketch fields a CADIR write carries because the design restates them
//! (CADIR v11 §3.2; §6.1 "Flags").

use std::collections::HashSet;

use cadmpeg_ir::design::ops::sketch::{DefiningRole, EntityDefinition, EntityRole, PlanarCurve};
use cadmpeg_ir::document::CadIr;

use super::sketch_and_extrude;
use super::state_zero::{entity, project};

/// The fields a CADIR write of `ir` names for the record `id` of the model
/// arena `arena`: none where it carries the record whole.
pub(super) fn charge(ir: &CadIr, arena: &str, id: &str) -> Vec<String> {
    use cadmpeg_ir::codec::write::{CadirEncoder, EncodeInput, Encoder};

    let plan = CadirEncoder
        .plan(EncodeInput::new(ir, None))
        .expect("the CADIR writer drops what it cannot write");
    let Some(loss) = plan.report().losses.iter().find(|loss| {
        loss.code.local_code() == "arena.omitted"
            && loss.message.contains(&format!("model arena `{arena}`"))
    }) else {
        return Vec::new();
    };
    let (omitted, part) = loss
        .message
        .split_once(" stated in part: ")
        .unwrap_or((loss.message.as_str(), ""));
    assert!(!omitted.contains(id), "{}", loss.message);
    part.split_once(&format!("{id} ("))
        .and_then(|(_, fields)| fields.split_once(')'))
        .map(|(fields, _)| fields.split(", ").map(str::to_owned).collect())
        .unwrap_or_default()
}

fn design_entity<'a>(
    ir: &'a CadIr,
    id: &cadmpeg_ir::sketches::SketchEntityId,
) -> &'a cadmpeg_ir::design::ops::sketch::SketchEntity<cadmpeg_ir::design::ops::sketch::Planar> {
    ir.design
        .sketch_entities
        .iter()
        .find(|entity| &entity.id == id)
        .unwrap()
}

/// A line whose stored record states it is not construction geometry is a
/// design entity that states `false`, and a CADIR write carries the flag; a
/// line whose flag the decoder did not read states none.
#[test]
fn a_read_construction_flag_false_is_stated() {
    let (mut ir, _) = sketch_and_extrude();
    let (read, unread) = (entity("l0"), entity("l1"));
    for model in &mut ir.model.sketch_entities {
        if model.id() == &read {
            model.read_construction(false);
        }
    }
    project(&mut ir, &HashSet::new());
    assert_eq!(design_entity(&ir, &read).construction, Some(false));
    assert_eq!(design_entity(&ir, &unread).construction, None);
    for id in [&read, &unread] {
        let fields = charge(&ir, "sketch_entities", id.as_str());
        assert!(
            !fields.iter().any(|field| field == "construction"),
            "{fields:?}"
        );
    }
}

/// The ends of a line whose endpoint records no point entity of the sketch
/// carries, as B-rep edges of a sketch store them, are points the design
/// states at those ends; a CADIR write carries the line's endpoint records.
#[test]
fn endpoint_records_without_point_entities_are_stated_as_points() {
    use cadmpeg_ir::sketches::SketchConstraintDefinitionInput as Kind;

    let (mut ir, _) = sketch_and_extrude();
    let coincidence = ir
        .model
        .sketch_constraints
        .iter_mut()
        .find(|constraint| matches!(constraint.definition.kind(), Kind::CoincidentLoci { .. }))
        .unwrap();
    coincidence.native_ref = Some("native:corner-relation".to_owned());
    let coincidence = coincidence.id.clone();
    ir.model.sketch_entities.retain(|model| {
        !matches!(
            model.geometry.definition(),
            cadmpeg_ir::sketches::SketchGeometryDefinition::Point { .. }
        )
    });
    project(&mut ir, &HashSet::new());
    let (first, second) = (entity("l0"), entity("l1"));
    let shared = ir
        .design
        .sketch_entities
        .iter()
        .filter(|point| point.native_ref.as_deref() == Some("native:b"))
        .collect::<Vec<_>>();
    let [shared] = shared.as_slice() else {
        panic!("one point carries the shared corner: {shared:#?}");
    };
    assert!(matches!(
        shared.definition,
        EntityDefinition::Free(PlanarCurve::Point(position))
            if position.get().u == 50.0 && position.get().v == 0.0
    ));
    assert_eq!(
        shared.roles,
        [
            EntityRole {
                of: first.clone(),
                role: DefiningRole::End,
            },
            EntityRole {
                of: second.clone(),
                role: DefiningRole::Start,
            },
        ]
    );
    assert!(!ir
        .design
        .sketch_constraints
        .iter()
        .any(|constraint| constraint.id == coincidence));
    assert_eq!(
        charge(&ir, "sketch_constraints", coincidence.as_str()),
        ["native_ref"]
    );
    for line in 0..4 {
        let id = entity(&format!("l{line}"));
        let fields = charge(&ir, "sketch_entities", id.as_str());
        assert!(
            !fields.iter().any(|field| field == "endpoint_refs"),
            "{fields:?}"
        );
    }
}

#[test]
fn a_read_flag_takes_precedence_over_outside_profile_inference() {
    let (mut ir, _) = sketch_and_extrude();
    let id = entity("diagonal");
    let model = cadmpeg_ir::sketches::SketchEntity::new(
        id.clone(),
        ir.model.sketches[0].id.clone(),
        cadmpeg_ir::sketches::SketchGeometryDefinition::Line {
            start: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            end: cadmpeg_ir::math::Point2::new(50.0, 30.0),
        }
        .try_into()
        .unwrap(),
    )
    .with_read_construction(false);
    ir.model.sketch_entities.push(model);
    project(&mut ir, &HashSet::from([id.clone()]));
    assert_eq!(design_entity(&ir, &id).construction, Some(false));
    assert!(!charge(&ir, "sketch_entities", id.as_str())
        .iter()
        .any(|field| field == "construction"));
}

#[test]
fn a_bounded_ellipse_carries_its_endpoint_records() {
    use cadmpeg_core::serde_erase::json;
    use cadmpeg_ir::sketches::SketchEntity;

    let (mut ir, _) = sketch_and_extrude();
    let id = entity("ellipse");
    let ellipse: SketchEntity = json::from_value(cadmpeg_core::json!({
        "id": id.as_str(),
        "sketch": ir.model.sketches[0].id.as_str(),
        "construction": false,
        "endpoint_refs": ["native:ellipse-start", "native:ellipse-end"],
        "geometry": {
            "kind": "ellipse",
            "center": {"u": 10.0, "v": 10.0},
            "major_angle": 0.0,
            "major_radius": 3.0,
            "minor_radius": 2.0,
            "bounds": [0.0, std::f64::consts::FRAC_PI_2]
        }
    }))
    .unwrap();
    ir.model.sketch_entities.push(ellipse);
    project(&mut ir, &HashSet::new());
    let end = ir
        .design
        .sketch_entities
        .iter()
        .find(|point| point.native_ref.as_deref() == Some("native:ellipse-end"))
        .unwrap();
    assert!(matches!(
        end.definition,
        EntityDefinition::Free(PlanarCurve::Point(position))
            if position.u == 10.0 && position.v == 12.0
    ));
    assert_eq!(
        end.roles,
        [EntityRole {
            of: id.clone(),
            role: DefiningRole::End
        }]
    );
    assert!(!charge(&ir, "sketch_entities", id.as_str())
        .iter()
        .any(|field| field == "endpoint_refs"));
}
