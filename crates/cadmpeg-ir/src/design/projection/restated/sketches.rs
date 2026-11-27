// SPDX-License-Identifier: Apache-2.0
//! The fields a design sketch or sketch entity leaves out of the model
//! record of its identity, found by comparing the two records.

use std::collections::{BTreeSet, HashMap};

use crate::design::ops::sketch::{
    DefiningRole, EntityDefinition, EntityForm, Planar, Sketch, SketchEntity, SketchPlane,
};
use crate::document::CadIr;
use crate::sketches::{
    Sketch as ModelSketch, SketchEntity as ModelEntity, SketchGeometryDefinition,
};
use crate::units::UnitVector3;

use super::super::fields::{Field, ModelSketchEntityField, ModelSketchField};
use super::super::sketch::planar_curve;

/// The fields each model sketch and sketch entity of `ir` that `compared`
/// accepts leaves out of the design record of its identity.
pub(super) fn sketch_cuts<'ir>(
    ir: &'ir CadIr,
    compared: &dyn Fn(&str) -> bool,
) -> Vec<(&'ir str, BTreeSet<&'static str>)> {
    let sketches = ir
        .design
        .sketches
        .iter()
        .map(|sketch| (sketch.id.as_str(), sketch))
        .collect::<HashMap<_, _>>();
    let entities = ir
        .design
        .sketch_entities
        .iter()
        .map(|entity| (entity.id.as_str(), entity))
        .collect::<HashMap<_, _>>();
    let mut points = HashMap::<(&str, &str), Vec<&SketchEntity<Planar>>>::new();
    for entity in &ir.design.sketch_entities {
        if let Some(native) = &entity.native_ref {
            points
                .entry((entity.sketch.as_str(), native.as_str()))
                .or_default()
                .push(entity);
        }
    }
    let sketch_cuts = ir.model.sketches.iter().filter_map(|model| {
        let id = model.id.as_str();
        let design = sketches.get(id).filter(|_| compared(id))?;
        Some((id, sketch_cut(model, design)))
    });
    let entity_cuts = ir.model.sketch_entities.iter().filter_map(|model| {
        let id = model.id().as_str();
        let design = entities.get(id).filter(|_| compared(id))?;
        Some((id, entity_cut(model, design, &points)))
    });
    sketch_cuts.chain(entity_cuts).collect()
}

/// The fields `design` leaves out of `model`.
fn sketch_cut(model: &ModelSketch, design: &Sketch) -> BTreeSet<&'static str> {
    let mut cut = BTreeSet::new();
    let mut leave = |field: ModelSketchField, carried: bool| {
        if !carried {
            cut.insert(field.key());
        }
    };
    leave(
        ModelSketchField::Name,
        model.name.is_none() || model.name == design.name,
    );
    leave(
        ModelSketchField::Configuration,
        model.configuration.is_none(),
    );
    leave(ModelSketchField::Visible, model.visible.is_none());
    leave(
        ModelSketchField::Placement,
        placement_carried(model, design),
    );
    leave(ModelSketchField::Profiles, model.profiles.is_empty());
    leave(
        ModelSketchField::NativeRef,
        model.native_ref.is_none() || model.native_ref == design.native_ref,
    );
    cut
}

/// Whether the plane of `design` states the frame `model` stores: its
/// origin, and its normal and first axis as unit vectors.
fn placement_carried(model: &ModelSketch, design: &Sketch) -> bool {
    let Some((origin, normal, u_axis)) = model.placement.resolved() else {
        return true;
    };
    let SketchPlane::Placed(construction) = &design.plane else {
        return false;
    };
    construction.resolved.as_ref().is_some_and(|frame| {
        frame.origin == origin
            && Some(frame.z_axis) == UnitVector3::normalized_nonzero(normal)
            && Some(frame.x_axis) == UnitVector3::normalized_nonzero(u_axis)
    })
}

/// The fields `design` leaves out of `model`. `points` holds the design
/// entities by sketch and native record.
fn entity_cut(
    model: &ModelEntity,
    design: &SketchEntity<Planar>,
    points: &HashMap<(&str, &str), Vec<&SketchEntity<Planar>>>,
) -> BTreeSet<&'static str> {
    let mut cut = BTreeSet::new();
    let mut leave = |field: ModelSketchEntityField, carried: bool| {
        if !carried {
            cut.insert(field.key());
        }
    };
    leave(
        ModelSketchEntityField::Sketch,
        model.sketch == design.sketch,
    );
    leave(
        ModelSketchEntityField::Construction,
        design.construction == Some(model.construction),
    );
    leave(
        ModelSketchEntityField::NativeRef,
        model.native_ref.is_none() || model.native_ref == design.native_ref,
    );
    leave(
        ModelSketchEntityField::GeometryRef,
        model.geometry_ref.is_none(),
    );
    leave(
        ModelSketchEntityField::EndpointRefs,
        endpoints_carried(model, design, points),
    );
    leave(
        ModelSketchEntityField::Geometry,
        geometry_carried(model, design),
    );
    cut
}

/// Whether the design states the two endpoint records of `model`: each is
/// the native record of a design entity of its sketch that is the start, or
/// the end, of `model`.
fn endpoints_carried(
    model: &ModelEntity,
    design: &SketchEntity<Planar>,
    points: &HashMap<(&str, &str), Vec<&SketchEntity<Planar>>>,
) -> bool {
    match model.endpoint_refs.as_slice() {
        [] => true,
        [start, end] => [(start, DefiningRole::Start), (end, DefiningRole::End)]
            .into_iter()
            .all(|(native, role)| {
                points
                    .get(&(design.sketch.as_str(), native.as_str()))
                    .is_some_and(|candidates| {
                        candidates.iter().any(|point| {
                            point
                                .roles
                                .iter()
                                .any(|held| held.of == design.id && held.role == role)
                        })
                    })
            }),
        _ => false,
    }
}

/// Whether the design states the solved geometry of `model`: its planar
/// curve as the free curve or the solved curve, or its native kind as the
/// native form.
fn geometry_carried(model: &ModelEntity, design: &SketchEntity<Planar>) -> bool {
    match (planar_curve(&model.geometry), &design.definition) {
        (Some(curve), EntityDefinition::Free(stated)) => *stated == curve,
        (
            Some(curve),
            EntityDefinition::Defined {
                solved: Some(stated),
                ..
            },
        ) => *stated == curve,
        (None, EntityDefinition::Defined { form, .. }) => matches!(
            (model.geometry.definition(), form.as_ref()),
            (SketchGeometryDefinition::Native { native_kind }, EntityForm::Native(stated))
                if stated == native_kind
        ),
        (Some(_), EntityDefinition::Defined { solved: None, .. })
        | (None, EntityDefinition::Free(_)) => false,
    }
}
