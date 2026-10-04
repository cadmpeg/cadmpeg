// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::schema::structural::project;

// These direct call sites cover all 38 source records and four additional
// roots. Positives require bounded derived fields, fixed callback bodies,
// finite conversions or exact borrowed-wire lineage. Other roots retain
// source-side traversal, copying or callback findings.

pub fn appearance(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::appearance::Appearance,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 Appearance source")?);
    Ok(())
}

pub fn appearance_binding(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::appearance::AppearanceBinding,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 AppearanceBinding source")?);
    Ok(())
}

pub fn asset(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::assets::Asset,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 Asset source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn source_attribute(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::attributes::SourceAttribute,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 SourceAttribute source")?);
    Ok(())
}

pub fn drawing(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::drawings::Drawing,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 Drawing source")?);
    Ok(())
}

pub fn design_configuration(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::features::DesignConfiguration,
) -> Result<(), CodecError> {
drop(project(ctx, source, "x220 DesignConfiguration source")?);
    Ok(())
}

pub fn design_parameter(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::features::DesignParameter,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 DesignParameter source")?);
    Ok(())
}

pub fn feature_input_topology(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::features::FeatureInputTopology,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 FeatureInputTopology source")?);
    Ok(())
}

pub fn feature_result_topology(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::features::FeatureResultTopology,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 FeatureResultTopology source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn curve(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::geometry::Curve,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 Curve source")?);
    Ok(())
}

pub fn procedural_curve(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::geometry::ProceduralCurve,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 ProceduralCurve source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn procedural_surface(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::geometry::ProceduralSurface,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 ProceduralSurface source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn surface(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::geometry::Surface,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 Surface source")?);
    Ok(())
}

pub fn pcurve(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::geometry::pcurve::Pcurve,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 Pcurve source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn pmi_annotation(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::pmi::PmiAnnotation,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 PmiAnnotation source")?);
    Ok(())
}

pub fn presentation_document(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::presentation::PresentationDocument,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 PresentationDocument source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn presentation_layer(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::presentation::PresentationLayer,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 PresentationLayer source")?);
    Ok(())
}

pub fn view_presentation(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::presentation::ViewPresentation,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 ViewPresentation source")?);
    Ok(())
}

pub fn assembly_joint(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::products::AssemblyJoint,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 AssemblyJoint source")?);
    Ok(())
}

pub fn occurrence(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::products::Occurrence,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 Occurrence source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn product_definition(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::products::ProductDefinition,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 ProductDefinition source")?);
    Ok(())
}

pub fn semantic_annotation(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::semantic_annotations::SemanticAnnotation,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 SemanticAnnotation source")?);
    Ok(())
}

pub fn sketch(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::sketches::Sketch,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 Sketch source")?);
    Ok(())
}

pub fn sketch_constraint(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::sketches::SketchConstraint,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 SketchConstraint source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn sketch_entity(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::sketches::SketchEntity,
) -> Result<(), CodecError> {
drop(project(ctx, source, "x220 SketchEntity source")?);
    Ok(())
}

pub fn spatial_sketch(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::sketches::SpatialSketch,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 SpatialSketch source")?);
    Ok(())
}

pub fn spatial_sketch_constraint(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::sketches::SpatialSketchConstraint,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 SpatialSketchConstraint source")?);
    Ok(())
}

pub fn spatial_sketch_entity(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::sketches::SpatialSketchEntity,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 SpatialSketchEntity source")?);
    Ok(())
}

pub fn subd_surface(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::subd::SubdSurface,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 SubdSurface source")?);
    Ok(())
}

pub fn body(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::topology::Body,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 Body source")?);
    Ok(())
}

pub fn coedge(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::topology::Coedge,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 Coedge source")?);
    Ok(())
}

pub fn edge(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::topology::Edge,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 Edge source")?);
    Ok(())
}

pub fn face(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::topology::Face,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 Face source")?);
    Ok(())
}

pub fn loop_(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::topology::Loop,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 Loop source")?);
    Ok(())
}

pub fn point(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::topology::Point,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 Point source")?);
    Ok(())
}

pub fn region(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::topology::Region,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 Region source")?);
    Ok(())
}

pub fn shell(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::topology::Shell,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 Shell source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn vertex(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::topology::Vertex,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 Vertex source")?);
    Ok(())
}

pub fn feature(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::features::Feature,
) -> Result<(), CodecError> {
drop(project(ctx, source, "x220 Feature source")?);
    Ok(())
}

pub fn native_record(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::native::NativeRecord,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 NativeRecord source")?);
    Ok(())
}

pub fn spreadsheet(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::spreadsheets::Spreadsheet,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 Spreadsheet source")?); // finding: unproven_decode_charge
    Ok(())
}

pub fn tessellation(
    ctx: &DecodeContext<'_>,
    source: &cadmpeg_ir::tessellation::Tessellation,
) -> Result<(), CodecError> {
    drop(project(ctx, source, "x220 Tessellation source")?); // finding: unproven_decode_charge
    Ok(())
}
