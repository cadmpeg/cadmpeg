// SPDX-License-Identifier: Apache-2.0
//! Exhaustive typed walk over `Design` replacing whole-design JSON
//! serialization for `check_native_keys` and `check_regions`.
//!
//! Field traversal preserves the exact LIFO reverse-alphabetical-key,
//! reverse-sequence, and nearest-`"id"`-owner order of the legacy JSON walk.

mod ops;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use crate::design::construction::{Construction, ConstructionKind};
use crate::design::expr::{Expression, Measure, MeasureTarget, Neutral};
use crate::design::ops::datum::{Datum, DatumElement};
use crate::design::ops::sketch::{
    AngleOperands, Derivation, DerivationMode, DistanceAxis, EntityDefinition, EntityForm, Operand,
    Planar, Sketch, SketchConstraint, SketchEntity, SketchPattern, SketchPatternAxis,
    SketchPatternDirection, SketchPatternLayout, SketchPlane, SketchRelation, SketchSpace, Spatial,
    SpatialSketch,
};
use crate::design::options::NativeOptions;
use crate::design::selection::{
    MemberOrder, Members, PickDomain, PickTarget, Picked, Picks, RuleTerm, Selection, SketchRegion,
};
use crate::document::Design;
use crate::sketches::SketchId;

use crate::design::configuration::{
    Arrangement, ArrangementMove, DesignConfiguration, FeatureStateOverride, MaterialOverride,
    Overrides, ParameterOverride, Publication,
};
use crate::design::construction::{AnyConstruction, ConstructionInput, InputReference, SourceMode};
use crate::design::container::Container;
use crate::design::feature::{Feature, FeatureDefinition, FeatureOperation, Link, StoredOrigin};
use crate::design::options::{
    Boolean, NativeOption, ProjectionDirection, SectionCondition, SheetOutput, SolidOutput,
    Support, Targets,
};
use crate::design::parameter::{DesignParameter, ParameterDefinition};
use crate::design::program::{ParameterProgram, ProgramBody, Statement};
use crate::design::scope::{DesignScope, OccurrenceDesign};
use crate::design::selection::SelectionRule;

pub(super) struct WireFound<'a> {
    pub(super) native_keys: Vec<(&'a str, &'a NativeOptions, &'a NativeOptions)>,
    pub(super) sketch_regions: Vec<(&'a SketchId, &'a SketchRegion)>,
}

impl<'a> WireFound<'a> {
    pub(super) fn record_native_options(
        &mut self,
        owner: &'a str,
        native_options: &'a NativeOptions,
        source_settings: &'a NativeOptions,
    ) {
        if !native_options.is_empty() && !source_settings.is_empty() {
            self.native_keys
                .push((owner, native_options, source_settings));
        }
    }
}

pub(super) fn walk_design(design: &Design) -> WireFound<'_> {
    let mut out = WireFound {
        native_keys: Vec::new(),
        sketch_regions: Vec::new(),
    };
    let Design {
        scopes,
        containers,
        features,
        sketches,
        sketch_entities,
        sketch_constraints,
        spatial_sketches,
        spatial_sketch_entities,
        spatial_sketch_constraints,
        parameters,
        spreadsheets: _,
        sheet_metal_rules,
        programs,
        configurations,
        arrangements,
        publications,
        evaluations: _,
        names: _,
    } = design;
    spatial_sketches.walk_wire("design", &mut out);
    spatial_sketch_entities.walk_wire("design", &mut out);
    spatial_sketch_constraints.walk_wire("design", &mut out);
    sketches.walk_wire("design", &mut out);
    sketch_entities.walk_wire("design", &mut out);
    sketch_constraints.walk_wire("design", &mut out);
    sheet_metal_rules.walk_wire("design", &mut out);
    scopes.walk_wire("design", &mut out);
    publications.walk_wire("design", &mut out);
    programs.walk_wire("design", &mut out);
    parameters.walk_wire("design", &mut out);
    features.walk_wire("design", &mut out);
    containers.walk_wire("design", &mut out);
    configurations.walk_wire("design", &mut out);
    arrangements.walk_wire("design", &mut out);
    out
}

pub(super) trait WireWalk {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>);
}

impl<T: WireWalk> WireWalk for Option<T> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        if let Some(value) = self {
            value.walk_wire(owner, out);
        }
    }
}

impl<T: WireWalk + ?Sized> WireWalk for Box<T> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        (**self).walk_wire(owner, out);
    }
}

impl<T: WireWalk> WireWalk for Vec<T> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        for item in self.iter().rev() {
            item.walk_wire(owner, out);
        }
    }
}

impl<T: WireWalk, const N: usize> WireWalk for [T; N] {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        for item in self.iter().rev() {
            item.walk_wire(owner, out);
        }
    }
}

impl<T: WireWalk, O> WireWalk for Members<T, O> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        for item in self.as_slice().iter().rev() {
            item.walk_wire(owner, out);
        }
    }
}

impl<K, V: WireWalk> WireWalk for BTreeMap<K, V> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        for value in self.values().rev() {
            value.walk_wire(owner, out);
        }
    }
}

impl<O: MemberOrder> WireWalk for Selection<O> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            at: _,
            picks,
            native: _,
        } = self;
        picks.walk_wire(owner, out);
    }
}

impl<O: MemberOrder> WireWalk for Picks<O> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Unread => {}
            Self::Named(picks) => picks.walk_wire(owner, out),
            Self::Rule {
                terms,
                resolved,
                excluded,
            } => {
                terms.walk_wire(owner, out);
                resolved.walk_wire(owner, out);
                excluded.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for Picked {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            witness: _,
            target,
            resolved: _,
            native: _,
            domain,
            context: _,
        } = self;
        target.walk_wire(owner, out);
        domain.walk_wire(owner, out);
    }
}

impl WireWalk for PickTarget {
    fn walk_wire<'a>(&'a self, _owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::SketchRegion { sketch, region } => out.sketch_regions.push((sketch, region)),
            Self::Entity(_)
            | Self::Native { .. }
            | Self::Sketch(_)
            | Self::SketchEntity(_)
            | Self::SpatialSketchEntity(_)
            | Self::SketchFrame { .. }
            | Self::Datum { .. }
            | Self::Principal { .. }
            | Self::Feature(_)
            | Self::FeatureResult(_)
            | Self::TemplateInput { .. }
            | Self::Component
            | Self::Publication(_) => {}
        }
    }
}

impl<R> WireWalk for Expression<R> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Neutral(neutral) => neutral.walk_wire(owner, out),
            Self::Source { .. } => {}
        }
    }
}

impl<R> WireWalk for Neutral<R> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        for measure in self.measures().into_iter().rev() {
            measure.walk_wire(owner, out);
        }
    }
}

impl WireWalk for Measure {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            of,
            kind: _,
            cached: _,
        } = self;
        of.walk_wire(owner, out);
    }
}

impl WireWalk for MeasureTarget {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Model => {}
            Self::Selection(selection) => selection.walk_wire(owner, out),
        }
    }
}

impl<K: ConstructionKind> WireWalk for Construction<K> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            solution: _,
            resolved_in: _,
            resolved: _,
            mode,
            inputs,
            after: _,
        } = self;
        mode.walk_wire(owner, out);
        inputs.walk_wire(owner, out);
    }
}

trait SketchSpaceIds: SketchSpace {
    fn entity_id_str(id: &Self::Entity) -> &str;
    fn constraint_id_str(id: &Self::Constraint) -> &str;
}

impl SketchSpaceIds for Planar {
    fn entity_id_str(id: &Self::Entity) -> &str {
        id.as_str()
    }

    fn constraint_id_str(id: &Self::Constraint) -> &str {
        id.as_str()
    }
}

impl SketchSpaceIds for Spatial {
    fn entity_id_str(id: &Self::Entity) -> &str {
        id.as_str()
    }

    fn constraint_id_str(id: &Self::Constraint) -> &str {
        id.as_str()
    }
}

impl<S: SketchSpaceIds> WireWalk for SketchEntity<S> {
    fn walk_wire<'a>(&'a self, _owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            sketch: _,
            roles: _,
            native_ref: _,
            native_options,
            native: _,
            id,
            definition,
            construction: _,
            centerline: _,
        } = self;
        let owner = S::entity_id_str(id);
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        definition.walk_wire(owner, out);
    }
}

impl<S: SketchSpaceIds> WireWalk for SketchConstraint<S> {
    fn walk_wire<'a>(&'a self, _owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            sketch: _,
            relation,
            native_ref: _,
            native_options,
            name: _,
            id,
            active: _,
        } = self;
        let owner = S::constraint_id_str(id);
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        relation.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
    }
}

impl<S: SketchSpace> WireWalk for AngleOperands<S> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Between(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Sweep(_) => {}
            Self::Points(_) => {}
        }
    }
}

impl WireWalk for AnyConstruction {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Point(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Direction(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Axis(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Plane(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Frame(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Curve(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for Arrangement {
    fn walk_wire<'a>(&'a self, _owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            scope: _,
            overrides,
            nested: _,
            native_ref: _,
            name: _,
            moves,
            kind: _,
            id,
            configuration: _,
            appearances: _,
        } = self;
        let owner = id.as_str();
        overrides.walk_wire(owner, out);
        moves.walk_wire(owner, out);
    }
}

impl WireWalk for ArrangementMove {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            step,
            occurrences: _,
        } = self;
        step.walk_wire(owner, out);
    }
}

impl WireWalk for Boolean {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::NewBody => {}
            Self::Apply { targets, op: _ } => {
                targets.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for ConstructionInput {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            value: _,
            role: _,
            reversed: _,
            reference,
        } = self;
        reference.walk_wire(owner, out);
    }
}

impl WireWalk for Container {
    fn walk_wire<'a>(&'a self, _owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Ordered {
                rollback: _,
                placement,
                members: _,
                kind: _,
                id,
                folders: _,
            } => {
                let owner = id.as_str();
                placement.walk_wire(owner, out);
            }
            Self::Unordered {
                members: _,
                kind: _,
                id: _,
            } => {}
        }
    }
}

impl WireWalk for Datum {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            native_options,
            link,
            element,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        link.walk_wire(owner, out);
        element.walk_wire(owner, out);
    }
}

impl WireWalk for DatumElement {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Points(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Axis(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Plane(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Frame(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for Derivation {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source,
            piece: _,
            mode,
            link: _,
        } = self;
        source.walk_wire(owner, out);
        mode.walk_wire(owner, out);
    }
}

impl WireWalk for DerivationMode {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Project => {}
            Self::Include => {}
            Self::Intersect => {}
            Self::Silhouette => {}
            Self::Offset { distance: _ } => {}
            Self::OntoFaces { faces, direction } => {
                faces.walk_wire(owner, out);
                direction.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for DesignConfiguration {
    fn walk_wire<'a>(&'a self, _owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            scope: _,
            parent: _,
            overrides,
            native_ref: _,
            name: _,
            position: _,
            materials,
            id,
            comment: _,
            active_arrangements: _,
        } = self;
        let owner = id.as_str();
        overrides.walk_wire(owner, out);
        materials.walk_wire(owner, out);
    }
}

impl WireWalk for DesignParameter {
    fn walk_wire<'a>(&'a self, _owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            value: _,
            unit: _,
            tolerance: _,
            source_settings,
            scope: _,
            owner: _,
            position: _,
            native_ref: _,
            native_options,
            name: _,
            kind: _,
            id,
            display: _,
            definition,
            decimals: _,
            comment: _,
        } = self;
        let owner = id.as_str();
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        definition.walk_wire(owner, out);
    }
}

impl WireWalk for DesignScope {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            unproduced: _,
            roots: _,
            occurrences,
            definition: _,
            current: _,
            active_configuration: _,
            active_arrangements: _,
        } = self;
        occurrences.walk_wire(owner, out);
    }
}

impl<S: SketchSpace> WireWalk for DistanceAxis<S> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Aligned => {}
            Self::Horizontal => {}
            Self::Vertical => {}
            Self::Along(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl<S: SketchSpace> WireWalk for EntityDefinition<S> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Free(_) => {}
            Self::Defined { solved: _, form } => {
                form.walk_wire(owner, out);
            }
        }
    }
}

impl<S: SketchSpace> WireWalk for EntityForm<S> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Interpolated(_) => {}
            Self::Conic { rho: _ } => {}
            Self::Derived(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Text(_) => {}
            Self::BlockInstance(_) => {}
            Self::Instanced { of: _, instance: _ } => {}
            Self::Frame => {}
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for Feature {
    fn walk_wire<'a>(&'a self, _owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            suppressed: _,
            placement,
            native_ref: _,
            name: _,
            id,
            driven_by: _,
            definition,
            component: _,
        } = self;
        let owner = id.as_str();
        placement.walk_wire(owner, out);
        definition.walk_wire(owner, out);
    }
}

impl WireWalk for FeatureDefinition {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            refine: _,
            operation,
            fuzzy_tolerance: _,
            assembly,
        } = self;
        operation.walk_wire(owner, out);
        assembly.walk_wire(owner, out);
    }
}

impl WireWalk for FeatureOperation {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Sketch(_) => {}
            Self::SpatialSketch(_) => {}
            Self::Spreadsheet(_) => {}
            Self::Datum(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Extrude(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Revolve(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Sweep(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::HelicalSweep(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Rib(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Primitive(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Loft(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Fillet(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Chamfer(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Shell(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Draft(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Thicken(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Offset(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Hole(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Thread(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Emboss(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Dome(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Decal(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Pattern(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::TemplateInstance(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Combine(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Split(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::DeleteBodies(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Extract(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::TransformBodies(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Flex(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::MoveFaces(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::DeleteFaces(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::ReplaceFaces(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Graft(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Patch(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Knit(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Unsew(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::UnsewFaces(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Extend(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::ImplicitSweep(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::BoundaryFill(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Curve(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Form(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::SheetMetal(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Mesh(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Place(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::PlaceSet(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Joint(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::RigidGroup(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::ComponentPattern(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::MirrorComponents(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::MaterialRemoval => {}
            Self::Stored {
                tessellations: _,
                origin,
                bodies: _,
            } => {
                origin.walk_wire(owner, out);
            }
            Self::Unresolved { family: _ } => {}
            Self::Native {
                source_settings,
                native_options,
                kind: _,
            } => {
                out.record_native_options(owner, native_options, source_settings);
                source_settings.walk_wire(owner, out);
                native_options.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for FeatureStateOverride {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            suppressed: _,
            definition,
        } = self;
        definition.walk_wire(owner, out);
    }
}

impl WireWalk for InputReference {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Picks(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Built(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for Link {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            transform: _,
            state: _,
            source,
            scale: _,
            native_ref: _,
            document: _,
            associative: _,
        } = self;
        source.walk_wire(owner, out);
    }
}

impl WireWalk for MaterialOverride {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            target,
            material: _,
        } = self;
        target.walk_wire(owner, out);
    }
}

impl WireWalk for NativeOption {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Bool(_) => {}
            Self::Integer(_) => {}
            Self::Real(_) => {}
            Self::Length(_) => {}
            Self::Angle(_) => {}
            Self::Text(_) => {}
            Self::Selection(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::OrderedSelection(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for OccurrenceDesign {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            parameters,
            grounded: _,
            flexible: _,
            configuration: _,
            arrangements: _,
        } = self;
        parameters.walk_wire(owner, out);
    }
}

impl<S: SketchSpace> WireWalk for Operand<S> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Entity(_) => {}
            Self::Frame(_) => {}
            Self::Model(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for Overrides {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            parameters,
            occurrences: _,
            features,
            definitions: _,
        } = self;
        parameters.walk_wire(owner, out);
        features.walk_wire(owner, out);
    }
}

impl WireWalk for ParameterDefinition {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Expression(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Driven => {}
            Self::Assigned(_) => {}
            Self::Unset => {}
        }
    }
}

impl WireWalk for ParameterOverride {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            value: _,
            expression,
        } = self;
        expression.walk_wire(owner, out);
    }
}

impl WireWalk for ParameterProgram {
    fn walk_wire<'a>(&'a self, _owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            stage: _,
            scope: _,
            owner: _,
            native_ref: _,
            name: _,
            id,
            body,
        } = self;
        let owner = id.as_str();
        body.walk_wire(owner, out);
    }
}

impl WireWalk for PickDomain {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::EdgeInterval { start: _, end: _ } => {}
            Self::Limits { start, end } => {
                start.walk_wire(owner, out);
                end.walk_wire(owner, out);
            }
            Self::Fragment(_) => {}
            Self::Containing(_) => {}
        }
    }
}

impl WireWalk for ProgramBody {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Neutral(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Source {
                tokens: _,
                dialect: _,
            } => {}
        }
    }
}

impl WireWalk for ProjectionDirection {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::SurfaceNormal => {}
            Self::SketchNormal => {}
            Self::Along(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for Publication {
    fn walk_wire<'a>(&'a self, _owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            target,
            scope: _,
            name: _,
            id,
        } = self;
        let owner = id.as_str();
        target.walk_wire(owner, out);
    }
}

impl<O: MemberOrder> WireWalk for RuleTerm<O> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            seeds,
            rule,
            propagation: _,
        } = self;
        seeds.walk_wire(owner, out);
        rule.walk_wire(owner, out);
    }
}

impl WireWalk for SectionCondition {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Free { support } => {
                support.walk_wire(owner, out);
            }
            Self::Default => {}
            Self::Direction {
                weight: _,
                direction,
                angle: _,
            } => {
                direction.walk_wire(owner, out);
            }
            Self::NormalToProfile {
                weight: _,
                angle: _,
            } => {}
            Self::Tangent {
                weight: _,
                support,
                flip: _,
            } => {
                support.walk_wire(owner, out);
            }
            Self::Curvature {
                weight: _,
                support,
                flip: _,
            } => {
                support.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for SelectionRule {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Explicit => {}
            Self::All { kind: _ } => {}
            Self::Of { kind: _ } => {}
            Self::LoopEdges => {}
            Self::Boundary => {}
            Self::Chain { end } => {
                end.walk_wire(owner, out);
            }
            Self::Region { boundary } => {
                boundary.walk_wire(owner, out);
            }
            Self::Intersection { with } => {
                with.walk_wire(owner, out);
            }
            Self::Affected => {}
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for SheetOutput {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { solid: _, boolean } = self;
        boolean.walk_wire(owner, out);
    }
}

impl WireWalk for Sketch {
    fn walk_wire<'a>(&'a self, _owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            solve: _,
            plane,
            owner: _,
            native_ref: _,
            native_options,
            name: _,
            id,
        } = self;
        let owner = id.as_str();
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        plane.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
    }
}

impl<S: SketchSpace> WireWalk for SketchPattern<S> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            seeds: _,
            layout,
            instances: _,
        } = self;
        layout.walk_wire(owner, out);
    }
}

impl<S: SketchSpace> WireWalk for SketchPatternAxis<S> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            spacing,
            sides,
            direction,
        } = self;
        spacing.walk_wire(owner, out);
        sides.walk_wire(owner, out);
        direction.walk_wire(owner, out);
    }
}

impl<S: SketchSpace> WireWalk for SketchPatternDirection<S> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Along { reversed: _, line } => {
                line.walk_wire(owner, out);
            }
            Self::Angle(_) => {}
        }
    }
}

impl<S: SketchSpace> WireWalk for SketchPatternLayout<S> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Linear { directions } => {
                directions.walk_wire(owner, out);
            }
            Self::Circular {
                spacing,
                sides,
                center,
            } => {
                spacing.walk_wire(owner, out);
                sides.walk_wire(owner, out);
                center.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for SketchPlane {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Placed(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Block => {}
        }
    }
}

impl WireWalk for SketchRegion {
    fn walk_wire<'a>(&'a self, _owner: &'a str, _out: &mut WireFound<'a>) {
        let Self { outer: _, holes: _ } = self;
    }
}

impl<S: SketchSpace> WireWalk for SketchRelation<S> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Coincident { b, a } => {
                b.walk_wire(owner, out);
                a.walk_wire(owner, out);
            }
            Self::Concentric { b, a } => {
                b.walk_wire(owner, out);
                a.walk_wire(owner, out);
            }
            Self::Collinear { b, a } => {
                b.walk_wire(owner, out);
                a.walk_wire(owner, out);
            }
            Self::Parallel { b, a } => {
                b.walk_wire(owner, out);
                a.walk_wire(owner, out);
            }
            Self::Perpendicular { b, a } => {
                b.walk_wire(owner, out);
                a.walk_wire(owner, out);
            }
            Self::Tangent { contact: _, b, a } => {
                b.walk_wire(owner, out);
                a.walk_wire(owner, out);
            }
            Self::Curvature { contact: _, b, a } => {
                b.walk_wire(owner, out);
                a.walk_wire(owner, out);
            }
            Self::Horizontal { b, a } => {
                b.walk_wire(owner, out);
                a.walk_wire(owner, out);
            }
            Self::Vertical { b, a } => {
                b.walk_wire(owner, out);
                a.walk_wire(owner, out);
            }
            Self::Midpoint { point: _, of } => {
                of.walk_wire(owner, out);
            }
            Self::Symmetric { b, about, a } => {
                b.walk_wire(owner, out);
                about.walk_wire(owner, out);
                a.walk_wire(owner, out);
            }
            Self::Equal { members } => {
                members.walk_wire(owner, out);
            }
            Self::EqualDistance { second, first } => {
                second.walk_wire(owner, out);
                first.walk_wire(owner, out);
            }
            Self::Fixed { of } => {
                of.walk_wire(owner, out);
            }
            Self::Distance {
                value: _,
                doubled: _,
                b,
                along,
                a,
            } => {
                b.walk_wire(owner, out);
                along.walk_wire(owner, out);
                a.walk_wire(owner, out);
            }
            Self::Angle {
                value: _,
                sector: _,
                between,
            } => {
                between.walk_wire(owner, out);
            }
            Self::Radial {
                value: _,
                of: _,
                measure: _,
            } => {}
            Self::Length { value: _, of: _ } => {}
            Self::Offset(_) => {}
            Self::Pattern(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Polygon(_) => {}
            Self::Slot(_) => {}
            Self::Pierce { point: _, curve } => {
                curve.walk_wire(owner, out);
            }
            Self::Weight { value: _, pole: _ } => {}
            Self::FixedLength { of: _ } => {}
            Self::FixedAngle { of: _ } => {}
            Self::Native {
                values: _,
                operands,
                kind: _,
            } => {
                operands.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for SolidOutput {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { boolean, body: _ } = self;
        boolean.walk_wire(owner, out);
    }
}

impl WireWalk for SourceMode {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            native_options,
            method: _,
            codec: _,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
    }
}

impl WireWalk for SpatialSketch {
    fn walk_wire<'a>(&'a self, _owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            solve: _,
            owner: _,
            native_ref: _,
            native_options,
            name: _,
            id,
        } = self;
        let owner = id.as_str();
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
    }
}

impl WireWalk for Statement {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Assign { value, target: _ } => {
                value.walk_wire(owner, out);
            }
            Self::If {
                then,
                otherwise,
                condition,
            } => {
                then.walk_wire(owner, out);
                otherwise.walk_wire(owner, out);
                condition.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for StoredOrigin {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Base {} => {}
            Self::Imported {} => {}
            Self::Snapshot {} => {}
            Self::Linked(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for Support {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Faces(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Computed => {}
        }
    }
}

impl WireWalk for Targets {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::All => {}
            Self::Selected(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}
