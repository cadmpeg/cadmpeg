// SPDX-License-Identifier: Apache-2.0
//! Every sheet-metal type, written, read back strictly, and
//! written again; the two texts are equal. Their stated wire forms, and
//! their schemas.

use std::collections::BTreeMap;
use std::path::Path;

use cadmpeg_core::json;

use super::*;
use crate::design::ops::shared::tests::fixtures::*;
use crate::design::ops::shared::tests::{joint_samples, pair_element_samples};
use crate::design::options::SymmetricMeasure;
use crate::design::tests::unit_variants::{assert_units_refuse_value, reads, Reader};

fn sheet_metal_samples() -> Vec<SheetMetal> {
    vec![
        SheetMetal {
            kind: sheet_metal_kind_samples().remove(0),
            native_options: native_options(),
            source_settings: native_options(),
        },
        SheetMetal {
            kind: sheet_metal_kind_samples().remove(0),
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        },
    ]
}

fn sheet_metal_rule_samples() -> Vec<SheetMetalRule> {
    vec![
        SheetMetalRule {
            id: id("sheet-metal-rule", 1),
            scope: id("product-definition", 1),
            name: Some(text("codec:native")),
            thickness: Some(lit(positive(2.0))),
            bend: bend_spec_samples().remove(0),
            allowance: Some(bend_allowance_samples().remove(0)),
            native_options: native_options(),
            source_settings: native_options(),
            native_ref: Some("native-ref".to_owned()),
        },
        SheetMetalRule {
            id: id("sheet-metal-rule", 1),
            scope: id("product-definition", 1),
            name: None,
            thickness: None,
            bend: bend_spec_samples().remove(0),
            allowance: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
            native_ref: None,
        },
    ]
}

fn bend_allowance_samples() -> Vec<BendAllowance> {
    vec![
        BendAllowance::KFactor(lit(real(0.44))),
        BendAllowance::KFactorTable(k_factor_table_samples().remove(0)),
        BendAllowance::Table(bend_table_samples().remove(0)),
        BendAllowance::Equations(bend_equations_samples().remove(0)),
    ]
}

fn k_factor_table_samples() -> Vec<KFactorTable> {
    vec![
        KFactorTable {
            rows: vec![k_factor_row_samples().remove(0)],
            lookup: Some(table_lookup_samples().remove(0)),
        },
        KFactorTable {
            rows: Vec::new(),
            lookup: None,
        },
    ]
}

fn bend_table_samples() -> Vec<BendTable> {
    vec![
        BendTable {
            blocks: vec![bend_table_block_samples().remove(0)],
            angle: Some(bend_table_angle_samples().remove(0)),
            value: bend_table_value_samples().remove(0),
            lookup: bend_table_lookup_samples().remove(0),
        },
        BendTable {
            blocks: Vec::new(),
            angle: None,
            value: bend_table_value_samples().remove(0),
            lookup: bend_table_lookup_samples().remove(0),
        },
    ]
}

fn bend_table_block_samples() -> Vec<BendTableBlock> {
    vec![
        BendTableBlock {
            thickness: positive(1.0),
            radii: vec![positive(1.0)],
            angles: vec![angle(0.5)],
            values: vec![vec![real(0.4)]],
        },
        BendTableBlock {
            thickness: positive(1.0),
            radii: Vec::new(),
            angles: Vec::new(),
            values: Vec::new(),
        },
    ]
}

fn bend_table_angle_samples() -> Vec<BendTableAngle> {
    vec![BendTableAngle::Deflection, BendTableAngle::Opening]
}

fn bend_table_value_samples() -> Vec<BendTableValue> {
    vec![
        BendTableValue::Deduction {
            flanges: Some(flange_measure_samples().remove(0)),
        },
        BendTableValue::Deduction { flanges: None },
        BendTableValue::Allowance,
        BendTableValue::KFactor,
    ]
}

fn flange_measure_samples() -> Vec<FlangeMeasure> {
    vec![
        FlangeMeasure::OuterSharp,
        FlangeMeasure::OuterSharpThenTangent,
    ]
}

fn bend_table_lookup_samples() -> Vec<BendTableLookup> {
    vec![
        BendTableLookup {
            thickness: Some(axis_lookup_samples().remove(0)),
            radius: Some(axis_lookup_samples().remove(0)),
            angle: Some(axis_lookup_samples().remove(0)),
            outside: Some(outside_grid_samples().remove(0)),
        },
        BendTableLookup {
            thickness: None,
            radius: None,
            angle: None,
            outside: None,
        },
    ]
}

fn axis_lookup_samples() -> Vec<AxisLookup> {
    vec![AxisLookup::Exact, AxisLookup::Linear]
}

fn outside_grid_samples() -> Vec<OutsideGrid> {
    vec![
        OutsideGrid::BackupKFactor {
            k_factor: Some(lit(real(0.44))),
        },
        OutsideGrid::BackupKFactor { k_factor: None },
    ]
}

fn bend_equations_samples() -> Vec<BendEquations> {
    vec![
        BendEquations {
            yields: bend_quantity_samples().remove(0),
            angle: bend_table_angle_samples().remove(0),
            ranges: vec![angle_range_samples().remove(0)],
        },
        BendEquations {
            yields: bend_quantity_samples().remove(0),
            angle: bend_table_angle_samples().remove(0),
            ranges: Vec::new(),
        },
    ]
}

fn bend_quantity_samples() -> Vec<BendQuantity> {
    vec![
        BendQuantity::Compensation {
            flanges: Some(flange_measure_samples().remove(0)),
        },
        BendQuantity::Compensation { flanges: None },
        BendQuantity::Deduction {
            flanges: Some(flange_measure_samples().remove(0)),
        },
        BendQuantity::Deduction { flanges: None },
        BendQuantity::Allowance,
        BendQuantity::KFactor,
        BendQuantity::Native(text("codec:native")),
    ]
}

fn angle_range_samples() -> Vec<AngleRange> {
    vec![AngleRange {
        from: angle(0.5),
        from_inclusive: true,
        to: angle(0.5),
        to_inclusive: true,
        value: expression(),
    }]
}

fn table_lookup_samples() -> Vec<TableLookup> {
    vec![TableLookup::NextAtOrAbove, TableLookup::Linear]
}

fn k_factor_row_samples() -> Vec<KFactorRow> {
    vec![KFactorRow {
        ratio: nonnegative_real(0.5),
        k_factor: lit(real(0.44)),
    }]
}

fn sheet_metal_kind_samples() -> Vec<SheetMetalKind> {
    vec![
        SheetMetalKind::ApplyRule {
            rule: id("sheet-metal-rule", 1),
            bodies: Some(selection(1)),
        },
        SheetMetalKind::ApplyRule {
            rule: id("sheet-metal-rule", 1),
            bodies: None,
        },
        SheetMetalKind::Wall {
            profile: selection(3),
            thickness: Some(lit(positive(2.0))),
            side: Some(normal_side_samples().remove(0)),
            rule: Some(id("sheet-metal-rule", 1)),
        },
        SheetMetalKind::Wall {
            profile: selection(3),
            thickness: None,
            side: None,
            rule: None,
        },
        SheetMetalKind::ContourFlange {
            profile: selection(3),
            edges: Some(selection(2)),
            width: Some(flange_width_samples().remove(0)),
            extent: Some(Span::Symmetric {
                amount: lit(length(4.0)),
                measure: Some(SymmetricMeasure::Total),
            }),
            thickness: Some(Thickness::Symmetric(lit(positive(1.0)))),
            bend: bend_spec_samples().remove(0),
            vertex_bends: vec![vertex_bend_samples().remove(0)],
            rule: Some(id("sheet-metal-rule", 1)),
        },
        SheetMetalKind::ContourFlange {
            profile: selection(3),
            edges: None,
            width: None,
            extent: None,
            thickness: None,
            bend: bend_spec_samples().remove(0),
            vertex_bends: Vec::new(),
            rule: None,
        },
        SheetMetalKind::Flange(flange_samples().remove(0)),
        SheetMetalKind::Hem(hem_samples().remove(0)),
        SheetMetalKind::Bend {
            line: selection(1),
            angle: Some(bound(1)),
            radius: Some(lit(positive(2.0))),
            radius_side: Some(radius_side_samples().remove(0)),
            flat_length: Some(lit(positive(2.0))),
            zone: Some(bend_zone_samples().remove(0)),
            line_face: Some(bend_line_face_samples().remove(0)),
            fixed_side: Some(line_side_samples().remove(0)),
        },
        SheetMetalKind::Bend {
            line: selection(1),
            angle: None,
            radius: None,
            radius_side: None,
            flat_length: None,
            zone: None,
            line_face: None,
            fixed_side: None,
        },
        SheetMetalKind::Connect {
            between: connect_between_samples().remove(0),
            radius: Some(lit(positive(2.0))),
            extension: Some(bend_extension_samples().remove(0)),
            double: Some(double_bend_samples().remove(0)),
        },
        SheetMetalKind::Connect {
            between: connect_between_samples().remove(0),
            radius: None,
            extension: None,
            double: None,
        },
        SheetMetalKind::Rip(rip_samples().remove(0)),
        SheetMetalKind::Convert(convert_samples().remove(0)),
        SheetMetalKind::BreakCorner {
            corners: selection(1),
            size: lit(positive(2.0)),
            shape: corner_break_samples().remove(0),
        },
        SheetMetalKind::ExtendWall {
            edge: selection(1),
            to: plane(),
        },
        SheetMetalKind::ExtrudedHole {
            bore: stamp_outline_samples().remove(0),
            entry: Some(selection(1)),
            height: lit(positive(2.0)),
            root_fillet: Some(lit(positive(2.0))),
            rule: Some(id("sheet-metal-rule", 1)),
        },
        SheetMetalKind::ExtrudedHole {
            bore: stamp_outline_samples().remove(0),
            entry: None,
            height: lit(positive(2.0)),
            root_fillet: None,
            rule: None,
        },
        SheetMetalKind::Stamp {
            entry: Some(selection(1)),
            form: stamp_form_samples().remove(0),
            rule: Some(id("sheet-metal-rule", 1)),
        },
        SheetMetalKind::Stamp {
            entry: None,
            form: stamp_form_samples().remove(0),
            rule: None,
        },
        SheetMetalKind::ToolForm {
            tool: DefinitionRef {
                definition: id("product-definition", 1),
                configuration: None,
            },
            placement: Some(frame()),
            constraints: vec![joint_samples().remove(0)],
            rule: Some(id("sheet-metal-rule", 1)),
        },
        SheetMetalKind::ToolForm {
            tool: DefinitionRef {
                definition: id("product-definition", 1),
                configuration: None,
            },
            placement: None,
            constraints: Vec::new(),
            rule: None,
        },
        SheetMetalKind::Unfold {
            fixed: Some(selection(1)),
            bends: Some(bend_set_samples().remove(0)),
        },
        SheetMetalKind::Unfold {
            fixed: None,
            bends: None,
        },
        SheetMetalKind::Refold {
            bends: Some(bend_set_samples().remove(0)),
        },
        SheetMetalKind::Refold { bends: None },
        SheetMetalKind::FlatPattern {
            fixed: Some(selection(1)),
        },
        SheetMetalKind::FlatPattern { fixed: None },
        SheetMetalKind::Native(text("codec:native")),
    ]
}

fn connect_between_samples() -> Vec<ConnectBetween> {
    vec![
        ConnectBetween::Apart([selection(1), selection(1)]),
        ConnectBetween::SharedEdge(selection(1)),
    ]
}

fn rip_samples() -> Vec<Rip> {
    vec![
        Rip::Faces {
            faces: selection(1),
        },
        Rip::Along {
            path: rip_path_samples().remove(0),
            width: rip_width_samples().remove(0),
        },
    ]
}

fn rip_path_samples() -> Vec<RipPath> {
    vec![
        RipPath::Ruling {
            face: selection(1),
            at: Box::new(point_construction()),
        },
        RipPath::Line {
            face: selection(1),
            from: Box::new(point_construction()),
            to: Box::new(point_construction()),
        },
        RipPath::Edges(selection(1)),
        RipPath::Sketched {
            profile: selection(3),
            through: sketched_rip_depth_samples().remove(0),
        },
    ]
}

fn sketched_rip_depth_samples() -> Vec<SketchedRipDepth> {
    vec![SketchedRipDepth::SketchWall, SketchedRipDepth::Projected]
}

fn rip_width_samples() -> Vec<RipWidth> {
    vec![
        RipWidth::Tear,
        RipWidth::Gap {
            gap: lit(positive(2.0)),
            side: Some(kerf_side_samples().remove(0)),
        },
        RipWidth::Gap {
            gap: lit(positive(2.0)),
            side: None,
        },
    ]
}

fn kerf_side_samples() -> Vec<KerfSide> {
    vec![KerfSide::Centered, KerfSide::Left, KerfSide::Right]
}

fn convert_samples() -> Vec<Convert> {
    vec![
        Convert {
            body: selection(1),
            fixed: selection(1),
            thickness: lit(positive(2.0)),
            from: convert_from_samples().remove(0),
            rips: vec![convert_rip_samples().remove(0)],
            bend: bend_spec_samples().remove(0),
            keep_solid: Some(true),
            rule: Some(id("sheet-metal-rule", 1)),
        },
        Convert {
            body: selection(1),
            fixed: selection(1),
            thickness: lit(positive(2.0)),
            from: convert_from_samples().remove(0),
            rips: Vec::new(),
            bend: bend_spec_samples().remove(0),
            keep_solid: None,
            rule: None,
        },
    ]
}

fn convert_from_samples() -> Vec<ConvertFrom> {
    vec![
        ConvertFrom::Faces {
            side: Some(convert_side_samples().remove(0)),
            bends: vec![convert_bend_samples().remove(0)],
        },
        ConvertFrom::Faces {
            side: None,
            bends: Vec::new(),
        },
        ConvertFrom::ThinSolid,
    ]
}

fn convert_rip_samples() -> Vec<ConvertRip> {
    vec![ConvertRip {
        path: rip_path_samples().remove(0),
        gap: lit(positive(2.0)),
    }]
}

fn convert_side_samples() -> Vec<ConvertSide> {
    vec![ConvertSide::Inside, ConvertSide::Outside]
}

fn corner_break_samples() -> Vec<CornerBreak> {
    vec![
        CornerBreak::Round,
        CornerBreak::Chamfer,
        CornerBreak::Native(text("codec:native")),
    ]
}

fn convert_bend_samples() -> Vec<ConvertBend> {
    vec![
        ConvertBend::Edge {
            edge: selection(1),
            radius: Some(lit(positive(2.0))),
        },
        ConvertBend::Edge {
            edge: selection(1),
            radius: None,
        },
        ConvertBend::Face { face: selection(1) },
    ]
}

fn normal_side_samples() -> Vec<NormalSide> {
    vec![
        NormalSide::Along,
        NormalSide::Against,
        NormalSide::Symmetric,
    ]
}

fn vertex_bend_samples() -> Vec<VertexBend> {
    vec![
        VertexBend {
            vertex: selection(1),
            radius: lit(positive(2.0)),
            radius_side: Some(radius_side_samples().remove(0)),
        },
        VertexBend {
            vertex: selection(1),
            radius: lit(positive(2.0)),
            radius_side: None,
        },
    ]
}

fn stamp_outline_samples() -> Vec<StampOutline> {
    vec![
        StampOutline::Profile(selection(3)),
        StampOutline::Circle {
            center: point_construction(),
            diameter: lit(positive(2.0)),
        },
    ]
}

fn stamp_form_samples() -> Vec<StampForm> {
    vec![
        StampForm::Raised {
            outline: stamp_outline_samples().remove(0),
            height: lit(positive(2.0)),
            wall_angle: bound(1),
            rim_radius: Some(lit(positive(2.0))),
            floor_radius: Some(lit(positive(2.0))),
        },
        StampForm::Raised {
            outline: stamp_outline_samples().remove(0),
            height: lit(positive(2.0)),
            wall_angle: bound(1),
            rim_radius: None,
            floor_radius: None,
        },
        StampForm::Bridge {
            placement: Some(frame()),
            height: lit(positive(2.0)),
            wall_angle: bound(1),
            length: lit(positive(2.0)),
            width: lit(positive(2.0)),
            rim_radius: Some(lit(positive(2.0))),
            floor_radius: Some(lit(positive(2.0))),
            relief_length: [Some(lit(positive(2.0))), Some(lit(positive(2.0)))],
        },
        StampForm::Bridge {
            placement: None,
            height: lit(positive(2.0)),
            wall_angle: bound(1),
            length: lit(positive(2.0)),
            width: lit(positive(2.0)),
            rim_radius: None,
            floor_radius: None,
            relief_length: [None, None],
        },
        StampForm::Louver {
            profile: selection(3),
            open_edge: selection(1),
            depth: lit(positive(2.0)),
            flap_angle: bound(1),
            wall_angle: bound(1),
            rim_radius: Some(lit(positive(2.0))),
        },
        StampForm::Louver {
            profile: selection(3),
            open_edge: selection(1),
            depth: lit(positive(2.0)),
            flap_angle: bound(1),
            wall_angle: bound(1),
            rim_radius: None,
        },
        StampForm::Bead {
            path: selection(3),
            height: lit(positive(2.0)),
            section_radius: lit(positive(2.0)),
            end_radius: lit(positive(2.0)),
        },
        StampForm::StiffeningRib {
            bend: selection(1),
            at: point_construction(),
            length: lit(positive(2.0)),
            bottom_radius: lit(positive(2.0)),
            wall_angle: bound(1),
            rim_radius: lit(positive(2.0)),
        },
    ]
}

fn bend_set_samples() -> Vec<BendSet> {
    vec![BendSet::All, BendSet::Selected(selection(1))]
}

fn bend_spec_samples() -> Vec<BendSpec> {
    vec![
        BendSpec {
            radius: Some(lit(positive(2.0))),
            radius_side: Some(radius_side_samples().remove(0)),
            relief: Some(relief_samples().remove(0)),
        },
        BendSpec {
            radius: None,
            radius_side: None,
            relief: None,
        },
    ]
}

fn radius_side_samples() -> Vec<RadiusSide> {
    vec![RadiusSide::Inside, RadiusSide::Outside]
}

fn relief_samples() -> Vec<Relief> {
    vec![
        Relief {
            shape: Some(relief_shape_samples().remove(0)),
            width: Some(lit(positive(2.0))),
            width_ratio: Some(lit(positive_real(1.5))),
            depth: Some(lit(positive(2.0))),
        },
        Relief {
            shape: None,
            width: None,
            width_ratio: None,
            depth: None,
        },
    ]
}

fn relief_shape_samples() -> Vec<ReliefShape> {
    vec![
        ReliefShape::Rule,
        ReliefShape::Obround,
        ReliefShape::Native(text("codec:native")),
    ]
}

fn flange_samples() -> Vec<Flange> {
    vec![
        Flange {
            edges: selection(2),
            height: Some(flange_height_samples().remove(0)),
            angle: Some(bound(1)),
            width: Some(flange_width_samples().remove(0)),
            bend: bend_spec_samples().remove(0),
            rule: Some(id("sheet-metal-rule", 1)),
            height_datum: Some(height_datum_samples().remove(0)),
            position: Some(bend_position_samples().remove(0)),
            outline: Some(selection(3)),
        },
        Flange {
            edges: selection(2),
            height: None,
            angle: None,
            width: None,
            bend: bend_spec_samples().remove(0),
            rule: None,
            height_datum: None,
            position: None,
            outline: None,
        },
    ]
}

fn flange_height_samples() -> Vec<FlangeHeight> {
    vec![
        FlangeHeight::Distance(lit(positive(2.0))),
        FlangeHeight::UpTo {
            target: selection(1),
            offset: Some(lit(length(-1.5))),
        },
        FlangeHeight::UpTo {
            target: selection(1),
            offset: None,
        },
    ]
}

fn flange_width_samples() -> Vec<FlangeWidth> {
    vec![
        FlangeWidth::Full,
        FlangeWidth::Centered {
            width: lit(positive(2.0)),
        },
        FlangeWidth::FromEnds {
            first: lit(nonnegative(0.5)),
            second: lit(nonnegative(0.5)),
        },
        FlangeWidth::FromReference {
            from: Box::new(point_construction()),
            offset: lit(length(-1.5)),
            width: lit(positive(2.0)),
            reversed: true,
        },
        FlangeWidth::PerEdge(vec![[lit(positive(2.0)), lit(positive(2.0))]]),
        FlangeWidth::CenteredPerEdge(vec![lit(positive(2.0))]),
    ]
}

fn height_datum_samples() -> Vec<HeightDatum> {
    vec![
        HeightDatum::InnerSharp,
        HeightDatum::OuterSharp,
        HeightDatum::TangentToBend,
    ]
}

fn bend_position_samples() -> Vec<BendPosition> {
    vec![
        BendPosition::MaterialInside,
        BendPosition::MaterialOutside,
        BendPosition::AtEdge,
        BendPosition::TangentToSide,
    ]
}

fn bend_extension_samples() -> Vec<BendExtension> {
    vec![
        BendExtension::AlignedToSideFaces,
        BendExtension::PerpendicularToSideFaces,
    ]
}

fn bend_zone_samples() -> Vec<BendZone> {
    vec![
        BendZone::MovingSide,
        BendZone::Centered,
        BendZone::FixedSide,
    ]
}

fn bend_line_face_samples() -> Vec<BendLineFace> {
    vec![BendLineFace::Inner, BendLineFace::Outer]
}

fn line_side_samples() -> Vec<LineSide> {
    vec![LineSide::Left, LineSide::Right]
}

fn double_bend_samples() -> Vec<DoubleBend> {
    vec![
        DoubleBend::EqualBends,
        DoubleBend::Deg45 {
            fixed: Some(pair_element_samples().remove(0)),
        },
        DoubleBend::Deg45 { fixed: None },
        DoubleBend::Deg90 {
            fixed: Some(pair_element_samples().remove(0)),
        },
        DoubleBend::Deg90 { fixed: None },
        DoubleBend::FullRadius {
            fixed: Some(pair_element_samples().remove(0)),
        },
        DoubleBend::FullRadius { fixed: None },
    ]
}

fn hem_samples() -> Vec<Hem> {
    vec![
        Hem {
            edges: selection(2),
            form: hem_form_samples().remove(0),
            side: Some(hem_side_samples().remove(0)),
            position: Some(bend_position_samples().remove(0)),
            rule: Some(id("sheet-metal-rule", 1)),
        },
        Hem {
            edges: selection(2),
            form: hem_form_samples().remove(0),
            side: None,
            position: None,
            rule: None,
        },
    ]
}

fn hem_side_samples() -> Vec<HemSide> {
    vec![HemSide::SelectedEdgeFace, HemSide::OppositeFace]
}

fn hem_form_samples() -> Vec<HemForm> {
    vec![
        HemForm::Flat {
            length: lit(positive(2.0)),
            gap: Some(lit(positive(2.0))),
        },
        HemForm::Flat {
            length: lit(positive(2.0)),
            gap: None,
        },
        HemForm::Open {
            length: lit(positive(2.0)),
            gap: lit(positive(2.0)),
        },
        HemForm::Teardrop {
            radius: lit(positive(2.0)),
            length: lit(positive(2.0)),
            gap: lit(positive(2.0)),
        },
        HemForm::Rolled {
            radius: lit(positive(2.0)),
            angle: bound(1),
        },
        HemForm::Native(text("codec:native")),
    ]
}

#[test]
fn every_sheet_metal_type_round_trips() {
    let mut report = Report::default();
    report.check("SheetMetal", sheet_metal_samples());
    report.check("SheetMetalRule", sheet_metal_rule_samples());
    report.check("BendAllowance", bend_allowance_samples());
    report.check("KFactorTable", k_factor_table_samples());
    report.check("BendTable", bend_table_samples());
    report.check("BendTableBlock", bend_table_block_samples());
    report.check("BendTableAngle", bend_table_angle_samples());
    report.check("BendTableValue", bend_table_value_samples());
    report.check("FlangeMeasure", flange_measure_samples());
    report.check("BendTableLookup", bend_table_lookup_samples());
    report.check("AxisLookup", axis_lookup_samples());
    report.check("OutsideGrid", outside_grid_samples());
    report.check("BendEquations", bend_equations_samples());
    report.check("BendQuantity", bend_quantity_samples());
    report.check("AngleRange", angle_range_samples());
    report.check("TableLookup", table_lookup_samples());
    report.check("KFactorRow", k_factor_row_samples());
    report.check("SheetMetalKind", sheet_metal_kind_samples());
    report.check("ConnectBetween", connect_between_samples());
    report.check("Rip", rip_samples());
    report.check("RipPath", rip_path_samples());
    report.check("SketchedRipDepth", sketched_rip_depth_samples());
    report.check("RipWidth", rip_width_samples());
    report.check("KerfSide", kerf_side_samples());
    report.check("Convert", convert_samples());
    report.check("ConvertFrom", convert_from_samples());
    report.check("ConvertRip", convert_rip_samples());
    report.check("ConvertSide", convert_side_samples());
    report.check("CornerBreak", corner_break_samples());
    report.check("ConvertBend", convert_bend_samples());
    report.check("NormalSide", normal_side_samples());
    report.check("VertexBend", vertex_bend_samples());
    report.check("StampOutline", stamp_outline_samples());
    report.check("StampForm", stamp_form_samples());
    report.check("BendSet", bend_set_samples());
    report.check("BendSpec", bend_spec_samples());
    report.check("RadiusSide", radius_side_samples());
    report.check("Relief", relief_samples());
    report.check("ReliefShape", relief_shape_samples());
    report.check("Flange", flange_samples());
    report.check("FlangeHeight", flange_height_samples());
    report.check("FlangeWidth", flange_width_samples());
    report.check("HeightDatum", height_datum_samples());
    report.check("BendPosition", bend_position_samples());
    report.check("BendExtension", bend_extension_samples());
    report.check("BendZone", bend_zone_samples());
    report.check("BendLineFace", bend_line_face_samples());
    report.check("LineSide", line_side_samples());
    report.check("DoubleBend", double_bend_samples());
    report.check("Hem", hem_samples());
    report.check("HemSide", hem_side_samples());
    report.check("HemForm", hem_form_samples());
    assert!(report.failures.is_empty(), "{:#?}", report.failures);
}

/// Each enum writes its stated tag form, and an arm with no payload in an
/// internally tagged enum refuses another key.
#[test]
fn each_sheet_metal_enum_has_its_tag_form() {
    assert_eq!(to_value(&RadiusSide::Inside), json!("inside"));
    assert_eq!(
        to_value(&SheetMetalKind::FlatPattern { fixed: None }),
        json!({"kind": "flat_pattern", "value": {}})
    );
    assert_eq!(to_value(&BendSet::All), json!({"kind": "all"}));
    assert_eq!(
        to_value(&ConvertFrom::ThinSolid),
        json!({"kind": "thin_solid"})
    );
    assert_eq!(to_value(&FlangeWidth::Full), json!({"kind": "full"}));
    assert_eq!(
        to_value(&FlangeWidth::CenteredPerEdge(vec![lit(positive(2.0))])),
        json!({"kind": "centered_per_edge", "value": [{"kind": "literal", "value": 2.0}]})
    );
    assert_eq!(
        to_value(&BendTableValue::Deduction { flanges: None }),
        json!({"kind": "deduction"})
    );
    assert_eq!(
        to_value(&DoubleBend::Deg45 {
            fixed: Some(PairElement::Second)
        }),
        json!({"kind": "deg45", "fixed": "second"})
    );
    assert_eq!(
        to_value(&OutsideGrid::BackupKFactor { k_factor: None }),
        json!({"kind": "backup_k_factor"})
    );
    refuses::<BendSet>(r#"{"kind": "all", "extra": 1}"#);
    refuses::<ConvertFrom>(r#"{"kind": "thin_solid", "side": "inside"}"#);
    refuses::<OutsideGrid>(r#"{"kind": "backup_k_factor", "k_factor": null}"#);
    refuses::<BendSpec>(r#"{"radius_side": null}"#);
    for extra in [r#""value": null"#, r#""value": {}"#, r#""x": 1"#] {
        refuses::<BendTableValue>(&format!(r#"{{"kind": "allowance", {extra}}}"#));
        refuses::<BendTableValue>(&format!(r#"{{"kind": "k_factor", {extra}}}"#));
        refuses::<RipWidth>(&format!(r#"{{"kind": "tear", {extra}}}"#));
        refuses::<ConvertFrom>(&format!(r#"{{"kind": "thin_solid", {extra}}}"#));
        refuses::<BendSet>(&format!(r#"{{"kind": "all", {extra}}}"#));
        refuses::<DoubleBend>(&format!(r#"{{"kind": "equal_bends", {extra}}}"#));
    }
}

/// §2.5: each value has one spelling, so a unit variant of an adjacently
/// tagged enum is its `kind` alone and refuses a stated `value`.
#[test]
fn every_adjacent_unit_variant_refuses_a_stated_value() {
    let readers: [(&'static str, Reader); 4] = [
        ("BendQuantity", reads::<BendQuantity>),
        ("CornerBreak", reads::<CornerBreak>),
        ("FlangeWidth", reads::<FlangeWidth>),
        ("ReliefShape", reads::<ReliefShape>),
    ];
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/design/ops/sheet_metal.rs");
    assert_units_refuse_value(&[file], &BTreeMap::from(readers));
}

/// A bend table block's grid is one row per angle, one column per radius.
#[test]
fn a_bend_table_grid_is_rows_of_angles() {
    let block = BendTableBlock {
        thickness: positive(1.0),
        radii: vec![positive(1.0), positive(2.0)],
        angles: vec![angle(0.5)],
        values: vec![vec![real(0.1), real(0.2)]],
    };
    assert_eq!(
        to_value(&block),
        json!({"thickness": 1.0, "radii": [1.0, 2.0], "angles": [0.5], "values": [[0.1, 0.2]]})
    );
    round_trip(&block).unwrap();
}

/// A connect between two faces that do not touch picks exactly two.
#[test]
fn a_connect_apart_is_a_fixed_pair() {
    let written = to_value(&ConnectBetween::Apart([selection(1), selection(2)]));
    let mut one = written.clone();
    one["value"].as_array_mut().unwrap().pop();
    refuses::<ConnectBetween>(&one.to_string());
}

/// §5.10: a sheet-metal operation is the `FeatureOperation` arm
/// `sheet_metal`, holding the operation as its value.
#[test]
fn every_sheet_metal_operation_is_a_feature_operation_arm() {
    use crate::design::feature::{FeatureOperation as Op, OperationFamily as Family};
    use crate::design::tests::arm_failures;
    let failures = arm_failures(Family::SheetMetal, sheet_metal_samples(), Op::SheetMetal);
    assert!(failures.is_empty(), "{failures:#?}");
}

/// §5: a sheet-metal rule is a record of the design root's
/// `sheet_metal_rules` arena.
#[test]
fn sheet_metal_rules_are_an_arena_of_the_design_root() {
    let design = crate::document::Design {
        sheet_metal_rules: sheet_metal_rule_samples(),
        ..Default::default()
    };
    let value = to_value(&design);
    assert_eq!(
        value["sheet_metal_rules"],
        json!(sheet_metal_rule_samples()
            .iter()
            .map(to_value)
            .collect::<Vec<_>>())
    );
    round_trip(&design).unwrap();
}

#[cfg(feature = "schema")]
mod schema {
    use schemars::{JsonSchema, SchemaGenerator};

    use super::super::*;

    fn generates<T: JsonSchema>(generator: &mut SchemaGenerator) {
        let schema = T::json_schema(generator);
        assert!(schema.as_object().is_some(), "{}", T::schema_name());
    }

    #[test]
    fn every_sheet_metal_type_has_a_schema() {
        let mut generator = SchemaGenerator::default();
        generates::<SheetMetal>(&mut generator);
        generates::<SheetMetalRule>(&mut generator);
        generates::<BendAllowance>(&mut generator);
        generates::<KFactorTable>(&mut generator);
        generates::<BendTable>(&mut generator);
        generates::<BendTableBlock>(&mut generator);
        generates::<BendTableAngle>(&mut generator);
        generates::<BendTableValue>(&mut generator);
        generates::<FlangeMeasure>(&mut generator);
        generates::<BendTableLookup>(&mut generator);
        generates::<AxisLookup>(&mut generator);
        generates::<OutsideGrid>(&mut generator);
        generates::<BendEquations>(&mut generator);
        generates::<BendQuantity>(&mut generator);
        generates::<AngleRange>(&mut generator);
        generates::<TableLookup>(&mut generator);
        generates::<KFactorRow>(&mut generator);
        generates::<SheetMetalKind>(&mut generator);
        generates::<ConnectBetween>(&mut generator);
        generates::<Rip>(&mut generator);
        generates::<RipPath>(&mut generator);
        generates::<SketchedRipDepth>(&mut generator);
        generates::<RipWidth>(&mut generator);
        generates::<KerfSide>(&mut generator);
        generates::<Convert>(&mut generator);
        generates::<ConvertFrom>(&mut generator);
        generates::<ConvertRip>(&mut generator);
        generates::<ConvertSide>(&mut generator);
        generates::<CornerBreak>(&mut generator);
        generates::<ConvertBend>(&mut generator);
        generates::<NormalSide>(&mut generator);
        generates::<VertexBend>(&mut generator);
        generates::<StampOutline>(&mut generator);
        generates::<StampForm>(&mut generator);
        generates::<BendSet>(&mut generator);
        generates::<BendSpec>(&mut generator);
        generates::<RadiusSide>(&mut generator);
        generates::<Relief>(&mut generator);
        generates::<ReliefShape>(&mut generator);
        generates::<Flange>(&mut generator);
        generates::<FlangeHeight>(&mut generator);
        generates::<FlangeWidth>(&mut generator);
        generates::<HeightDatum>(&mut generator);
        generates::<BendPosition>(&mut generator);
        generates::<BendExtension>(&mut generator);
        generates::<BendZone>(&mut generator);
        generates::<BendLineFace>(&mut generator);
        generates::<LineSide>(&mut generator);
        generates::<DoubleBend>(&mut generator);
        generates::<Hem>(&mut generator);
        generates::<HemSide>(&mut generator);
        generates::<HemForm>(&mut generator);
    }
}
