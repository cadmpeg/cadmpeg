// SPDX-License-Identifier: Apache-2.0
//! Every surfacing, wireframe, form, and mesh type, written, read back strictly, and
//! written again; the two texts are equal. Their stated wire forms, and
//! their schemas.

use std::collections::BTreeMap;
use std::path::Path;

use cadmpeg_core::json;

use super::*;
use crate::design::ops::shared::tests::fixtures::*;
use crate::design::ops::shared::tests::{text_content_samples, tool_disposition_samples};
use crate::design::ops::shared::RadialDimension;
use crate::design::ops::swept::{HelixLaw, HelixShape, HelixStart, HelixTaper, SpiralLaw};
use crate::design::options::Hand;
use crate::design::tests::unit_variants::{assert_units_refuse_value, reads, Reader};

fn corner_transition_samples() -> Vec<CornerTransition> {
    vec![
        CornerTransition::Mitre,
        CornerTransition::Round,
        CornerTransition::Open,
        CornerTransition::Carried,
        CornerTransition::Native(text("codec:native")),
    ]
}

fn spine_limits_samples() -> Vec<SpineLimits> {
    vec![
        SpineLimits {
            start: Some(selection(1)),
            end: Some(selection(1)),
        },
        SpineLimits {
            start: None,
            end: None,
        },
    ]
}

fn helix_samples() -> Vec<Helix> {
    vec![
        Helix {
            placement: frame(),
            start: Some(helix_start_samples().remove(0)),
            shape: helix_shape_samples().remove(0),
            hand: Some(lit(Hand::Left)),
        },
        Helix {
            placement: frame(),
            start: None,
            shape: helix_shape_samples().remove(0),
            hand: None,
        },
    ]
}

fn helix_start_samples() -> Vec<HelixStart> {
    vec![
        HelixStart::Radius {
            radius: RadialDimension::Diameter(lit(positive(3.0))),
            angle: Some(bound(1)),
        },
        HelixStart::Radius {
            radius: RadialDimension::Diameter(lit(positive(3.0))),
            angle: None,
        },
        HelixStart::Point(point_construction()),
        HelixStart::Circle(selection(2)),
    ]
}

fn helix_shape_samples() -> Vec<HelixShape> {
    vec![
        HelixShape::Helix {
            law: Some(helix_law_samples().remove(0)),
            taper: Some(helix_taper_samples().remove(0)),
        },
        HelixShape::Helix {
            law: None,
            taper: None,
        },
        HelixShape::Spiral {
            law: Some(spiral_law_samples().remove(0)),
        },
        HelixShape::Spiral { law: None },
        HelixShape::Native(text("codec:native")),
    ]
}

fn helix_law_samples() -> Vec<HelixLaw> {
    vec![
        HelixLaw::PitchHeight {
            pitch: lit(positive(2.0)),
            height: lit(positive(2.0)),
        },
        HelixLaw::PitchTurns {
            pitch: lit(positive(2.0)),
            turns: lit(positive_real(1.5)),
        },
        HelixLaw::HeightTurns {
            height: lit(positive(2.0)),
            turns: lit(positive_real(1.5)),
        },
        HelixLaw::Native(text("codec:native")),
    ]
}

fn helix_taper_samples() -> Vec<HelixTaper> {
    vec![
        HelixTaper::Angle(bound(1)),
        HelixTaper::Growth(lit(length(-1.5))),
        HelixTaper::Radius(Law::Constant(lit(length(2.0)))),
    ]
}

fn spiral_law_samples() -> Vec<SpiralLaw> {
    vec![
        SpiralLaw::PitchTurns {
            pitch: lit(nonzero(-0.5)),
            turns: lit(positive_real(1.5)),
        },
        SpiralLaw::EndRadiusTurns {
            end: lit(positive(2.0)),
            turns: lit(positive_real(1.5)),
        },
        SpiralLaw::EndRadiusPitch {
            end: lit(positive(2.0)),
            pitch: lit(nonzero(-0.5)),
        },
    ]
}

fn patch_samples() -> Vec<Patch> {
    vec![
        Patch {
            bounds: patch_bounds_samples().remove(0),
            passing: Some(selection(1)),
            planar: Some(lit(true)),
            simplified: Some(lit(true)),
            output: sheet_output(),
            native_options: native_options(),
            source_settings: native_options(),
        },
        Patch {
            bounds: patch_bounds_samples().remove(0),
            passing: None,
            planar: None,
            simplified: None,
            output: sheet_output(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn patch_bounds_samples() -> Vec<PatchBounds> {
    vec![
        PatchBounds::Profile(selection(3)),
        PatchBounds::Loops {
            outer: vec![patch_edge_samples().remove(0)],
            inner: vec![vec![patch_edge_samples().remove(0)]],
        },
        PatchBounds::Loops {
            outer: Vec::new(),
            inner: Vec::new(),
        },
    ]
}

fn patch_edge_samples() -> Vec<PatchEdge> {
    vec![
        PatchEdge {
            curve: selection(4),
            condition: Some(SectionCondition::Default),
        },
        PatchEdge {
            curve: selection(4),
            condition: None,
        },
    ]
}

fn knit_samples() -> Vec<Knit> {
    vec![
        Knit {
            elements: selection(2),
            tolerance: Some(lit(nonnegative(0.5))),
            simplify: Some(lit(true)),
            healing: Some(healing_samples().remove(0)),
            federation: Some(selection(1)),
            output: sheet_output(),
            native_options: native_options(),
            source_settings: native_options(),
        },
        Knit {
            elements: selection(2),
            tolerance: None,
            simplify: None,
            healing: None,
            federation: None,
            output: sheet_output(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn healing_samples() -> Vec<Healing> {
    vec![
        Healing {
            objective: Some(lit(nonnegative(0.5))),
            tangency: Some(bound(1)),
            tangency_objective: Some(bound(1)),
            frozen: Some(selection(1)),
        },
        Healing {
            objective: None,
            tangency: None,
            tangency_objective: None,
            frozen: None,
        },
    ]
}

fn unsew_samples() -> Vec<Unsew> {
    vec![
        Unsew {
            edges: selection(1),
            native_options: native_options(),
            source_settings: native_options(),
        },
        Unsew {
            edges: selection(1),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn unsew_faces_samples() -> Vec<UnsewFaces> {
    vec![
        UnsewFaces {
            faces: selection(1),
            native_options: native_options(),
            source_settings: native_options(),
        },
        UnsewFaces {
            faces: selection(1),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn extend_samples() -> Vec<Extend> {
    vec![
        Extend {
            edges: selection(1),
            limit: Some(extend_limit_samples().remove(0)),
            method: Some(extend_method_samples().remove(0)),
            output: Some(sheet_output()),
            native_options: native_options(),
            source_settings: native_options(),
        },
        Extend {
            edges: selection(1),
            limit: None,
            method: None,
            output: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn extend_limit_samples() -> Vec<ExtendLimit> {
    vec![
        ExtendLimit::Distance(lit(length(-1.5))),
        ExtendLimit::UpTo(selection(1)),
        ExtendLimit::Natural,
        ExtendLimit::Native(text("codec:native")),
    ]
}

fn extend_method_samples() -> Vec<ExtendMethod> {
    vec![
        ExtendMethod::Natural,
        ExtendMethod::Tangent,
        ExtendMethod::Curvature,
        ExtendMethod::Perpendicular,
        ExtendMethod::Native(text("codec:native")),
    ]
}

fn implicit_sweep_samples() -> Vec<ImplicitSweep> {
    vec![
        ImplicitSweep {
            section: implicit_section_samples().remove(0),
            spine: Some(selection(4)),
            limits: spine_limits_samples().remove(0),
            solution: Some(2),
            corner: Some(corner_transition_samples().remove(0)),
            output: solid_output(),
            native_options: native_options(),
            source_settings: native_options(),
        },
        ImplicitSweep {
            section: implicit_section_samples().remove(0),
            spine: None,
            limits: spine_limits_samples().remove(0),
            solution: None,
            corner: None,
            output: solid_output(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn implicit_section_samples() -> Vec<ImplicitSection> {
    vec![
        ImplicitSection::Line {
            mode: line_mode_samples().remove(0),
            lengths: [
                Some(line_length_samples().remove(0)),
                Some(line_length_samples().remove(0)),
            ],
        },
        ImplicitSection::Line {
            mode: line_mode_samples().remove(0),
            lengths: [None, None],
        },
        ImplicitSection::Circle(circle_mode_samples().remove(0)),
        ImplicitSection::Conic {
            guides: vec![selection(4)],
            supports: [Some(selection(1)), Some(selection(1))],
            parameter: Some(Law::Constant(lit(fraction(0.5)))),
        },
        ImplicitSection::Conic {
            guides: Vec::new(),
            supports: [None, None],
            parameter: None,
        },
        ImplicitSection::Native(text("codec:native")),
    ]
}

fn line_mode_samples() -> Vec<LineMode> {
    vec![
        LineMode::TwoGuides {
            first: selection(4),
            second: selection(4),
        },
        LineMode::Middle {
            guide: selection(4),
            middle: selection(4),
        },
        LineMode::Surface {
            guide: selection(4),
            reference: face_reference_samples().remove(0),
            from: Some(surface_relation_samples().remove(0)),
            angle: Some(Law::Linear {
                start: bound(2),
                end: lit(angle(0.5)),
            }),
        },
        LineMode::Surface {
            guide: selection(4),
            reference: face_reference_samples().remove(0),
            from: None,
            angle: None,
        },
        LineMode::Curve {
            guide: selection(4),
            curve: selection(4),
            from: Some(curve_relation_samples().remove(0)),
            angle: Some(Law::Linear {
                start: bound(2),
                end: lit(angle(0.5)),
            }),
        },
        LineMode::Curve {
            guide: selection(4),
            curve: selection(4),
            from: None,
            angle: None,
        },
        LineMode::Direction {
            guide: selection(4),
            direction: direction(),
            from: Some(direction_relation_samples().remove(0)),
            angle: Some(Law::Linear {
                start: bound(2),
                end: lit(angle(0.5)),
            }),
        },
        LineMode::Direction {
            guide: selection(4),
            direction: direction(),
            from: None,
            angle: None,
        },
        LineMode::TangentTo {
            guide: selection(4),
            surface: selection(1),
        },
        LineMode::TwoTangents {
            first: selection(1),
            second: selection(1),
        },
        LineMode::Native(text("codec:native")),
    ]
}

fn face_reference_samples() -> Vec<FaceReference> {
    vec![
        FaceReference::Faces(selection(1)),
        FaceReference::Adjacent {
            alternate: Some(lit(true)),
        },
        FaceReference::Adjacent { alternate: None },
    ]
}

fn surface_relation_samples() -> Vec<SurfaceRelation> {
    vec![SurfaceRelation::Tangent, SurfaceRelation::Normal]
}

fn direction_relation_samples() -> Vec<DirectionRelation> {
    vec![
        DirectionRelation::Parallel,
        DirectionRelation::Perpendicular,
    ]
}

fn curve_relation_samples() -> Vec<CurveRelation> {
    vec![CurveRelation::TowardReference]
}

fn line_length_samples() -> Vec<LineLength> {
    vec![
        LineLength::Law(Law::Constant(lit(length(2.0)))),
        LineLength::UpTo(selection(1)),
        LineLength::Native(text("codec:native")),
    ]
}

fn circle_mode_samples() -> Vec<CircleMode> {
    vec![
        CircleMode::ThreeGuides {
            guides: [selection(4), selection(4), selection(4)],
        },
        CircleMode::TwoGuidesRadius {
            guides: [selection(4), selection(4)],
            radius: Law::Constant(lit(length(2.0))),
        },
        CircleMode::CenterAngles {
            center: selection(4),
            reference: selection(4),
            from: Some(curve_relation_samples().remove(0)),
            angles: [
                Some(Law::Linear {
                    start: bound(2),
                    end: lit(angle(0.5)),
                }),
                Some(Law::Linear {
                    start: bound(2),
                    end: lit(angle(0.5)),
                }),
            ],
        },
        CircleMode::CenterAngles {
            center: selection(4),
            reference: selection(4),
            from: None,
            angles: [None, None],
        },
        CircleMode::CenterRadius {
            center: selection(4),
            radius: Law::Constant(lit(length(2.0))),
        },
        CircleMode::TangentTwoGuides {
            passing: selection(4),
            limit: selection(4),
            surface: selection(1),
        },
        CircleMode::TangentGuideRadius {
            passing: selection(4),
            surface: selection(1),
            radius: Law::Constant(lit(length(2.0))),
            trim: Some(lit(true)),
        },
        CircleMode::TangentGuideRadius {
            passing: selection(4),
            surface: selection(1),
            radius: Law::Constant(lit(length(2.0))),
            trim: None,
        },
        CircleMode::TangentLimitRadius {
            limit: selection(4),
            surface: selection(1),
            radius: Law::Constant(lit(length(2.0))),
            angles: [
                Some(Law::Linear {
                    start: bound(2),
                    end: lit(angle(0.5)),
                }),
                Some(Law::Linear {
                    start: bound(2),
                    end: lit(angle(0.5)),
                }),
            ],
        },
        CircleMode::TangentLimitRadius {
            limit: selection(4),
            surface: selection(1),
            radius: Law::Constant(lit(length(2.0))),
            angles: [None, None],
        },
        CircleMode::Native(text("codec:native")),
    ]
}

fn boundary_fill_samples() -> Vec<BoundaryFill> {
    vec![
        BoundaryFill {
            tools: selection(1),
            cells: vec![selection(1)],
            regions: Some(region_kind_samples().remove(0)),
            cap_planar: Some(lit(true)),
            merge: Some(lit(true)),
            tools_after: Some(tool_disposition_samples().remove(0)),
            boolean: Some(Boolean::NewBody),
            native_options: native_options(),
            source_settings: native_options(),
        },
        BoundaryFill {
            tools: selection(1),
            cells: Vec::new(),
            regions: None,
            cap_planar: None,
            merge: None,
            tools_after: None,
            boolean: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn region_kind_samples() -> Vec<RegionKind> {
    vec![
        RegionKind::Intersecting,
        RegionKind::Internal,
        RegionKind::Both,
    ]
}

fn curve_feature_samples() -> Vec<CurveFeature> {
    vec![
        CurveFeature {
            definition: curve_definition_samples().remove(0),
            native_options: native_options(),
            source_settings: native_options(),
        },
        CurveFeature {
            definition: curve_definition_samples().remove(0),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn curve_definition_samples() -> Vec<CurveDefinition> {
    vec![
        CurveDefinition::Spline(spline_curve_samples().remove(0)),
        CurveDefinition::Helix(helix_samples().remove(0)),
        CurveDefinition::Projection {
            source: selection(1),
            onto: selection(1),
            direction: Some(ProjectionDirection::SurfaceNormal),
        },
        CurveDefinition::Projection {
            source: selection(1),
            onto: selection(1),
            direction: None,
        },
        CurveDefinition::Intersection {
            first: selection(1),
            second: selection(1),
        },
        CurveDefinition::Bridge {
            ends: [
                bridge_end_samples().remove(0),
                bridge_end_samples().remove(0),
            ],
        },
        CurveDefinition::Text(text_curve_samples().remove(0)),
        CurveDefinition::Constructed(curve()),
        CurveDefinition::Corner {
            arc: curve(),
            trim: Some(lit(true)),
        },
        CurveDefinition::Corner {
            arc: curve(),
            trim: None,
        },
        CurveDefinition::Polygon {
            placement: frame(),
            sides: lit(6),
            circumradius: lit(positive(2.0)),
        },
        CurveDefinition::Point(point_construction()),
        CurveDefinition::Equation {
            frame: frame(),
            coordinates: equation_coordinates_samples().remove(0),
        },
        CurveDefinition::Combined {
            first: combine_operand_samples().remove(0),
            second: combine_operand_samples().remove(0),
            nearest: Some(lit(true)),
        },
        CurveDefinition::Combined {
            first: combine_operand_samples().remove(0),
            second: combine_operand_samples().remove(0),
            nearest: None,
        },
        CurveDefinition::Offset {
            curve: selection(2),
            support: selection(1),
            offset: curve_offset_samples().remove(0),
            measure: Some(offset_measure_samples().remove(0)),
            corner: Some(corner_transition_samples().remove(0)),
            smoothing: Some(Continuity::G1),
        },
        CurveDefinition::Offset {
            curve: selection(2),
            support: selection(1),
            offset: curve_offset_samples().remove(0),
            measure: None,
            corner: None,
            smoothing: None,
        },
        CurveDefinition::Smoothed {
            curve: selection(2),
            max_deviation: Some(lit(nonnegative(0.5))),
            tangency_threshold: Some(bound(1)),
            curvature_ratio: Some(lit(fraction(0.25))),
            continuity: Some(Continuity::G1),
        },
        CurveDefinition::Smoothed {
            curve: selection(2),
            max_deviation: None,
            tangency_threshold: None,
            curvature_ratio: None,
            continuity: None,
        },
        CurveDefinition::Native(text("codec:native")),
    ]
}

fn equation_coordinates_samples() -> Vec<EquationCoordinates> {
    vec![
        EquationCoordinates::Cartesian {
            x: Law::Constant(lit(length(2.0))),
            y: Law::Constant(lit(length(2.0))),
            z: Law::Constant(lit(length(2.0))),
        },
        EquationCoordinates::Cylindrical {
            r: Law::Constant(lit(length(2.0))),
            theta: Law::Linear {
                start: bound(2),
                end: lit(angle(0.5)),
            },
            z: Law::Constant(lit(length(2.0))),
        },
        EquationCoordinates::Spherical {
            rho: Law::Constant(lit(length(2.0))),
            theta: Law::Linear {
                start: bound(2),
                end: lit(angle(0.5)),
            },
            phi: Law::Linear {
                start: bound(2),
                end: lit(angle(0.5)),
            },
        },
        EquationCoordinates::Native(text("codec:native")),
    ]
}

fn spline_curve_samples() -> Vec<SplineCurve> {
    vec![
        SplineCurve {
            points: vec![spline_point_samples().remove(0)],
            closed: Some(lit(true)),
            support: Some(selection(1)),
        },
        SplineCurve {
            points: Vec::new(),
            closed: None,
            support: None,
        },
    ]
}

fn spline_point_samples() -> Vec<SplinePoint> {
    vec![
        SplinePoint {
            point: point_construction(),
            tangent: Some(direction()),
            tension: Some(lit(positive_real(1.5))),
        },
        SplinePoint {
            point: point_construction(),
            tangent: None,
            tension: None,
        },
    ]
}

fn bridge_end_samples() -> Vec<BridgeEnd> {
    vec![
        BridgeEnd {
            curve: selection(2),
            at: Some(point_construction()),
            continuity: Some(Continuity::G1),
            tension: Some(lit(positive_real(1.5))),
            reversed: Some(lit(true)),
        },
        BridgeEnd {
            curve: selection(2),
            at: None,
            continuity: None,
            tension: None,
            reversed: None,
        },
    ]
}

fn text_curve_samples() -> Vec<TextCurve> {
    vec![TextCurve {
        text: text_content_samples().remove(0),
        placement: text_placement_samples().remove(0),
    }]
}

fn text_placement_samples() -> Vec<TextPlacement> {
    vec![
        TextPlacement::Plane(frame()),
        TextPlacement::OnCurve { path: selection(4) },
        TextPlacement::OnFace {
            faces: selection(1),
            path: selection(4),
        },
    ]
}

fn combine_operand_samples() -> Vec<CombineOperand> {
    vec![
        CombineOperand {
            curve: selection(2),
            direction: Some(direction()),
        },
        CombineOperand {
            curve: selection(2),
            direction: None,
        },
    ]
}

fn curve_offset_samples() -> Vec<CurveOffset> {
    vec![
        CurveOffset::Law(Law::Constant(lit(length(2.0)))),
        CurveOffset::Through(point_construction()),
    ]
}

fn offset_measure_samples() -> Vec<OffsetMeasure> {
    vec![OffsetMeasure::Euclidean, OffsetMeasure::Geodesic]
}

fn form_samples() -> Vec<Form> {
    vec![
        Form {
            cage: Topology::default(),
            native_options: native_options(),
            source_settings: native_options(),
        },
        Form {
            cage: Topology::default(),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn mesh_feature_samples() -> Vec<MeshFeature> {
    vec![
        MeshFeature {
            source: selection(1),
            kind: mesh_kind_samples().remove(0),
            native_options: native_options(),
            source_settings: native_options(),
        },
        MeshFeature {
            source: selection(1),
            kind: mesh_kind_samples().remove(0),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn mesh_kind_samples() -> Vec<MeshKind> {
    vec![
        MeshKind::Section {
            sketch: id("sketch", 1),
        },
        MeshKind::PlaneCut {
            plane: Some(plane()),
            cut: Some(mesh_cut_samples().remove(0)),
            fill: Some(mesh_fill_samples().remove(0)),
        },
        MeshKind::PlaneCut {
            plane: None,
            cut: None,
            fill: None,
        },
        MeshKind::Reduce {
            target: Some(reduce_target_samples().remove(0)),
        },
        MeshKind::Reduce { target: None },
        MeshKind::Repair,
        MeshKind::Remesh {
            edge_length: Some(lit(positive(2.0))),
            shape: Some(lit(fraction(0.25))),
        },
        MeshKind::Remesh {
            edge_length: None,
            shape: None,
        },
        MeshKind::Smooth {
            smoothness: Some(lit(fraction(0.25))),
        },
        MeshKind::Smooth { smoothness: None },
        MeshKind::Combine {
            tools: selection(2),
            op: Some(BooleanOp::Cut),
        },
        MeshKind::Combine {
            tools: selection(2),
            op: None,
        },
        MeshKind::Remove,
        MeshKind::ReverseNormals,
        MeshKind::Convert {
            method: Some(mesh_conversion_samples().remove(0)),
        },
        MeshKind::Convert { method: None },
        MeshKind::Native(text("codec:native")),
    ]
}

fn mesh_cut_samples() -> Vec<MeshCut> {
    vec![
        MeshCut::Trim { flip: Some(true) },
        MeshCut::Trim { flip: None },
        MeshCut::SplitBodies,
    ]
}

fn mesh_fill_samples() -> Vec<MeshFill> {
    vec![MeshFill::Open, MeshFill::Minimal]
}

fn reduce_target_samples() -> Vec<ReduceTarget> {
    vec![
        ReduceTarget::Deviation(lit(nonnegative(0.5))),
        ReduceTarget::Proportion(lit(fraction(0.25))),
        ReduceTarget::FaceCount(lit(6)),
    ]
}

fn mesh_conversion_samples() -> Vec<MeshConversion> {
    vec![
        MeshConversion::Faceted,
        MeshConversion::Native(text("codec:native")),
    ]
}

#[test]
fn every_surface_type_round_trips() {
    let mut report = Report::default();
    report.check("Patch", patch_samples());
    report.check("PatchBounds", patch_bounds_samples());
    report.check("PatchEdge", patch_edge_samples());
    report.check("Knit", knit_samples());
    report.check("Healing", healing_samples());
    report.check("Unsew", unsew_samples());
    report.check("UnsewFaces", unsew_faces_samples());
    report.check("Extend", extend_samples());
    report.check("ExtendLimit", extend_limit_samples());
    report.check("ExtendMethod", extend_method_samples());
    report.check("ImplicitSweep", implicit_sweep_samples());
    report.check("ImplicitSection", implicit_section_samples());
    report.check("LineMode", line_mode_samples());
    report.check("FaceReference", face_reference_samples());
    report.check("SurfaceRelation", surface_relation_samples());
    report.check("DirectionRelation", direction_relation_samples());
    report.check("CurveRelation", curve_relation_samples());
    report.check("LineLength", line_length_samples());
    report.check("CircleMode", circle_mode_samples());
    report.check("BoundaryFill", boundary_fill_samples());
    report.check("RegionKind", region_kind_samples());
    report.check("CurveFeature", curve_feature_samples());
    report.check("CurveDefinition", curve_definition_samples());
    report.check("EquationCoordinates", equation_coordinates_samples());
    report.check("SplineCurve", spline_curve_samples());
    report.check("SplinePoint", spline_point_samples());
    report.check("BridgeEnd", bridge_end_samples());
    report.check("TextCurve", text_curve_samples());
    report.check("TextPlacement", text_placement_samples());
    report.check("CombineOperand", combine_operand_samples());
    report.check("CurveOffset", curve_offset_samples());
    report.check("OffsetMeasure", offset_measure_samples());
    report.check("Form", form_samples());
    report.check("MeshFeature", mesh_feature_samples());
    report.check("MeshKind", mesh_kind_samples());
    report.check("MeshCut", mesh_cut_samples());
    report.check("MeshFill", mesh_fill_samples());
    report.check("ReduceTarget", reduce_target_samples());
    report.check("MeshConversion", mesh_conversion_samples());
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
}

/// An option field is omitted for `None`, a list or map field is always
/// written, and `null` refuses.
#[test]
fn an_option_field_is_omitted_and_a_list_field_is_always_written() {
    let patch = Patch {
        bounds: PatchBounds::Loops {
            outer: Vec::new(),
            inner: Vec::new(),
        },
        passing: None,
        planar: None,
        simplified: None,
        output: SheetOutput {
            solid: None,
            boolean: None,
        },
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    };
    assert_eq!(
        to_value(&patch),
        json!({
            "bounds": {"kind": "loops", "outer": [], "inner": []},
            "output": {},
            "native_options": {},
            "source_settings": {}
        })
    );
    let mut stated = to_value(&patch);
    stated["planar"] = serde_json::Value::Null;
    refuses::<Patch>(&stated.to_string());
    let mut unknown = to_value(&patch);
    unknown["flat"] = json!(true);
    refuses::<Patch>(&unknown.to_string());
}

/// Each enum writes its stated tag form, and an arm with no payload in an
/// internally tagged enum refuses another key.
#[test]
fn each_surface_enum_has_its_tag_form() {
    assert_eq!(to_value(&RegionKind::Both), json!("both"));
    assert_eq!(to_value(&ExtendLimit::Natural), json!({"kind": "natural"}));
    assert_eq!(
        to_value(&FaceReference::Adjacent { alternate: None }),
        json!({"kind": "adjacent"})
    );
    assert_eq!(
        to_value(&MeshCut::SplitBodies),
        json!({"kind": "split_bodies"})
    );
    assert_eq!(to_value(&MeshKind::Repair), json!({"kind": "repair"}));
    assert_eq!(
        to_value(&ReduceTarget::FaceCount(lit(500))),
        json!({"kind": "face_count", "value": {"kind": "literal", "value": 500}})
    );
    refuses::<FaceReference>(r#"{"kind": "adjacent", "alternate": null}"#);
    refuses::<MeshCut>(r#"{"kind": "split_bodies", "extra": 1}"#);
    refuses::<MeshKind>(r#"{"kind": "remesh", "value": {"shape": null}}"#);
    refuses::<MeshCut>(r#"{"kind": "split_bodies", "value": null}"#);
}

/// §2.5: each value has one spelling, so a unit variant of an adjacently
/// tagged enum is its `kind` alone and refuses a stated `value`.
#[test]
fn every_adjacent_unit_variant_refuses_a_stated_value() {
    let readers: [(&'static str, Reader); 4] = [
        ("ExtendLimit", reads::<ExtendLimit>),
        ("ExtendMethod", reads::<ExtendMethod>),
        ("MeshConversion", reads::<MeshConversion>),
        ("MeshKind", reads::<MeshKind>),
    ];
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/design/ops/surface.rs");
    assert_units_refuse_value(&[file], &BTreeMap::from(readers));
}

/// A line section's two extents are a fixed pair whose members may be
/// `null`, and three refuse.
#[test]
fn line_lengths_are_a_fixed_pair() {
    let section = ImplicitSection::Line {
        mode: LineMode::TwoGuides {
            first: selection(1),
            second: selection(2),
        },
        lengths: [None, Some(LineLength::Law(Law::Constant(lit(length(5.0)))))],
    };
    let written = to_value(&section);
    assert!(written["value"]["lengths"][0].is_null());
    let mut three = written.clone();
    three["value"]["lengths"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::Value::Null);
    refuses::<ImplicitSection>(&three.to_string());
}

/// A mesh operation whose operand records are not read keeps an `Unread`
/// source.
#[test]
fn a_mesh_operation_takes_an_unread_source() {
    let feature = MeshFeature {
        source: unread(),
        kind: MeshKind::Convert {
            method: Some(MeshConversion::Faceted),
        },
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
    };
    round_trip(&feature).unwrap();
}

/// §5.10: each surfacing, wireframe, form, and mesh operation is the
/// `FeatureOperation` arm its family names, holding the operation as its
/// value.
#[test]
fn every_surface_operation_is_a_feature_operation_arm() {
    use crate::design::feature::{FeatureOperation as Op, OperationFamily as Family};
    use crate::design::tests::arm_failures;
    let failures = [
        arm_failures(Family::Patch, patch_samples(), Op::Patch),
        arm_failures(Family::Knit, knit_samples(), Op::Knit),
        arm_failures(Family::Unsew, unsew_samples(), Op::Unsew),
        arm_failures(Family::UnsewFaces, unsew_faces_samples(), Op::UnsewFaces),
        arm_failures(Family::Extend, extend_samples(), Op::Extend),
        arm_failures(
            Family::ImplicitSweep,
            implicit_sweep_samples(),
            Op::ImplicitSweep,
        ),
        arm_failures(
            Family::BoundaryFill,
            boundary_fill_samples(),
            Op::BoundaryFill,
        ),
        arm_failures(Family::Curve, curve_feature_samples(), Op::Curve),
        arm_failures(Family::Form, form_samples(), Op::Form),
        arm_failures(Family::Mesh, mesh_feature_samples(), Op::Mesh),
    ]
    .concat();
    assert!(failures.is_empty(), "{failures:#?}");
}
