// SPDX-License-Identifier: Apache-2.0

use super::SourceUnitCarriers;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    DesignParameter, Feature, FeatureDefinition, FeatureEvaluation, FeatureOperation,
    FuzzyTolerance, ParameterValue,
};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, HelixCurveConstruction, HelixFrame, ProceduralCurve,
    ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::products::{Occurrence, OccurrenceParent, PrototypeReference};
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintDefinitionInput,
    SketchEntity, SketchGeometry, SketchGeometryDefinition, SketchLocus, SketchPlacement,
    SketchProfiles,
};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, CoedgeUseCurve, Edge, EdgeCarrier, Face, FaceLoops, ParameterInterval,
    Point, Sense, Vertex,
};
use cadmpeg_ir::transform::Transform;

fn zero_collection_ctx<T>(run: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    run(&ctx)
}

fn admission_pcurve() -> cadmpeg_ir::geometry::pcurve::Pcurve {
    cadmpeg_ir::geometry::pcurve::Pcurve {
        id: cadmpeg_ir::ids::PcurveId::mint("creo:test:pcurve#0").expect("identity grammar"),
        geometry: cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(0.0, 0.0),
                cadmpeg_ir::math::Point2::new(1.0, 0.0),
            )
            .expect("source pcurve"),
        ),
        metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(None, None, None),
    }
}

fn admission_plane() -> SurfaceGeometry {
    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("source plane"),
    ))
}

fn admission_edge(range: Option<[f64; 2]>) -> Edge {
    let vertex = cadmpeg_ir::ids::VertexId::mint("creo:test:vertex#0").expect("identity grammar");
    Edge {
        id: cadmpeg_ir::ids::EdgeId::mint("creo:test:edge#0").expect("identity grammar"),
        carrier: EdgeCarrier::new(
            Some(CurveId::mint("creo:test:curve#0").expect("identity grammar")),
            range,
        )
        .expect("source edge carrier"),
        start: vertex.clone(),
        end: vertex,
        tolerance: None,
    }
}

fn source_feature(definition: FeatureDefinition) -> Feature {
    Feature {
        id: cadmpeg_ir::features::FeatureId::mint("creo:test:feature#1").expect("identity grammar"),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(definition),
        native_ref: None,
    }
}

fn source_length_parameter(value: f64) -> DesignParameter {
    DesignParameter {
        id: cadmpeg_ir::features::ParameterId::mint("creo:test:parameter#1")
            .expect("identity grammar"),
        owner: None,
        ordinal: 0,
        name: "length".into(),
        expression: "length".into(),
        display: None,
        value: Some(ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(value).expect("finite source length"),
        )),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: std::collections::BTreeMap::new(),
        pmi: None,
        native_ref: None,
    }
}

fn source_sketch(origin: Point3) -> Sketch {
    Sketch {
        id: cadmpeg_ir::sketches::SketchId::mint("creo:test:sketch#1").expect("identity grammar"),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            origin,
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("source placement"),
        profiles: SketchProfiles::default(),
        native_ref: None,
    }
}

fn source_distance_constraint(value: f64) -> SketchConstraint {
    let entity = cadmpeg_ir::sketches::SketchEntityId::mint("creo:test:sketch_entity#1")
        .expect("identity grammar");
    SketchConstraint {
        id: cadmpeg_ir::sketches::SketchConstraintId::mint("creo:test:sketch_constraint#1")
            .expect("identity grammar"),
        sketch: cadmpeg_ir::sketches::SketchId::mint("creo:test:sketch#1")
            .expect("identity grammar"),
        definition: SketchConstraintDefinition::try_from(
            SketchConstraintDefinitionInput::DistanceLociValue {
                first: SketchLocus::Start(entity.clone()),
                second: SketchLocus::End(entity),
                distance: cadmpeg_ir::scalar::Length::new(value).expect("finite source distance"),
                parameter: None,
            },
        )
        .expect("valid source distance"),
        name: None,
        driving: None,
        active: None,
        virtual_space: None,
        visible: None,
        orientation: None,
        label_distance: None,
        label_position: None,
        metadata: None,
        native_ref: None,
    }
}

fn translated_product_transform(x: f64) -> Transform {
    Transform::affine([
        [1.0, 0.0, 0.0, x],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .expect("finite source translation")
}

fn source_occurrence(transform: Transform, linked_prototype: Option<Transform>) -> Occurrence {
    Occurrence {
        id: cadmpeg_ir::ids::OccurrenceId::mint("creo:test:occurrence#0")
            .expect("identity grammar"),
        prototype: PrototypeReference::Local {
            definition: cadmpeg_ir::ids::ProductDefinitionId::mint("creo:test:product#0")
                .expect("identity grammar"),
        },
        parent: OccurrenceParent::Root {},
        ordinal: 0,
        transform,
        linked_prototype,
        scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
        name: None,
        visible: None,
        link: None,
        native_ref: None,
    }
}

mod admission;
mod pcurve_work;
mod units;

mod cache;
