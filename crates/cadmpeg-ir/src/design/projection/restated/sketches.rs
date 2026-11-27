// SPDX-License-Identifier: Apache-2.0
//! The fields a design sketch, sketch entity or sketch constraint leaves out
//! of the model record of its identity, found by comparing the two records.

use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap, HashSet};

use crate::design::dimension::Dimension;
use crate::design::ops::sketch::{
    DefiningRole, EntityDefinition, EntityForm, Planar, Sketch, SketchConstraint, SketchEntity,
    SketchPlane,
};
use crate::design::options::NativeOption;
use crate::design::parameter::ParameterDefinition;
use crate::document::CadIr;
use crate::sketches::{
    Sketch as ModelSketch, SketchConstraint as ModelConstraint, SketchEntity as ModelEntity,
    SketchGeometryDefinition,
};
use crate::units::UnitVector3;

use super::super::fields::{
    Field, ModelSketchConstraintField, ModelSketchEntityField, ModelSketchField,
};
use super::super::sketch::planar_curve;
use super::{json_strings, record_cut};

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
        match design.construction {
            Some(construction) => construction == model.construction,
            None => !model.construction && !model.construction_read(),
        },
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
/// native form, alone or in the namespace of a codec.
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
                if names_kind(stated.as_str(), native_kind.as_str())
        ),
        (Some(_), EntityDefinition::Defined { solved: None, .. })
        | (None, EntityDefinition::Free(_)) => false,
    }
}

/// Whether the native kind `stated` is `kind`, or `kind` in the namespace of
/// a codec (`<codec>:<kind>`).
fn names_kind(stated: &str, kind: &str) -> bool {
    stated == kind || stated.split_once(':').is_some_and(|(_, bare)| bare == kind)
}

/// The fields each model sketch constraint of `ir` that `compared` accepts
/// leaves out of the design constraint of its identity: `driving`, `active`
/// and `orientation` by what the design states of each, and every other
/// field where the design constraint does not hold its value under its name.
/// A constraint whose records do not serialize is left out, so the write
/// charges it whole.
pub(super) fn constraint_cuts<'ir>(
    ir: &'ir CadIr,
    compared: &dyn Fn(&str) -> bool,
) -> Vec<(&'ir str, BTreeSet<Cow<'static, str>>)> {
    let constraints = ir
        .design
        .sketch_constraints
        .iter()
        .map(|constraint| (constraint.id.as_str(), constraint))
        .collect::<HashMap<_, _>>();
    let driven = ir
        .design
        .parameters
        .iter()
        .filter(|parameter| matches!(parameter.definition, ParameterDefinition::Driven))
        .map(|parameter| parameter.id.as_str())
        .collect::<HashSet<_>>();
    ir.model
        .sketch_constraints
        .iter()
        .filter_map(|model| {
            let id = model.id.as_str();
            let design = constraints.get(id).filter(|_| compared(id))?;
            Some((id, constraint_cut(model, design, &driven)?))
        })
        .collect()
}

/// The fields `design` leaves out of `model`. `driven` holds the identities
/// of the `Driven` design parameters.
fn constraint_cut(
    model: &ModelConstraint,
    design: &SketchConstraint<Planar>,
    driven: &HashSet<&str>,
) -> Option<BTreeSet<Cow<'static, str>>> {
    let typed = [
        (
            ModelSketchConstraintField::Driving,
            driving_carried(model, design, driven),
        ),
        (
            ModelSketchConstraintField::Active,
            model
                .active
                .is_none_or(|flag| design.active == Some(Dimension::Literal(flag))),
        ),
        (
            ModelSketchConstraintField::Orientation,
            orientation_carried(model, design),
        ),
    ];
    let mut cut = record_cut(
        &cadmpeg_core::serde_erase::json::to_value(model).ok()?,
        &cadmpeg_core::serde_erase::json::to_value(design).ok()?,
    );
    for (field, carried) in typed {
        cut.remove(field.key());
        if !carried {
            cut.insert(Cow::Borrowed(field.key()));
        }
    }
    Some(cut)
}

/// Whether `design` states the driving flag of `model`: a relation is a
/// reference where it binds a `Driven` parameter, and drives otherwise.
fn driving_carried(
    model: &ModelConstraint,
    design: &SketchConstraint<Planar>,
    driven: &HashSet<&str>,
) -> bool {
    model.driving.is_none_or(|driving| {
        let reference = json_strings(&design.relation)
            .iter()
            .any(|id| driven.contains(id.as_str()));
        driving != reference
    })
}

/// Whether a native option `<codec>:orientation` of `design` holds the
/// orientation of `model`.
fn orientation_carried(model: &ModelConstraint, design: &SketchConstraint<Planar>) -> bool {
    let key = ModelSketchConstraintField::Orientation.key();
    model.orientation.is_none_or(|value| {
        design.native_options.iter().any(|(name, option)| {
            name.as_str()
                .split_once(':')
                .is_some_and(|(_, name)| name == key)
                && matches!(
                    option,
                    NativeOption::Integer(Dimension::Literal(stated))
                        if *stated == i64::from(value)
                )
        })
    })
}
