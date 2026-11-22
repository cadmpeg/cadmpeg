// SPDX-License-Identifier: Apache-2.0
//! The XML streams and fixed-form streams of a generated part
//! (`docs/formats/sldprt.md` §2 `KeyWords`, §8.2).

use super::{ids, Facts, Feature, MATERIAL_NAME, TIME_STAMP};

/// The byte that opens a `KeyWords` payload before its XML declaration.
const KEYWORDS_LEAD: u8 = 0x86;

/// The XML declaration of the `swXmlContents` payloads, with the line break
/// after it.
const XML_DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n";

/// The `swSolidWorks` namespace of `swXmlContents/Features`.
const FEATURES_NAMESPACE: &str = "http://www.solidworks.com/sw2003/schema";

/// One `KeyWords` record: element tag, identifier, name, type label (empty
/// for a user sketch, which writes none), whether it is dissectable, and its
/// dimensions.
struct Record<'a> {
    tag: &'static str,
    id: u32,
    name: &'a str,
    label: &'a str,
    dissectable: bool,
    dimensions: Vec<(&'static str, String)>,
}

/// `swXmlContents/KeyWords`: the lead byte, the XML declaration and a
/// `Keywords` element holding the configuration and one record per named
/// tree node, grouped by tag and ordered by identifier compared as text.
pub(super) fn keywords(facts: &Facts<'_>) -> Vec<u8> {
    let mut records = vec![
        Record {
            tag: "Feature",
            id: facts.id_base + ids::ANNOTATIONS,
            name: "Annotations",
            label: "Annotations",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::FRONT_PLANE,
            name: "Front Plane",
            label: "Plane",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::TOP_PLANE,
            name: "Top Plane",
            label: "Plane",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::RIGHT_PLANE,
            name: "Right Plane",
            label: "Plane",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Sketch",
            id: facts.id_base + ids::ORIGIN,
            name: "Origin",
            label: "Origin",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::LIGHTS,
            name: "Lights, Cameras and Scene",
            label: "Lights, Cameras and Scene",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::DESIGN_BINDER,
            name: "Design Binder",
            label: "Design Binder",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::COMMENTS,
            name: "Comments",
            label: "Comments",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::SOLID_BODIES,
            name: "Solid Bodies",
            label: "Solid Bodies",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::SURFACE_BODIES,
            name: "Surface Bodies",
            label: "Surface Bodies",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::MATERIAL,
            name: MATERIAL_NAME,
            label: "SOLIDWORKS Materials",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::AMBIENT,
            name: "Ambient",
            label: "Ambient",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::DIRECTIONAL_1,
            name: "Directional1",
            label: "Directional",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::DIRECTIONAL_2,
            name: "Directional2",
            label: "Directional",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::EQUATIONS,
            name: "Equations",
            label: "Equations",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::EXPLODED_VIEWS,
            name: "",
            label: "Exploded Views",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::SENSORS,
            name: "Sensors",
            label: "Sensors",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::FAVORITES,
            name: "Favorites",
            label: "Favorites",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::HISTORY,
            name: "History",
            label: "History",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::SELECTION_SETS,
            name: "Selection Sets",
            label: "Selection Sets",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::NOTES,
            name: "Notes",
            label: "Notes",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::NOTES_END,
            name: "Notes1___EndTag___",
            label: "Notes",
            dissectable: false,
            dimensions: Vec::new(),
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::MARKUPS,
            name: "Markups",
            label: "Markups",
            dissectable: false,
            dimensions: Vec::new(),
        },
    ];
    for body in facts.bodies {
        records.push(Record {
            tag: "Feature",
            id: body.feature_id,
            name: &body.name,
            label: "Imported",
            dissectable: false,
            dimensions: Vec::new(),
        });
    }
    for feature in facts.features {
        records.push(match feature {
            Feature::Sketch(sketch) => Record {
                tag: "Sketch",
                id: sketch.feature_id,
                name: &sketch.name,
                label: "",
                dissectable: true,
                dimensions: Vec::new(),
            },
            Feature::Extrusion(extrusion) => Record {
                tag: if extrusion.cut { "Cut" } else { "Extrusion" },
                id: extrusion.feature_id,
                name: &extrusion.name,
                label: if extrusion.cut {
                    "Cut-Extrude"
                } else {
                    "Boss-Extrude"
                },
                dissectable: false,
                dimensions: vec![("D1", format!("{}", extrusion.depth * MILLIMETRES_PER_METRE))],
            },
        });
    }
    records.sort_by(|left, right| {
        (left.tag, left.id.to_string()).cmp(&(right.tag, right.id.to_string()))
    });
    let mut xml = String::from(XML_DECLARATION);
    xml.push_str("<Keywords id=\"");
    xml.push_str(&TIME_STAMP.to_string());
    xml.push_str("\" Name=\"");
    push_escaped(&mut xml, facts.name);
    xml.push_str("\"><Configuration id=\"0\" Name=\"");
    push_escaped(&mut xml, facts.active);
    xml.push_str("\" Type=\"ConfigurationManager\" Material=\"");
    push_escaped(&mut xml, MATERIAL_NAME);
    xml.push_str("\"/>");
    for record in &records {
        xml.push('<');
        xml.push_str(record.tag);
        xml.push_str(" id=\"");
        xml.push_str(&record.id.to_string());
        xml.push_str("\" Name=\"");
        push_escaped(&mut xml, record.name);
        xml.push('"');
        if !record.label.is_empty() {
            xml.push_str(" Type=\"");
            push_escaped(&mut xml, record.label);
            xml.push('"');
        }
        if record.dissectable {
            xml.push_str(" Dissectable=\"true\"");
        }
        if record.dimensions.is_empty() {
            xml.push_str("/>");
            continue;
        }
        xml.push('>');
        for (name, text) in &record.dimensions {
            xml.push_str("<Dimension Name=\"");
            push_escaped(&mut xml, name);
            xml.push_str("\">");
            xml.push_str(text);
            xml.push_str("</Dimension>");
        }
        xml.push_str("</");
        xml.push_str(record.tag);
        xml.push('>');
    }
    xml.push_str("</Keywords>");
    let mut payload = vec![KEYWORDS_LEAD];
    payload.extend_from_slice(xml.as_bytes());
    payload
}

/// Metres to millimetres.
const MILLIMETRES_PER_METRE: f64 = 1000.0;

/// `swXmlContents/Features`: the `swSolidWorks` document listing the file,
/// its model and its configuration. The `swVersion` attribute is left out:
/// the writer's synthesis catalog is `sldprt:unknown`, which declares none.
pub(super) fn features(facts: &Facts<'_>) -> Vec<u8> {
    let path = format!("{}.SLDPRT", facts.name);
    let mut xml = String::from(XML_DECLARATION);
    xml.push_str("<swSolidWorks xmlns=\"");
    xml.push_str(FEATURES_NAMESPACE);
    xml.push_str("\" swObjCount=\"3\"><swHeader swObjCount=\"1\"><swFile id=\"3\" swDocType=\"PART\" swCreationTime=\"");
    xml.push_str(&TIME_STAMP.to_string());
    xml.push_str("\" swPath=\"");
    push_escaped(&mut xml, &path);
    xml.push_str("\"/></swHeader><swModelList swObjCount=\"1\"><swModel id=\"2\" swName=\"");
    push_escaped(&mut xml, &path);
    xml.push_str("\" swConfigurationName=\"");
    push_escaped(&mut xml, facts.active);
    xml.push_str("\" swConfigurationId=\"0\" swLastModifiedStamp=\"1\" swConfigurationFlags=\"0\" swFileRef=\"3\"/></swModelList><swConfigurationList swObjCount=\"1\"><swConfiguration id=\"1\" swName=\"");
    push_escaped(&mut xml, facts.active);
    xml.push_str("\" swID=\"0\" swReference=\"");
    push_escaped(&mut xml, facts.name);
    xml.push_str("\" swMostRecentConfiguration=\"YES\" swConfigurationNeedsUpdate=\"NO\" swModelRef=\"2\"/></swConfigurationList><swExtFeatureList swObjCount=\"0\"/></swSolidWorks>");
    xml.into_bytes()
}

/// `Contents/View Orientation Data`: an empty `VIEWS` document.
pub(super) fn view_orientations() -> Vec<u8> {
    b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<VIEWS/>\n".to_vec()
}

/// `Contents/CnfgObjs` of a part with no configuration objects: u32 `0` and
/// an empty `CString`.
pub(super) fn configuration_objects() -> Vec<u8> {
    vec![0, 0, 0, 0, 0xff, 0xfe, 0xff, 0]
}

/// Escapes `value` as XML attribute text, leaving out the characters XML 1.0
/// does not admit.
fn push_escaped(output: &mut String, value: &str) {
    for character in value.chars() {
        match character {
            '\t' | '\n' | '\r' => output.push(' '),
            character if u32::from(character) < 0x20 => {}
            '\u{fffe}' | '\u{ffff}' => {}
            '&' => output.push_str("&amp;"),
            '"' => output.push_str("&quot;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            _ => output.push(character),
        }
    }
}
