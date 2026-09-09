use super::*;

mod identity;
mod nominals;
mod parsing;

fn dimension_nominal(annotations: &[PmiAnnotation], id: &str) -> Option<PmiValue> {
    let PmiDefinition::Dimension { nominal, .. } = &annotations
        .iter()
        .find(|annotation| annotation.id == pmi_id(id).unwrap())
        .expect("dimension annotation")
        .definition
    else {
        panic!("dimension definition");
    };
    *nominal
}

fn reference(id: &str, class: &str) -> Reference {
    Reference {
        id: id.into(),
        class: format!("PrizMetrik.GdtAnalysis.{class},gdtanalysis.net"),
    }
}

fn entity(class: &str) -> Entity {
    Entity {
        class: format!("PrizMetrik.GdtAnalysis.{class}"),
        ..Entity::default()
    }
}

fn semantic_root() -> Entity {
    let mut datum = entity("GdtDatum");
    datum.strings.insert("ObjectName".into(), "Datum A".into());
    datum.strings.insert("DatumIdentifier".into(), "A".into());
    datum.features.references.push(reference("F10", "GdtPlane"));

    let mut applied = entity("GdtAppliedDatum");
    applied.integers.insert("Modifier".into(), 2);
    applied
        .annotations
        .references
        .push(reference("A10", "GdtDatum"));
    let mut collection = entity("GdtAppliedDatumCollection");
    collection.related.push(RelatedObject {
        name: "SubAnnotation0".into(),
        class: "PrizMetrik.GdtAnalysis.GdtAppliedDatum".into(),
        entity: applied,
    });

    let mut position = entity("GdtPosition");
    position
        .strings
        .insert("ObjectName".into(), "Position 1".into());
    position.integers.insert("Modifier".into(), 1);
    position.integers.insert("ProjectedZoneEnabled".into(), 1);
    position.doubles.insert("Tolerance".into(), 0.25);
    position.doubles.insert("ProjectedZoneValue".into(), 4.0);
    position
        .features
        .references
        .push(reference("FP", "GdtPattern"));
    position.related.push(RelatedObject {
        name: "PrimaryDatums".into(),
        class: "PrizMetrik.GdtAnalysis.GdtAppliedDatumCollection".into(),
        entity: collection,
    });

    let mut diameter = entity("GdtDiameter");
    diameter
        .strings
        .insert("ObjectName".into(), "Diameter 1".into());
    diameter.doubles.insert("Nominal".into(), 0.0);
    diameter.doubles.insert("MinusTolerance".into(), -0.1);
    diameter.doubles.insert("PlusTolerance".into(), 0.2);
    diameter.doubles.insert("LowerLimit".into(), 0.0);
    diameter.doubles.insert("UpperLimit".into(), 0.0);
    diameter
        .features
        .references
        .push(reference("F20", "GdtCylinder"));

    let mut angle = entity("GdtAngleBetween");
    angle.integers.insert("Dimension".into(), 1);
    angle.doubles.insert("Nominal".into(), 0.0);
    angle.doubles.insert("MinusTolerance".into(), -0.01);
    angle.doubles.insert("PlusTolerance".into(), 0.01);
    angle.doubles.insert("LowerLimit".into(), 0.0);
    angle.doubles.insert("UpperLimit".into(), 0.0);

    let mut root = Entity {
        class: ROOT_CLASS.into(),
        ..Entity::default()
    };
    let cylinder = entity("GdtCylinder");
    let second_cylinder = cylinder.clone();
    let mut subfeatures = entity("GdtAppliedFeatureCollection");
    for (ordinal, id) in ["F20", "F21"].into_iter().enumerate() {
        let mut applied = entity("GdtAppliedFeature");
        applied
            .features
            .references
            .push(reference(id, "GdtCylinder"));
        subfeatures.related.push(RelatedObject {
            name: format!("SubFeature{ordinal}"),
            class: "PrizMetrik.GdtAnalysis.GdtAppliedFeature".into(),
            entity: applied,
        });
    }
    let mut pattern = entity("GdtPattern");
    pattern.related.push(RelatedObject {
        name: "SubFeatures".into(),
        class: "PrizMetrik.GdtAnalysis.GdtAppliedFeatureCollection".into(),
        entity: subfeatures,
    });
    root.features.references = vec![
        reference("FP", "GdtPattern"),
        reference("F20", "GdtCylinder"),
        reference("F21", "GdtCylinder"),
    ];
    root.features.entities = vec![pattern, cylinder, second_cylinder];
    root.annotations.references = vec![
        reference("A10", "GdtDatum"),
        reference("A20", "GdtPosition"),
        reference("A30", "GdtDiameter"),
        reference("A40", "GdtAngleBetween"),
    ];
    root.annotations.entities = vec![datum, position, diameter, angle];
    root
}

fn neutral_feature(
    id: &str,
    name: &str,
    ordinal: u64,
    dependencies: Vec<cadmpeg_ir::features::FeatureId>,
    definition: cadmpeg_ir::features::FeatureDefinition,
) -> cadmpeg_ir::features::Feature {
    cadmpeg_ir::features::Feature {
        id: cadmpeg_ir::features::FeatureId::mint(format!("sldprt:model:feature#{id}"))
            .expect("identity grammar"),
        ordinal,
        name: Some(name.into()),
        suppressed: None,
        dependencies: (dependencies).try_into().unwrap(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(definition),
        native_ref: Some(format!("sldprt:history:feature#{id}")),
    }
}

fn simple_hole_definition(diameter: f64) -> cadmpeg_ir::features::FeatureDefinition {
    use cadmpeg_ir::features::{FeatureDefinition, HoleKind};

    FeatureDefinition::Hole {
        profile: None,
        profile_filter: None,
        face: None,
        direction: None,
        placements: None,
        shape: cadmpeg_ir::features::HoleShape::new(
            cadmpeg_ir::features::HoleConstruction::form(HoleKind::Simple),
            None,
            Some(cadmpeg_ir::features::PositiveLength::new(diameter).unwrap()),
        )
        .unwrap(),

        extent: None,
        bottom: None,
        taper_angle: None,
        allow_multi_profile_faces: None,
    }
}
