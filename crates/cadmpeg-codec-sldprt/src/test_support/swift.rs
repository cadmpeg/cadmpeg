use super::container::{make_block, sldprt_with_body};
use super::parasolid::owned_triangle;

struct Entity {
    class: &'static str,
    strings: Vec<(&'static str, &'static str)>,
    doubles: Vec<(&'static str, f64)>,
    features: Section,
    annotations: Section,
    related: Vec<RelatedObject>,
}

#[derive(Default)]
struct Section {
    references: Vec<(&'static str, &'static str)>,
    entities: Vec<Entity>,
}

struct RelatedObject {
    name: &'static str,
    class: &'static str,
    entity: Entity,
}

impl Entity {
    fn new(class: &'static str) -> Self {
        Self {
            class,
            strings: Vec::new(),
            doubles: Vec::new(),
            features: Section::default(),
            annotations: Section::default(),
            related: Vec::new(),
        }
    }
}

pub(crate) fn empty_hole_pattern_nominal_fixture() -> Vec<u8> {
    let mut cad_reference = Entity::new("PrizMetrik.GdtAnalysis.CadRef");
    cad_reference.strings.push(("CadIdentifier", ""));
    let mut cad_references = Entity::new("PrizMetrik.GdtAnalysis.CadRefCollection");
    cad_references.related.push(RelatedObject {
        name: "CadRef0",
        class: "PrizMetrik.GdtAnalysis.CadRef",
        entity: cad_reference,
    });

    let mut pattern = Entity::new("PrizMetrik.GdtAnalysis.GdtPattern");
    pattern.strings.push(("ObjectName", "Hole Pattern6"));
    pattern.related.push(RelatedObject {
        name: "SubFeatures",
        class: "PrizMetrik.GdtAnalysis.GdtAppliedFeatureCollection",
        entity: Entity::new("PrizMetrik.GdtAnalysis.GdtAppliedFeatureCollection"),
    });
    pattern.related.push(RelatedObject {
        name: "CadReferences",
        class: "PrizMetrik.GdtAnalysis.CadRefCollection",
        entity: cad_references,
    });

    let mut diameter = Entity::new("PrizMetrik.GdtAnalysis.GdtDiameter");
    diameter.strings.push(("ObjectName", "Diameter8"));
    diameter.doubles.push(("Nominal", 0.0));
    diameter
        .features
        .references
        .push(("FP", "PrizMetrik.GdtAnalysis.GdtPattern,gdtanalysis.net"));

    let mut root = Entity::new("PrizMetrik.GdtAnalysisSupport.GdtPart");
    root.features
        .references
        .push(("FP", "PrizMetrik.GdtAnalysis.GdtPattern,gdtanalysis.net"));
    root.features.entities.push(pattern);
    root.annotations
        .references
        .push(("A8", "PrizMetrik.GdtAnalysis.GdtDiameter,gdtanalysis.net"));
    root.annotations.entities.push(diameter);

    let mut payload = vec![0x11, 0x22, 0x33];
    encode_entity(&root, &mut payload);

    let body = owned_triangle(0, 700, 0.0);
    let mut source = sldprt_with_body(&body);
    source.extend(make_block(
        0x42,
        "Contents/Keywords",
        br#"<Keywords>
            <Feature Name="Seed" Type="NativeSeed" id="1"/>
            <Pattern Name="LPattern6" Type="LinearPattern" id="2" Seeds="1" Direction="1,0,0">
                <Dimension Name="Spacing">1mm</Dimension>
                <Dimension Name="Count">2</Dimension>
            </Pattern>
            <Hole Name="Hole6" Type="Hole" id="3" Profile="1">
                <Dimension Name="Diameter">6mm</Dimension>
                <Dimension Name="Depth">5mm</Dimension>
            </Hole>
        </Keywords>"#,
    ));
    source.extend(make_block(0x40, "SWIFT/Schema", &payload));
    source
}

fn encode_entity(entity: &Entity, bytes: &mut Vec<u8>) {
    put_pstr(bytes, "Entity");
    put_pstr(bytes, entity.class);
    put_pstr(bytes, "gdtanalysis.net");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    encode_strings("Strings", "EndStrings", &entity.strings, bytes);
    encode_doubles("Doubles", "EndDoubles", &entity.doubles, bytes);
    encode_section("Features", "EndFeatures", &entity.features, bytes);
    encode_section("Annotations", "EndAnnotations", &entity.annotations, bytes);
    if !entity.related.is_empty() {
        put_pstr(bytes, "RelatedObjects");
        push_count(bytes, entity.related.len());
        for object in &entity.related {
            put_pstr(bytes, object.name);
            put_pstr(bytes, object.class);
        }
        for object in &entity.related {
            encode_entity(&object.entity, bytes);
        }
        put_pstr(bytes, "EndRelatedObjects");
    }
    put_pstr(bytes, "EndEntity");
}

fn encode_strings(name: &str, end: &str, values: &[(&str, &str)], bytes: &mut Vec<u8>) {
    if values.is_empty() {
        return;
    }
    put_pstr(bytes, name);
    push_count(bytes, values.len());
    for (key, value) in values {
        put_pstr(bytes, key);
        put_pstr(bytes, value);
    }
    put_pstr(bytes, end);
}

fn encode_doubles(name: &str, end: &str, values: &[(&str, f64)], bytes: &mut Vec<u8>) {
    if values.is_empty() {
        return;
    }
    put_pstr(bytes, name);
    push_count(bytes, values.len());
    for (key, value) in values {
        put_pstr(bytes, key);
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    put_pstr(bytes, end);
}

fn encode_section(name: &str, end: &str, section: &Section, bytes: &mut Vec<u8>) {
    if section.references.is_empty() && section.entities.is_empty() {
        return;
    }
    put_pstr(bytes, name);
    push_count(bytes, section.references.len());
    for (id, class) in &section.references {
        put_pstr(bytes, id);
        put_pstr(bytes, class);
    }
    for entity in &section.entities {
        encode_entity(entity, bytes);
    }
    put_pstr(bytes, end);
}

fn put_pstr(bytes: &mut Vec<u8>, value: &str) {
    bytes.push(u8::try_from(value.len()).expect("SWIFT fixture string fits Pascal length"));
    bytes.extend_from_slice(value.as_bytes());
}

fn push_count(bytes: &mut Vec<u8>, count: usize) {
    bytes.extend_from_slice(
        &u32::try_from(count)
            .expect("SWIFT fixture count fits u32")
            .to_le_bytes(),
    );
}
