// SPDX-License-Identifier: Apache-2.0
//! Project a paste of copies within one component as a body move, and a
//! paste into another component as inserted bodies.

use crate::ids::{self, native_stream};
use crate::records::bodies::DesignBodyBinding;
use crate::records::feature::scope::DesignParameterScope;
use cadmpeg_ir::math::Vector3;

/// The one body binding of `suffix` in the stream of `scope`.
fn scope_binding<'a>(
    scope: &DesignParameterScope,
    body_bindings: &'a [DesignBodyBinding],
    suffix: u64,
) -> Option<&'a DesignBodyBinding> {
    let stream = native_stream(&scope.id).unwrap_or(ids::DEFAULT_STREAM);
    let mut matches = body_bindings.iter().filter(|binding| {
        native_stream(binding.id()) == Some(stream) && binding.entity_suffix == suffix
    });
    let binding = matches.next()?;
    matches.next().is_none().then_some(binding)
}

/// A paste of distinct copies whose every source and copy are current bodies
/// of one component: a move with one copy and no transform.
pub(super) fn same_component_paste_copy(
    scope: &DesignParameterScope,
    construction: &crate::records::feature::base_feature::DesignBaseFeatureConstruction,
    body_bindings: &[DesignBodyBinding],
) -> Option<(
    cadmpeg_ir::features::FeatureDefinition,
    Vec<cadmpeg_ir::ids::BodyId>,
)> {
    use cadmpeg_ir::features::selections::BodySelection;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let current_body = |suffix: u64| {
        let binding = scope_binding(scope, body_bindings, suffix)?;
        let component = binding.component_owner.as_ref()?.component_for(suffix)?;
        Some((binding.body.as_ref()?, component))
    };
    let sources = construction.copy_source_suffixes();
    let copies = construction.body_entity_suffixes();
    if sources.len() == 0 || sources.len() != copies.len() {
        return None;
    }
    let mut source_bodies = Vec::new();
    let mut copy_bodies = Vec::new();
    let mut component = None;
    for (source, copy) in sources.zip(copies) {
        let (source_body, source_component) = current_body(source)?;
        let (copy_body, copy_component) = current_body(copy)?;
        if source_component != copy_component
            || *component.get_or_insert(source_component) != source_component
        {
            return None;
        }
        source_bodies.push(source_body.clone());
        copy_bodies.push(copy_body.clone());
    }
    let distinct_copies =
        cadmpeg_ir::features::selections::DistinctMembers::try_from(copy_bodies.clone()).is_ok();
    if !distinct_copies || copy_bodies.iter().any(|copy| source_bodies.contains(copy)) {
        return None;
    }
    Some((
        FeatureDefinition::Operation(FeatureOperation::MoveBody {
            bodies: BodySelection::Resolved {
                bodies: cadmpeg_ir::features::selections::DistinctMembers::try_from(source_bodies)
                    .ok()?,
                native: scope.id.clone(),
            },
            translation: cadmpeg_ir::features::geometry::FiniteVector3::new(Vector3::new(
                0.0, 0.0, 0.0,
            ))?,
            rotation: None,
            copies: 1,
        }),
        copy_bodies,
    ))
}

/// A paste of distinct copies into one component that holds none of their
/// sources: the copies are the bodies the paste inserts.
pub(super) fn cross_component_paste(
    scope: &DesignParameterScope,
    construction: &crate::records::feature::base_feature::DesignBaseFeatureConstruction,
    body_bindings: &[DesignBodyBinding],
) -> Option<(
    cadmpeg_ir::features::FeatureDefinition,
    Vec<cadmpeg_ir::ids::BodyId>,
)> {
    use cadmpeg_ir::features::selections::InsertedBodies;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let binding = |suffix: u64| scope_binding(scope, body_bindings, suffix);
    let sources = construction.copy_source_suffixes();
    let copies = construction.body_entity_suffixes();
    if sources.len() == 0 || sources.len() != copies.len() {
        return None;
    }
    let mut copy_bodies = Vec::new();
    let mut component = None;
    for copy in copies {
        let binding = binding(copy)?;
        let copy_component = binding.component_owner.as_ref()?.component_for(copy)?;
        if *component.get_or_insert(copy_component) != copy_component {
            return None;
        }
        copy_bodies.push(binding.body.as_ref()?.clone());
    }
    let component = component?;
    for source in sources {
        let source_component = binding(source)?
            .component_owner
            .as_ref()?
            .component_for(source)?;
        if source_component == component {
            return None;
        }
    }
    cadmpeg_ir::features::selections::DistinctMembers::try_from(copy_bodies.clone()).ok()?;
    Some((
        FeatureDefinition::Operation(FeatureOperation::InsertBodies {
            bodies: InsertedBodies::Resolved {
                native: scope.id.clone(),
            },
        }),
        copy_bodies,
    ))
}

/// The name of the timeline item of the paste object that owns `scope`: its
/// stored kind and family-local ordinal.
pub(super) fn timeline_name(scope: &DesignParameterScope) -> Option<String> {
    let name = scope.base_feature_construction()?.owner_timeline_name()?;
    Some(format!("{}{}", name.kind.value, name.ordinal.value))
}

#[cfg(test)]
mod tests;
