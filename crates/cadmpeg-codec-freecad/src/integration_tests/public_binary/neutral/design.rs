// SPDX-License-Identifier: Apache-2.0
//! Independent effects of binary construction controls on the BEFORE document.
use crate::native::entry_contents::EntryContents;
use crate::native::{EntryRecord, ObjectRecord, ProductNodeRecord, PropertyRecord};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::CadIr;
use cadmpeg_test_support::framing::freecad::binary::BinaryKind;
use std::collections::BTreeMap;

fn property<'a>(
    properties: &'a [PropertyRecord],
    owner: &str,
    name: &str,
) -> Option<&'a PropertyRecord> {
    let mut found = properties
        .iter()
        .filter(|property| property.owner == owner && property.name == name);
    let value = found.next();
    assert!(
        found.next().is_none(),
        "independent construction controls are unique"
    );
    value
}

fn scalar(properties: &[PropertyRecord], owner: &str, name: &str) -> Option<f64> {
    let value = property(properties, owner, name)?
        .values()
        .iter()
        .find(|value| value.parent.is_none())?;
    value.attributes.get("value")?.parse().ok()
}

fn contents<'a>(
    entries: &'a [EntryRecord],
    property: &PropertyRecord,
) -> Option<&'a EntryContents> {
    let file = property
        .values()
        .iter()
        .find(|value| value.parent.is_none())?
        .attributes
        .get("file")?;
    let entry = entries.iter().find(|entry| entry.name() == file)?;
    entry.contents().iter().find(|content| {
        matches!(
            (property.type_name.as_str(), content),
            ("App::PropertyFloatList", EntryContents::Float { .. })
                | ("App::PropertyVectorList", EntryContents::Vector { .. })
                | (
                    "App::PropertyPlacementList",
                    EntryContents::Placement { .. }
                )
                | ("Part::PropertyFilletEdges", EntryContents::Fillet { .. })
        )
    })
}

fn numbers(rows: &[EntryRecord], property: Option<&PropertyRecord>) -> Vec<f64> {
    let Some(EntryContents::Float { values }) =
        property.and_then(|property| contents(rows, property))
    else {
        return Vec::new();
    };
    values
        .iter()
        .map(|value| value.expect("valid independent float slot").get())
        .collect()
}

fn unavailable_intervals(
    properties: &[PropertyRecord],
    rows: &[EntryRecord],
    owner: &str,
    suffix: &str,
) -> bool {
    let name = |base: &str| format!("{base}{suffix}");
    let count = property(properties, owner, &name("Occurrences"))
        .and_then(|property| {
            property
                .values()
                .iter()
                .find(|value| value.parent.is_none())
        })
        .and_then(|value| value.attributes.get("value"))
        .map(|value| {
            value
                .parse::<usize>()
                .expect("actual nonnegative integer occurrence count")
        })
        .unwrap_or(if suffix.is_empty() { 2 } else { 1 });
    if count <= 1 || scalar(properties, owner, &name("Mode")).unwrap_or(0.0) != 1.0 {
        return false;
    }
    let spacings = numbers(rows, property(properties, owner, &name("Spacings")));
    let repeated = numbers(rows, property(properties, owner, &name("SpacingPattern")));
    let fallback = scalar(properties, owner, &name("Offset"))
        .expect("spacing mode has an actual scalar interval");
    (0..count - 1).any(|gap| {
        let interval = spacings
            .get(gap)
            .copied()
            .filter(|value| *value != -1.0)
            .unwrap_or_else(|| {
                if repeated.len() > 1 {
                    repeated[gap % repeated.len()]
                } else {
                    fallback
                }
            });
        interval <= 0.0
    })
}

fn replace_operation(definition: &mut FeatureDefinition, operation: FeatureOperation) {
    match definition {
        FeatureDefinition::Operation(current)
        | FeatureDefinition::PostProcess {
            operation: current, ..
        } => *current = operation,
    }
}

fn native_pattern(
    ir: &CadIr,
    feature: &cadmpeg_ir::features::Feature,
    object: &ObjectRecord,
    properties: &[PropertyRecord],
) -> FeatureOperation {
    // These registered scalar controls retain their exact source meanings.
    // Direction/Originals are references, not copied scalar parameters.
    let names = [
        "Length",
        "Length2",
        "Offset",
        "Offset2",
        "Mode",
        "Mode2",
        "Occurrences",
        "Occurrences2",
        "Refine",
        "Reversed",
        "Reversed2",
        "TransformMode",
    ];
    let mut parameters = BTreeMap::new();
    for name in names {
        let Some(property) = property(properties, object.id(), name) else {
            continue;
        };
        let Some(text) = property
            .values()
            .iter()
            .find(|value| value.parent.is_none())
            .and_then(|value| value.attributes.get("value"))
        else {
            continue;
        };
        let value = ir
            .model
            .parameters
            .iter()
            .find(|parameter| {
                parameter.owner.as_ref() == Some(&feature.id) && parameter.name == name
            })
            .map_or_else(
                || cadmpeg_ir::Dimension::Literal(text.clone().into()),
                |parameter| cadmpeg_ir::Dimension::Parameter(parameter.id.clone()),
            );
        parameters.insert(
            cadmpeg_core::text::NonBlankString::new(name).unwrap(),
            value,
        );
    }
    FeatureOperation::Native {
        kind: object.type_name.clone().into(),
        parameters,
    }
}

fn scales(
    ir: &mut CadIr,
    owner: &str,
    rows: &[EntryRecord],
    source: &PropertyRecord,
) -> Result<(), CodecError> {
    let Some(EntryContents::Vector { values, .. }) = contents(rows, source) else {
        panic!("actual independent Vector entry");
    };
    let values = values
        .iter()
        .enumerate()
        .map(|(ordinal, value)| {
            (
                ordinal,
                value
                    .expect("valid independent vector slot")
                    .map(|value| value.get()),
            )
        })
        .collect::<Vec<_>>();
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut products = ir
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<ProductNodeRecord>("product_nodes")?;
    let product = products
        .iter_mut()
        .find(|product| product.object == owner)
        .expect("ScaleList has a real product owner");
    let previous = product
        .element_scales()
        .iter()
        .copied()
        .map(|value| value.expect("valid baseline scale slot").get())
        .collect::<Vec<_>>();
    assert_eq!(values.len(), previous.len());
    for (index, (order, value)) in values.iter().enumerate() {
        assert_eq!(index, *order);
        let identity = crate::native::model_id("occurrence", owner, index.to_string());
        for occurrence in ir.model.occurrences.iter_mut().filter(|occurrence| {
            occurrence.native_ref.as_deref() == Some(owner) && occurrence.id.as_str() == identity
        }) {
            if let Some(transform) = occurrence.transform {
                let mut matrix = transform.affine_rows();
                for axis in 0..3 {
                    let old = previous[index][axis];
                    let new = value[axis];
                    if old == new {
                        continue;
                    }
                    if old == 0.0 {
                        return Err(CodecError::malformed(
                            "independent scale oracle cannot invert a previously collapsed axis",
                        ));
                    }
                    for row in &mut matrix {
                        row[axis] *= new / old;
                    }
                }
                occurrence.transform = Some(
                    cadmpeg_ir::transform::Transform::affine(matrix)
                        .expect("independent finite scaled occurrence"),
                );
            }
        }
    }
    let array = crate::native::LinkArray::from_admitted(
        product.element_count(),
        Some(product.element_transforms().to_vec()),
        Some(
            values
                .into_iter()
                .map(|(_, value)| Some(cadmpeg_ir::units::FiniteVector::new(value).unwrap()))
                .collect(),
        ),
        product.element_visibility().to_vec(),
        product.element_objects().to_vec(),
    );
    let crate::native::ProductNode::Occurrence(node) = &mut product.node else {
        panic!("ScaleList requires a link occurrence");
    };
    node.array = array;
    ir.native
        .namespace_mut("fcstd")
        .set_arena(&ctx, "product_nodes", &products)?;
    Ok(())
}

fn unchanged_placements(
    ir: &CadIr,
    rows: &[EntryRecord],
    source: &PropertyRecord,
) -> Result<(), CodecError> {
    let products = ir
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<ProductNodeRecord>("product_nodes")?;
    let product = products
        .iter()
        .find(|row| row.object == source.owner)
        .expect("actual PlacementList product owner");
    let Some(EntryContents::Placement { values, .. }) = contents(rows, source) else {
        panic!("actual independent Placement entry");
    };
    assert_eq!(product.element_transforms().len(), values.len());
    for (ordinal, value) in values.iter().enumerate() {
        let [px, py, pz, x, y, z, w] = value
            .expect("valid independent placement slot")
            .map(|value| value.get());
        let norm = x * x + y * y + z * z + w * w;
        assert!(
            norm > 0.0,
            "zero quaternion is a source failure, not a valid value mutation"
        );
        let scale = 2.0 / norm;
        let expected = [
            [
                1.0 - scale * (y * y + z * z),
                scale * (x * y - z * w),
                scale * (x * z + y * w),
                px,
            ],
            [
                scale * (x * y + z * w),
                1.0 - scale * (x * x + z * z),
                scale * (y * z - x * w),
                py,
            ],
            [
                scale * (x * z - y * w),
                scale * (y * z + x * w),
                1.0 - scale * (x * x + y * y),
                pz,
            ],
            [0.0, 0.0, 0.0, 1.0],
        ];
        assert_eq!(
            product.element_transforms()[ordinal]
                .expect("valid placement slot")
                .rows(),
            expected,
            "valid public Placement rows preserve their independently known transforms"
        );
    }
    Ok(())
}

pub(super) fn update(
    ir: &mut CadIr,
    entry: &EntryRecord,
    kind: BinaryKind,
) -> Result<(), CodecError> {
    if !matches!(
        kind,
        BinaryKind::Float | BinaryKind::Vector | BinaryKind::Fillet | BinaryKind::Placement
    ) {
        return Ok(());
    }
    let namespace = ir.native.namespace("fcstd").unwrap();
    let rows = namespace.arena_as::<EntryRecord>("entries")?;
    let properties = namespace.arena_as::<PropertyRecord>("properties")?;
    let objects = namespace.arena_as::<ObjectRecord>("objects")?;
    for source in properties.iter().filter(|property| {
        property
            .side_entries()
            .iter()
            .any(|name| name == entry.name())
    }) {
        let content = contents(&rows, source);
        let empty = match content {
            Some(EntryContents::Float { values }) => values.is_empty(),
            Some(EntryContents::Vector { values, .. }) => values.is_empty(),
            Some(EntryContents::Placement { values, .. }) => values.is_empty(),
            Some(EntryContents::Fillet { values }) => values.is_empty(),
            _ => false,
        };
        if !matches!(source.name.as_str(), "ScaleList" | "PlacementList") && empty {
            continue;
        }
        let object = objects
            .iter()
            .find(|object| object.id() == &source.owner)
            .expect("binary property has an actual application object");
        match (kind, source.name.as_str(), object.type_name.as_str()) {
            (BinaryKind::Placement, "PlacementList", "App::Link" | "App::LinkElement") => {
                unchanged_placements(ir, &rows, source)?
            }
            (BinaryKind::Vector, "ScaleList", "App::Link" | "App::LinkElement") => {
                scales(ir, object.id(), &rows, source)?
            }
            // BoxCorners is retained binary application content. No registered
            // neutral dimension geometry consumes this vector-list field.
            (BinaryKind::Vector, "BoxCorners", "TechDraw::DrawViewDimension") => {}
            (BinaryKind::Fillet, "Edges", "Part::Fillet" | "Part::Chamfer") => {
                let invalid = match content {
                    Some(EntryContents::Fillet { values }) => values
                        .iter()
                        .flatten()
                        .any(|value| value.radius1.get() <= 0.0 || value.radius2.get() <= 0.0),
                    _ => false,
                };
                if invalid {
                    assert!(
                        property(&properties, object.id(), "Shape")
                            .is_some_and(|property| !property.side_entries().is_empty()),
                        "invalid radii retain an actual held cached shape"
                    );
                    for feature in ir
                        .model
                        .features
                        .iter_mut()
                        .filter(|feature| feature.native_ref.as_deref() == Some(object.id()))
                    {
                        feature.evaluation.edit(|definition, _| {
                            replace_operation(definition, FeatureOperation::StoredGeometry {})
                        });
                        // #209 D26: StoredGeometry has no construction operands.
                        feature.dependencies.clear();
                    }
                }
            }
            (
                BinaryKind::Float,
                "SpacingPattern" | "SpacingPattern2" | "Spacings" | "Spacings2",
                "PartDesign::LinearPattern" | "PartDesign::PolarPattern",
            ) => {
                if unavailable_intervals(&properties, &rows, object.id(), "")
                    || unavailable_intervals(&properties, &rows, object.id(), "2")
                {
                    let changes = ir
                        .model
                        .features
                        .iter()
                        .enumerate()
                        .filter(|(_, feature)| feature.native_ref.as_deref() == Some(object.id()))
                        .map(|(index, feature)| {
                            (index, native_pattern(ir, feature, object, &properties))
                        })
                        .collect::<Vec<_>>();
                    for (index, operation) in changes {
                        ir.model.features[index]
                            .evaluation
                            .edit(|definition, _| replace_operation(definition, operation));
                    }
                }
            }
            _ => {
                return Err(CodecError::malformed(format_args!(
                    "independent neutral binary oracle has no rule for {}.{} ({kind:?})",
                    object.type_name, source.name
                )))
            }
        }
    }
    // The existing census names semantic feature families. It is native
    // document content, so a changed neutral family changes this field too.
    let mut census = ir
        .native
        .namespace("fcstd")
        .unwrap()
        .arena_as::<crate::native::DesignCensusRecord>("design_census")?;
    for row in &mut census {
        if let Some(feature) = ir
            .model
            .features
            .iter()
            .find(|feature| feature.native_ref.as_deref() == Some(row.object.as_str()))
        {
            let (operation, post_processed) = match feature.evaluation.definition() {
                FeatureDefinition::Operation(operation) => (operation, false),
                FeatureDefinition::PostProcess { operation, .. } => (operation, true),
            };
            match operation {
                FeatureOperation::Native { .. } => row.semantic_kind = "native".into(),
                FeatureOperation::StoredGeometry { .. } => {
                    row.semantic_kind = "stored_geometry".into()
                }
                _ => continue,
            }
            row.post_processed = post_processed;
        }
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    ir.native
        .namespace_mut("fcstd")
        .set_arena(&ctx, "design_census", &census)?;
    Ok(())
}
