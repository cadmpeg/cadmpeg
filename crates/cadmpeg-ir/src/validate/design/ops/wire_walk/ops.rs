// SPDX-License-Identifier: Apache-2.0
//! [`WireWalk`] implementations for the operation family types.

use super::{WireFound, WireWalk};

use crate::design::ops::assembly::{
    AssemblyContext, ComponentPattern, MirrorComponents, Place, PlaceSet, RigidGroup,
};
use crate::design::ops::body::{
    BodyMate, Combine, DeleteBodies, DeleteFaces, Extract, Flex, Graft, InstanceChange,
    InstanceOverride, Kept, MoveFaces, Pattern, PatternInstances, PatternSeeds, ReplaceFaces,
    ScaleFactors, Split, SplitMethod, SplitMode, SplitTool, TemplateInput, TemplateInstance,
    TransformBodies, TransformStep, Transformation,
};
use crate::design::ops::dressup::{
    Along, BlendControls, BlendLimit, Chamfer, ChamferSet, ChamferSize, Corner, Decal,
    DecalPlacement, Dome, Draft, DraftAngle, DraftSet, EdgeSetback, Emboss, FaceSide,
    FaceThickness, Fillet, FilletSection, FilletSet, FilletSize, Hole, HolePositions, HoleShape,
    LimitAt, Offset, OffsetResult, OffsetSet, RoundSide, SeriesStage, Setbacks, ShellFeature,
    StageHole, StationAt, Stations, Thicken, Thread, ThreadLength, ThreadSpec, Transition,
    TransitionKind, ValueStation,
};
use crate::design::ops::shared::{
    ChainLayout, ChainLink, ChainLinks, CircularLayout, CompositeStage, InstanceOrientation, Joint,
    JointConnector, JointKind, Law, Limit, LinearLayout, LinearRun, Locus, MirrorLayout,
    PathReference, PathRollControl, PatternLayout, PatternRun, RunSides, ScaleLayout, Spacing,
    Span,
};
use crate::design::ops::sheet_metal::{
    AngleRange, BendAllowance, BendEquations, BendSet, ConnectBetween, Convert, ConvertBend,
    ConvertFrom, ConvertRip, Flange, FlangeHeight, FlangeWidth, Hem, Rip, RipPath, SheetMetal,
    SheetMetalKind, SheetMetalRule, StampForm, StampOutline, VertexBend,
};
use crate::design::ops::surface::{
    BoundaryFill, BridgeEnd, CircleMode, CombineOperand, CurveDefinition, CurveFeature,
    CurveOffset, EquationCoordinates, Extend, ExtendLimit, FaceReference, Form, Healing,
    ImplicitSection, ImplicitSweep, Knit, LineLength, LineMode, MeshFeature, MeshKind, Patch,
    PatchBounds, PatchEdge, SplineCurve, SplinePoint, TextCurve, TextPlacement, Unsew, UnsewFaces,
};
use crate::design::ops::swept::{
    Connector, Correspondence, Extrude, ExtrudeDirection, ExtrudeOrigin, Guide, HelicalSection,
    HelicalSweep, Helix, HelixShape, HelixStart, HelixTaper, Loft, PointCondition, PointSection,
    Primitive, ProfilePlacement, ProfilePosition, Revolve, Rib, RibGrowth, ScalingLaw, Seam,
    Section, SectionForm, Spine, SpineLimits, Sweep, SweepControls, SweepGuide, SweepOrientation,
    SweepSection, Twist,
};

impl WireWalk for Along {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Chain {} => {}
            Self::Edge(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for AngleRange {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            value,
            to_inclusive: _,
            to: _,
            from_inclusive: _,
            from: _,
        } = self;
        value.walk_wire(owner, out);
    }
}

impl WireWalk for AssemblyContext {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { participants } = self;
        participants.walk_wire(owner, out);
    }
}

impl WireWalk for BendAllowance {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::KFactor(_) => {}
            Self::KFactorTable(_) => {}
            Self::Table(_) => {}
            Self::Equations(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for BendEquations {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            yields: _,
            ranges,
            angle: _,
        } = self;
        ranges.walk_wire(owner, out);
    }
}

impl WireWalk for BendSet {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::All => {}
            Self::Selected(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for BlendControls {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            transitions,
            round_corners: _,
            overflow: _,
            limits,
            keep_features: _,
            keep,
            corners,
            corner_style: _,
        } = self;
        transitions.walk_wire(owner, out);
        limits.walk_wire(owner, out);
        keep.walk_wire(owner, out);
        corners.walk_wire(owner, out);
    }
}

impl WireWalk for BlendLimit {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { reversed: _, at } = self;
        at.walk_wire(owner, out);
    }
}

impl WireWalk for BodyMate {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            kind,
            connectors,
            alignment: _,
        } = self;
        kind.walk_wire(owner, out);
        connectors.walk_wire(owner, out);
    }
}

impl WireWalk for BoundaryFill {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            tools_after: _,
            tools,
            source_settings,
            regions: _,
            native_options,
            merge: _,
            cells,
            cap_planar: _,
            boolean,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        tools.walk_wire(owner, out);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        cells.walk_wire(owner, out);
        boolean.walk_wire(owner, out);
    }
}

impl WireWalk for BridgeEnd {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            tension: _,
            reversed: _,
            curve,
            continuity: _,
            at,
        } = self;
        curve.walk_wire(owner, out);
        at.walk_wire(owner, out);
    }
}

impl WireWalk for ChainLayout {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { path, links } = self;
        path.walk_wire(owner, out);
        links.walk_wire(owner, out);
    }
}

impl WireWalk for ChainLink {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { pins } = self;
        pins.walk_wire(owner, out);
    }
}

impl WireWalk for ChainLinks {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Connected(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Native { picks, kind: _ } => {
                picks.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for Chamfer {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            sets,
            native_options,
            controls,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        sets.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        controls.walk_wire(owner, out);
    }
}

impl WireWalk for ChamferSet {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            size,
            reference,
            measure: _,
            flip: _,
            edges,
        } = self;
        size.walk_wire(owner, out);
        reference.walk_wire(owner, out);
        edges.walk_wire(owner, out);
    }
}

impl WireWalk for ChamferSize {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Distance(_) => {}
            Self::Distances {
                second: _,
                first: _,
            } => {}
            Self::DistanceAngle {
                distance: _,
                angle: _,
            } => {}
            Self::WidthAngle { width: _, angle: _ } => {}
            Self::HeightAngle {
                height: _,
                angle: _,
            } => {}
            Self::Vertex(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for CircleMode {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::ThreeGuides { guides } => {
                guides.walk_wire(owner, out);
            }
            Self::TwoGuidesRadius { radius, guides } => {
                radius.walk_wire(owner, out);
                guides.walk_wire(owner, out);
            }
            Self::CenterAngles {
                reference,
                from: _,
                center,
                angles,
            } => {
                reference.walk_wire(owner, out);
                center.walk_wire(owner, out);
                angles.walk_wire(owner, out);
            }
            Self::CenterRadius { radius, center } => {
                radius.walk_wire(owner, out);
                center.walk_wire(owner, out);
            }
            Self::TangentTwoGuides {
                surface,
                passing,
                limit,
            } => {
                surface.walk_wire(owner, out);
                passing.walk_wire(owner, out);
                limit.walk_wire(owner, out);
            }
            Self::TangentGuideRadius {
                trim: _,
                surface,
                radius,
                passing,
            } => {
                surface.walk_wire(owner, out);
                radius.walk_wire(owner, out);
                passing.walk_wire(owner, out);
            }
            Self::TangentLimitRadius {
                surface,
                radius,
                limit,
                angles,
            } => {
                surface.walk_wire(owner, out);
                radius.walk_wire(owner, out);
                limit.walk_wire(owner, out);
                angles.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for CircularLayout {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            radial,
            orientation,
            axis,
            angular,
        } = self;
        radial.walk_wire(owner, out);
        orientation.walk_wire(owner, out);
        axis.walk_wire(owner, out);
        angular.walk_wire(owner, out);
    }
}

impl WireWalk for Combine {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            tools_after: _,
            tools,
            target,
            source_settings,
            op: _,
            new_component: _,
            native_options,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        tools.walk_wire(owner, out);
        target.walk_wire(owner, out);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
    }
}

impl WireWalk for CombineOperand {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { direction, curve } = self;
        direction.walk_wire(owner, out);
        curve.walk_wire(owner, out);
    }
}

impl WireWalk for ComponentPattern {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            seeds,
            native_options,
            layout,
            instances: _,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        seeds.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        layout.walk_wire(owner, out);
    }
}

impl WireWalk for CompositeStage {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Linear(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Circular(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Mirror(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Scale(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for ConnectBetween {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Apart(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::SharedEdge(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for Connector {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { points } = self;
        points.walk_wire(owner, out);
    }
}

impl WireWalk for Convert {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            thickness: _,
            rule: _,
            rips,
            keep_solid: _,
            from,
            fixed,
            body,
            bend: _,
        } = self;
        rips.walk_wire(owner, out);
        from.walk_wire(owner, out);
        fixed.walk_wire(owner, out);
        body.walk_wire(owner, out);
    }
}

impl WireWalk for ConvertBend {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Edge { radius: _, edge } => {
                edge.walk_wire(owner, out);
            }
            Self::Face { face } => {
                face.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for ConvertFrom {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Faces { side: _, bends } => {
                bends.walk_wire(owner, out);
            }
            Self::ThinSolid => {}
        }
    }
}

impl WireWalk for ConvertRip {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { path, gap: _ } = self;
        path.walk_wire(owner, out);
    }
}

impl WireWalk for Corner {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { vertex, setbacks } = self;
        vertex.walk_wire(owner, out);
        setbacks.walk_wire(owner, out);
    }
}

impl WireWalk for Correspondence {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            mode: _,
            connectors,
        } = self;
        connectors.walk_wire(owner, out);
    }
}

impl WireWalk for CurveDefinition {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Spline(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Helix(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Projection {
                source,
                onto,
                direction,
            } => {
                source.walk_wire(owner, out);
                onto.walk_wire(owner, out);
                direction.walk_wire(owner, out);
            }
            Self::Intersection { second, first } => {
                second.walk_wire(owner, out);
                first.walk_wire(owner, out);
            }
            Self::Bridge { ends } => {
                ends.walk_wire(owner, out);
            }
            Self::Text(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Constructed(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Corner { trim: _, arc } => {
                arc.walk_wire(owner, out);
            }
            Self::Polygon {
                sides: _,
                placement,
                circumradius: _,
            } => {
                placement.walk_wire(owner, out);
            }
            Self::Point(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Equation { frame, coordinates } => {
                frame.walk_wire(owner, out);
                coordinates.walk_wire(owner, out);
            }
            Self::Combined {
                second,
                nearest: _,
                first,
            } => {
                second.walk_wire(owner, out);
                first.walk_wire(owner, out);
            }
            Self::Offset {
                support,
                smoothing: _,
                offset,
                measure: _,
                curve,
                corner: _,
            } => {
                support.walk_wire(owner, out);
                offset.walk_wire(owner, out);
                curve.walk_wire(owner, out);
            }
            Self::Smoothed {
                tangency_threshold: _,
                max_deviation: _,
                curve,
                curvature_ratio: _,
                continuity: _,
            } => {
                curve.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for CurveFeature {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            native_options,
            definition,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        definition.walk_wire(owner, out);
    }
}

impl WireWalk for CurveOffset {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Law(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Through(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for Decal {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            placement,
            opacity: _,
            native_options,
            image: _,
            faces,
            chain_faces: _,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        placement.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        faces.walk_wire(owner, out);
    }
}

impl WireWalk for DecalPlacement {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            width: _,
            mirrored: _,
            height: _,
            frame,
        } = self;
        frame.walk_wire(owner, out);
    }
}

impl WireWalk for DeleteBodies {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            retain: _,
            native_options,
            bodies,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        bodies.walk_wire(owner, out);
    }
}

impl WireWalk for DeleteFaces {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            native_options,
            heal: _,
            faces,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        faces.walk_wire(owner, out);
    }
}

impl WireWalk for Dome {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            shape: _,
            points,
            native_options,
            height: _,
            faces,
            direction,
            concave: _,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        points.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        faces.walk_wire(owner, out);
        direction.walk_wire(owner, out);
    }
}

impl WireWalk for Draft {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            steps: _,
            source_settings,
            sets,
            pull,
            parting,
            neutral,
            native_options,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        sets.walk_wire(owner, out);
        pull.walk_wire(owner, out);
        parting.walk_wire(owner, out);
        neutral.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
    }
}

impl WireWalk for DraftAngle {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::One(_) => {}
            Self::Two {
                second: _,
                first: _,
            } => {}
            Self::Symmetric(_) => {}
            Self::Variable(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for DraftSet {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            fixed,
            faces,
            angle,
        } = self;
        fixed.walk_wire(owner, out);
        faces.walk_wire(owner, out);
        angle.walk_wire(owner, out);
    }
}

impl WireWalk for EdgeSetback {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { edge, distance: _ } = self;
        edge.walk_wire(owner, out);
    }
}

impl WireWalk for Emboss {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            targets,
            source_settings,
            profile,
            native_options,
            mapping: _,
            face,
            effect: _,
            direction,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        targets.walk_wire(owner, out);
        source_settings.walk_wire(owner, out);
        profile.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        face.walk_wire(owner, out);
        direction.walk_wire(owner, out);
    }
}

impl WireWalk for EquationCoordinates {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Cartesian { z, y, x } => {
                z.walk_wire(owner, out);
                y.walk_wire(owner, out);
                x.walk_wire(owner, out);
            }
            Self::Cylindrical { z, theta, r } => {
                z.walk_wire(owner, out);
                theta.walk_wire(owner, out);
                r.walk_wire(owner, out);
            }
            Self::Spherical { theta, rho, phi } => {
                theta.walk_wire(owner, out);
                rho.walk_wire(owner, out);
                phi.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for Extend {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            output,
            native_options,
            method: _,
            limit,
            edges,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        output.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        limit.walk_wire(owner, out);
        edges.walk_wire(owner, out);
    }
}

impl WireWalk for ExtendLimit {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Distance(_) => {}
            Self::UpTo(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Natural => {}
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for Extract {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            source,
            reversed: _,
            native_options,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        source.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
    }
}

impl WireWalk for Extrude {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            span,
            source_settings,
            sheet_cut: _,
            profile,
            outside: _,
            output,
            origin,
            native_options,
            loops: _,
            fit: _,
            draft: _,
            direction,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        span.walk_wire(owner, out);
        source_settings.walk_wire(owner, out);
        profile.walk_wire(owner, out);
        output.walk_wire(owner, out);
        origin.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        direction.walk_wire(owner, out);
    }
}

impl WireWalk for ExtrudeDirection {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::ProfileNormal { reversed: _ } => {}
            Self::Along {
                lengths_along_normal: _,
                direction,
            } => {
                direction.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for ExtrudeOrigin {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::ProfilePlane => {}
            Self::Offset(_) => {}
            Self::Plane(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Point(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Faces { offset: _, faces } => {
                faces.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for FaceReference {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Faces(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Adjacent { alternate: _ } => {}
        }
    }
}

impl WireWalk for FaceSide {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { hold, faces } = self;
        hold.walk_wire(owner, out);
        faces.walk_wire(owner, out);
    }
}

impl WireWalk for FaceThickness {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            thickness: _,
            faces,
        } = self;
        faces.walk_wire(owner, out);
    }
}

impl WireWalk for Fillet {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            sets,
            native_options,
            controls,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        sets.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        controls.walk_wire(owner, out);
    }
}

impl WireWalk for FilletSection {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            weight: _,
            size,
            profile: _,
        } = self;
        size.walk_wire(owner, out);
    }
}

impl WireWalk for FilletSet {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Edges {
                spine,
                section,
                hold,
                edges,
            } => {
                spine.walk_wire(owner, out);
                section.walk_wire(owner, out);
                hold.walk_wire(owner, out);
                edges.walk_wire(owner, out);
            }
            Self::Faces {
                trim: _,
                spine,
                section,
                second,
                near,
                first,
            } => {
                spine.walk_wire(owner, out);
                section.walk_wire(owner, out);
                second.walk_wire(owner, out);
                near.walk_wire(owner, out);
                first.walk_wire(owner, out);
            }
            Self::FullRound {
                second,
                first,
                center,
            } => {
                second.walk_wire(owner, out);
                first.walk_wire(owner, out);
                center.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for FilletSize {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Radius(_) => {}
            Self::Chord(_) => {}
            Self::Offsets {
                second: _,
                first: _,
            } => {}
            Self::Variable(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Held => {}
            Self::ByProfile => {}
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for Flange {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            width,
            rule: _,
            position: _,
            outline,
            height_datum: _,
            height,
            edges,
            bend: _,
            angle: _,
        } = self;
        width.walk_wire(owner, out);
        outline.walk_wire(owner, out);
        height.walk_wire(owner, out);
        edges.walk_wire(owner, out);
    }
}

impl WireWalk for FlangeHeight {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Distance(_) => {}
            Self::UpTo { target, offset: _ } => {
                target.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for FlangeWidth {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Full => {}
            Self::Centered { width: _ } => {}
            Self::FromEnds {
                second: _,
                first: _,
            } => {}
            Self::FromReference {
                width: _,
                reversed: _,
                offset: _,
                from,
            } => {
                from.walk_wire(owner, out);
            }
            Self::PerEdge(_) => {}
            Self::CenteredPerEdge(_) => {}
        }
    }
}

impl WireWalk for Flex {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            trims: _,
            source_settings,
            native_options,
            kind: _,
            frame,
            bodies,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        frame.walk_wire(owner, out);
        bodies.walk_wire(owner, out);
    }
}

impl WireWalk for Form {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            native_options,
            cage: _,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
    }
}

impl WireWalk for Graft {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            simplify: _,
            sheet,
            native_options,
            flip: _,
            body,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        sheet.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        body.walk_wire(owner, out);
    }
}

impl WireWalk for Guide {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            tangency,
            path,
            influence: _,
        } = self;
        tangency.walk_wire(owner, out);
        path.walk_wire(owner, out);
    }
}

impl WireWalk for Healing {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            tangency_objective: _,
            tangency: _,
            objective: _,
            frozen,
        } = self;
        frozen.walk_wire(owner, out);
    }
}

impl WireWalk for HelicalSection {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Profile(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Generated {
                shape: _,
                placement: _,
            } => {}
        }
    }
}

impl WireWalk for HelicalSweep {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            section,
            output,
            native_options,
            helix,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        section.walk_wire(owner, out);
        output.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        helix.walk_wire(owner, out);
    }
}

impl WireWalk for Helix {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            start,
            shape,
            placement,
            hand: _,
        } = self;
        start.walk_wire(owner, out);
        shape.walk_wire(owner, out);
        placement.walk_wire(owner, out);
    }
}

impl WireWalk for HelixShape {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Helix { taper, law: _ } => {
                taper.walk_wire(owner, out);
            }
            Self::Spiral { law: _ } => {}
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for HelixStart {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Radius {
                radius: _,
                angle: _,
            } => {}
            Self::Point(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Circle(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for HelixTaper {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Angle(_) => {}
            Self::Growth(_) => {}
            Self::Radius(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for Hem {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            side: _,
            rule: _,
            position: _,
            form: _,
            edges,
        } = self;
        edges.walk_wire(owner, out);
    }
}

impl WireWalk for Hole {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            thread,
            targets,
            start,
            standard: _,
            source_settings,
            shape,
            series,
            positions,
            native_options,
            extent,
            direction,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        thread.walk_wire(owner, out);
        targets.walk_wire(owner, out);
        start.walk_wire(owner, out);
        source_settings.walk_wire(owner, out);
        shape.walk_wire(owner, out);
        series.walk_wire(owner, out);
        positions.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        extent.walk_wire(owner, out);
        direction.walk_wire(owner, out);
    }
}

impl WireWalk for HolePositions {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Points { points, centers: _ } => {
                points.walk_wire(owner, out);
            }
            Self::Placed(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for HoleShape {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Bore(_) => {}
            Self::Revolved(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Slot {
                measure: _,
                length: _,
                direction,
                bore: _,
            } => {
                direction.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for ImplicitSection {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Line { mode, lengths } => {
                mode.walk_wire(owner, out);
                lengths.walk_wire(owner, out);
            }
            Self::Circle(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Conic {
                supports,
                parameter,
                guides,
            } => {
                supports.walk_wire(owner, out);
                parameter.walk_wire(owner, out);
                guides.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for ImplicitSweep {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            spine,
            source_settings,
            solution: _,
            section,
            output,
            native_options,
            limits,
            corner: _,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        spine.walk_wire(owner, out);
        source_settings.walk_wire(owner, out);
        section.walk_wire(owner, out);
        output.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        limits.walk_wire(owner, out);
    }
}

impl WireWalk for InstanceChange {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Skipped => {}
            Self::Varied { parameters } => {
                parameters.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for InstanceOrientation {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Fixed => {}
            Self::Follow => {}
            Self::Frenet => {}
            Self::Rolled {
                seed_axis,
                reference_axis: _,
                reference,
            } => {
                seed_axis.walk_wire(owner, out);
                reference.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for InstanceOverride {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { index: _, change } = self;
        change.walk_wire(owner, out);
    }
}

impl WireWalk for Joint {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            native_options,
            kind,
            connectors,
            alignment: _,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        kind.walk_wire(owner, out);
        connectors.walk_wire(owner, out);
    }
}

impl WireWalk for JointConnector {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Frame(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Plane(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Axis(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Point(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Entity(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for JointKind {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Motion {
                offset: _,
                motion: _,
                limits: _,
                drives: _,
                axes,
            } => {
                axes.walk_wire(owner, out);
            }
            Self::Coincident { offset: _ } => {}
            Self::Concentric => {}
            Self::Insert { offset: _ } => {}
            Self::Distance {
                limits: _,
                distance: _,
            } => {}
            Self::Angle {
                limits: _,
                angle: _,
            } => {}
            Self::Parallel => {}
            Self::Perpendicular => {}
            Self::Tangent { contact: _ } => {}
            Self::PointOnAxis { distance: _ } => {}
            Self::Symmetric { plane } => {
                plane.walk_wire(owner, out);
            }
            Self::Width { pair, mode: _ } => {
                pair.walk_wire(owner, out);
            }
            Self::Slot {
                position: _,
                lock_rotation: _,
            } => {}
            Self::Path {
                roll,
                position: _,
                pitch_yaw: _,
            } => {
                roll.walk_wire(owner, out);
            }
            Self::Coupling {
                joints,
                coupling: _,
            } => {
                joints.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl<S> WireWalk for Kept<S> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::All { form: _ } => {}
            Self::Side { side: _ } => {}
            Self::Pieces { remove, keep } => {
                remove.walk_wire(owner, out);
                keep.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for Knit {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            tolerance: _,
            source_settings,
            simplify: _,
            output,
            native_options,
            healing,
            federation,
            elements,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        output.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        healing.walk_wire(owner, out);
        federation.walk_wire(owner, out);
        elements.walk_wire(owner, out);
    }
}

impl<Q> WireWalk for Law<Q> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Constant(_) => {}
            Self::Linear { start: _, end: _ } => {}
            Self::Curve(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Expression(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl<Q> WireWalk for Limit<Q> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::AtOrigin => {}
            Self::Amount(_) => {}
            Self::ThroughAll => {}
            Self::Next => {}
            Self::Last => {}
            Self::ToGeometry {
                target,
                offset: _,
                far: _,
                extend: _,
            } => {
                target.walk_wire(owner, out);
            }
            Self::ToPlane(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::ToPoint(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for LimitAt {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Element(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Point(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for LineLength {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Law(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::UpTo(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for LineMode {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::TwoGuides { second, first } => {
                second.walk_wire(owner, out);
                first.walk_wire(owner, out);
            }
            Self::Middle { middle, guide } => {
                middle.walk_wire(owner, out);
                guide.walk_wire(owner, out);
            }
            Self::Surface {
                reference,
                guide,
                from: _,
                angle,
            } => {
                reference.walk_wire(owner, out);
                guide.walk_wire(owner, out);
                angle.walk_wire(owner, out);
            }
            Self::Curve {
                guide,
                from: _,
                curve,
                angle,
            } => {
                guide.walk_wire(owner, out);
                curve.walk_wire(owner, out);
                angle.walk_wire(owner, out);
            }
            Self::Direction {
                guide,
                from: _,
                direction,
                angle,
            } => {
                guide.walk_wire(owner, out);
                direction.walk_wire(owner, out);
                angle.walk_wire(owner, out);
            }
            Self::TangentTo { surface, guide } => {
                surface.walk_wire(owner, out);
                guide.walk_wire(owner, out);
            }
            Self::TwoTangents { second, first } => {
                second.walk_wire(owner, out);
                first.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for LinearLayout {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            seed_only: _,
            second,
            first,
        } = self;
        second.walk_wire(owner, out);
        first.walk_wire(owner, out);
    }
}

impl WireWalk for LinearRun {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { run, direction } = self;
        run.walk_wire(owner, out);
        direction.walk_wire(owner, out);
    }
}

impl WireWalk for Locus {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Centroid => {}
            Self::Point(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for Loft {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            start,
            spine,
            source_settings,
            shape: _,
            sections,
            output,
            native_options,
            guides,
            end,
            cross,
            correspondence,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        start.walk_wire(owner, out);
        spine.walk_wire(owner, out);
        source_settings.walk_wire(owner, out);
        sections.walk_wire(owner, out);
        output.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        guides.walk_wire(owner, out);
        end.walk_wire(owner, out);
        cross.walk_wire(owner, out);
        correspondence.walk_wire(owner, out);
    }
}

impl WireWalk for MeshFeature {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            source,
            native_options,
            kind,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        source.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        kind.walk_wire(owner, out);
    }
}

impl WireWalk for MeshKind {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Section { sketch: _ } => {}
            Self::PlaneCut {
                plane,
                fill: _,
                cut: _,
            } => {
                plane.walk_wire(owner, out);
            }
            Self::Reduce { target: _ } => {}
            Self::Repair => {}
            Self::Remesh {
                shape: _,
                edge_length: _,
            } => {}
            Self::Smooth { smoothness: _ } => {}
            Self::Combine { tools, op: _ } => {
                tools.walk_wire(owner, out);
            }
            Self::Remove => {}
            Self::ReverseNormals => {}
            Self::Convert { method: _ } => {}
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for MirrorComponents {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            seeds,
            results: _,
            plane,
            native_options,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        seeds.walk_wire(owner, out);
        plane.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
    }
}

impl WireWalk for MirrorLayout {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            stitch_tolerance: _,
            plane,
        } = self;
        plane.walk_wire(owner, out);
    }
}

impl WireWalk for MoveFaces {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            transformation,
            source_settings,
            native_options,
            faces,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        transformation.walk_wire(owner, out);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        faces.walk_wire(owner, out);
    }
}

impl WireWalk for Offset {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            sets,
            result,
            native_options,
            join: _,
            face_meeting: _,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        sets.walk_wire(owner, out);
        result.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
    }
}

impl WireWalk for OffsetResult {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Move {} => {}
            Self::Copy(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for OffsetSet {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { faces, distance: _ } = self;
        faces.walk_wire(owner, out);
    }
}

impl WireWalk for Patch {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            simplified: _,
            planar: _,
            passing,
            output,
            native_options,
            bounds,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        passing.walk_wire(owner, out);
        output.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        bounds.walk_wire(owner, out);
    }
}

impl WireWalk for PatchBounds {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Profile(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Loops { outer, inner } => {
                outer.walk_wire(owner, out);
                inner.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for PatchEdge {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { curve, condition } = self;
        curve.walk_wire(owner, out);
        condition.walk_wire(owner, out);
    }
}

impl WireWalk for PathReference {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::PlaneNormal => {}
            Self::Direction(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for PathRollControl {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Free => {}
            Self::UpVector {
                vector,
                flip: _,
                axis: _,
            } => {
                vector.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for Pattern {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            seeds,
            native_options,
            layout,
            instances,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        seeds.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        layout.walk_wire(owner, out);
        instances.walk_wire(owner, out);
    }
}

impl WireWalk for PatternInstances {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Implicit(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Members(_) => {}
        }
    }
}

impl WireWalk for PatternLayout {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Linear(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Circular(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Path {
                run,
                path,
                orientation,
            } => {
                run.walk_wire(owner, out);
                path.walk_wire(owner, out);
                orientation.walk_wire(owner, out);
            }
            Self::Points { reference, points } => {
                reference.walk_wire(owner, out);
                points.walk_wire(owner, out);
            }
            Self::Dimensions(_) => {}
            Self::Table(_) => {}
            Self::Mirror(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Composite(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Scale(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Follow(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Chain(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl<Q> WireWalk for PatternRun<Q> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            spacing,
            sides,
            seed: _,
            increments: _,
        } = self;
        spacing.walk_wire(owner, out);
        sides.walk_wire(owner, out);
    }
}

impl WireWalk for PatternSeeds {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Features {
                method: _,
                features,
            } => {
                features.walk_wire(owner, out);
            }
            Self::Faces(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Bodies { join: _, bodies } => {
                bodies.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for Place {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            scale,
            placement,
            packaged: _,
            occurrence: _,
            native_options,
            constraints,
            base: _,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        scale.walk_wire(owner, out);
        placement.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        constraints.walk_wire(owner, out);
    }
}

impl WireWalk for PlaceSet {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            occurrences: _,
            native_options,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
    }
}

impl WireWalk for PointCondition {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Sharp => {}
            Self::Tangent { weight: _ } => {}
            Self::TangentToPlane { weight: _, plane } => {
                plane.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for PointSection {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { point, condition } = self;
        point.walk_wire(owner, out);
        condition.walk_wire(owner, out);
    }
}

impl WireWalk for Primitive {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            shape: _,
            placement,
            output,
            native_options,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        placement.walk_wire(owner, out);
        output.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
    }
}

impl WireWalk for ProfilePlacement {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::InPlace => {}
            Self::Positioned(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for ProfilePosition {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            swap_axes: _,
            origin,
            direction,
            angle: _,
        } = self;
        origin.walk_wire(owner, out);
        direction.walk_wire(owner, out);
    }
}

impl WireWalk for ReplaceFaces {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            replacement,
            native_options,
            faces,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        replacement.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        faces.walk_wire(owner, out);
    }
}

impl WireWalk for Revolve {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            span,
            source_settings,
            profile,
            output,
            native_options,
            loops: _,
            fit: _,
            axis,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        span.walk_wire(owner, out);
        source_settings.walk_wire(owner, out);
        profile.walk_wire(owner, out);
        output.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        axis.walk_wire(owner, out);
    }
}

impl WireWalk for Rib {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            thickness_at: _,
            thickness: _,
            source_settings,
            root_fillet: _,
            profile,
            native_options,
            growth,
            flipped: _,
            extent,
            extend_profile: _,
            draft: _,
            boolean,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        profile.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        growth.walk_wire(owner, out);
        extent.walk_wire(owner, out);
        boolean.walk_wire(owner, out);
    }
}

impl WireWalk for RibGrowth {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::InPlane => {}
            Self::Normal => {}
            Self::Along(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for RigidGroup {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            native_options,
            members: _,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
    }
}

impl WireWalk for Rip {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Faces { faces } => {
                faces.walk_wire(owner, out);
            }
            Self::Along { width: _, path } => {
                path.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for RipPath {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Ruling { face, at } => {
                face.walk_wire(owner, out);
                at.walk_wire(owner, out);
            }
            Self::Line { to, from, face } => {
                to.walk_wire(owner, out);
                from.walk_wire(owner, out);
                face.walk_wire(owner, out);
            }
            Self::Edges(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Sketched {
                through: _,
                profile,
            } => {
                profile.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for RoundSide {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Automatic {} => {}
            Self::Faces(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl<Q> WireWalk for RunSides<Q> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::One => {}
            Self::Against => {}
            Self::Symmetric {
                extent: _,
                count: _,
            } => {}
            Self::Reverse(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for ScaleFactors {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Uniform(_) => {}
            Self::PerAxis { frame, factors: _ } => {
                frame.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for ScaleLayout {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            factor: _,
            count: _,
            center,
        } = self;
        center.walk_wire(owner, out);
    }
}

impl WireWalk for ScalingLaw {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Taper(_) => {}
            Self::Law(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Area => {}
            Self::Perimeter => {}
            Self::Point(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Curve(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for Seam {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Vertex(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Parameter(_) => {}
        }
    }
}

impl WireWalk for Section {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            trim_support: _,
            spine_point,
            seam,
            form,
            flip: _,
            condition,
        } = self;
        spine_point.walk_wire(owner, out);
        seam.walk_wire(owner, out);
        form.walk_wire(owner, out);
        condition.walk_wire(owner, out);
    }
}

impl WireWalk for SectionForm {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Profile(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Placed { profile, placement } => {
                profile.walk_wire(owner, out);
                placement.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for SeriesStage {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { parts, hole } = self;
        parts.walk_wire(owner, out);
        hole.walk_wire(owner, out);
    }
}

impl WireWalk for Setbacks {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Uniform(_) => {}
            Self::PerEdge(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for SheetMetal {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            native_options,
            kind,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        kind.walk_wire(owner, out);
    }
}

impl WireWalk for SheetMetalKind {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::ApplyRule { rule: _, bodies } => {
                bodies.walk_wire(owner, out);
            }
            Self::Wall {
                thickness: _,
                side: _,
                rule: _,
                profile,
            } => {
                profile.walk_wire(owner, out);
            }
            Self::ContourFlange {
                width,
                vertex_bends,
                thickness: _,
                rule: _,
                profile,
                extent,
                edges,
                bend: _,
            } => {
                width.walk_wire(owner, out);
                vertex_bends.walk_wire(owner, out);
                profile.walk_wire(owner, out);
                extent.walk_wire(owner, out);
                edges.walk_wire(owner, out);
            }
            Self::Flange(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Hem(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Bend {
                zone: _,
                radius_side: _,
                radius: _,
                line_face: _,
                line,
                flat_length: _,
                fixed_side: _,
                angle: _,
            } => {
                line.walk_wire(owner, out);
            }
            Self::Connect {
                radius: _,
                extension: _,
                double: _,
                between,
            } => {
                between.walk_wire(owner, out);
            }
            Self::Rip(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Convert(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::BreakCorner {
                size: _,
                shape: _,
                corners,
            } => {
                corners.walk_wire(owner, out);
            }
            Self::ExtendWall { to, edge } => {
                to.walk_wire(owner, out);
                edge.walk_wire(owner, out);
            }
            Self::ExtrudedHole {
                rule: _,
                root_fillet: _,
                height: _,
                entry,
                bore,
            } => {
                entry.walk_wire(owner, out);
                bore.walk_wire(owner, out);
            }
            Self::Stamp {
                rule: _,
                form,
                entry,
            } => {
                form.walk_wire(owner, out);
                entry.walk_wire(owner, out);
            }
            Self::ToolForm {
                tool: _,
                rule: _,
                placement,
                constraints,
            } => {
                placement.walk_wire(owner, out);
                constraints.walk_wire(owner, out);
            }
            Self::Unfold { fixed, bends } => {
                fixed.walk_wire(owner, out);
                bends.walk_wire(owner, out);
            }
            Self::Refold { bends } => {
                bends.walk_wire(owner, out);
            }
            Self::FlatPattern { fixed } => {
                fixed.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for SheetMetalRule {
    fn walk_wire<'a>(&'a self, _owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            thickness: _,
            source_settings,
            scope: _,
            native_ref: _,
            native_options,
            name: _,
            id,
            bend: _,
            allowance,
        } = self;
        let owner = id.as_str();
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        allowance.walk_wire(owner, out);
    }
}

impl WireWalk for ShellFeature {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            thickness: _,
            source_settings,
            removed,
            native_options,
            join: _,
            faces,
            face_meeting: _,
            bodies,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        removed.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        faces.walk_wire(owner, out);
        bodies.walk_wire(owner, out);
    }
}

impl<Q> WireWalk for Spacing<Q> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Pitch { pitch: _, count: _ } => {}
            Self::Span { span: _, count: _ } => {}
            Self::PitchSpan { span: _, pitch: _ } => {}
            Self::Fit { count: _ } => {}
            Self::UpTo {
                reference,
                offset: _,
                from,
                fill: _,
            } => {
                reference.walk_wire(owner, out);
                from.walk_wire(owner, out);
            }
            Self::List(_) => {}
            Self::Native(_) => {}
        }
    }
}

impl<Q> WireWalk for Span<Q> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Between { start, end } => {
                start.walk_wire(owner, out);
                end.walk_wire(owner, out);
            }
            Self::Symmetric {
                measure: _,
                amount: _,
            } => {}
        }
    }
}

impl WireWalk for Spine {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Path { path, controls } => {
                path.walk_wire(owner, out);
                controls.walk_wire(owner, out);
            }
            Self::Centerline {
                stations: _,
                sections: _,
                path,
            } => {
                path.walk_wire(owner, out);
            }
            Self::Computed => {}
        }
    }
}

impl WireWalk for SpineLimits {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { start, end } = self;
        start.walk_wire(owner, out);
        end.walk_wire(owner, out);
    }
}

impl WireWalk for SplineCurve {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            support,
            points,
            closed: _,
        } = self;
        support.walk_wire(owner, out);
        points.walk_wire(owner, out);
    }
}

impl WireWalk for SplinePoint {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            tension: _,
            tangent,
            point,
        } = self;
        tangent.walk_wire(owner, out);
        point.walk_wire(owner, out);
    }
}

impl WireWalk for Split {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            native_options,
            mode,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        mode.walk_wire(owner, out);
    }
}

impl WireWalk for SplitMethod {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Intersect => {}
            Self::Project(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for SplitMode {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::ByTools {
                tools,
                targets,
                method,
                kept,
            } => {
                tools.walk_wire(owner, out);
                targets.walk_wire(owner, out);
                method.walk_wire(owner, out);
                kept.walk_wire(owner, out);
            }
            Self::Mutual { kept, elements } => {
                kept.walk_wire(owner, out);
                elements.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for SplitTool {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Plane(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Geometry { tools, extend: _ } => {
                tools.walk_wire(owner, out);
            }
            Self::Silhouette(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for StageHole {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Stated {
                thread,
                standard: _,
                shape,
                extent,
            } => {
                thread.walk_wire(owner, out);
                shape.walk_wire(owner, out);
                extent.walk_wire(owner, out);
            }
            Self::PartFeature(_) => {}
        }
    }
}

impl WireWalk for StampForm {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Raised {
                wall_angle: _,
                rim_radius: _,
                outline,
                height: _,
                floor_radius: _,
            } => {
                outline.walk_wire(owner, out);
            }
            Self::Bridge {
                width: _,
                wall_angle: _,
                rim_radius: _,
                relief_length: _,
                placement,
                length: _,
                height: _,
                floor_radius: _,
            } => {
                placement.walk_wire(owner, out);
            }
            Self::Louver {
                wall_angle: _,
                rim_radius: _,
                profile,
                open_edge,
                flap_angle: _,
                depth: _,
            } => {
                profile.walk_wire(owner, out);
                open_edge.walk_wire(owner, out);
            }
            Self::Bead {
                section_radius: _,
                path,
                height: _,
                end_radius: _,
            } => {
                path.walk_wire(owner, out);
            }
            Self::StiffeningRib {
                wall_angle: _,
                rim_radius: _,
                length: _,
                bottom_radius: _,
                bend,
                at,
            } => {
                bend.walk_wire(owner, out);
                at.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for StampOutline {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Profile(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Circle {
                diameter: _,
                center,
            } => {
                center.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for StationAt {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Parameter { fraction: _, along } => {
                along.walk_wire(owner, out);
            }
            Self::Point(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl<Q> WireWalk for Stations<Q> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            points,
            law: _,
            chain_start,
        } = self;
        points.walk_wire(owner, out);
        chain_start.walk_wire(owner, out);
    }
}

impl WireWalk for Sweep {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            transition: _,
            source_settings,
            section,
            placement,
            path,
            output,
            native_options,
            limits,
            guides,
            extent: _,
            controls,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        section.walk_wire(owner, out);
        placement.walk_wire(owner, out);
        path.walk_wire(owner, out);
        output.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        limits.walk_wire(owner, out);
        guides.walk_wire(owner, out);
        controls.walk_wire(owner, out);
    }
}

impl WireWalk for SweepControls {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            twist,
            scaling,
            orientation,
        } = self;
        twist.walk_wire(owner, out);
        scaling.walk_wire(owner, out);
        orientation.walk_wire(owner, out);
    }
}

impl WireWalk for SweepGuide {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { path, extent: _ } = self;
        path.walk_wire(owner, out);
    }
}

impl WireWalk for SweepOrientation {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::FollowPath => {}
            Self::MinimumTwist => {}
            Self::Frenet => {}
            Self::Fixed => {}
            Self::Guides(_) => {}
            Self::Direction(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Surface(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Point(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Curve {
                tangent: _,
                path,
                curvilinear: _,
            } => {
                path.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for SweepSection {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Profile(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::Generated { turn: _, shape: _ } => {}
        }
    }
}

impl WireWalk for TemplateInput {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { role: _, reference } = self;
        reference.walk_wire(owner, out);
    }
}

impl WireWalk for TemplateInstance {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            template: _,
            source_settings,
            parameters: _,
            native_options,
            members: _,
            inputs,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        inputs.walk_wire(owner, out);
    }
}

impl WireWalk for TextCurve {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { text: _, placement } = self;
        placement.walk_wire(owner, out);
    }
}

impl WireWalk for TextPlacement {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Plane(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::OnCurve { path } => {
                path.walk_wire(owner, out);
            }
            Self::OnFace { path, faces } => {
                path.walk_wire(owner, out);
                faces.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for Thicken {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            thickness: _,
            source_settings,
            native_options,
            faces,
            boolean,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        faces.walk_wire(owner, out);
        boolean.walk_wire(owner, out);
    }
}

impl WireWalk for Thread {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            start,
            spec,
            source_settings,
            native_options,
            face,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        start.walk_wire(owner, out);
        spec.walk_wire(owner, out);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        face.walk_wire(owner, out);
    }
}

impl WireWalk for ThreadLength {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Full => {}
            Self::Blind(_) => {}
            Self::ToElement { offset: _, element } => {
                element.walk_wire(owner, out);
            }
            Self::Standard => {}
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for ThreadSpec {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            standard: _,
            pitch_diameter: _,
            pitch: _,
            modeled: _,
            minor_diameter: _,
            major_diameter: _,
            length,
            hand: _,
        } = self;
        length.walk_wire(owner, out);
    }
}

impl WireWalk for TransformBodies {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            transformation,
            source_settings,
            native_options,
            mode: _,
            bodies,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        transformation.walk_wire(owner, out);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        bodies.walk_wire(owner, out);
    }
}

impl WireWalk for TransformStep {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Translate {
                distance: _,
                direction,
            } => {
                direction.walk_wire(owner, out);
            }
            Self::Components { frame, delta: _ } => {
                frame.walk_wire(owner, out);
            }
            Self::PointToPoint { to, from } => {
                to.walk_wire(owner, out);
                from.walk_wire(owner, out);
            }
            Self::Rotate { axis, angle: _ } => {
                axis.walk_wire(owner, out);
            }
            Self::FrameToFrame { to, from } => {
                to.walk_wire(owner, out);
                from.walk_wire(owner, out);
            }
            Self::Scale { factors, center } => {
                factors.walk_wire(owner, out);
                center.walk_wire(owner, out);
            }
            Self::Fixed(_) => {}
            Self::Constrained { mates } => {
                mates.walk_wire(owner, out);
            }
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for Transformation {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { steps, resolved: _ } = self;
        steps.walk_wire(owner, out);
    }
}

impl WireWalk for Transition {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            native_options,
            kind,
            between: _,
            at,
        } = self;
        native_options.walk_wire(owner, out);
        kind.walk_wire(owner, out);
        at.walk_wire(owner, out);
    }
}

impl WireWalk for TransitionKind {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Continue => {}
            Self::Intersect => {}
            Self::Stop => {}
            Self::StopAt(v0) => {
                v0.walk_wire(owner, out);
            }
            Self::CornerSphere {
                radius: _,
                lengths: _,
            } => {}
            Self::RollingBall { radius: _, against } => {
                against.walk_wire(owner, out);
            }
            Self::SweptAround { radius: _, against } => {
                against.walk_wire(owner, out);
            }
            Self::Blend => {}
            Self::Native(_) => {}
        }
    }
}

impl WireWalk for Twist {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        match self {
            Self::Angle(_) => {}
            Self::Law(v0) => {
                v0.walk_wire(owner, out);
            }
        }
    }
}

impl WireWalk for Unsew {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            native_options,
            edges,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        edges.walk_wire(owner, out);
    }
}

impl WireWalk for UnsewFaces {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            source_settings,
            native_options,
            faces,
        } = self;
        out.record_native_options(owner, native_options, source_settings);
        source_settings.walk_wire(owner, out);
        native_options.walk_wire(owner, out);
        faces.walk_wire(owner, out);
    }
}

impl<Q> WireWalk for ValueStation<Q> {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self { value: _, at } = self;
        at.walk_wire(owner, out);
    }
}

impl WireWalk for VertexBend {
    fn walk_wire<'a>(&'a self, owner: &'a str, out: &mut WireFound<'a>) {
        let Self {
            vertex,
            radius_side: _,
            radius: _,
        } = self;
        vertex.walk_wire(owner, out);
    }
}
