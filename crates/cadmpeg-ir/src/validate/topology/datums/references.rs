use crate::design::construction::{
    AnyConstruction, Construction, ConstructionInput, ConstructionKind, InputReference,
};
use crate::design::names::{Generator, TopoName};
use crate::design::options::{NativeOption, NativeOptions};
use crate::design::refs::{ConfigurationRef, EntityRef, Member, Position};
use crate::design::selection::{
    Anchor, DatumPart, DefinitionRef, MemberOrder, NativeOwner, OccurrencePath, PickContext,
    PickDomain, PickTarget, Picked, Picks, Selection, SelectionRule, StepRef,
};
use crate::features::{datums::DatumAxis, Feature, FeatureOperation, UnresolvedFamily};
use crate::index::ModelIndex;
use crate::report::check::{Check, Finding};
use crate::report::Severity;
use crate::schema::EntityKind;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

pub(super) fn validate(
    ctx: &DecodeContext<'_>,
    ids: &ModelIndex<'_>,
    owner: &Feature,
    axis: &DatumAxis,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut walk = References {
        ctx,
        ids,
        owner,
        findings,
    };
    walk.inputs(axis.inputs())?;
    walk.options(axis.native_options())?;
    walk.options(axis.source_settings())
}

struct References<'a, 'b, 'ir> {
    ctx: &'a DecodeContext<'b>,
    ids: &'a ModelIndex<'ir>,
    owner: &'a Feature,
    findings: &'a mut Vec<Finding>,
}

impl References<'_, '_, '_> {
    fn require(&mut self, id: &str, kind: EntityKind, description: &str) {
        if self.ids.neutral_kind(id) != Some(kind) {
            super::error(
                self.findings,
                self.owner,
                &format!("datum-axis {description}: {id}"),
            );
        }
    }
    fn undeclared(&mut self, id: &str, kind: &str) {
        super::error(
            self.findings,
            self.owner,
            &format!("datum-axis {kind} target has no declared {kind} arena: {id}"),
        );
    }
    fn configuration(&mut self, value: &ConfigurationRef) {
        if let ConfigurationRef::Local(id) = value {
            self.require(
                id.as_str(),
                EntityKind::DesignConfiguration,
                "configuration target must name a design configuration",
            );
        }
    }
    fn position(&mut self, value: &Position) {
        match value {
            Position::After(Member::Feature(id)) => self.require(
                id.as_str(),
                EntityKind::Feature,
                "position target must name a feature",
            ),
            Position::Start(id) | Position::After(Member::Container(id)) => {
                self.undeclared(id.as_str(), "container");
            }
        }
    }
    fn path(&mut self, value: &OccurrencePath) {
        for step in &value.0 {
            if let StepRef::Occurrence(id) = &step.occurrence {
                self.require(
                    id.as_str(),
                    EntityKind::Occurrence,
                    "path target must name an occurrence",
                );
            }
            if let Some(configuration) = &step.configuration {
                self.configuration(configuration);
            }
        }
    }
    fn definition(&mut self, value: &DefinitionRef) {
        self.require(
            value.definition.as_str(),
            EntityKind::ProductDefinition,
            "context target must name a product definition",
        );
        if let Some(configuration) = &value.configuration {
            self.configuration(configuration);
        }
    }
    fn context(&mut self, value: &PickContext) {
        match value {
            PickContext::Occurrence(path) => self.path(path),
            PickContext::Definition(value) => self.definition(value),
            PickContext::InContext {
                assembly,
                own,
                target,
            } => {
                self.definition(assembly);
                for path in own.iter().chain(target) {
                    self.path(path);
                }
            }
        }
    }
    fn native_owner(&mut self, value: &NativeOwner) {
        match value {
            NativeOwner::Feature(id) => self.require(
                id.as_str(),
                EntityKind::Feature,
                "native owner must name a feature",
            ),
            NativeOwner::Sketch(id) => self.require(
                id.as_str(),
                EntityKind::Sketch,
                "native owner must name a sketch",
            ),
            NativeOwner::SpatialSketch(id) => self.require(
                id.as_str(),
                EntityKind::SpatialSketch,
                "native owner must name a spatial sketch",
            ),
        }
    }
    fn name(&mut self, value: &TopoName) -> Result<(), CodecError> {
        let _depth = self
            .ctx
            .enter_nested("datum-axis name reference validation")?;
        match value {
            TopoName::Generated {
                feature, generator, ..
            } => {
                self.require(
                    feature.as_str(),
                    EntityKind::Feature,
                    "name provenance must name a feature",
                );
                match generator {
                    Some(Generator::SketchEntity(id)) => self.require(
                        id.as_str(),
                        EntityKind::SketchEntity,
                        "name generator must name a sketch entity",
                    ),
                    Some(Generator::SpatialSketchEntity(id)) => self.require(
                        id.as_str(),
                        EntityKind::SpatialSketchEntity,
                        "name generator must name a spatial sketch entity",
                    ),
                    Some(Generator::Entity(value)) => self.name(value)?,
                    None => {}
                }
            }
            TopoName::Instance { feature, of, .. } | TopoName::Modified { feature, from: of } => {
                self.require(
                    feature.as_str(),
                    EntityKind::Feature,
                    "name provenance must name a feature",
                );
                self.name(of)?;
            }
            TopoName::Blend {
                feature, between, ..
            } => {
                self.require(
                    feature.as_str(),
                    EntityKind::Feature,
                    "name provenance must name a feature",
                );
                for value in between {
                    self.name(value)?;
                }
            }
            TopoName::Bounded { faces, ends, .. } => {
                for value in faces.iter().chain(ends) {
                    self.name(value)?;
                }
            }
            TopoName::Meeting { faces, .. } => {
                for value in faces {
                    self.name(value)?;
                }
            }
            TopoName::Split { feature, from, .. } => {
                if let Some(id) = feature {
                    self.require(
                        id.as_str(),
                        EntityKind::Feature,
                        "name provenance must name a feature",
                    );
                }
                self.name(from)?;
            }
            TopoName::Merged { feature, from } => {
                if let Some(id) = feature {
                    self.require(
                        id.as_str(),
                        EntityKind::Feature,
                        "name provenance must name a feature",
                    );
                }
                for value in from {
                    self.name(value)?;
                }
            }
            TopoName::Root { container } => self.undeclared(container.as_str(), "container"),
        }
        Ok(())
    }
    fn entity(&mut self, value: &EntityRef) {
        let (id, kind) = match value {
            EntityRef::Body(id) => (id.as_str(), EntityKind::Body),
            EntityRef::Face(id) => (id.as_str(), EntityKind::Face),
            EntityRef::Edge(id) => (id.as_str(), EntityKind::Edge),
            EntityRef::Vertex(id) => (id.as_str(), EntityKind::Vertex),
            EntityRef::Surface(id) => (id.as_str(), EntityKind::Surface),
            EntityRef::Curve(id) => (id.as_str(), EntityKind::Curve),
            EntityRef::Subd(id) => (id.as_str(), EntityKind::SubdSurface),
        };
        self.require(
            id,
            kind,
            "resolved entity target must name its declared arena",
        );
    }
    fn pick(&mut self, value: &Picked) -> Result<(), CodecError> {
        let _depth = self
            .ctx
            .enter_nested("datum-axis pick reference validation")?;
        match &value.target {
            PickTarget::Entity(value) => self.name(value)?,
            PickTarget::Native { within } => {
                if let Some(value) = within {
                    self.native_owner(value);
                }
            }
            PickTarget::Feature(id)
            | PickTarget::FeatureResult(id)
            | PickTarget::TemplateInput { instance: id, .. } => self.require(
                id.as_str(),
                EntityKind::Feature,
                "selection target must name a feature",
            ),
            PickTarget::Datum { feature, part } => {
                self.require(
                    feature.as_str(),
                    EntityKind::Feature,
                    "selection target must name a feature",
                );
                if let Some(target) = self
                    .ids
                    .features(feature.as_str(), crate::index::StandardIndex)
                {
                    let operation = target.evaluation.definition().operation();
                    if !matches!(
                        operation,
                        FeatureOperation::DatumPrincipalPlane { .. }
                            | FeatureOperation::DatumPlane { .. }
                            | FeatureOperation::DatumThreePointPlane { .. }
                            | FeatureOperation::DatumOffsetPlane { .. }
                            | FeatureOperation::DatumAxis { .. }
                            | FeatureOperation::DatumPoint { .. }
                            | FeatureOperation::DatumCoordinateSystem { .. }
                            | FeatureOperation::Unresolved {
                                family: UnresolvedFamily::DatumPlane
                                    | UnresolvedFamily::DatumAxis
                                    | UnresolvedFamily::DatumPoint
                                    | UnresolvedFamily::DatumCoordinateSystem
                            }
                    ) {
                        super::error(
                            self.findings,
                            self.owner,
                            "datum-axis datum selection must target a datum feature",
                        );
                    } else if let Some(part) = part {
                        let valid = match (part, operation) {
                            (
                                DatumPart::Principal(_),
                                FeatureOperation::DatumCoordinateSystem { .. }
                                | FeatureOperation::Unresolved {
                                    family: UnresolvedFamily::DatumCoordinateSystem,
                                },
                            ) => Some(true),
                            (DatumPart::Point(index), FeatureOperation::DatumPoint { .. }) => {
                                Some(*index == 0)
                            }
                            (
                                DatumPart::Point(_),
                                FeatureOperation::Unresolved {
                                    family: UnresolvedFamily::DatumPoint,
                                },
                            ) => None,
                            _ => Some(false),
                        };
                        match valid {
                            Some(true) => {}
                            Some(false) => super::error(self.findings, self.owner, "datum-axis datum part is incompatible with its selected datum family"),
                            None => crate::validate::record_finding(
                                self.findings, Check::ReferentialIntegrity, Severity::Warning, Some(self.owner.id.as_str()),
                                format_args!("datum-axis datum point part remains unresolved because the target's point count is unknown"),
                            ),
                        }
                    }
                }
            }
            PickTarget::Sketch(id) | PickTarget::SketchFrame { sketch: id, .. } => self.require(
                id.as_str(),
                EntityKind::Sketch,
                "selection target must name a sketch",
            ),
            PickTarget::SketchRegion { sketch, region } => {
                self.require(
                    sketch.as_str(),
                    EntityKind::Sketch,
                    "selection target must name a sketch",
                );
                for boundary in region.outer.iter().chain(region.holes.iter().flatten()) {
                    self.require(
                        boundary.entity.as_str(),
                        EntityKind::SketchEntity,
                        "region boundary must name a sketch entity",
                    );
                    if self
                        .ids
                        .sketch_entities(boundary.entity.as_str(), crate::index::StandardIndex)
                        .is_some_and(|entity| &entity.sketch != sketch)
                    {
                        super::error(
                            self.findings,
                            self.owner,
                            "datum-axis region boundary must belong to its selected sketch",
                        );
                    }
                }
            }
            PickTarget::SketchEntity(id) => self.require(
                id.as_str(),
                EntityKind::SketchEntity,
                "selection target must name a sketch entity",
            ),
            PickTarget::SpatialSketchEntity(id) => self.require(
                id.as_str(),
                EntityKind::SpatialSketchEntity,
                "selection target must name a spatial sketch entity",
            ),
            PickTarget::Principal {
                container: Some(id),
                ..
            } => self.undeclared(id.as_str(), "container"),
            PickTarget::Publication(id) => self.undeclared(id.as_str(), "publication"),
            PickTarget::Principal {
                container: None, ..
            }
            | PickTarget::Component => {}
        }
        if let Some(context) = &value.context {
            self.context(context);
        }
        if let Some(values) = &value.resolved {
            for value in values {
                self.entity(value);
            }
        }
        if let Some(PickDomain::Limits { start, end }) = &value.domain {
            for value in start.iter().chain(end) {
                self.pick(value)?;
            }
        }
        Ok(())
    }
    fn selection<O: MemberOrder>(&mut self, value: &Selection<O>) -> Result<(), CodecError> {
        if let Anchor::Pinned(position) = &value.at {
            self.position(position);
        }
        match &value.picks {
            Picks::Unread => {}
            Picks::Named(values) => {
                for value in values.as_slice() {
                    self.pick(value)?;
                }
            }
            Picks::Rule {
                terms,
                excluded,
                resolved,
            } => {
                for term in terms.iter().chain(excluded) {
                    for value in term.seeds.as_slice() {
                        self.pick(value)?;
                    }
                    match &term.rule {
                        SelectionRule::Chain { end: Some(value) } => self.pick(value)?,
                        SelectionRule::Region { boundary }
                        | SelectionRule::Intersection { with: boundary } => {
                            for value in boundary.as_slice() {
                                self.pick(value)?;
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(values) = resolved {
                    for value in values.as_slice() {
                        self.pick(value)?;
                    }
                }
            }
        }
        Ok(())
    }
    fn construction<K: ConstructionKind>(
        &mut self,
        value: &Construction<K>,
    ) -> Result<(), CodecError> {
        self.inputs(&value.inputs)?;
        for id in value.resolved_in.keys() {
            self.require(
                id.as_str(),
                EntityKind::DesignConfiguration,
                "resolved configuration must name a design configuration",
            );
        }
        if let Some(mode) = &value.mode {
            self.options(&mode.native_options)?;
            self.options(&mode.source_settings)?;
        }
        Ok(())
    }
    fn inputs(&mut self, values: &[ConstructionInput]) -> Result<(), CodecError> {
        let _depth = self
            .ctx
            .enter_nested("datum-axis construction reference validation")?;
        for value in values {
            match &value.reference {
                Some(InputReference::Picks(value)) => self.selection(value)?,
                Some(InputReference::Built(value)) => match value.as_ref() {
                    AnyConstruction::Point(value) => self.construction(value)?,
                    AnyConstruction::Direction(value) => self.construction(value)?,
                    AnyConstruction::Axis(value) => self.construction(value)?,
                    AnyConstruction::Plane(value) => self.construction(value)?,
                    AnyConstruction::Frame(value) => self.construction(value)?,
                    AnyConstruction::Curve(value) => self.construction(value)?,
                },
                None => {}
            }
        }
        Ok(())
    }
    fn options(&mut self, values: &NativeOptions) -> Result<(), CodecError> {
        for value in values.values() {
            match value {
                NativeOption::Selection(value) => self.selection(value)?,
                NativeOption::OrderedSelection(value) => self.selection(value)?,
                _ => {}
            }
        }
        Ok(())
    }
}
