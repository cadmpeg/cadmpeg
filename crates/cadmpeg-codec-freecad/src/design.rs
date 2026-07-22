// SPDX-License-Identifier: Apache-2.0
//! Transfer of `FCStd` construction history into neutral design entities.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    BinderConstruction, BinderCopyOnChange, BinderLifecycle, BinderOffset, BinderOffsetJoin,
    BinderPlacement, BinderSource, BinderTarget, BodySelection, BooleanOp, ChamferSpec,
    DesignParameter, EdgeSelection, Extent, ExtrusionDirectionSource, ExtrusionFaceMaker, Feature,
    FeatureDefinition, FeatureId, FeatureTreeNodeRole, FuzzyTolerance, GeometryImportFormat,
    HelicalSweepConstruction, HelicalSweepLaw, HelixConstructionStyle, HoleBottom, HoleKind,
    HoleProfileFilter, HoleSpecification, HoleThreadDepth, InnerWireTaper, Length, ParameterId,
    ParameterValue, PathRef, PatternKind, PatternScaleCenter, PatternSeed, PatternStage,
    PatternStageCombination, PrimitiveSolid, ProfileRef, RadiusSpec, RevolutionAxis,
    RevolutionConstruction, RevolutionFuseOrder, RuledCurveOrientation, ScaleCenter, ScaleFactors,
    ShellJoin, ShellMode, SurfaceProjectionMode, SweepMode, SweepOrientation, SweepTransformation,
    SweepTransition, ThreadHand,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchConstraintDefinition, SketchConstraintId, SketchEntity,
    SketchEntityId, SketchEntityUse, SketchGeometry, SketchId, SketchLocus, SketchNativeOperand,
};
use cadmpeg_ir::spreadsheets::{
    Spreadsheet, SpreadsheetDimension, SpreadsheetId, SpreadsheetRange,
};

use crate::brep::ShapePayloadRecord;
use crate::native::{EntryRecord, ObjectRecord, PropertyRecord, ValueRecord};

const MAX_SKETCH_RECORDS: usize = 1_000_000;

pub(crate) fn transfer(
    ir: &mut CadIr,
    objects: &[ObjectRecord],
    properties: &[PropertyRecord],
    payloads: &[ShapePayloadRecord],
    entries: &[EntryRecord],
) -> Result<(), CodecError> {
    let properties_by_owner = properties.iter().fold(
        HashMap::<&str, Vec<&PropertyRecord>>::new(),
        |mut map, property| {
            map.entry(&property.owner).or_default().push(property);
            map
        },
    );
    let feature_ids = objects
        .iter()
        .filter(|object| is_design_object(&object.type_name))
        .map(|object| (object.id.as_str(), feature_id(object)))
        .collect::<HashMap<_, _>>();
    let parent_by_member = objects
        .iter()
        .filter(|object| is_body(&object.type_name))
        .flat_map(|body| {
            properties_by_owner
                .get(body.id.as_str())
                .into_iter()
                .flatten()
                .filter(|property| property.name == "Group")
                .flat_map(|property| &property.links)
                .filter_map(|link| link.object.as_deref())
                .map(move |member| (member, feature_id(body)))
        })
        .collect::<HashMap<_, _>>();
    let mut sketch_ids = objects
        .iter()
        .filter(|object| is_sketch(&object.type_name))
        .map(|object| {
            (
                object.id.as_str(),
                SketchId(format!("fcstd:design:sketch#{}", object.name)),
            )
        })
        .collect::<HashMap<_, _>>();
    let body_ids = ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    let feature_order = objects
        .iter()
        .map(|candidate| (feature_id(candidate), candidate.order))
        .collect::<HashMap<_, _>>();

    for object in objects {
        if !is_design_object(&object.type_name) {
            continue;
        }
        let owned = properties_by_owner
            .get(object.id.as_str())
            .cloned()
            .unwrap_or_default();
        let id = feature_id(object);
        let definition = if is_spreadsheet(&object.type_name) {
            ir.model.spreadsheets.push(append_spreadsheet(
                &mut ir.model.parameters,
                object,
                &owned,
            )?);
            FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::Equations,
                children: Vec::new(),
                active_child: None,
            }
        } else if is_body(&object.type_name) {
            FeatureDefinition::TreeNode {
                role: FeatureTreeNodeRole::SolidBodies,
                children: linked_feature_ids(&owned, "Group", &feature_ids),
                active_child: linked_feature_ids(&owned, "Tip", &feature_ids)
                    .into_iter()
                    .next(),
            }
        } else if is_datum(&object.type_name) {
            datum_definition(&object.type_name, &owned).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone(),
                    parameters: native_parameters(&owned),
                    properties: BTreeMap::new(),
                }
            })
        } else if is_sketch(&object.type_name) {
            let decoded = parse_sketch(object, &owned)?;
            let sketch = decoded.sketch;
            let sketch_id = sketch.id.clone();
            sketch_ids.insert(object.id.as_str(), sketch_id.clone());
            ir.model.sketches.push(sketch);
            ir.model.sketch_entities.extend(decoded.entities);
            ir.model.sketch_constraints.extend(decoded.constraints);
            ir.model.parameters.extend(decoded.parameters);
            FeatureDefinition::Sketch {
                space: cadmpeg_ir::features::SketchSpace::Planar,
                sketch: Some(sketch_id),
            }
        } else if is_stored_geometry_feature(&object.type_name) {
            FeatureDefinition::StoredGeometry
        } else if object.type_name == "PartDesign::FeatureBase" {
            feature_base_definition(&owned, &feature_ids).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone(),
                    parameters: native_parameters(&owned),
                    properties: BTreeMap::new(),
                }
            })
        } else if is_imported_geometry(&object.type_name) {
            imported_geometry_definition(&object.type_name, &owned).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone(),
                    parameters: native_parameters(&owned),
                    properties: BTreeMap::new(),
                }
            })
        } else if is_part_construction_geometry(&object.type_name) {
            part_construction_geometry_definition(&object.type_name, &owned).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone(),
                    parameters: native_parameters(&owned),
                    properties: BTreeMap::new(),
                }
            })
        } else if is_primitive(&object.type_name) {
            primitive_definition(&object.type_name, &owned).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone(),
                    parameters: native_parameters(&owned),
                    properties: BTreeMap::new(),
                }
            })
        } else if is_boolean(&object.type_name) {
            boolean_definition(&object.type_name, &owned).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone(),
                    parameters: native_parameters(&owned),
                    properties: BTreeMap::new(),
                }
            })
        } else if is_loft(&object.type_name) {
            loft_definition(&object.type_name, &owned, &sketch_ids).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone(),
                    parameters: native_parameters(&owned),
                    properties: BTreeMap::new(),
                }
            })
        } else if is_sweep(&object.type_name) {
            sweep_definition(&object.type_name, &owned, &sketch_ids).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone(),
                    parameters: native_parameters(&owned),
                    properties: BTreeMap::new(),
                }
            })
        } else if is_helical_sweep(&object.type_name) {
            helical_sweep_definition(&object.type_name, &object.id, &owned, &sketch_ids)
                .unwrap_or_else(|| FeatureDefinition::Native {
                    kind: object.type_name.clone(),
                    parameters: native_parameters(&owned),
                    properties: BTreeMap::new(),
                })
        } else if matches!(object.type_name.as_str(), "Part::Helix" | "Part::Spiral") {
            parametric_helix_definition(&object.type_name, &owned).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone(),
                    parameters: native_parameters(&owned),
                    properties: BTreeMap::new(),
                }
            })
        } else if is_binder(&object.type_name) {
            binder_definition(&object.type_name, &owned, &feature_ids).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone(),
                    parameters: native_parameters(&owned),
                    properties: BTreeMap::new(),
                }
            })
        } else if is_pattern(&object.type_name) {
            pattern_definition(
                &object.type_name,
                &owned,
                &feature_ids,
                objects,
                &properties_by_owner,
            )
            .unwrap_or_else(|| FeatureDefinition::Native {
                kind: object.type_name.clone(),
                parameters: native_parameters(&owned),
                properties: BTreeMap::new(),
            })
        } else if object.type_name == "Part::Scale" {
            scale_definition(&owned).unwrap_or_else(|| FeatureDefinition::Native {
                kind: object.type_name.clone(),
                parameters: native_parameters(&owned),
                properties: BTreeMap::new(),
            })
        } else if is_hole(&object.type_name) {
            hole_definition(
                &object.id,
                &owned,
                &sketch_ids,
                objects,
                &properties_by_owner,
            )
            .unwrap_or_else(|| FeatureDefinition::Native {
                kind: object.type_name.clone(),
                parameters: native_parameters(&owned),
                properties: BTreeMap::new(),
            })
        } else if is_extrusion(&object.type_name) {
            let profile = profile_ref(&object.id, &owned, &sketch_ids);
            extrusion_definition(&object.type_name, &owned, profile, &ir.model.sketches)
                .unwrap_or_else(|| FeatureDefinition::Native {
                    kind: object.type_name.clone(),
                    parameters: native_parameters(&owned),
                    properties: BTreeMap::new(),
                })
        } else if is_revolution(&object.type_name) {
            revolution_definition(&object.type_name, &object.id, &owned, &sketch_ids)
                .unwrap_or_else(|| FeatureDefinition::Native {
                    kind: object.type_name.clone(),
                    parameters: native_parameters(&owned),
                    properties: BTreeMap::new(),
                })
        } else if matches!(
            object.type_name.as_str(),
            "PartDesign::Thickness" | "Part::Thickness"
        ) {
            thickness_definition(&object.type_name, &owned).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone(),
                    parameters: native_parameters(&owned),
                    properties: BTreeMap::new(),
                }
            })
        } else if matches!(object.type_name.as_str(), "Part::Offset" | "Part::Offset2D") {
            offset_shape_definition(&object.type_name, &owned).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone(),
                    parameters: native_parameters(&owned),
                    properties: BTreeMap::new(),
                }
            })
        } else if matches!(
            object.type_name.as_str(),
            "Part::Compound" | "Part::Compound2" | "Part::Refine" | "Part::Reverse"
        ) {
            derived_shape_definition(&object.type_name, &owned).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone(),
                    parameters: native_parameters(&owned),
                    properties: BTreeMap::new(),
                }
            })
        } else if object.type_name == "Part::RuledSurface" {
            ruled_surface_definition(&owned).unwrap_or_else(|| FeatureDefinition::Native {
                kind: object.type_name.clone(),
                parameters: native_parameters(&owned),
                properties: BTreeMap::new(),
            })
        } else if object.type_name == "Part::Section" {
            section_shape_definition(&owned).unwrap_or_else(|| FeatureDefinition::Native {
                kind: object.type_name.clone(),
                parameters: native_parameters(&owned),
                properties: BTreeMap::new(),
            })
        } else if object.type_name == "Part::Mirroring" {
            mirror_shape_definition(&owned).unwrap_or_else(|| FeatureDefinition::Native {
                kind: object.type_name.clone(),
                parameters: native_parameters(&owned),
                properties: BTreeMap::new(),
            })
        } else if object.type_name == "Part::ProjectOnSurface" {
            project_on_surface_definition(&owned).unwrap_or_else(|| FeatureDefinition::Native {
                kind: object.type_name.clone(),
                parameters: native_parameters(&owned),
                properties: BTreeMap::new(),
            })
        } else if object.type_name == "PartDesign::Draft" {
            draft_definition(&owned, objects, &properties_by_owner).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone(),
                    parameters: native_parameters(&owned),
                    properties: BTreeMap::new(),
                }
            })
        } else if object.type_name.contains("Fillet") {
            fillet_definition(&object.type_name, &owned, entries).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone(),
                    parameters: native_parameters(&owned),
                    properties: BTreeMap::new(),
                }
            })
        } else if object.type_name.contains("Chamfer") {
            chamfer_definition(&object.type_name, &owned, entries).unwrap_or_else(|| {
                FeatureDefinition::Native {
                    kind: object.type_name.clone(),
                    parameters: native_parameters(&owned),
                    properties: BTreeMap::new(),
                }
            })
        } else {
            FeatureDefinition::Native {
                kind: object.type_name.clone(),
                parameters: native_parameters(&owned),
                properties: BTreeMap::new(),
            }
        };
        let definition = post_processed_definition(definition, &owned).unwrap_or_else(|| {
            FeatureDefinition::Native {
                kind: object.type_name.clone(),
                parameters: native_parameters(&owned),
                properties: BTreeMap::new(),
            }
        });
        append_operation_parameters(&mut ir.model.parameters, object, &owned);
        let outputs = payloads
            .iter()
            .filter(|payload| owned.iter().any(|property| property.id == payload.property))
            .flat_map(|payload| {
                body_ids
                    .iter()
                    .filter(move |body| {
                        body.0
                            .starts_with(&crate::native::model_id("body", &payload.id, ""))
                    })
                    .cloned()
            })
            .collect();
        let mut dependency_objects = object
            .dependencies
            .iter()
            .map(String::as_str)
            .chain(
                owned
                    .iter()
                    .flat_map(|property| &property.links)
                    .filter_map(|link| link.object.as_deref()),
            )
            .collect::<Vec<_>>();
        let mut seen_dependencies = BTreeSet::new();
        dependency_objects.retain(|dependency| seen_dependencies.insert(*dependency));
        let dependencies = dependency_objects
            .into_iter()
            .filter_map(|dependency| feature_ids.get(dependency).cloned())
            .filter(|dependency| {
                feature_order
                    .get(dependency)
                    .is_some_and(|order| *order < object.order)
            })
            .collect();
        ir.model.features.push(Feature {
            id,
            ordinal: object.order as u64,
            name: Some(object.name.clone()),
            suppressed: bool_property(&owned, "Suppressed"),
            parent: parent_by_member.get(object.id.as_str()).cloned(),
            dependencies,
            source_properties: feature_state(&owned),
            source_tag: Some(object.type_name.clone()),
            source_text: None,
            source_content: Vec::new(),
            outputs,
            definition,
            native_ref: Some(object.id.clone()),
        });
    }
    bind_parameter_dependencies(&mut ir.model.parameters, objects);
    Ok(())
}

fn post_processed_definition(
    definition: FeatureDefinition,
    properties: &[&PropertyRecord],
) -> Option<FeatureDefinition> {
    if property(properties, "Refine").is_none() && property(properties, "FuzzyTolerance").is_none()
    {
        return Some(definition);
    }
    let refine = if property(properties, "Refine").is_some() {
        bool_property(properties, "Refine")?
    } else {
        false
    };
    let value = if property(properties, "FuzzyTolerance").is_some() {
        scalar_named(properties, "FuzzyTolerance")?
    } else {
        0.0
    };
    let fuzzy_tolerance = if value < 0.0 {
        FuzzyTolerance::Automatic
    } else if value == 0.0 {
        FuzzyTolerance::KernelDefault
    } else if value.is_finite() {
        FuzzyTolerance::Explicit(value)
    } else {
        return None;
    };
    Some(FeatureDefinition::PostProcess {
        operation: Box::new(definition),
        refine,
        fuzzy_tolerance,
    })
}

fn append_spreadsheet(
    parameters: &mut Vec<DesignParameter>,
    object: &ObjectRecord,
    properties: &[&PropertyRecord],
) -> Result<Spreadsheet, CodecError> {
    let property = properties
        .iter()
        .copied()
        .find(|property| property.type_name.contains("PropertySheet") || property.name == "cells")
        .ok_or_else(|| {
            CodecError::Malformed(format!("spreadsheet {} has no cells property", object.id))
        })?;
    let xml = roxmltree::Document::parse(&property.raw_xml).map_err(|error| {
        CodecError::Malformed(format!("invalid spreadsheet {}: {error}", property.id))
    })?;
    let Some(cells) = xml.descendants().find(|node| node.has_tag_name("Cells")) else {
        return Err(CodecError::Malformed(format!(
            "{} has no Cells value",
            property.id
        )));
    };
    let declared = cells
        .attribute("Count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| CodecError::Malformed(format!("{} has invalid Cells Count", property.id)))?;
    if declared > MAX_SKETCH_RECORDS {
        return Err(CodecError::Malformed(format!(
            "{} cell count exceeds {MAX_SKETCH_RECORDS}",
            property.id
        )));
    }
    let records = cells
        .children()
        .filter(|node| node.has_tag_name("Cell"))
        .collect::<Vec<_>>();
    if declared != records.len() {
        return Err(CodecError::Malformed(format!(
            "{} declares {declared} cells but contains {}",
            property.id,
            records.len()
        )));
    }
    let mut cell_ids = Vec::with_capacity(records.len());
    let mut merged_ranges = Vec::new();
    for (index, cell) in records.into_iter().enumerate() {
        let address = cell
            .attribute("address")
            .ok_or_else(|| CodecError::Malformed(format!("{} cell has no address", property.id)))?;
        let content = cell.attribute("content").unwrap_or_default();
        let name = cell.attribute("alias").unwrap_or(address);
        let mut retained = BTreeMap::from([("address".into(), address.to_owned())]);
        for attribute in [
            "alias",
            "alignment",
            "style",
            "foregroundColor",
            "backgroundColor",
            "displayUnit",
            "rowSpan",
            "colSpan",
        ] {
            if let Some(value) = cell.attribute(attribute) {
                retained.insert(attribute.into(), value.to_owned());
            }
        }
        let id = ParameterId(format!(
            "fcstd:design:parameter#{}:cell:{address}",
            object.name
        ));
        cell_ids.push(id.clone());
        if let Some(range) = merged_range(cell)? {
            merged_ranges.push(range);
        }
        parameters.push(DesignParameter {
            id,
            owner: Some(feature_id(object)),
            ordinal: index as u32,
            name: name.to_owned(),
            expression: content.to_owned(),
            display: None,
            value: (!content.starts_with('='))
                .then(|| content.parse::<f64>().ok().map(ParameterValue::Real))
                .flatten(),
            dependencies: Vec::new(),
            properties: retained,
            pmi: None,
            native_ref: Some(property.id.clone()),
        });
    }
    Ok(Spreadsheet {
        id: SpreadsheetId(format!("fcstd:design:spreadsheet#{}", object.name)),
        feature: feature_id(object),
        cells: cell_ids,
        column_widths: spreadsheet_dimensions(
            properties,
            "PropertyColumnWidths",
            "ColumnInfo",
            "Column",
            "width",
        )?,
        row_heights: spreadsheet_dimensions(
            properties,
            "PropertyRowHeights",
            "RowInfo",
            "Row",
            "height",
        )?,
        merged_ranges,
        native_ref: Some(object.id.clone()),
    })
}

fn spreadsheet_dimensions(
    properties: &[&PropertyRecord],
    type_name: &str,
    container: &str,
    element: &str,
    value_name: &str,
) -> Result<Vec<SpreadsheetDimension>, CodecError> {
    let Some(property) = properties
        .iter()
        .copied()
        .find(|property| property.type_name.contains(type_name))
    else {
        return Ok(Vec::new());
    };
    let xml = roxmltree::Document::parse(&property.raw_xml).map_err(|error| {
        CodecError::Malformed(format!(
            "invalid spreadsheet dimension {}: {error}",
            property.id
        ))
    })?;
    let root = xml
        .descendants()
        .find(|node| node.has_tag_name(container))
        .ok_or_else(|| {
            CodecError::Malformed(format!("{} has no dimension container", property.id))
        })?;
    let records = root
        .children()
        .filter(|node| node.has_tag_name(element))
        .collect::<Vec<_>>();
    let declared = root
        .attribute("Count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| {
            CodecError::Malformed(format!("{} has invalid dimension count", property.id))
        })?;
    if declared != records.len() || declared > MAX_SKETCH_RECORDS {
        return Err(CodecError::Malformed(format!(
            "{} dimension count does not match its records",
            property.id
        )));
    }
    records
        .into_iter()
        .map(|record| {
            let name = record.attribute("name").ok_or_else(|| {
                CodecError::Malformed(format!("{} dimension has no name", property.id))
            })?;
            let pixels = record
                .attribute(value_name)
                .and_then(|value| value.parse::<u32>().ok())
                .ok_or_else(|| {
                    CodecError::Malformed(format!("{} dimension has invalid size", property.id))
                })?;
            Ok(SpreadsheetDimension {
                name: name.to_owned(),
                pixels,
            })
        })
        .collect()
}

fn merged_range(cell: roxmltree::Node<'_, '_>) -> Result<Option<SpreadsheetRange>, CodecError> {
    let rows = cell
        .attribute("rowSpan")
        .map_or(Ok(1_u32), str::parse::<u32>)
        .map_err(|_| CodecError::Malformed("spreadsheet cell has invalid row span".into()))?;
    let columns = cell
        .attribute("colSpan")
        .map_or(Ok(1_u32), str::parse::<u32>)
        .map_err(|_| CodecError::Malformed("spreadsheet cell has invalid column span".into()))?;
    if rows == 0 || columns == 0 {
        return Err(CodecError::Malformed(
            "spreadsheet cell has a zero span".into(),
        ));
    }
    if rows == 1 && columns == 1 {
        return Ok(None);
    }
    let start = cell
        .attribute("address")
        .ok_or_else(|| CodecError::Malformed("spreadsheet cell has no address".into()))?;
    let end = offset_cell_address(start, rows - 1, columns - 1)
        .ok_or_else(|| CodecError::Malformed("spreadsheet cell span is out of range".into()))?;
    Ok(Some(SpreadsheetRange {
        start: start.to_owned(),
        end,
    }))
}

fn offset_cell_address(address: &str, rows: u32, columns: u32) -> Option<String> {
    let split = address.find(|character: char| character.is_ascii_digit())?;
    let mut column = address[..split].bytes().try_fold(0_u32, |value, byte| {
        byte.is_ascii_uppercase().then(|| {
            value
                .checked_mul(26)?
                .checked_add(u32::from(byte - b'A' + 1))
        })?
    })?;
    let row = address[split..].parse::<u32>().ok()?.checked_add(rows)?;
    column = column.checked_add(columns)?;
    if row == 0 || column == 0 {
        return None;
    }
    let mut label = Vec::new();
    while column > 0 {
        column -= 1;
        label.push(b'A' + (column % 26) as u8);
        column /= 26;
    }
    label.reverse();
    Some(format!("{}{row}", String::from_utf8(label).ok()?))
}

fn append_operation_parameters(
    parameters: &mut Vec<DesignParameter>,
    object: &ObjectRecord,
    properties: &[&PropertyRecord],
) {
    const NAMES: &[&str] = &[
        "Angle",
        "Angle2",
        "Radius",
        "Size",
        "Size2",
        "Length",
        "Length2",
        "Value",
        "Diameter",
        "Depth",
        "HoleCutDiameter",
        "HoleCutDepth",
        "HoleCutCountersinkAngle",
        "DrillPointAngle",
        "TaperedAngle",
        "ThreadPitch",
        "ThreadDiameter",
        "ThreadDepth",
        "CustomThreadClearance",
    ];
    for property in properties
        .iter()
        .copied()
        .filter(|property| NAMES.contains(&property.name.as_str()))
    {
        if parameters.iter().any(|parameter| {
            parameter.owner.as_ref() == Some(&feature_id(object)) && parameter.name == property.name
        }) {
            continue;
        }
        let Some(value) = scalar_value(property) else {
            continue;
        };
        let expression = expression_binding(properties, &property.name);
        let is_angle = property.type_name.contains("Angle");
        let mut retained = BTreeMap::new();
        if let Some((native_ref, _)) = &expression {
            retained.insert("expression_native_ref".into(), native_ref.clone());
        }
        parameters.push(DesignParameter {
            id: ParameterId(format!(
                "fcstd:design:parameter#{}:{}",
                object.name, property.name
            )),
            owner: Some(feature_id(object)),
            ordinal: property.order as u32,
            name: property.name.clone(),
            expression: expression.map_or_else(
                || scalar_text(property).unwrap_or_else(|| value.to_string()),
                |(_, expression)| expression,
            ),
            display: None,
            value: Some(if is_angle {
                ParameterValue::Angle(cadmpeg_ir::features::Angle(value.to_radians()))
            } else {
                ParameterValue::Length(Length(value))
            }),
            dependencies: Vec::new(),
            properties: retained,
            pmi: None,
            native_ref: Some(property.id.clone()),
        });
    }
}

struct SketchTransfer {
    sketch: Sketch,
    entities: Vec<SketchEntity>,
    constraints: Vec<SketchConstraint>,
    parameters: Vec<DesignParameter>,
}

fn parse_sketch(
    object: &ObjectRecord,
    properties: &[&PropertyRecord],
) -> Result<SketchTransfer, CodecError> {
    let id = SketchId(format!("fcstd:design:sketch#{}", object.name));
    let mut entities = Vec::new();
    if let Some(geometry) = property(properties, "Geometry") {
        let xml = roxmltree::Document::parse(&geometry.raw_xml).map_err(|error| {
            CodecError::Malformed(format!("invalid sketch geometry {}: {error}", geometry.id))
        })?;
        validate_declared_count(&xml, "GeometryList", "Geometry", &geometry.id)?;
        for (index, node) in xml
            .descendants()
            .filter(|node| node.has_tag_name("Geometry"))
            .enumerate()
        {
            let carrier = node.children().find(|child| {
                child.is_element()
                    && !matches!(child.tag_name().name(), "Construction" | "GeoExtensions")
            });
            let native_kind = node
                .attribute("type")
                .or_else(|| carrier.map(|child| child.tag_name().name()))
                .unwrap_or("unknown")
                .to_owned();
            let attributes = carrier.map_or_else(BTreeMap::new, |child| {
                child
                    .attributes()
                    .map(|attribute| (attribute.name().to_owned(), attribute.value().to_owned()))
                    .collect()
            });
            let geometry_value = carrier
                .and_then(|carrier| sketch_nurbs(&native_kind, carrier))
                .unwrap_or_else(|| sketch_geometry(&native_kind, &attributes));
            entities.push(SketchEntity {
                id: SketchEntityId(format!(
                    "fcstd:design:sketch-entity#{}:{}",
                    object.name,
                    index + 1
                )),
                sketch: id.clone(),
                construction: node.descendants().any(|child| {
                    child.has_tag_name("Construction")
                        && child.attribute("value").is_some_and(|value| value != "0")
                }),
                native_ref: Some(geometry.id.clone()),
                geometry_ref: None,
                endpoint_refs: Vec::new(),
                geometry: geometry_value,
            });
        }
    }
    if let Some(external_geometry) = property(properties, "ExternalGeo") {
        let xml = roxmltree::Document::parse(&external_geometry.raw_xml).map_err(|error| {
            CodecError::Malformed(format!(
                "invalid external sketch geometry {}: {error}",
                external_geometry.id
            ))
        })?;
        validate_declared_count(&xml, "GeometryList", "Geometry", &external_geometry.id)?;
        let references = property(properties, "ExternalGeometry");
        for (external_index, node) in xml
            .descendants()
            .filter(|node| node.has_tag_name("Geometry"))
            .skip(2)
            .enumerate()
        {
            let carrier = node.children().find(|child| {
                child.is_element()
                    && !matches!(child.tag_name().name(), "Construction" | "GeoExtensions")
            });
            let native_kind = node
                .attribute("type")
                .or_else(|| carrier.map(|child| child.tag_name().name()))
                .unwrap_or("unknown")
                .to_owned();
            let attributes = carrier.map_or_else(BTreeMap::new, |child| {
                child
                    .attributes()
                    .map(|attribute| (attribute.name().to_owned(), attribute.value().to_owned()))
                    .collect()
            });
            let geometry = carrier
                .and_then(|carrier| sketch_nurbs(&native_kind, carrier))
                .unwrap_or_else(|| sketch_geometry(&native_kind, &attributes));
            let reference = references.and_then(|property| property.links.get(external_index));
            entities.push(SketchEntity {
                id: SketchEntityId(format!(
                    "fcstd:design:sketch-entity#{}:external:{external_index}",
                    object.name
                )),
                sketch: id.clone(),
                construction: true,
                native_ref: Some(external_geometry.id.clone()),
                geometry_ref: references.map(|property| property.id.clone()),
                endpoint_refs: reference
                    .map(|reference| reference.subelements.clone())
                    .unwrap_or_default(),
                geometry,
            });
        }
    }
    let (horizontal_axis, vertical_axis, root_point) = builtin_reference_usage(properties);
    if horizontal_axis {
        entities.push(SketchEntity {
            id: SketchEntityId(format!(
                "fcstd:design:sketch-entity#{}:reference-horizontal-axis",
                object.name
            )),
            sketch: id.clone(),
            construction: true,
            native_ref: Some(object.id.clone()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::ReferenceLine {
                origin: Point2::new(0.0, 0.0),
                direction: Point2::new(1.0, 0.0),
            },
        });
    }
    if vertical_axis {
        entities.push(SketchEntity {
            id: SketchEntityId(format!(
                "fcstd:design:sketch-entity#{}:reference-vertical-axis",
                object.name
            )),
            sketch: id.clone(),
            construction: true,
            native_ref: Some(object.id.clone()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::ReferenceLine {
                origin: Point2::new(0.0, 0.0),
                direction: Point2::new(0.0, 1.0),
            },
        });
    }
    if root_point {
        entities.push(SketchEntity {
            id: SketchEntityId(format!(
                "fcstd:design:sketch-entity#{}:reference-root-point",
                object.name
            )),
            sketch: id.clone(),
            construction: true,
            native_ref: Some(object.id.clone()),
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry: SketchGeometry::Point {
                position: Point2::new(0.0, 0.0),
            },
        });
    }
    let profiles = build_profiles(&entities);
    let (constraints, parameters) = parse_constraints(object, properties, &id, &entities)?;
    let (origin, normal, u_axis) = sketch_frame(properties);
    Ok(SketchTransfer {
        sketch: Sketch {
            id,
            name: Some(object.name.clone()),
            configuration: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::Resolved {
                origin,
                normal,
                u_axis,
            },
            profiles,
            native_ref: Some(object.id.clone()),
        },
        entities,
        constraints,
        parameters,
    })
}

fn builtin_reference_usage(properties: &[&PropertyRecord]) -> (bool, bool, bool) {
    let Some(property) = property(properties, "Constraints") else {
        return (false, false, false);
    };
    let Ok(xml) = roxmltree::Document::parse(&property.raw_xml) else {
        return (false, false, false);
    };
    let mut horizontal = false;
    let mut vertical = false;
    let mut root = false;
    for node in xml
        .descendants()
        .filter(|node| node.has_tag_name("Constrain"))
    {
        let Ok(operands) = constraint_operands(node) else {
            continue;
        };
        for (entity, position) in operands {
            horizontal |= entity == -1 && position == 0;
            root |= entity == -1 && position == 1;
            vertical |= entity == -2 && position == 0;
        }
    }
    (horizontal, vertical, root)
}

fn sketch_nurbs(kind: &str, node: roxmltree::Node<'_, '_>) -> Option<SketchGeometry> {
    if !kind.contains("BSpline") && !node.has_tag_name("BSplineCurve") {
        return None;
    }
    let degree = node.attribute("Degree")?.parse::<u32>().ok()?;
    let periodic = matches!(node.attribute("IsPeriodic")?, "1" | "true" | "True");
    let pole_count = node.attribute("PolesCount")?.parse::<usize>().ok()?;
    let knot_count = node.attribute("KnotsCount")?.parse::<usize>().ok()?;
    if pole_count == 0
        || knot_count == 0
        || pole_count > MAX_SKETCH_RECORDS
        || knot_count > MAX_SKETCH_RECORDS
    {
        return None;
    }
    let poles = node
        .children()
        .filter(|child| child.has_tag_name("Pole"))
        .map(|pole| {
            Some((
                Point2::new(
                    pole.attribute("X")?.parse().ok()?,
                    pole.attribute("Y")?.parse().ok()?,
                ),
                pole.attribute("Z")?.parse::<f64>().ok()?,
                pole.attribute("Weight")?.parse::<f64>().ok()?,
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    let knots = node
        .children()
        .filter(|child| child.has_tag_name("Knot"))
        .map(|knot| {
            Some((
                knot.attribute("Value")?.parse::<f64>().ok()?,
                knot.attribute("Mult")?.parse::<usize>().ok()?,
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    if poles.len() != pole_count
        || knots.len() != knot_count
        || degree == 0
        || usize::try_from(degree)
            .ok()
            .is_none_or(|degree| degree >= pole_count)
        || poles.iter().any(|(point, z, weight)| {
            !point.u.is_finite()
                || !point.v.is_finite()
                || !z.is_finite()
                || z.abs() > f64::EPSILON
                || !weight.is_finite()
                || *weight <= 0.0
        })
        || knots.iter().any(|(value, multiplicity)| {
            !value.is_finite() || *multiplicity == 0 || *multiplicity > MAX_SKETCH_RECORDS
        })
        || knots.windows(2).any(|pair| pair[0].0 >= pair[1].0)
    {
        return None;
    }
    let expanded_count = knots.iter().try_fold(0_usize, |count, (_, multiplicity)| {
        count.checked_add(*multiplicity)
    })?;
    if expanded_count > MAX_SKETCH_RECORDS {
        return None;
    }
    if !periodic
        && expanded_count
            != pole_count
                .checked_add(usize::try_from(degree).ok()?)?
                .checked_add(1)?
    {
        return None;
    }
    let full_knots = knots
        .iter()
        .flat_map(|(value, multiplicity)| std::iter::repeat_n(*value, *multiplicity))
        .collect();
    let control_points = poles.iter().map(|(point, _, _)| *point).collect();
    let weights = poles
        .iter()
        .map(|(_, _, weight)| *weight)
        .collect::<Vec<_>>();
    Some(SketchGeometry::Nurbs {
        degree,
        knots: full_knots,
        control_points,
        weights: weights
            .iter()
            .any(|weight| (*weight - 1.0).abs() > f64::EPSILON)
            .then_some(weights),
        periodic,
    })
}

fn sketch_frame(properties: &[&PropertyRecord]) -> (Point3, Vector3, Vector3) {
    placement_frame(properties).map_or_else(
        || {
            (
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
        },
        |(origin, normal, x_axis, _)| (origin, normal, x_axis),
    )
}

fn placement_frame(properties: &[&PropertyRecord]) -> Option<(Point3, Vector3, Vector3, Vector3)> {
    let value = property(properties, "Placement")
        .or_else(|| property(properties, "AttachmentOffset"))
        .and_then(|property| {
            property
                .values
                .iter()
                .find(|value| value.tag == "PropertyPlacement")
        })?;
    let component = |name: &str, default: f64| {
        value
            .attributes
            .get(name)
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let quaternion = [
        component("Q0", 0.0),
        component("Q1", 0.0),
        component("Q2", 0.0),
        component("Q3", 1.0),
    ];
    Some((
        Point3::new(
            component("Px", 0.0),
            component("Py", 0.0),
            component("Pz", 0.0),
        ),
        rotate_vector(quaternion, [0.0, 0.0, 1.0]),
        rotate_vector(quaternion, [1.0, 0.0, 0.0]),
        rotate_vector(quaternion, [0.0, 1.0, 0.0]),
    ))
}

fn rotate_vector(quaternion: [f64; 4], vector: [f64; 3]) -> Vector3 {
    let [x, y, z, w] = quaternion;
    let norm = (x * x + y * y + z * z + w * w).sqrt();
    if norm <= f64::EPSILON {
        return Vector3::new(vector[0], vector[1], vector[2]);
    }
    let (x, y, z, w) = (x / norm, y / norm, z / norm, w / norm);
    let [vx, vy, vz] = vector;
    Vector3::new(
        (1.0 - 2.0 * (y * y + z * z)) * vx
            + 2.0 * (x * y - z * w) * vy
            + 2.0 * (x * z + y * w) * vz,
        2.0 * (x * y + z * w) * vx
            + (1.0 - 2.0 * (x * x + z * z)) * vy
            + 2.0 * (y * z - x * w) * vz,
        2.0 * (x * z - y * w) * vx
            + 2.0 * (y * z + x * w) * vy
            + (1.0 - 2.0 * (x * x + y * y)) * vz,
    )
}

fn feature_state(properties: &[&PropertyRecord]) -> BTreeMap<String, String> {
    const STATE_NAMES: &[&str] = &[
        "Active",
        "Frozen",
        "Invalid",
        "MapMode",
        "Support",
        "Suppressed",
        "Tip",
        "Touched",
        "Visibility",
    ];
    properties
        .iter()
        .filter(|property| STATE_NAMES.contains(&property.name.as_str()))
        .map(|property| {
            let value = property
                .links
                .first()
                .and_then(|link| link.object.clone())
                .or_else(|| scalar_text(property))
                .unwrap_or_else(|| property.raw_xml.clone());
            (property.name.clone(), value)
        })
        .collect()
}

fn bool_property(properties: &[&PropertyRecord], name: &str) -> Option<bool> {
    let value = scalar_text(property(properties, name)?)?;
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

fn parse_constraints(
    object: &ObjectRecord,
    properties: &[&PropertyRecord],
    sketch: &SketchId,
    entities: &[SketchEntity],
) -> Result<(Vec<SketchConstraint>, Vec<DesignParameter>), CodecError> {
    let Some(property) = property(properties, "Constraints") else {
        return Ok((Vec::new(), Vec::new()));
    };
    let xml = roxmltree::Document::parse(&property.raw_xml).map_err(|error| {
        CodecError::Malformed(format!(
            "invalid sketch constraints {}: {error}",
            property.id
        ))
    })?;
    validate_declared_count(&xml, "ConstraintList", "Constrain", &property.id)?;
    let mut constraints = Vec::new();
    let mut parameters = Vec::new();
    for (index, node) in xml
        .descendants()
        .filter(|node| node.has_tag_name("Constrain"))
        .enumerate()
    {
        let type_code = int_attr(node, "Type").unwrap_or(0);
        let operands = constraint_operands(node).map_err(|message| {
            CodecError::Malformed(format!(
                "{} constraint {}: {message}",
                property.id,
                index + 1
            ))
        })?;
        let resolved = operands
            .iter()
            .filter_map(|(entity, position)| resolve_operand(*entity, *position, entities))
            .collect::<Vec<_>>();
        let all_resolved = resolved.len() == operands.len();
        let parameter = if matches!(type_code, 6..=9 | 11 | 16 | 18 | 19) {
            node.attribute("Value")
                .and_then(|value| value.parse::<f64>().ok())
                .map(|value| {
                    let id = ParameterId(format!(
                        "fcstd:design:parameter#{}:constraint:{}",
                        object.name,
                        index + 1
                    ));
                    let value = match type_code {
                        9 => ParameterValue::Angle(cadmpeg_ir::features::Angle(value)),
                        16 | 19 => ParameterValue::Real(value),
                        _ => ParameterValue::Length(Length(value)),
                    };
                    let path = format!("Constraints[{index}]");
                    let expression = expression_binding(properties, &path);
                    let mut parameter_properties = [(
                        "is_driving".into(),
                        node.attribute("IsDriving").unwrap_or("1").to_owned(),
                    )]
                    .into_iter()
                    .collect::<BTreeMap<_, _>>();
                    if let Some((native_ref, _)) = &expression {
                        parameter_properties
                            .insert("expression_native_ref".into(), native_ref.clone());
                    }
                    parameters.push(DesignParameter {
                        id: id.clone(),
                        owner: Some(feature_id(object)),
                        ordinal: index as u32,
                        name: node
                            .attribute("Name")
                            .filter(|name| !name.is_empty())
                            .map_or_else(|| format!("Constraint{}", index + 1), str::to_owned),
                        expression: expression.map_or_else(
                            || node.attribute("Value").unwrap_or_default().to_owned(),
                            |(_, expression)| expression,
                        ),
                        display: None,
                        value: Some(value),
                        dependencies: Vec::new(),
                        properties: parameter_properties,
                        pmi: None,
                        native_ref: Some(property.id.clone()),
                    });
                    id
                })
        } else {
            None
        };
        let internal_alignment = || {
            use cadmpeg_ir::sketches::SketchInternalAlignment as Alignment;
            let alignment = match int_attr(node, "InternalAlignmentType")? {
                1 => Alignment::EllipseMajorDiameter,
                2 => Alignment::EllipseMinorDiameter,
                3 => Alignment::EllipseFocus1,
                4 => Alignment::EllipseFocus2,
                5 => Alignment::HyperbolaMajor,
                6 => Alignment::HyperbolaMinor,
                7 => Alignment::HyperbolaFocus,
                8 => Alignment::ParabolaFocus,
                9 => Alignment::BsplineControlPoint,
                10 => Alignment::BsplineKnotPoint,
                11 => Alignment::ParabolaFocalAxis,
                _ => return None,
            };
            Some(SketchConstraintDefinition::InternalAlignment {
                helper: locus_entity(resolved.first()?).clone(),
                parent: locus_entity(resolved.get(1)?).clone(),
                alignment,
                index: node
                    .attribute("InternalAlignmentIndex")
                    .and_then(|value| value.parse::<u32>().ok()),
            })
        };
        let grouped_geometry = || {
            if !all_resolved || resolved.is_empty() {
                return None;
            }
            match type_code {
                20 => Some(SketchConstraintDefinition::Group {
                    elements: resolved.clone(),
                }),
                21 => {
                    let metadata = node.attribute("MetaData")?;
                    let metadata: serde_json::Value = serde_json::from_str(metadata).ok()?;
                    Some(SketchConstraintDefinition::Text {
                        elements: resolved.clone(),
                        text: metadata.get("text")?.as_str()?.to_owned(),
                        font: metadata
                            .get("font")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned),
                        is_text_height: metadata
                            .get("isTextHeight")
                            .and_then(serde_json::Value::as_bool)
                            .unwrap_or(true),
                    })
                }
                _ => None,
            }
        };
        let definition = (type_code == 15 && all_resolved)
            .then(internal_alignment)
            .flatten()
            .or_else(grouped_geometry)
            .or_else(|| neutral_constraint(type_code, &resolved, parameter.clone(), all_resolved))
            .unwrap_or_else(|| SketchConstraintDefinition::Native {
                native_kind: constraint_kind(type_code).into(),
                native_state: None,
                entities: resolved.iter().map(locus_entity).cloned().collect(),
                parameter,
                operands: operands
                    .iter()
                    .filter_map(|(entity, position)| {
                        if *entity < 0 || resolve_operand(*entity, *position, entities).is_none() {
                            Some(SketchNativeOperand {
                                native_kind: format!("position:{position}"),
                                native_field: None,
                                native_role: None,
                                object_index: u32::try_from(*entity).unwrap_or(u32::MAX),
                                native_ref: None,
                            })
                        } else {
                            None
                        }
                    })
                    .collect(),
            });
        constraints.push(SketchConstraint {
            id: SketchConstraintId(format!(
                "fcstd:design:sketch-constraint#{}:{}",
                object.name,
                index + 1
            )),
            sketch: sketch.clone(),
            definition,
            name: nonempty_attr(node, "Name"),
            driving: bool_attr(node, "IsDriving"),
            active: bool_attr(node, "IsActive"),
            virtual_space: bool_attr(node, "IsInVirtualSpace"),
            visible: bool_attr(node, "IsVisible"),
            orientation: node
                .attribute("Orientation")
                .and_then(|value| value.parse().ok()),
            label_distance: finite_attr(node, "LabelDistance"),
            label_position: finite_attr(node, "LabelPosition"),
            metadata: nonempty_attr(node, "MetaData"),
            native_ref: Some(property.id.clone()),
        });
    }
    Ok((constraints, parameters))
}

fn bool_attr(node: roxmltree::Node<'_, '_>, name: &str) -> Option<bool> {
    match node.attribute(name)?.to_ascii_lowercase().as_str() {
        "1" | "true" => Some(true),
        "0" | "false" => Some(false),
        _ => None,
    }
}

fn finite_attr(node: roxmltree::Node<'_, '_>, name: &str) -> Option<f64> {
    node.attribute(name)
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite())
}

fn nonempty_attr(node: roxmltree::Node<'_, '_>, name: &str) -> Option<String> {
    node.attribute(name)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn expression_binding(properties: &[&PropertyRecord], path: &str) -> Option<(String, String)> {
    let engine = property(properties, "ExpressionEngine")?;
    engine
        .values
        .iter()
        .find(|value| {
            value.tag == "Expression"
                && value
                    .attributes
                    .get("path")
                    .is_some_and(|value| value == path)
        })
        .and_then(|value| {
            Some((
                engine.id.clone(),
                value.attributes.get("expression")?.clone(),
            ))
        })
}

fn bind_parameter_dependencies(parameters: &mut [DesignParameter], objects: &[ObjectRecord]) {
    let object_names = objects
        .iter()
        .map(|object| (feature_id(object), object.name.as_str()))
        .collect::<HashMap<_, _>>();
    let candidates = parameters
        .iter()
        .map(|parameter| {
            (
                parameter.id.clone(),
                parameter.owner.clone(),
                parameter.name.clone(),
            )
        })
        .collect::<Vec<_>>();
    let mut local = HashMap::<(FeatureId, String), ParameterId>::new();
    let mut qualified = HashMap::<String, ParameterId>::new();
    for (id, owner, name) in &candidates {
        let Some(owner) = owner else { continue };
        local.insert((owner.clone(), name.clone()), id.clone());
        if let Some(object) = object_names.get(owner) {
            qualified.insert(format!("{object}.{name}"), id.clone());
        }
    }
    for parameter in parameters {
        let mut dependencies = BTreeSet::new();
        for identifier in expression_identifiers(&parameter.expression) {
            let dependency = qualified.get(identifier).or_else(|| {
                parameter
                    .owner
                    .as_ref()
                    .and_then(|owner| local.get(&(owner.clone(), identifier.to_owned())))
            });
            if let Some(dependency) = dependency.filter(|id| **id != parameter.id) {
                dependencies.insert(dependency.clone());
            }
        }
        parameter.dependencies = dependencies.into_iter().collect();
    }
}

fn expression_identifiers(expression: &str) -> impl Iterator<Item = &str> {
    expression
        .split(|character: char| {
            !character.is_ascii_alphanumeric() && character != '_' && character != '.'
        })
        .filter(|identifier| !identifier.is_empty())
}

fn neutral_constraint(
    kind: i64,
    loci: &[SketchLocus],
    parameter: Option<ParameterId>,
    complete: bool,
) -> Option<SketchConstraintDefinition> {
    if !complete {
        return None;
    }
    let entity = |index| loci.get(index).map(locus_entity).cloned();
    let pair = || Some((entity(0)?, entity(1)?));
    Some(match kind {
        0 => SketchConstraintDefinition::Disabled,
        1 => SketchConstraintDefinition::CoincidentLoci {
            loci: loci.to_vec(),
        },
        2 => SketchConstraintDefinition::Horizontal { entity: entity(0)? },
        3 => SketchConstraintDefinition::Vertical { entity: entity(0)? },
        4 => {
            let (first, second) = pair()?;
            SketchConstraintDefinition::Parallel { first, second }
        }
        5 => {
            let (first, second) = pair()?;
            SketchConstraintDefinition::Tangent { first, second }
        }
        10 => {
            let (first, second) = pair()?;
            SketchConstraintDefinition::Perpendicular { first, second }
        }
        12 => {
            let (first, second) = pair()?;
            SketchConstraintDefinition::Equal { first, second }
        }
        13 => SketchConstraintDefinition::PointOnObject {
            point: loci.first()?.clone(),
            entity: entity(1)?,
        },
        17 => SketchConstraintDefinition::Fixed { entity: entity(0)? },
        6 if loci.len() == 2 => SketchConstraintDefinition::DistanceLoci {
            first: loci[0].clone(),
            second: loci[1].clone(),
            parameter: parameter?,
        },
        6 => SketchConstraintDefinition::Distance {
            entities: loci.iter().map(locus_entity).cloned().collect(),
            parameter: parameter?,
        },
        7 => SketchConstraintDefinition::HorizontalDistance {
            first: loci.first()?.clone(),
            second: loci.get(1)?.clone(),
            parameter: parameter?,
        },
        8 => SketchConstraintDefinition::VerticalDistance {
            first: loci.first()?.clone(),
            second: loci.get(1)?.clone(),
            parameter: parameter?,
        },
        9 => SketchConstraintDefinition::Angle {
            first: entity(0)?,
            second: entity(1)?,
            parameter: parameter?,
        },
        11 => SketchConstraintDefinition::Radius {
            entity: entity(0)?,
            parameter: parameter?,
        },
        18 => SketchConstraintDefinition::Diameter {
            entity: entity(0)?,
            parameter: parameter?,
        },
        16 => SketchConstraintDefinition::SnellsLaw {
            incident: loci.first()?.clone(),
            refracted: loci.get(1)?.clone(),
            interface: entity(2)?,
            parameter: parameter?,
        },
        19 => SketchConstraintDefinition::Weight {
            entity: entity(0)?,
            parameter: parameter?,
        },
        14 => SketchConstraintDefinition::Symmetric {
            first: loci.first()?.clone(),
            second: loci.get(1)?.clone(),
            axis: entity(2)?,
        },
        _ => return None,
    })
}

fn constraint_operands(node: roxmltree::Node<'_, '_>) -> Result<Vec<(i64, i64)>, &'static str> {
    let ids = node
        .attribute("ElementIds")
        .map(split_ints)
        .unwrap_or_default();
    let positions = node
        .attribute("ElementPositions")
        .map(split_ints)
        .unwrap_or_default();
    if node.attribute("ElementIds").is_some() || node.attribute("ElementPositions").is_some() {
        if ids.len() != positions.len() {
            return Err("ElementIds and ElementPositions counts differ");
        }
        return Ok(ids
            .into_iter()
            .zip(positions)
            .filter(|(entity, _)| *entity != -2000)
            .collect());
    }
    Ok(["First", "Second", "Third"]
        .into_iter()
        .zip(["FirstPos", "SecondPos", "ThirdPos"])
        .filter_map(|(entity, position)| Some((int_attr(node, entity)?, int_attr(node, position)?)))
        .filter(|(entity, _)| *entity != -2000)
        .collect())
}

fn validate_declared_count(
    xml: &roxmltree::Document<'_>,
    container_tag: &str,
    record_tag: &str,
    owner: &str,
) -> Result<(), CodecError> {
    let Some(container) = xml
        .descendants()
        .find(|node| node.has_tag_name(container_tag))
    else {
        return Err(CodecError::Malformed(format!(
            "{owner} has no {container_tag} value"
        )));
    };
    let declared = container
        .attribute("count")
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| CodecError::Malformed(format!("{owner} has an invalid record count")))?;
    if declared > MAX_SKETCH_RECORDS {
        return Err(CodecError::Malformed(format!(
            "{owner} record count exceeds {MAX_SKETCH_RECORDS}"
        )));
    }
    let actual = container
        .children()
        .filter(|node| node.has_tag_name(record_tag))
        .count();
    if declared != actual {
        return Err(CodecError::Malformed(format!(
            "{owner} declares {declared} records but contains {actual}"
        )));
    }
    Ok(())
}

fn split_ints(value: &str) -> Vec<i64> {
    value
        .split(|character: char| character == ',' || character.is_ascii_whitespace())
        .filter_map(|part| part.parse().ok())
        .collect()
}

fn int_attr(node: roxmltree::Node<'_, '_>, name: &str) -> Option<i64> {
    node.attribute(name)?.parse().ok()
}

fn resolve_operand(entity: i64, position: i64, entities: &[SketchEntity]) -> Option<SketchLocus> {
    let reference = |suffix: &str| {
        entities
            .iter()
            .find(|candidate| candidate.id.0.ends_with(suffix))
            .map(|candidate| SketchLocus::Entity(candidate.id.clone()))
    };
    match (entity, position) {
        (-1, 0) => return reference(":reference-horizontal-axis"),
        (-1, 1) => return reference(":reference-root-point"),
        (-2, 0) => return reference(":reference-vertical-axis"),
        _ => {}
    }
    if entity <= -3 {
        let external_index = usize::try_from(-entity - 3).ok()?;
        let suffix = format!(":external:{external_index}");
        let id = entities
            .iter()
            .find(|candidate| candidate.id.0.ends_with(&suffix))?
            .id
            .clone();
        return Some(match position {
            0 => SketchLocus::Entity(id),
            1 => SketchLocus::Start(id),
            2 => SketchLocus::End(id),
            3 => SketchLocus::Center(id),
            _ => return None,
        });
    }
    let id = entities.get(usize::try_from(entity).ok()?)?.id.clone();
    Some(match position {
        0 => SketchLocus::Entity(id),
        1 => SketchLocus::Start(id),
        2 => SketchLocus::End(id),
        3 => SketchLocus::Center(id),
        _ => return None,
    })
}

fn locus_entity(locus: &SketchLocus) -> &SketchEntityId {
    match locus {
        SketchLocus::Entity(entity)
        | SketchLocus::Start(entity)
        | SketchLocus::End(entity)
        | SketchLocus::Center(entity) => entity,
    }
}

fn constraint_kind(kind: i64) -> &'static str {
    match kind {
        0 => "none",
        1 => "coincident",
        2 => "horizontal",
        3 => "vertical",
        4 => "parallel",
        5 => "tangent",
        6 => "distance",
        7 => "distance_x",
        8 => "distance_y",
        9 => "angle",
        10 => "perpendicular",
        11 => "radius",
        12 => "equal",
        13 => "point_on_object",
        14 => "symmetric",
        15 => "internal_alignment",
        16 => "snells_law",
        17 => "block",
        18 => "diameter",
        19 => "weight",
        20 => "group",
        21 => "text",
        _ => "unknown_future_constraint",
    }
}

fn sketch_geometry(kind: &str, attributes: &BTreeMap<String, String>) -> SketchGeometry {
    let number = |name: &str| attributes.get(name).and_then(|value| value.parse().ok());
    let native = || SketchGeometry::Native {
        native_kind: kind.to_owned(),
    };
    if kind.contains("Line") {
        match (
            number("StartX"),
            number("StartY"),
            number("EndX"),
            number("EndY"),
        ) {
            (Some(start_x), Some(start_y), Some(end_x), Some(end_y)) => SketchGeometry::Line {
                start: Point2::new(start_x, start_y),
                end: Point2::new(end_x, end_y),
            },
            _ => native(),
        }
    } else if kind.contains("Ellipse") {
        let major_angle = number("MajorAngle")
            .or_else(|| number("AngleXU"))
            .or_else(|| Some(number("MajorAxisY")?.atan2(number("MajorAxisX")?)));
        let bounds = if kind.contains("Arc") {
            number("StartAngle")
                .or_else(|| number("FirstParameter"))
                .zip(number("EndAngle").or_else(|| number("LastParameter")))
                .map(|(start, end)| (Some(start), Some(end)))
        } else {
            Some((None, None))
        };
        match (
            number("CenterX"),
            number("CenterY"),
            major_angle,
            number("MajorRadius"),
            number("MinorRadius"),
            bounds,
        ) {
            (Some(x), Some(y), Some(angle), Some(major), Some(minor), Some((start, end)))
                if major > 0.0 && minor > 0.0 =>
            {
                SketchGeometry::Ellipse {
                    center: Point2::new(x, y),
                    major_angle: cadmpeg_ir::features::Angle(angle),
                    major_radius: Length(major),
                    minor_radius: Length(minor),
                    start_angle: start.map(cadmpeg_ir::features::Angle),
                    end_angle: end.map(cadmpeg_ir::features::Angle),
                }
            }
            _ => native(),
        }
    } else if kind.contains("Hyperbola") {
        let bounds = if kind.contains("Arc") {
            number("StartAngle")
                .or_else(|| number("FirstParameter"))
                .zip(number("EndAngle").or_else(|| number("LastParameter")))
                .map(|(start, end)| (Some(start), Some(end)))
        } else {
            Some((None, None))
        };
        match (
            number("CenterX"),
            number("CenterY"),
            number("AngleXU").or_else(|| number("MajorAngle")),
            number("MajorRadius"),
            number("MinorRadius"),
            bounds,
        ) {
            (Some(x), Some(y), Some(angle), Some(major), Some(minor), Some((start, end)))
                if major > 0.0 && minor > 0.0 =>
            {
                SketchGeometry::Hyperbola {
                    center: Point2::new(x, y),
                    major_angle: cadmpeg_ir::features::Angle(angle),
                    major_radius: Length(major),
                    minor_radius: Length(minor),
                    start_parameter: start,
                    end_parameter: end,
                }
            }
            _ => native(),
        }
    } else if kind.contains("Parabola") {
        let bounds = if kind.contains("Arc") {
            number("StartAngle")
                .or_else(|| number("FirstParameter"))
                .zip(number("EndAngle").or_else(|| number("LastParameter")))
                .map(|(start, end)| (Some(start), Some(end)))
        } else {
            Some((None, None))
        };
        match (
            number("CenterX"),
            number("CenterY"),
            number("AngleXU").or_else(|| number("AxisAngle")),
            number("Focal"),
            bounds,
        ) {
            (Some(x), Some(y), Some(angle), Some(focal), Some((start, end))) if focal > 0.0 => {
                SketchGeometry::Parabola {
                    vertex: Point2::new(x, y),
                    axis_angle: cadmpeg_ir::features::Angle(angle),
                    focal_length: Length(focal),
                    start_parameter: start,
                    end_parameter: end,
                }
            }
            _ => native(),
        }
    } else if kind.contains("Arc") {
        match (
            number("CenterX"),
            number("CenterY"),
            number("Radius"),
            number("StartAngle").or_else(|| number("FirstParameter")),
            number("EndAngle").or_else(|| number("LastParameter")),
        ) {
            (Some(x), Some(y), Some(radius), Some(start), Some(end)) if radius > 0.0 => {
                SketchGeometry::Arc {
                    center: Point2::new(x, y),
                    radius: Length(radius),
                    start_angle: cadmpeg_ir::features::Angle(start),
                    end_angle: cadmpeg_ir::features::Angle(end),
                }
            }
            _ => native(),
        }
    } else if kind.contains("Circle") {
        match (number("CenterX"), number("CenterY"), number("Radius")) {
            (Some(x), Some(y), Some(radius)) if radius > 0.0 => SketchGeometry::Circle {
                center: Point2::new(x, y),
                radius: Length(radius),
            },
            _ => native(),
        }
    } else if kind.contains("Point") {
        match (number("X"), number("Y")) {
            (Some(x), Some(y)) => SketchGeometry::Point {
                position: Point2::new(x, y),
            },
            _ => native(),
        }
    } else {
        native()
    }
}

fn build_profiles(entities: &[SketchEntity]) -> Vec<Vec<SketchEntityUse>> {
    let mut unused = entities
        .iter()
        .filter(|entity| !entity.construction)
        .map(|entity| entity.id.clone())
        .collect::<BTreeSet<_>>();
    let by_id = entities
        .iter()
        .map(|entity| (entity.id.clone(), entity))
        .collect::<HashMap<_, _>>();
    let mut profiles = Vec::new();
    while let Some(first) = unused.iter().next().cloned() {
        unused.remove(&first);
        let mut chain = vec![SketchEntityUse {
            entity: first.clone(),
            reversed: false,
        }];
        let mut end = endpoints(by_id[&first]).map(|(_, end)| end);
        while let Some(point) = end {
            let next = unused.iter().find_map(|id| {
                let (start, finish) = endpoints(by_id[id])?;
                if near(point, start) {
                    Some((id.clone(), false, finish))
                } else if near(point, finish) {
                    Some((id.clone(), true, start))
                } else {
                    None
                }
            });
            let Some((id, reversed, next_end)) = next else {
                break;
            };
            unused.remove(&id);
            chain.push(SketchEntityUse {
                entity: id,
                reversed,
            });
            end = Some(next_end);
        }
        profiles.push(chain);
    }
    profiles
}

fn endpoints(entity: &SketchEntity) -> Option<(Point2, Point2)> {
    match entity.geometry {
        SketchGeometry::Line { start, end } => Some((start, end)),
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Some((
            Point2::new(
                center.u + radius.0 * start_angle.0.cos(),
                center.v + radius.0 * start_angle.0.sin(),
            ),
            Point2::new(
                center.u + radius.0 * end_angle.0.cos(),
                center.v + radius.0 * end_angle.0.sin(),
            ),
        )),
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            start_angle: Some(start),
            end_angle: Some(end),
        } => {
            let point = |parameter: f64| {
                let major = Point2::new(major_angle.0.cos(), major_angle.0.sin());
                let minor = Point2::new(-major.v, major.u);
                Point2::new(
                    center.u
                        + major_radius.0 * parameter.cos() * major.u
                        + minor_radius.0 * parameter.sin() * minor.u,
                    center.v
                        + major_radius.0 * parameter.cos() * major.v
                        + minor_radius.0 * parameter.sin() * minor.v,
                )
            };
            Some((point(start.0), point(end.0)))
        }
        _ => None,
    }
}

fn near(a: Point2, b: Point2) -> bool {
    (a.u - b.u).abs() <= 1e-9 && (a.v - b.v).abs() <= 1e-9
}

#[cfg(test)]
mod profile_tests {
    use super::*;

    fn entity(id: &str, geometry: SketchGeometry) -> SketchEntity {
        SketchEntity {
            id: cadmpeg_ir::sketches::SketchEntityId(id.into()),
            sketch: SketchId("test:sketch#curved".into()),
            construction: false,
            native_ref: None,
            geometry_ref: None,
            endpoint_refs: Vec::new(),
            geometry,
        }
    }

    #[test]
    fn curved_segments_chain_by_their_evaluated_endpoints() {
        let entities = [
            entity(
                "test:entity#line",
                SketchGeometry::Line {
                    start: Point2::new(0.0, 1.0),
                    end: Point2::new(0.0, 0.0),
                },
            ),
            entity(
                "test:entity#arc",
                SketchGeometry::Arc {
                    center: Point2::new(0.0, 0.0),
                    radius: Length(1.0),
                    start_angle: cadmpeg_ir::features::Angle(0.0),
                    end_angle: cadmpeg_ir::features::Angle(std::f64::consts::FRAC_PI_2),
                },
            ),
        ];
        let profiles = build_profiles(&entities);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].len(), 2);
    }
}

fn profile_ref(
    owner: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
) -> ProfileRef {
    let property_and_target = ["Profile", "Base", "Source"].iter().find_map(|name| {
        let property = property(properties, name)?;
        let target = property
            .links
            .iter()
            .find_map(|link| link.object.as_deref())?;
        (!target.is_empty()).then_some((property, target))
    });
    let Some((property, target)) = property_and_target else {
        return ProfileRef::Unresolved(owner.to_owned());
    };
    sketches.get(target).cloned().map_or_else(
        || ProfileRef::Native(property.id.clone()),
        ProfileRef::Sketch,
    )
}

fn revolution_axis(properties: &[&PropertyRecord]) -> Option<RevolutionAxis> {
    Some(RevolutionAxis {
        origin: vector_property(properties, "Base").map_or_else(
            || Point3::new(0.0, 0.0, 0.0),
            |vector| Point3::new(vector.x, vector.y, vector.z),
        ),
        direction: vector_property(properties, "Axis")?,
    })
}

fn revolution_definition(
    kind: &str,
    owner: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
) -> Option<FeatureDefinition> {
    let profile = profile_ref(owner, properties, sketches);
    if matches!(&profile, ProfileRef::Unresolved(_)) {
        return None;
    }
    let mut axis = revolution_axis(properties)?;
    axis.direction = unit_vector(axis.direction)?;
    let angle = || {
        scalar_named(properties, "Angle")
            .filter(|angle| angle.is_finite() && *angle > 0.0)
            .map(|angle| cadmpeg_ir::features::Angle(angle.to_radians()))
    };
    let mode = integer_property(properties, "Type").unwrap_or(0);
    let extent = if kind == "Part::Revolution" {
        let angle = angle()?;
        if bool_property(properties, "Symmetric").unwrap_or(false) {
            Extent::SymmetricAngle { angle }
        } else {
            Extent::Angle { angle }
        }
    } else {
        match mode {
            0 => {
                let angle = angle()?;
                if bool_property(properties, "Midplane").unwrap_or(false) {
                    Extent::SymmetricAngle { angle }
                } else {
                    Extent::Angle { angle }
                }
            }
            1 => Extent::ThroughAll,
            2 => Extent::ToFirst,
            3 => Extent::ToFace {
                face: cadmpeg_ir::features::FaceSelection::Native(
                    property(properties, "UpToFace")?.id.clone(),
                ),
                offset: None,
            },
            4 => Extent::TwoSidedAngles {
                first: angle()?,
                second: cadmpeg_ir::features::Angle(
                    scalar_named(properties, "Angle2")
                        .filter(|angle| angle.is_finite() && *angle > 0.0)?
                        .to_radians(),
                ),
            },
            _ => return None,
        }
    };
    if bool_property(properties, "Reversed").unwrap_or(false) {
        axis.direction = Vector3::new(-axis.direction.x, -axis.direction.y, -axis.direction.z);
    }
    let axis_reference_property = ["AxisLink", "ReferenceAxis"]
        .iter()
        .find_map(|name| property(properties, name))
        .filter(|property| property.links.iter().any(nonempty_link));
    if axis_reference_property.is_some_and(|property| property.links.len() != 1) {
        return None;
    }
    let axis_reference =
        axis_reference_property.map(|property| PathRef::Native(property.id.clone()));
    let face_maker_class =
        if kind == "Part::Revolution" && property(properties, "FaceMakerClass").is_some() {
            Some(string_property_value(property(properties, "FaceMakerClass")?)?.to_owned())
        } else {
            None
        };
    let fuse_order =
        if kind.starts_with("PartDesign::") && property(properties, "FuseOrder").is_some() {
            Some(match integer_property(properties, "FuseOrder")? {
                0 => RevolutionFuseOrder::BaseFirst,
                1 => RevolutionFuseOrder::FeatureFirst,
                _ => return None,
            })
        } else {
            None
        };
    Some(FeatureDefinition::Revolve {
        construction: RevolutionConstruction {
            profile: Some(profile),
            axis: Some(axis),
            extent: Some(extent),
            axis_reference,
            solid: Some(if kind == "Part::Revolution" {
                if property(properties, "Solid").is_some() {
                    bool_property(properties, "Solid")?
                } else {
                    false
                }
            } else {
                true
            }),
            face_maker_class,
            fuse_order,
            allow_multi_profile_faces: if property(properties, "AllowMultiFace").is_some() {
                Some(bool_property(properties, "AllowMultiFace")?)
            } else {
                None
            },
        },
        op: if kind == "Part::Revolution" {
            BooleanOp::NewBody
        } else if kind.contains("Groove") {
            BooleanOp::Cut
        } else {
            BooleanOp::Join
        },
    })
}

fn vector_property(properties: &[&PropertyRecord], name: &str) -> Option<Vector3> {
    let value = property(properties, name)?.values.iter().find(|value| {
        value.attributes.contains_key("x")
            || value.attributes.contains_key("X")
            || value.attributes.contains_key("valueX")
    })?;
    let component = |lower: &str, upper: &str, property_name: &str| {
        value
            .attributes
            .get(lower)
            .or_else(|| value.attributes.get(upper))
            .or_else(|| value.attributes.get(property_name))?
            .parse::<f64>()
            .ok()
    };
    Some(Vector3::new(
        component("x", "X", "valueX")?,
        component("y", "Y", "valueY")?,
        component("z", "Z", "valueZ")?,
    ))
}

fn vector_list_property(properties: &[&PropertyRecord], name: &str) -> Option<Vec<Point3>> {
    let property = property(properties, name)?;
    property
        .values
        .iter()
        .filter(|value| value.attributes.contains_key("x") || value.attributes.contains_key("X"))
        .map(|value| {
            let component = |lower: &str, upper: &str| {
                value
                    .attributes
                    .get(lower)
                    .or_else(|| value.attributes.get(upper))?
                    .parse::<f64>()
                    .ok()
                    .filter(|component| component.is_finite())
            };
            Some(Point3::new(
                component("x", "X")?,
                component("y", "Y")?,
                component("z", "Z")?,
            ))
        })
        .collect()
}

fn part_construction_geometry_definition(
    kind: &str,
    properties: &[&PropertyRecord],
) -> Option<FeatureDefinition> {
    let point = |x: &str, y: &str, z: &str| {
        Some(Point3::new(
            scalar_named(properties, x)?,
            scalar_named(properties, y)?,
            scalar_named(properties, z)?,
        ))
    };
    let angle = |name: &str| {
        scalar_named(properties, name)
            .filter(|value| value.is_finite())
            .map(|value| cadmpeg_ir::features::Angle(value.to_radians()))
    };
    match kind {
        "Part::Vertex" => Some(FeatureDefinition::PointGeometry {
            position: point("X", "Y", "Z")?,
        }),
        "Part::Line" => Some(FeatureDefinition::LineSegment {
            start: point("X1", "Y1", "Z1")?,
            end: point("X2", "Y2", "Z2")?,
        }),
        "Part::Circle" => Some(FeatureDefinition::CircularArc {
            center: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            radius: Length(scalar_named(properties, "Radius").filter(|value| *value > 0.0)?),
            start_angle: angle("Angle1")?,
            end_angle: angle("Angle2")?,
        }),
        "Part::Ellipse" => {
            let major = scalar_named(properties, "MajorRadius").filter(|value| *value > 0.0)?;
            let minor = scalar_named(properties, "MinorRadius")
                .filter(|value| *value > 0.0 && *value <= major)?;
            Some(FeatureDefinition::EllipticArc {
                center: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                major_axis: Vector3::new(1.0, 0.0, 0.0),
                major_radius: Length(major),
                minor_radius: Length(minor),
                start_angle: angle("Angle1")?,
                end_angle: angle("Angle2")?,
            })
        }
        "Part::Polygon" => {
            let points = vector_list_property(properties, "Nodes")?;
            let closed = bool_property(properties, "Close").unwrap_or(false);
            if points.len() < 2 || (closed && points.len() < 3) {
                return None;
            }
            Some(FeatureDefinition::Polyline { points, closed })
        }
        "Part::RegularPolygon" => Some(FeatureDefinition::RegularPolygonCurve {
            sides: u32::try_from(integer_property(properties, "Polygon")?)
                .ok()
                .filter(|value| *value >= 3)?,
            circumradius: Length(
                scalar_named(properties, "Circumradius").filter(|value| *value > 0.0)?,
            ),
        }),
        "Part::Plane" => Some(FeatureDefinition::PlanarPatch {
            length: Length(scalar_named(properties, "Length").filter(|value| *value > 0.0)?),
            width: Length(scalar_named(properties, "Width").filter(|value| *value > 0.0)?),
        }),
        "Part::Face" => {
            let sources = property(properties, "Sources")?;
            if sources.links.is_empty() {
                return None;
            }
            Some(FeatureDefinition::FaceFromShapes {
                sources: BodySelection::Native(sources.id.clone()),
                face_maker_class: string_property_value(property(properties, "FaceMakerClass")?)?
                    .to_owned(),
            })
        }
        _ => None,
    }
}

fn parametric_helix_definition(
    kind: &str,
    properties: &[&PropertyRecord],
) -> Option<FeatureDefinition> {
    let radius = scalar_named(properties, "Radius").filter(|value| *value > 0.0)?;
    let segment_turns = if property(properties, "SegmentLength").is_some() {
        let value = scalar_named(properties, "SegmentLength").filter(|value| *value >= 0.0)?;
        (value > 0.0).then_some(value)
    } else {
        None
    };
    let (pitch, revolutions, clockwise, radial_growth, cone_angle, construction_style) =
        if kind == "Part::Helix" {
            let pitch = scalar_named(properties, "Pitch").filter(|value| *value > 0.0)?;
            let height = scalar_named(properties, "Height").filter(|value| *value > 0.0)?;
            let angle = scalar_named(properties, "Angle").unwrap_or(0.0);
            if !angle.is_finite() || angle.abs() >= 90.0 {
                return None;
            }
            let clockwise = match integer_property(properties, "LocalCoord").unwrap_or(0) {
                0 => false,
                1 => true,
                _ => return None,
            };
            let construction_style = match integer_property(properties, "Style") {
                None => None,
                Some(0) => Some(HelixConstructionStyle::Legacy),
                Some(1) => Some(HelixConstructionStyle::Corrected),
                Some(_) => return None,
            };
            (
                pitch,
                height / pitch,
                clockwise,
                None,
                (angle != 0.0).then_some(cadmpeg_ir::features::Angle(angle.to_radians())),
                construction_style,
            )
        } else {
            let growth = scalar_named(properties, "Growth").filter(|value| *value >= 0.0)?;
            let revolutions = scalar_named(properties, "Rotations").filter(|value| *value > 0.0)?;
            (0.0, revolutions, false, Some(Length(growth)), None, None)
        };
    (revolutions.is_finite() && revolutions > 0.0).then_some(FeatureDefinition::Helix {
        axis_origin: Point3::new(0.0, 0.0, 0.0),
        axis_direction: Vector3::new(0.0, 0.0, 1.0),
        radius: Length(radius),
        pitch: Length(pitch),
        revolutions,
        start_angle: cadmpeg_ir::features::Angle(0.0),
        clockwise,
        radial_growth,
        cone_angle,
        segment_turns,
        construction_style,
    })
}

fn extrusion_definition(
    kind: &str,
    properties: &[&PropertyRecord],
    profile: ProfileRef,
    sketches: &[Sketch],
) -> Option<FeatureDefinition> {
    if kind == "Part::Extrusion" {
        let raw_direction = vector_property(properties, "Dir")?;
        let magnitude = (raw_direction.x * raw_direction.x
            + raw_direction.y * raw_direction.y
            + raw_direction.z * raw_direction.z)
            .sqrt();
        let mut direction = unit_vector(raw_direction)?;
        let mut forward = scalar_named(properties, "LengthFwd").filter(|value| *value >= 0.0)?;
        let reverse = scalar_named(properties, "LengthRev").filter(|value| *value >= 0.0)?;
        if forward == 0.0 && reverse == 0.0 {
            forward = magnitude;
        }
        let symmetric = bool_property(properties, "Symmetric").unwrap_or(false);
        let (extent, reverse_direction) = if symmetric {
            (
                Extent::Symmetric {
                    length: Length((forward > 0.0).then_some(forward)?),
                },
                false,
            )
        } else {
            match (forward > 0.0, reverse > 0.0) {
                (true, false) => (
                    Extent::Blind {
                        length: Length(forward),
                    },
                    false,
                ),
                (false, true) => (
                    Extent::Blind {
                        length: Length(reverse),
                    },
                    true,
                ),
                (true, true) => (
                    Extent::TwoSided {
                        first: Length(forward),
                        second: Length(reverse),
                    },
                    false,
                ),
                (false, false) => return None,
            }
        };
        if reverse_direction {
            direction = Vector3::new(-direction.x, -direction.y, -direction.z);
        }
        let forward_draft = scalar_named(properties, "TaperAngle").unwrap_or(0.0);
        let reverse_draft = scalar_named(properties, "TaperAngleRev").unwrap_or(0.0);
        let (draft, reverse_draft) = match (symmetric, forward > 0.0, reverse > 0.0) {
            (true, _, _) => (forward_draft, forward_draft),
            (false, false, true) => (reverse_draft, 0.0),
            (false, true, true) => (forward_draft, reverse_draft),
            _ => (forward_draft, 0.0),
        };
        let direction_source = match integer_property(properties, "DirMode").unwrap_or(0) {
            0 => ExtrusionDirectionSource::Custom,
            1 => {
                let reference = property(properties, "DirLink")?;
                if reference.links.len() != 1 {
                    return None;
                }
                ExtrusionDirectionSource::Edge {
                    reference: PathRef::Native(reference.id.clone()),
                }
            }
            2 => ExtrusionDirectionSource::ProfileNormal,
            _ => return None,
        };
        let face_maker = if let Some(class_property) = property(properties, "FaceMakerClass") {
            let mode = if property(properties, "FaceMakerMode").is_some() {
                Some(u32::try_from(integer_property(properties, "FaceMakerMode")?).ok()?)
            } else {
                None
            };
            Some(ExtrusionFaceMaker {
                class: string_property_value(class_property)?.to_owned(),
                mode,
            })
        } else {
            None
        };
        let inner_wire_taper = if property(properties, "InnerWireTaper").is_some() {
            Some(match integer_property(properties, "InnerWireTaper")? {
                0 => InnerWireTaper::Inverted,
                1 => InnerWireTaper::SameAsOuter,
                _ => return None,
            })
        } else {
            None
        };
        return Some(FeatureDefinition::Extrude {
            profile,
            direction: cadmpeg_ir::features::ExtrudeDirection::Explicit(direction),
            start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
            extent,
            op: BooleanOp::NewBody,
            draft: (draft != 0.0).then_some(cadmpeg_ir::features::Angle(draft.to_radians())),
            second_draft: (reverse_draft != 0.0)
                .then_some(cadmpeg_ir::features::Angle(reverse_draft.to_radians())),
            direction_source: Some(direction_source),
            solid: Some(bool_property(properties, "Solid").unwrap_or(false)),
            face_maker,
            inner_wire_taper,
            first_offset: None,
            second_offset: None,
            length_along_profile_normal: None,
            allow_multi_profile_faces: None,
        });
    }
    let legacy_two_lengths = property(properties, "SideType").is_none()
        && integer_property(properties, "Type") == Some(4);
    let termination = |side: u8| {
        let suffix = if side == 1 { "" } else { "2" };
        let type_name = format!("Type{suffix}");
        let length_name = format!("Length{suffix}");
        let face_name = format!("UpToFace{suffix}");
        let shape_name = format!("UpToShape{suffix}");
        let termination_type = if legacy_two_lengths {
            0
        } else {
            integer_property(properties, &type_name).unwrap_or(0)
        };
        match termination_type {
            0 => Some(Extent::Blind {
                length: Length(
                    scalar_named(properties, &length_name).filter(|value| *value != 0.0)?,
                ),
            }),
            1 if kind.contains("Pocket") => Some(Extent::ThroughAll),
            1 => Some(Extent::ToLast),
            2 => Some(Extent::ToFirst),
            3 => Some(Extent::ToFace {
                face: cadmpeg_ir::features::FaceSelection::Native(
                    property(properties, &face_name)?.id.clone(),
                ),
                offset: None,
            }),
            5 => Some(Extent::ToShape {
                target: cadmpeg_ir::features::FaceSelection::Native(
                    property(properties, &shape_name)?.id.clone(),
                ),
            }),
            _ => None,
        }
    };
    let side_type = integer_property(properties, "SideType").unwrap_or_else(|| {
        if bool_property(properties, "Midplane").unwrap_or(false) {
            2
        } else {
            u64::from(integer_property(properties, "Type") == Some(4))
        }
    });
    let extent = match side_type {
        0 => termination(1)?,
        1 => Extent::TwoSidedExtents {
            first: Box::new(termination(1)?),
            second: Box::new(termination(2)?),
        },
        2 => match termination(1)? {
            Extent::Blind { length } => Extent::Symmetric { length },
            extent => Extent::SymmetricExtent {
                extent: Box::new(extent),
            },
        },
        _ => return None,
    };
    let use_custom = bool_property(properties, "UseCustomVector").unwrap_or(false);
    let is_nonempty_link = |link: &crate::native::LinkTarget| {
        link.document.is_some()
            || link
                .object
                .as_deref()
                .is_some_and(|object| !object.is_empty())
    };
    let reference_axis = property(properties, "ReferenceAxis")
        .filter(|property| property.links.iter().any(is_nonempty_link));
    if reference_axis.is_some_and(|property| {
        property
            .links
            .iter()
            .filter(|link| is_nonempty_link(link))
            .count()
            != 1
    }) {
        return None;
    }
    let (mut direction, direction_source) = if use_custom {
        (
            unit_vector(vector_property(properties, "Direction")?)?,
            ExtrusionDirectionSource::Custom,
        )
    } else if let Some(reference_axis) = reference_axis {
        (
            unit_vector(vector_property(properties, "Direction")?)?,
            ExtrusionDirectionSource::Edge {
                reference: PathRef::Native(reference_axis.id.clone()),
            },
        )
    } else {
        let ProfileRef::Sketch(sketch_id) = &profile else {
            return None;
        };
        (
            sketches
                .iter()
                .find(|sketch| sketch.id == *sketch_id)?
                .resolved_placement()?
                .1,
            ExtrusionDirectionSource::ProfileNormal,
        )
    };
    if bool_property(properties, "Reversed").unwrap_or(false) {
        direction = Vector3::new(-direction.x, -direction.y, -direction.z);
    }
    let draft = scalar_named(properties, "TaperAngle")
        .filter(|angle| *angle != 0.0)
        .map(|angle| cadmpeg_ir::features::Angle(angle.to_radians()));
    let reverse_draft = scalar_named(properties, "TaperAngle2")
        .filter(|angle| *angle != 0.0)
        .map(|angle| cadmpeg_ir::features::Angle(angle.to_radians()));
    let first_offset = if property(properties, "Offset").is_some() {
        Some(Length(scalar_named(properties, "Offset")?))
    } else {
        None
    };
    let second_offset = if property(properties, "Offset2").is_some() {
        Some(Length(scalar_named(properties, "Offset2")?))
    } else {
        None
    };
    let length_along_profile_normal = if property(properties, "AlongSketchNormal").is_some() {
        Some(bool_property(properties, "AlongSketchNormal")?)
    } else {
        None
    };
    let allow_multi_profile_faces = if property(properties, "AllowMultiFace").is_some() {
        Some(bool_property(properties, "AllowMultiFace")?)
    } else {
        None
    };
    Some(FeatureDefinition::Extrude {
        profile,
        direction: cadmpeg_ir::features::ExtrudeDirection::Explicit(direction),
        start: cadmpeg_ir::features::ExtrudeStart::ProfilePlane,
        extent,
        op: if kind.contains("Pocket") {
            BooleanOp::Cut
        } else {
            BooleanOp::Join
        },
        draft,
        second_draft: reverse_draft,
        direction_source: Some(direction_source),
        solid: Some(true),
        face_maker: None,
        inner_wire_taper: None,
        first_offset,
        second_offset,
        length_along_profile_normal,
        allow_multi_profile_faces,
    })
}

fn dress_up_edge_selection(properties: &[&PropertyRecord]) -> EdgeSelection {
    if bool_property(properties, "UseAllEdges").unwrap_or(false) {
        return EdgeSelection::All;
    }
    property(properties, "Base").map_or(EdgeSelection::Unresolved, |property| {
        EdgeSelection::Native(property.id.clone())
    })
}

fn scale_definition(properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let base = property(properties, "Base")?;
    if base.links.is_empty() {
        return None;
    }
    let factor =
        |name| scalar_named(properties, name).filter(|factor| factor.is_finite() && *factor != 0.0);
    let factors = if bool_property(properties, "Uniform").unwrap_or(true) {
        ScaleFactors {
            uniform: Some(factor("UniformScale")?),
            x: None,
            y: None,
            z: None,
        }
    } else {
        ScaleFactors {
            uniform: None,
            x: Some(factor("XScale")?),
            y: Some(factor("YScale")?),
            z: Some(factor("ZScale")?),
        }
    };
    Some(FeatureDefinition::Scale {
        bodies: BodySelection::Native(base.id.clone()),
        center: Some(ScaleCenter::ModelOrigin),
        factors,
    })
}

fn fillet_definition(
    kind: &str,
    properties: &[&PropertyRecord],
    entries: &[EntryRecord],
) -> Option<FeatureDefinition> {
    let edges = dress_up_edge_selection(properties);
    if matches!(edges, EdgeSelection::Unresolved) {
        return None;
    }
    let radius = if kind == "Part::Fillet" {
        let values = part_fillet_edge_values(properties, entries)?;
        let radius = values.first()?.1;
        values
            .iter()
            .all(|(_, first, second)| {
                *first == radius && *second == radius && radius.is_finite() && radius > 0.0
            })
            .then_some(radius)?
    } else {
        scalar_named(properties, "Radius").filter(|radius| radius.is_finite() && *radius > 0.0)?
    };
    Some(FeatureDefinition::Fillet {
        groups: vec![cadmpeg_ir::features::FilletGroup {
            edges,
            radius: RadiusSpec::Constant {
                radius: Length(radius),
            },
            tangency_weight: None,
        }],
    })
}

fn chamfer_definition(
    kind: &str,
    properties: &[&PropertyRecord],
    entries: &[EntryRecord],
) -> Option<FeatureDefinition> {
    let edges = dress_up_edge_selection(properties);
    if matches!(edges, EdgeSelection::Unresolved) {
        return None;
    }
    let spec = if kind == "Part::Chamfer" {
        let values = part_fillet_edge_values(properties, entries)?;
        let (_, first, second) = *values.first()?;
        if !first.is_finite() || first <= 0.0 || !second.is_finite() || second <= 0.0 {
            return None;
        }
        if !values.iter().all(|(_, candidate_first, candidate_second)| {
            *candidate_first == first && *candidate_second == second
        }) {
            return None;
        }
        if first == second {
            ChamferSpec::Distance {
                distance: Length(first),
            }
        } else {
            ChamferSpec::TwoDistances {
                first: Length(first),
                second: Length(second),
            }
        }
    } else {
        chamfer_spec(properties)?
    };
    Some(FeatureDefinition::Chamfer {
        groups: vec![cadmpeg_ir::features::ChamferGroup { edges, spec }],
        flip_direction: bool_property(properties, "FlipDirection").unwrap_or(false),
    })
}

fn part_fillet_edge_values(
    properties: &[&PropertyRecord],
    entries: &[EntryRecord],
) -> Option<Vec<(u32, f64, f64)>> {
    let property = property(properties, "Edges")?;
    let entry_name = property.side_entries.as_slice().first()?;
    let data = &entries.iter().find(|entry| entry.name == *entry_name)?.data;
    let count = u32::from_le_bytes(data.get(0..4)?.try_into().ok()?) as usize;
    let expected = 4_usize.checked_add(count.checked_mul(20)?)?;
    if data.len() != expected || count > MAX_SKETCH_RECORDS {
        return None;
    }
    data[4..]
        .chunks_exact(20)
        .map(|record| {
            Some((
                u32::from_le_bytes(record[0..4].try_into().ok()?),
                f64::from_le_bytes(record[4..12].try_into().ok()?),
                f64::from_le_bytes(record[12..20].try_into().ok()?),
            ))
        })
        .collect()
}

fn shell_mode(properties: &[&PropertyRecord]) -> Option<ShellMode> {
    match integer_property(properties, "Mode").unwrap_or(0) {
        0 => Some(ShellMode::Skin),
        1 => Some(ShellMode::Pipe),
        2 => Some(ShellMode::BothSides),
        _ => None,
    }
}

fn shell_join(properties: &[&PropertyRecord]) -> Option<ShellJoin> {
    match integer_property(properties, "Join").unwrap_or(0) {
        0 => Some(ShellJoin::Arc),
        1 => Some(ShellJoin::Tangent),
        2 => Some(ShellJoin::Intersection),
        _ => None,
    }
}

fn thickness_definition(kind: &str, properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let thickness = scalar_named(properties, "Value")?;
    if !thickness.is_finite() || thickness == 0.0 {
        return None;
    }
    let source_name = if kind == "Part::Thickness" {
        "Faces"
    } else {
        "Base"
    };
    let selection = property(properties, source_name)?;
    if selection.links.is_empty() {
        return None;
    }
    Some(FeatureDefinition::Shell {
        removed_faces: cadmpeg_ir::features::FaceSelection::Native(selection.id.clone()),
        thickness: Some(Length(thickness.abs())),
        outward: Some(if kind == "Part::Thickness" {
            thickness > 0.0
        } else {
            !bool_property(properties, "Reversed").unwrap_or(false)
        }),
        mode: Some(shell_mode(properties)?),
        join: Some(shell_join(properties)?),
        resolve_intersections: Some(bool_property(properties, "Intersection").unwrap_or(false)),
        allow_self_intersections: Some(
            bool_property(properties, "SelfIntersection").unwrap_or(false),
        ),
    })
}

fn offset_shape_definition(
    kind: &str,
    properties: &[&PropertyRecord],
) -> Option<FeatureDefinition> {
    let source = property(properties, "Source")?;
    if source.links.is_empty() {
        return None;
    }
    let distance = scalar_named(properties, "Value")
        .filter(|distance| distance.is_finite() && *distance != 0.0)?;
    let mode = shell_mode(properties)?;
    if kind == "Part::Offset2D" && mode == ShellMode::BothSides {
        return None;
    }
    Some(FeatureDefinition::OffsetShape {
        source: BodySelection::Native(source.id.clone()),
        distance: Length(distance),
        mode,
        join: shell_join(properties)?,
        resolve_intersections: bool_property(properties, "Intersection").unwrap_or(false),
        allow_self_intersections: bool_property(properties, "SelfIntersection").unwrap_or(false),
        fill: bool_property(properties, "Fill").unwrap_or(false),
        planar: kind == "Part::Offset2D",
    })
}

fn derived_shape_definition(
    kind: &str,
    properties: &[&PropertyRecord],
) -> Option<FeatureDefinition> {
    match kind {
        "Part::Compound" | "Part::Compound2" => {
            let links = property(properties, "Links")?;
            if links.links.is_empty() {
                return None;
            }
            Some(FeatureDefinition::Compound {
                members: BodySelection::Native(links.id.clone()),
            })
        }
        "Part::Refine" | "Part::Reverse" => {
            let source = property(properties, "Source")?;
            if source.links.len() != 1 {
                return None;
            }
            let source = BodySelection::Native(source.id.clone());
            Some(if kind == "Part::Refine" {
                FeatureDefinition::RefineShape { source }
            } else {
                FeatureDefinition::ReverseShape { source }
            })
        }
        _ => None,
    }
}

fn ruled_surface_definition(properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let curve = |name| {
        let property = property(properties, name)?;
        (property.links.len() == 1).then(|| PathRef::Native(property.id.clone()))
    };
    let orientation = match integer_property(properties, "Orientation").unwrap_or(0) {
        0 => RuledCurveOrientation::Automatic,
        1 => RuledCurveOrientation::Forward,
        2 => RuledCurveOrientation::Reversed,
        _ => return None,
    };
    Some(FeatureDefinition::RuledBetweenCurves {
        first: curve("Curve1")?,
        second: curve("Curve2")?,
        orientation,
    })
}

fn section_shape_definition(properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let operand = |name| {
        let property = property(properties, name)?;
        (property.links.len() == 1).then(|| BodySelection::Native(property.id.clone()))
    };
    Some(FeatureDefinition::SectionShape {
        first: operand("Base")?,
        second: operand("Tool")?,
        approximate: bool_property(properties, "Approximation").unwrap_or(false),
    })
}

fn mirror_shape_definition(properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let source = property(properties, "Source")?;
    if source.links.len() != 1 {
        return None;
    }
    let origin = vector_property(properties, "Base")?;
    let plane_reference = property(properties, "MirrorPlane")
        .filter(|property| property.links.iter().any(nonempty_link))
        .map(|property| cadmpeg_ir::features::FaceSelection::Native(property.id.clone()));
    Some(FeatureDefinition::MirrorShape {
        source: BodySelection::Native(source.id.clone()),
        plane_origin: Point3::new(origin.x, origin.y, origin.z),
        plane_normal: unit_vector(vector_property(properties, "Normal")?)?,
        plane_reference,
    })
}

fn project_on_surface_definition(properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let sources = property(properties, "Projection")?;
    if sources.links.is_empty() {
        return None;
    }
    let support = property(properties, "SupportFace")?;
    if support.links.len() != 1 {
        return None;
    }
    let mode = match integer_property(properties, "Mode").unwrap_or(0) {
        0 => SurfaceProjectionMode::All,
        1 => SurfaceProjectionMode::Faces,
        2 => SurfaceProjectionMode::Edges,
        _ => return None,
    };
    let height = if property(properties, "Height").is_some() {
        scalar_named(properties, "Height").filter(|value| *value >= 0.0)?
    } else {
        0.0
    };
    let offset = if property(properties, "Offset").is_some() {
        scalar_named(properties, "Offset")?
    } else {
        0.0
    };
    Some(FeatureDefinition::ProjectOnSurface {
        sources: PathRef::Native(sources.id.clone()),
        support_face: cadmpeg_ir::features::FaceSelection::Native(support.id.clone()),
        direction: unit_vector(vector_property(properties, "Direction")?)?,
        mode,
        height: Length(height),
        offset: Length(offset),
    })
}

fn draft_definition(
    properties: &[&PropertyRecord],
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
) -> Option<FeatureDefinition> {
    let faces = property(properties, "Base")?;
    let neutral_plane = property(properties, "NeutralPlane")?;
    let (_, plane_normal) =
        plane_reference(properties, "NeutralPlane", objects, properties_by_owner)?;
    let pull_direction = if property(properties, "PullDirection")
        .is_some_and(|property| !property.links.is_empty())
    {
        axis_reference(properties, "PullDirection", objects, properties_by_owner)?.1
    } else {
        plane_normal
    };
    let reversed = bool_property(properties, "Reversed").unwrap_or(false);
    let angle = scalar_named(properties, "Angle")?;
    if !angle.is_finite() {
        return None;
    }
    Some(FeatureDefinition::Draft {
        faces: cadmpeg_ir::features::FaceSelection::Native(faces.id.clone()),
        neutral_plane: cadmpeg_ir::features::FaceSelection::Native(neutral_plane.id.clone()),
        pull_direction: Some(pull_direction),
        angle: Some(cadmpeg_ir::features::Angle(
            if reversed { -angle } else { angle }.to_radians(),
        )),
        outward: Some(reversed),
    })
}

fn chamfer_spec(properties: &[&PropertyRecord]) -> Option<ChamferSpec> {
    let mode = property(properties, "ChamferType")
        .and_then(scalar_value)
        .unwrap_or(-1.0) as i64;
    let first = property(properties, "Size")
        .and_then(scalar_value)
        .filter(|value| value.is_finite() && *value > 0.0);
    match (mode, first) {
        (0, Some(distance)) => Some(ChamferSpec::Distance {
            distance: Length(distance),
        }),
        (1, Some(first)) => property(properties, "Size2")
            .and_then(scalar_value)
            .filter(|value| value.is_finite() && *value > 0.0)
            .map(|second| ChamferSpec::TwoDistances {
                first: Length(first),
                second: Length(second),
            }),
        (2, Some(distance)) => property(properties, "Angle")
            .and_then(scalar_value)
            .filter(|angle| angle.is_finite() && *angle > 0.0 && *angle < 180.0)
            .map(|angle| ChamferSpec::DistanceAngle {
                distance: Length(distance),
                angle: cadmpeg_ir::features::Angle(angle.to_radians()),
            }),
        _ => None,
    }
}

fn property<'a>(properties: &'a [&PropertyRecord], name: &str) -> Option<&'a PropertyRecord> {
    properties
        .iter()
        .copied()
        .find(|property| property.name == name)
}

fn nonempty_link(link: &crate::native::LinkTarget) -> bool {
    link.document.is_some()
        || link
            .object
            .as_deref()
            .is_some_and(|object| !object.is_empty())
}

fn scalar_value(property: &PropertyRecord) -> Option<f64> {
    property
        .values
        .iter()
        .find_map(|value| value_attribute(value).and_then(|value| value.parse().ok()))
}

fn scalar_text(property: &PropertyRecord) -> Option<String> {
    property
        .values
        .iter()
        .find_map(value_attribute)
        .map(str::to_owned)
}

fn value_attribute(value: &ValueRecord) -> Option<&str> {
    value
        .attributes
        .get("value")
        .or_else(|| value.attributes.get("Value"))
        .map(String::as_str)
}

fn native_parameters(properties: &[&PropertyRecord]) -> BTreeMap<String, String> {
    properties
        .iter()
        .filter_map(|property| scalar_text(property).map(|value| (property.name.clone(), value)))
        .collect()
}

fn primitive_definition(kind: &str, properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let length = |name: &str| {
        property(properties, name)
            .and_then(scalar_value)
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(Length)
    };
    let angle = |name: &str| {
        property(properties, name)
            .and_then(scalar_value)
            .filter(|value| value.is_finite())
            .map(|value| cadmpeg_ir::features::Angle(value.to_radians()))
    };
    let signed_length = |name: &str| {
        property(properties, name)
            .and_then(scalar_value)
            .filter(|value| value.is_finite())
            .map(Length)
    };
    let solid = if kind.ends_with("Box") {
        PrimitiveSolid::Box {
            length: length("Length").filter(|value| value.0 > 0.0)?,
            width: length("Width").filter(|value| value.0 > 0.0)?,
            height: length("Height").filter(|value| value.0 > 0.0)?,
        }
    } else if kind.ends_with("Cylinder") {
        PrimitiveSolid::Cylinder {
            radius: length("Radius").filter(|value| value.0 > 0.0)?,
            height: length("Height").filter(|value| value.0 > 0.0)?,
            angle: angle("Angle")?,
        }
    } else if kind.ends_with("Cone") {
        let radius1 = length("Radius1")?;
        let radius2 = length("Radius2")?;
        if radius1.0 == 0.0 && radius2.0 == 0.0 {
            return None;
        }
        PrimitiveSolid::Cone {
            radius1,
            radius2,
            height: length("Height").filter(|value| value.0 > 0.0)?,
            angle: angle("Angle")?,
        }
    } else if kind.ends_with("Sphere") {
        PrimitiveSolid::Sphere {
            radius: length("Radius").filter(|value| value.0 > 0.0)?,
            latitude1: angle("Angle1")?,
            latitude2: angle("Angle2")?,
            longitude: angle("Angle3")?,
        }
    } else if kind.ends_with("Ellipsoid") {
        let x_radius = length("Radius2").filter(|value| value.0 > 0.0)?;
        let y_radius = length("Radius3")?;
        PrimitiveSolid::Ellipsoid {
            x_radius,
            y_radius: if y_radius.0 == 0.0 {
                x_radius
            } else {
                y_radius
            },
            z_radius: length("Radius1").filter(|value| value.0 > 0.0)?,
            latitude1: angle("Angle1")?,
            latitude2: angle("Angle2")?,
            longitude: angle("Angle3")?,
        }
    } else if kind.ends_with("Torus") {
        PrimitiveSolid::Torus {
            major_radius: length("Radius1").filter(|value| value.0 > 0.0)?,
            minor_radius: length("Radius2").filter(|value| value.0 > 0.0)?,
            latitude1: angle("Angle1")?,
            latitude2: angle("Angle2")?,
            longitude: angle("Angle3")?,
        }
    } else if kind.ends_with("Prism") {
        PrimitiveSolid::Prism {
            sides: u32::try_from(integer_property(properties, "Polygon")?).ok()?,
            circumradius: length("Circumradius").filter(|value| value.0 > 0.0)?,
            height: length("Height").filter(|value| value.0 > 0.0)?,
        }
    } else if kind.ends_with("Wedge") {
        PrimitiveSolid::Wedge {
            xmin: signed_length("Xmin")?,
            ymin: signed_length("Ymin")?,
            zmin: signed_length("Zmin")?,
            x2min: signed_length("X2min")?,
            z2min: signed_length("Z2min")?,
            xmax: signed_length("Xmax")?,
            ymax: signed_length("Ymax")?,
            zmax: signed_length("Zmax")?,
            x2max: signed_length("X2max")?,
            z2max: signed_length("Z2max")?,
        }
    } else {
        return None;
    };
    let op = if kind.contains("Subtractive") {
        BooleanOp::Cut
    } else if kind.contains("Additive") {
        BooleanOp::Join
    } else {
        BooleanOp::NewBody
    };
    Some(FeatureDefinition::Primitive { solid, op })
}

fn datum_definition(kind: &str, properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let (origin, z_axis, x_axis, y_axis) = placement_frame(properties)?;
    Some(match kind {
        "PartDesign::Plane" => FeatureDefinition::DatumPlane {
            origin,
            normal: z_axis,
            u_axis: x_axis,
        },
        "PartDesign::Line" => FeatureDefinition::DatumAxis {
            origin,
            direction: z_axis,
        },
        "PartDesign::Point" => FeatureDefinition::DatumPoint { position: origin },
        "PartDesign::CoordinateSystem" => FeatureDefinition::DatumCoordinateSystem {
            origin,
            x_axis,
            y_axis,
            z_axis,
        },
        _ => return None,
    })
}

fn boolean_definition(kind: &str, properties: &[&PropertyRecord]) -> Option<FeatureDefinition> {
    let op = if kind == "PartDesign::Boolean" {
        match integer_property(properties, "Type").unwrap_or(0) {
            0 => BooleanOp::Join,
            1 => BooleanOp::Cut,
            2 => BooleanOp::Intersect,
            _ => return None,
        }
    } else if kind.ends_with("Cut") {
        BooleanOp::Cut
    } else if kind.ends_with("Common") || kind.ends_with("MultiCommon") {
        BooleanOp::Intersect
    } else if kind.ends_with("Fuse") || kind.ends_with("MultiFuse") {
        BooleanOp::Join
    } else {
        return None;
    };
    let (target, tools) = if kind == "PartDesign::Boolean" {
        let group = property(properties, "Group")?;
        if group.links.is_empty() {
            return None;
        }
        if let Some(base) =
            property(properties, "BaseFeature").filter(|base| !base.links.is_empty())
        {
            (
                BodySelection::Native(base.id.clone()),
                BodySelection::Native(group.id.clone()),
            )
        } else {
            let last = group.links.len() - 1;
            (
                BodySelection::Native(format!("{}:link:{last}", group.id)),
                BodySelection::Native(format!("{}:links:0..{last}", group.id)),
            )
        }
    } else if let (Some(base), Some(tool)) =
        (property(properties, "Base"), property(properties, "Tool"))
    {
        if base.links.is_empty() || tool.links.is_empty() {
            return None;
        }
        (
            BodySelection::Native(base.id.clone()),
            BodySelection::Native(tool.id.clone()),
        )
    } else {
        let shapes = property(properties, "Shapes")?;
        if shapes.links.len() < 2 {
            return None;
        }
        (
            BodySelection::Native(format!("{}:link:0", shapes.id)),
            BodySelection::Native(format!("{}:links:1..{}", shapes.id, shapes.links.len())),
        )
    };
    Some(FeatureDefinition::Combine { target, tools, op })
}

fn loft_definition(
    kind: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
) -> Option<FeatureDefinition> {
    let sections = property(properties, "Sections").or_else(|| property(properties, "Profile"))?;
    let profiles = sections
        .links
        .iter()
        .filter_map(|link| link.object.as_deref())
        .map(|object| {
            sketches
                .get(object)
                .cloned()
                .map_or_else(|| ProfileRef::Native(object.to_owned()), ProfileRef::Sketch)
        })
        .collect::<Vec<_>>();
    if profiles.len() < 2 {
        return None;
    }
    let max_degree = if property(properties, "MaxDegree").is_some() {
        let value = u32::try_from(integer_property(properties, "MaxDegree")?).ok()?;
        Some((value > 0).then_some(value)?)
    } else {
        None
    };
    Some(FeatureDefinition::Loft {
        sections: profiles
            .into_iter()
            .map(cadmpeg_ir::features::LoftSection::Profile)
            .collect(),
        guides: Vec::new(),
        centerline: None,
        op: operation_boolean(kind),
        closed: bool_property(properties, "Closed").unwrap_or(false),
        solid: kind.starts_with("PartDesign::")
            || bool_property(properties, "Solid").unwrap_or(true),
        ruled: bool_property(properties, "Ruled").unwrap_or(false),
        max_degree,
        check_compatibility: if property(properties, "CheckCompatibility").is_some() {
            Some(bool_property(properties, "CheckCompatibility")?)
        } else {
            None
        },
        allow_multi_profile_faces: if property(properties, "AllowMultiFace").is_some() {
            Some(bool_property(properties, "AllowMultiFace")?)
        } else {
            None
        },
    })
}

fn sweep_definition(
    kind: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
) -> Option<FeatureDefinition> {
    let profile_ref = |object: &str| {
        sketches
            .get(object)
            .cloned()
            .map_or_else(|| ProfileRef::Native(object.to_owned()), ProfileRef::Sketch)
    };
    let mut profiles = property(properties, "Profile")
        .into_iter()
        .chain(property(properties, "Sections"))
        .flat_map(|property| &property.links)
        .filter_map(|link| link.object.as_deref())
        .map(profile_ref)
        .collect::<Vec<_>>();
    profiles.dedup();
    if profiles.is_empty() {
        return None;
    }
    let profile = profiles.remove(0);
    let path_property = property(properties, "Spine").or_else(|| property(properties, "Path"))?;
    if path_property.links.is_empty() {
        return None;
    }
    let solid =
        kind.starts_with("PartDesign::") || bool_property(properties, "Solid").unwrap_or(false);
    let transition = match integer_property(properties, "Transition")
        .unwrap_or(u64::from(kind == "Part::Sweep"))
    {
        0 => SweepTransition::Transformed,
        1 => SweepTransition::RightCorner,
        2 => SweepTransition::RoundCorner,
        _ => return None,
    };
    let orientation = if kind == "Part::Sweep" {
        if bool_property(properties, "Frenet").unwrap_or(true) {
            SweepOrientation::Frenet
        } else {
            SweepOrientation::CorrectedFrenet
        }
    } else {
        match integer_property(properties, "Mode").unwrap_or(0) {
            0 => SweepOrientation::CorrectedFrenet,
            1 => SweepOrientation::Fixed,
            2 => SweepOrientation::Frenet,
            3 => {
                let auxiliary = property(properties, "AuxiliarySpine")?;
                if auxiliary.links.is_empty() {
                    return None;
                }
                SweepOrientation::Auxiliary {
                    path: PathRef::Native(auxiliary.id.clone()),
                    tangent: bool_property(properties, "AuxiliarySpineTangent").unwrap_or(false),
                    curvilinear: bool_property(properties, "AuxiliaryCurvilinear").unwrap_or(true),
                }
            }
            4 => SweepOrientation::Binormal {
                direction: unit_vector(vector_property(properties, "Binormal")?)?,
            },
            _ => return None,
        }
    };
    let transformation = if kind == "Part::Sweep" {
        SweepTransformation::Constant
    } else {
        match integer_property(properties, "Transformation").unwrap_or(0) {
            0 => SweepTransformation::Constant,
            1 => SweepTransformation::MultiSection,
            2 => SweepTransformation::Linear,
            3 => SweepTransformation::SShape,
            4 => SweepTransformation::Interpolation,
            _ => return None,
        }
    };
    Some(FeatureDefinition::Sweep {
        profile: Some(profile),
        sections: profiles,
        path: Some(PathRef::Native(path_property.id.clone())),
        mode: if solid {
            SweepMode::Solid {
                op: operation_boolean(kind),
            }
        } else {
            SweepMode::Surface
        },
        orientation: Some(orientation),
        transition: Some(transition),
        transformation: Some(transformation),
        path_tangent: bool_property(properties, "SpineTangent").unwrap_or(false),
        linearize: bool_property(properties, "Linearize").unwrap_or(false),
        twist: None,
        scale: None,
        allow_multi_profile_faces: if property(properties, "AllowMultiFace").is_some() {
            Some(bool_property(properties, "AllowMultiFace")?)
        } else {
            None
        },
    })
}

fn hole_definition(
    owner: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
) -> Option<FeatureDefinition> {
    let profile = profile_ref(owner, properties, sketches);
    if matches!(profile, ProfileRef::Unresolved(_)) {
        return None;
    }
    let filter_bits = integer_property(properties, "BaseProfileType").unwrap_or(6);
    let profile_filter = HoleProfileFilter {
        points: filter_bits & 1 != 0,
        circles: filter_bits & 2 != 0,
        arcs: filter_bits & 4 != 0,
    };
    if !profile_filter.points && !profile_filter.circles && !profile_filter.arcs {
        return None;
    }
    let positive = |name| scalar_named(properties, name).filter(|value| *value > 0.0);
    let diameter = positive("Diameter")?;
    let cut_angle = || {
        positive("HoleCutCountersinkAngle")
            .filter(|value| *value < 180.0)
            .map(|value| cadmpeg_ir::features::Angle(value.to_radians()))
    };
    let kind = match integer_property(properties, "HoleCutType").unwrap_or(0) {
        0 => HoleKind::Simple,
        1 => HoleKind::Counterbore {
            diameter: Length(positive("HoleCutDiameter")?),
            depth: Length(positive("HoleCutDepth")?),
        },
        2 => HoleKind::Countersink {
            diameter: Length(positive("HoleCutDiameter")?),
            angle: cut_angle()?,
        },
        3 => HoleKind::Counterdrill {
            diameter: Length(positive("HoleCutDiameter")?),
            depth: Length(positive("HoleCutDepth")?),
            angle: cut_angle()?,
        },
        _ => return None,
    };
    let extent = match integer_property(properties, "DepthType").unwrap_or(0) {
        0 => Extent::Blind {
            length: Length(positive("Depth")?),
        },
        1 => Extent::ThroughAll,
        _ => return None,
    };
    let bottom = match integer_property(properties, "DrillPoint").unwrap_or(1) {
        0 => HoleBottom::Flat,
        1 => HoleBottom::Angled {
            included_angle: cadmpeg_ir::features::Angle(positive("DrillPointAngle")?.to_radians()),
            depth_to_tip: bool_property(properties, "DrillForDepth").unwrap_or(false),
        },
        _ => return None,
    };
    let tapered = bool_property(properties, "Tapered").unwrap_or(false);
    let taper_angle = tapered
        .then(|| {
            positive("TaperedAngle")
                .filter(|value| *value < 180.0)
                .map(|value| cadmpeg_ir::features::Angle(value.to_radians()))
        })
        .flatten();
    if tapered && taper_angle.is_none() {
        return None;
    }
    let thread_type = integer_property(properties, "ThreadType").unwrap_or(0);
    let specification = if thread_type == 0 {
        None
    } else {
        let threaded = bool_property(properties, "Threaded").unwrap_or(false);
        Some(Box::new(HoleSpecification {
            standard: thread_standard(thread_type)?.into(),
            designation: enumeration_label(properties, "ThreadSize"),
            class: if threaded {
                enumeration_label(properties, "ThreadClass")
            } else {
                None
            },
            fit: if threaded {
                None
            } else {
                enumeration_label(properties, "ThreadFit")
            },
            threaded,
            modeled: bool_property(properties, "ModelThread").unwrap_or(false),
            cosmetic: bool_property(properties, "CosmeticThread").unwrap_or(false),
            pitch: positive("ThreadPitch").map(Length),
            major_diameter: positive("ThreadDiameter").map(Length),
            hand: match integer_property(properties, "ThreadDirection").unwrap_or(0) {
                0 => ThreadHand::Right,
                1 => ThreadHand::Left,
                _ => return None,
            },
            depth: match integer_property(properties, "ThreadDepthType").unwrap_or(0) {
                0 => HoleThreadDepth::HoleDepth,
                1 => HoleThreadDepth::Blind {
                    depth: Length(positive("ThreadDepth")?),
                },
                2 => HoleThreadDepth::TappedStandard,
                _ => return None,
            },
            clearance: if bool_property(properties, "UseCustomThreadClearance").unwrap_or(false) {
                Some(Length(scalar_named(properties, "CustomThreadClearance")?))
            } else {
                None
            },
        }))
    };
    let direction = axis_reference(properties, "Profile", objects, properties_by_owner)
        .map(|(_, direction)| direction);
    Some(FeatureDefinition::Hole {
        profile: Some(profile),
        profile_filter: Some(profile_filter),
        face: None,
        position: None,
        direction,
        placements: Vec::new(),
        kind,
        exit_kind: None,
        diameter: Some(Length(diameter)),
        extent: Some(extent),
        bottom: Some(bottom),
        taper_angle,
        specification,
        allow_multi_profile_faces: if property(properties, "AllowMultiFace").is_some() {
            Some(bool_property(properties, "AllowMultiFace")?)
        } else {
            None
        },
    })
}

fn thread_standard(value: u64) -> Option<&'static str> {
    [
        "None",
        "ISO metric",
        "ISO metric fine",
        "UNC",
        "UNF",
        "UNEF",
        "NPT",
        "BSP",
        "BSW",
        "BSF",
        "ISO tyre",
    ]
    .get(usize::try_from(value).ok()?)
    .copied()
}

fn helical_sweep_definition(
    kind: &str,
    owner: &str,
    properties: &[&PropertyRecord],
    sketches: &HashMap<&str, SketchId>,
) -> Option<FeatureDefinition> {
    let law = match integer_property(properties, "Mode")? {
        0 => HelicalSweepLaw::PitchHeightAngle,
        1 => HelicalSweepLaw::PitchTurnsAngle,
        2 => HelicalSweepLaw::HeightTurnsAngle,
        3 => HelicalSweepLaw::HeightTurnsGrowth,
        _ => return None,
    };
    let origin = vector_property(properties, "Base")?;
    let axis_direction = vector_property(properties, "Axis")?;
    let construction = HelicalSweepConstruction {
        profile: profile_ref(owner, properties, sketches),
        axis_origin: Point3::new(origin.x, origin.y, origin.z),
        axis_direction,
        law,
        pitch: Length(scalar_named(properties, "Pitch")?),
        height: Length(scalar_named(properties, "Height")?),
        turns: scalar_named(properties, "Turns")?,
        radial_growth: Length(scalar_named(properties, "Growth")?),
        cone_angle: cadmpeg_ir::features::Angle(scalar_named(properties, "Angle")?.to_radians()),
        left_handed: bool_property(properties, "LeftHanded")?,
        reversed: bool_property(properties, "Reversed")?,
        tolerance: scalar_named(properties, "Tolerance")?,
        allow_multi_profile_faces: if property(properties, "AllowMultiFace").is_some() {
            Some(bool_property(properties, "AllowMultiFace")?)
        } else {
            None
        },
    };
    let op = if kind.ends_with("SubtractiveHelix") {
        if bool_property(properties, "Outside").unwrap_or(false) {
            BooleanOp::Intersect
        } else {
            BooleanOp::Cut
        }
    } else {
        BooleanOp::Join
    };
    Some(FeatureDefinition::HelicalSweep { construction, op })
}

fn binder_definition(
    kind: &str,
    properties: &[&PropertyRecord],
    features: &HashMap<&str, FeatureId>,
) -> Option<FeatureDefinition> {
    let sources = property(properties, "Support")?
        .links
        .iter()
        .filter(|link| {
            link.object
                .as_deref()
                .is_some_and(|object| !object.is_empty())
        })
        .map(|link| {
            Some(BinderSource {
                target: binder_target(link, features)?,
                subelements: link_selectors(link).map(str::to_owned).collect(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let construction = if kind == "PartDesign::ShapeBinder" {
        BinderConstruction::Shape {
            trace_support: bool_property(properties, "TraceSupport").unwrap_or(false),
        }
    } else {
        let distance = scalar_named(properties, "Offset").unwrap_or(0.0);
        if !distance.is_finite() {
            return None;
        }
        let offset = if distance == 0.0 {
            None
        } else {
            Some(BinderOffset {
                distance: Length(distance),
                join: match integer_property(properties, "OffsetJoinType").unwrap_or(0) {
                    0 => BinderOffsetJoin::Arcs,
                    1 => BinderOffsetJoin::Tangent,
                    2 => BinderOffsetJoin::Intersection,
                    _ => return None,
                },
                fill: bool_property(properties, "OffsetFill").unwrap_or(false),
                open_result: bool_property(properties, "OffsetOpenResult").unwrap_or(false),
                intersection: bool_property(properties, "OffsetIntersection").unwrap_or(false),
            })
        };
        let context = property(properties, "Context")
            .and_then(|property| property.links.first())
            .filter(|link| {
                link.object
                    .as_deref()
                    .is_some_and(|object| !object.is_empty())
            })
            .and_then(|link| binder_target(link, features));
        BinderConstruction::SubShape {
            lifecycle: match integer_property(properties, "BindMode").unwrap_or(0) {
                0 => BinderLifecycle::Synchronized,
                1 => BinderLifecycle::Frozen,
                2 => BinderLifecycle::Detached,
                _ => return None,
            },
            placement: if bool_property(properties, "Relative").unwrap_or(true) {
                BinderPlacement::Relative
            } else {
                BinderPlacement::Global
            },
            copy_on_change: match integer_property(properties, "BindCopyOnChange").unwrap_or(0) {
                0 => BinderCopyOnChange::Disabled,
                1 => BinderCopyOnChange::Enabled,
                2 => BinderCopyOnChange::Mutated,
                _ => return None,
            },
            claim_children: bool_property(properties, "ClaimChildren").unwrap_or(false),
            fuse: bool_property(properties, "Fuse").unwrap_or(false),
            make_face: bool_property(properties, "MakeFace").unwrap_or(true),
            partial_load: bool_property(properties, "PartialLoad").unwrap_or(false),
            refine: bool_property(properties, "Refine").unwrap_or(true),
            offset,
            context,
        }
    };
    Some(FeatureDefinition::Binder {
        sources,
        construction,
    })
}

fn binder_target(
    link: &crate::native::LinkTarget,
    features: &HashMap<&str, FeatureId>,
) -> Option<BinderTarget> {
    let object = link.object.as_deref()?;
    if let Some(document) = link.document.as_ref() {
        return Some(BinderTarget::External {
            document: document.clone(),
            object: object.to_owned(),
        });
    }
    Some(features.get(object).cloned().map_or_else(
        || BinderTarget::Native {
            reference: object.to_owned(),
        },
        |feature| BinderTarget::Feature { feature },
    ))
}

fn enumeration_label(properties: &[&PropertyRecord], name: &str) -> Option<String> {
    let property = property(properties, name)?;
    let index = usize::try_from(integer_property(properties, name)?).ok()?;
    let document = roxmltree::Document::parse(&property.raw_xml).ok()?;
    document
        .descendants()
        .filter(|node| node.has_tag_name("Enum"))
        .filter_map(|node| node.attribute("value").or_else(|| node.attribute("Value")))
        .nth(index)
        .map(str::to_owned)
}

fn pattern_definition(
    kind: &str,
    properties: &[&PropertyRecord],
    features: &HashMap<&str, FeatureId>,
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
) -> Option<FeatureDefinition> {
    let originals = property(properties, "Originals")?;
    let seeds = originals
        .links
        .iter()
        .filter_map(|link| link.object.as_deref())
        .filter_map(|object| features.get(object).cloned())
        .collect::<Vec<_>>();
    if seeds.is_empty() || seeds.len() != originals.links.len() {
        return None;
    }

    let pattern = if kind.ends_with("MultiTransform") {
        let transformations = property(properties, "Transformations")?;
        if transformations.links.is_empty() {
            return None;
        }
        let stages = transformations
            .links
            .iter()
            .enumerate()
            .map(|(index, link)| {
                let target = link.object.as_deref()?;
                let object = objects.iter().find(|object| object.id == target)?;
                let owned = properties_by_owner.get(target).map(Vec::as_slice)?;
                let pattern = pattern_kind(&object.type_name, owned, objects, properties_by_owner)?;
                let combination = if index == 0 {
                    PatternStageCombination::Initialize
                } else if matches!(pattern, PatternKind::Scale { .. }) {
                    PatternStageCombination::AlignedSlices
                } else {
                    PatternStageCombination::CartesianProduct
                };
                Some(PatternStage {
                    pattern: Box::new(pattern),
                    combination,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        PatternKind::Composite { stages }
    } else {
        pattern_kind(kind, properties, objects, properties_by_owner)?
    };
    Some(FeatureDefinition::Pattern {
        seeds: seeds.into_iter().map(PatternSeed::Feature).collect(),
        pattern,
    })
}

fn pattern_kind(
    kind: &str,
    properties: &[&PropertyRecord],
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
) -> Option<PatternKind> {
    if kind.ends_with("Mirrored") {
        let (plane_origin, plane_normal) =
            plane_reference(properties, "MirrorPlane", objects, properties_by_owner)?;
        return Some(PatternKind::Mirror {
            plane_origin,
            plane_normal,
        });
    }

    let count = integer_property(properties, "Occurrences")?;
    if count == 0 || count > MAX_SKETCH_RECORDS as u64 {
        return None;
    }
    let count = count as u32;
    let mode = integer_property(properties, "Mode").unwrap_or(0);

    if kind.ends_with("Scaled") {
        let final_factor = scalar_named(properties, "Factor")?;
        return (final_factor.is_finite() && final_factor > 0.0 && count >= 2).then_some(
            PatternKind::Scale {
                center: PatternScaleCenter::FirstSeedCentroid,
                final_factor,
                count,
            },
        );
    }

    let pattern = if kind.ends_with("LinearPattern") {
        let first = linear_pattern_axis(properties, "", count, mode, objects, properties_by_owner)?;
        let count2 = integer_property(properties, "Occurrences2").unwrap_or(1);
        if count2 > MAX_SKETCH_RECORDS as u64 {
            return None;
        }
        if count2 > 1 {
            let mode2 = integer_property(properties, "Mode2").unwrap_or(0);
            let second = linear_pattern_axis(
                properties,
                "2",
                count2 as u32,
                mode2,
                objects,
                properties_by_owner,
            )?;
            PatternKind::Composite {
                stages: vec![
                    PatternStage {
                        pattern: Box::new(first),
                        combination: PatternStageCombination::Initialize,
                    },
                    PatternStage {
                        pattern: Box::new(second),
                        combination: PatternStageCombination::CartesianProduct,
                    },
                ],
            }
        } else {
            first
        }
    } else if kind.ends_with("PolarPattern") {
        let (axis_origin, mut axis_dir) =
            axis_reference(properties, "Axis", objects, properties_by_owner)?;
        if bool_property(properties, "Reversed").unwrap_or(false) {
            axis_dir = Vector3::new(-axis_dir.x, -axis_dir.y, -axis_dir.z);
        }
        let angles = pattern_locations(properties, "", count, mode, "Angle", "Offset")?;
        if let Some(step) = uniform_step(&angles) {
            PatternKind::Circular {
                axis_origin,
                axis_dir,
                angle: cadmpeg_ir::features::Angle((step * f64::from(count - 1)).to_radians()),
                count,
            }
        } else {
            PatternKind::CircularAngles {
                axis_origin,
                axis_dir,
                angles: angles
                    .into_iter()
                    .map(|angle| cadmpeg_ir::features::Angle(angle.to_radians()))
                    .collect(),
            }
        }
    } else {
        return None;
    };
    Some(pattern)
}

fn linear_pattern_axis(
    properties: &[&PropertyRecord],
    suffix: &str,
    count: u32,
    mode: u64,
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
) -> Option<PatternKind> {
    let name = |base: &str| format!("{base}{suffix}");
    let (_, mut direction) =
        axis_reference(properties, &name("Direction"), objects, properties_by_owner)?;
    if bool_property(properties, &name("Reversed")).unwrap_or(false) {
        direction = Vector3::new(-direction.x, -direction.y, -direction.z);
    }
    let offsets = pattern_locations(properties, suffix, count, mode, "Length", "Offset")?;
    if let Some(spacing) = uniform_step(&offsets) {
        Some(PatternKind::Linear {
            direction: Some(direction),
            spacing: Length(spacing),
            count,
            second: None,
        })
    } else {
        Some(PatternKind::LinearOffsets {
            direction: Some(direction),
            offsets: offsets.into_iter().map(Length).collect(),
        })
    }
}

fn pattern_locations(
    properties: &[&PropertyRecord],
    suffix: &str,
    count: u32,
    mode: u64,
    extent_base: &str,
    offset_base: &str,
) -> Option<Vec<f64>> {
    if count == 0 {
        return None;
    }
    if count == 1 {
        return Some(vec![0.0]);
    }
    let name = |base: &str| format!("{base}{suffix}");
    let intervals = match mode {
        0 => {
            let interval = scalar_named(properties, &name(extent_base))? / f64::from(count - 1);
            vec![interval; count as usize - 1]
        }
        1 => {
            let fallback = scalar_named(properties, &name(offset_base))?;
            let spacings = property(properties, &name("Spacings"))
                .map_or_else(|| Some(Vec::new()), numeric_list)?;
            let pattern = property(properties, &name("SpacingPattern"))
                .map_or_else(|| Some(Vec::new()), numeric_list)?;
            if !spacings.is_empty() && spacings.len() != count as usize - 1 {
                return None;
            }
            (0..count as usize - 1)
                .map(|index| {
                    let explicit = spacings.get(index).copied().unwrap_or(-1.0);
                    if explicit != -1.0 {
                        explicit
                    } else if pattern.len() > 1 {
                        pattern[index % pattern.len()]
                    } else {
                        fallback
                    }
                })
                .collect()
        }
        _ => return None,
    };
    let mut locations = Vec::with_capacity(count as usize);
    locations.push(0.0);
    let mut location = 0.0;
    for interval in intervals {
        if !interval.is_finite() || interval <= 0.0 {
            return None;
        }
        location += interval;
        if !location.is_finite() {
            return None;
        }
        locations.push(location);
    }
    Some(locations)
}

fn uniform_step(locations: &[f64]) -> Option<f64> {
    let step = *locations.get(1)?;
    locations
        .windows(2)
        .all(|pair| (pair[1] - pair[0] - step).abs() <= f64::EPSILON * step.abs().max(1.0))
        .then_some(step)
}

fn axis_reference(
    properties: &[&PropertyRecord],
    name: &str,
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
) -> Option<(Point3, Vector3)> {
    if let Some(direction) = vector_property(properties, name) {
        return Some((Point3::new(0.0, 0.0, 0.0), unit_vector(direction)?));
    }
    let link = property(properties, name)?.links.first()?;
    let target = link.object.as_deref()?;
    let object = objects.iter().find(|object| object.id == target)?;
    let owned = properties_by_owner.get(target).map(Vec::as_slice)?;
    let (origin, z_axis, x_axis, y_axis) = placement_frame(owned)?;
    let selector = link_selectors(link).next();
    let direction = match object.type_name.as_str() {
        "PartDesign::Line" => z_axis,
        "PartDesign::Plane" => z_axis,
        "PartDesign::CoordinateSystem" => match selector {
            Some("X_Axis" | "XAxis" | "X") => x_axis,
            Some("Y_Axis" | "YAxis" | "Y") => y_axis,
            Some("Z_Axis" | "ZAxis" | "Z") | None => z_axis,
            _ => return None,
        },
        kind if is_sketch(kind) => match selector {
            Some("H_Axis") => x_axis,
            Some("V_Axis") => y_axis,
            Some("N_Axis") | None => z_axis,
            _ => return None,
        },
        _ => return None,
    };
    Some((origin, unit_vector(direction)?))
}

fn plane_reference(
    properties: &[&PropertyRecord],
    name: &str,
    objects: &[ObjectRecord],
    properties_by_owner: &HashMap<&str, Vec<&PropertyRecord>>,
) -> Option<(Point3, Vector3)> {
    let link = property(properties, name)?.links.first()?;
    let target = link.object.as_deref()?;
    let object = objects.iter().find(|object| object.id == target)?;
    let owned = properties_by_owner.get(target).map(Vec::as_slice)?;
    let (origin, z_axis, x_axis, y_axis) = placement_frame(owned)?;
    let selector = link_selectors(link).next();
    let normal = match object.type_name.as_str() {
        "PartDesign::Plane" => z_axis,
        "PartDesign::CoordinateSystem" => match selector {
            Some("XY_Plane" | "XYPlane" | "XY") | None => z_axis,
            Some("XZ_Plane" | "XZPlane" | "XZ") => y_axis,
            Some("YZ_Plane" | "YZPlane" | "YZ") => x_axis,
            _ => return None,
        },
        kind if is_sketch(kind) => match selector {
            None | Some("N_Axis") => z_axis,
            Some("H_Axis") => y_axis,
            Some("V_Axis") => x_axis,
            _ => return None,
        },
        _ => return None,
    };
    Some((origin, unit_vector(normal)?))
}

fn unit_vector(vector: Vector3) -> Option<Vector3> {
    let magnitude = (vector.x * vector.x + vector.y * vector.y + vector.z * vector.z).sqrt();
    (magnitude.is_finite() && magnitude > f64::EPSILON).then(|| {
        Vector3::new(
            vector.x / magnitude,
            vector.y / magnitude,
            vector.z / magnitude,
        )
    })
}

fn link_selectors(link: &crate::native::LinkTarget) -> impl Iterator<Item = &str> {
    link.subelements
        .iter()
        .flat_map(|selector| selector.split_ascii_whitespace())
        .filter(|selector| !selector.is_empty())
}

fn scalar_named(properties: &[&PropertyRecord], name: &str) -> Option<f64> {
    property(properties, name).and_then(scalar_value)
}

fn string_property_value(property: &PropertyRecord) -> Option<&str> {
    let value = property.values.first()?;
    value
        .attributes
        .get("value")
        .map(String::as_str)
        .or(value.text.as_deref())
}

fn integer_property(properties: &[&PropertyRecord], name: &str) -> Option<u64> {
    let value = scalar_named(properties, name)?;
    (value.is_finite() && value >= 0.0 && value.fract() == 0.0).then_some(value as u64)
}

fn numeric_list(property: &PropertyRecord) -> Option<Vec<f64>> {
    let document = roxmltree::Document::parse(&property.raw_xml).ok()?;
    document
        .descendants()
        .filter_map(|node| node.attribute("value").or_else(|| node.attribute("Value")))
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()
}

fn operation_boolean(kind: &str) -> BooleanOp {
    if kind.contains("Subtractive") {
        BooleanOp::Cut
    } else if kind.contains("Additive") {
        BooleanOp::Join
    } else {
        BooleanOp::NewBody
    }
}

fn feature_id(object: &ObjectRecord) -> FeatureId {
    FeatureId(format!("fcstd:design:feature#{}", object.name))
}

fn feature_base_definition(
    properties: &[&PropertyRecord],
    feature_ids: &HashMap<&str, FeatureId>,
) -> Option<FeatureDefinition> {
    let source = property(properties, "BaseFeature")?
        .links
        .first()?
        .object
        .as_deref()?;
    Some(FeatureDefinition::DerivedGeometry {
        source: feature_ids.get(source)?.clone(),
    })
}

fn linked_feature_ids(
    properties: &[&PropertyRecord],
    name: &str,
    feature_ids: &HashMap<&str, FeatureId>,
) -> Vec<FeatureId> {
    property(properties, name)
        .into_iter()
        .flat_map(|property| &property.links)
        .filter_map(|link| link.object.as_deref())
        .filter_map(|object| feature_ids.get(object).cloned())
        .collect()
}

fn imported_geometry_definition(
    kind: &str,
    properties: &[&PropertyRecord],
) -> Option<FeatureDefinition> {
    let path = property(properties, "FileName")
        .and_then(|property| string_property_value(property))?
        .to_owned();
    if path.is_empty() {
        return None;
    }
    let format = match kind {
        "Part::ImportStep" => GeometryImportFormat::Step,
        "Part::ImportIges" => GeometryImportFormat::Iges,
        "Part::ImportBrep" | "Part::CurveNet" => GeometryImportFormat::Brep,
        _ => return None,
    };
    Some(FeatureDefinition::ImportedGeometry { path, format })
}

fn is_sketch(kind: &str) -> bool {
    kind.contains("Sketcher::SketchObject")
}
fn is_datum(kind: &str) -> bool {
    matches!(
        kind,
        "PartDesign::Plane"
            | "PartDesign::Line"
            | "PartDesign::Point"
            | "PartDesign::CoordinateSystem"
    )
}
fn is_extrusion(kind: &str) -> bool {
    kind.contains("PartDesign::Pad")
        || kind.contains("PartDesign::Pocket")
        || kind.contains("Part::Extrusion")
}
fn is_hole(kind: &str) -> bool {
    kind == "PartDesign::Hole"
}
fn is_revolution(kind: &str) -> bool {
    kind.contains("PartDesign::Revolution")
        || kind.contains("PartDesign::Groove")
        || kind.contains("Part::Revolution")
}
fn is_primitive(kind: &str) -> bool {
    [
        "Box",
        "Cylinder",
        "Cone",
        "Sphere",
        "Ellipsoid",
        "Torus",
        "Prism",
        "Wedge",
    ]
    .iter()
    .any(|primitive| kind.ends_with(primitive))
        && (kind.starts_with("Part::") || kind.starts_with("PartDesign::"))
}
fn is_part_construction_geometry(kind: &str) -> bool {
    matches!(
        kind,
        "Part::Vertex"
            | "Part::Line"
            | "Part::Circle"
            | "Part::Ellipse"
            | "Part::Polygon"
            | "Part::RegularPolygon"
            | "Part::Plane"
            | "Part::Face"
    )
}
fn is_stored_geometry_feature(kind: &str) -> bool {
    matches!(
        kind,
        "Part::Feature"
            | "Part::FeatureExt"
            | "Part::FeatureGeometrySet"
            | "Part::Spline"
            | "Part::Part2DObject"
            | "PartDesign::Feature"
    )
}
fn is_imported_geometry(kind: &str) -> bool {
    matches!(
        kind,
        "Part::ImportStep" | "Part::ImportIges" | "Part::ImportBrep" | "Part::CurveNet"
    )
}
fn is_boolean(kind: &str) -> bool {
    if kind == "PartDesign::Boolean" {
        return true;
    }
    ["Cut", "Fuse", "MultiFuse", "Common", "MultiCommon"]
        .iter()
        .any(|operation| kind == format!("Part::{operation}"))
}
fn is_loft(kind: &str) -> bool {
    kind == "Part::Loft"
        || matches!(
            kind,
            "PartDesign::AdditiveLoft" | "PartDesign::SubtractiveLoft"
        )
}
fn is_sweep(kind: &str) -> bool {
    kind == "Part::Sweep"
        || matches!(
            kind,
            "PartDesign::AdditivePipe" | "PartDesign::SubtractivePipe"
        )
}
fn is_helical_sweep(kind: &str) -> bool {
    matches!(
        kind,
        "PartDesign::AdditiveHelix" | "PartDesign::SubtractiveHelix"
    )
}
fn is_parametric_helix(kind: &str) -> bool {
    matches!(kind, "Part::Helix" | "Part::Spiral")
}
fn is_binder(kind: &str) -> bool {
    matches!(
        kind,
        "PartDesign::ShapeBinder" | "PartDesign::SubShapeBinder"
    )
}
fn is_pattern(kind: &str) -> bool {
    matches!(
        kind,
        "PartDesign::LinearPattern"
            | "PartDesign::PolarPattern"
            | "PartDesign::Mirrored"
            | "PartDesign::Scaled"
            | "PartDesign::MultiTransform"
    )
}
fn is_dress_up(kind: &str) -> bool {
    kind.contains("Fillet")
        || kind.contains("Chamfer")
        || matches!(
            kind,
            "PartDesign::Thickness" | "PartDesign::Draft" | "Part::Thickness"
        )
}
fn is_body(kind: &str) -> bool {
    kind.contains("PartDesign::Body")
}
fn is_spreadsheet(kind: &str) -> bool {
    kind.contains("Spreadsheet::Sheet")
}
fn is_design_object(kind: &str) -> bool {
    is_spreadsheet(kind)
        || is_body(kind)
        || is_datum(kind)
        || is_sketch(kind)
        || is_primitive(kind)
        || is_part_construction_geometry(kind)
        || is_stored_geometry_feature(kind)
        || is_imported_geometry(kind)
        || is_boolean(kind)
        || is_loft(kind)
        || is_sweep(kind)
        || is_helical_sweep(kind)
        || is_parametric_helix(kind)
        || is_binder(kind)
        || is_pattern(kind)
        || kind == "Part::Scale"
        || is_hole(kind)
        || is_extrusion(kind)
        || is_revolution(kind)
        || is_dress_up(kind)
        || matches!(kind, "Part::Offset" | "Part::Offset2D")
        || matches!(
            kind,
            "Part::Compound" | "Part::Compound2" | "Part::Refine" | "Part::Reverse"
        )
        || matches!(
            kind,
            "Part::RuledSurface" | "Part::Section" | "Part::Mirroring" | "Part::ProjectOnSurface"
        )
        || kind == "PartDesign::FeatureBase"
}

pub(crate) fn census(
    objects: &[ObjectRecord],
    features: &[Feature],
) -> Result<Vec<crate::native::DesignCensusRecord>, CodecError> {
    let features = features
        .iter()
        .filter_map(|feature| {
            feature
                .native_ref
                .as_deref()
                .map(|native_ref| (native_ref, feature))
        })
        .collect::<HashMap<_, _>>();
    objects
        .iter()
        .filter(|object| is_design_object(&object.type_name))
        .map(|object| {
            let feature = features.get(object.id.as_str()).ok_or_else(|| {
                CodecError::Malformed(format!(
                    "design object {} has no neutral history projection",
                    object.id
                ))
            })?;
            let (definition, post_processed) = match &feature.definition {
                FeatureDefinition::PostProcess { operation, .. } => (operation.as_ref(), true),
                definition => (definition, false),
            };
            let value = serde_json::to_value(definition).map_err(|error| {
                CodecError::Malformed(format!(
                    "cannot classify design feature {}: {error}",
                    feature.id
                ))
            })?;
            let semantic_kind = value
                .get("definition")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "design feature {} has no semantic family tag",
                        feature.id
                    ))
                })?
                .to_owned();
            Ok(crate::native::DesignCensusRecord {
                id: crate::native::native_child_id("design-census", &object.id, "projection"),
                object: object.id.clone(),
                type_name: object.type_name.clone(),
                feature: feature.id.0.clone(),
                neutral: !matches!(definition, FeatureDefinition::Native { .. }),
                semantic_kind,
                post_processed,
            })
        })
        .collect()
}
