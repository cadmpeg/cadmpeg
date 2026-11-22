// SPDX-License-Identifier: Apache-2.0
//! The XML streams and fixed-form streams of a generated part
//! (`docs/formats/sldprt.md` §2 `KeyWords`, §8.2).

use super::{ids, Facts, MATERIAL_NAME, TIME_STAMP};

/// The byte that opens a `KeyWords` payload before its XML declaration.
const KEYWORDS_LEAD: u8 = 0x86;

/// The XML declaration of the `swXmlContents` payloads, with the line break
/// after it.
const XML_DECLARATION: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n";

/// The `swSolidWorks` namespace of `swXmlContents/Features`.
const FEATURES_NAMESPACE: &str = "http://www.solidworks.com/sw2003/schema";

/// One `KeyWords` record: element tag, identifier, name and type label.
struct Record<'a> {
    tag: &'static str,
    id: u32,
    name: &'a str,
    label: &'a str,
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
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::FRONT_PLANE,
            name: "Front Plane",
            label: "Plane",
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::TOP_PLANE,
            name: "Top Plane",
            label: "Plane",
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::RIGHT_PLANE,
            name: "Right Plane",
            label: "Plane",
        },
        Record {
            tag: "Sketch",
            id: facts.id_base + ids::ORIGIN,
            name: "Origin",
            label: "Origin",
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::LIGHTS,
            name: "Lights, Cameras and Scene",
            label: "Lights, Cameras and Scene",
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::DESIGN_BINDER,
            name: "Design Binder",
            label: "Design Binder",
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::COMMENTS,
            name: "Comments",
            label: "Comments",
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::SOLID_BODIES,
            name: "Solid Bodies",
            label: "Solid Bodies",
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::SURFACE_BODIES,
            name: "Surface Bodies",
            label: "Surface Bodies",
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::MATERIAL,
            name: MATERIAL_NAME,
            label: "SOLIDWORKS Materials",
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::AMBIENT,
            name: "Ambient",
            label: "Ambient",
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::DIRECTIONAL_1,
            name: "Directional1",
            label: "Directional",
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::DIRECTIONAL_2,
            name: "Directional2",
            label: "Directional",
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::EQUATIONS,
            name: "Equations",
            label: "Equations",
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::EXPLODED_VIEWS,
            name: "",
            label: "Exploded Views",
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::SENSORS,
            name: "Sensors",
            label: "Sensors",
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::FAVORITES,
            name: "Favorites",
            label: "Favorites",
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::HISTORY,
            name: "History",
            label: "History",
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::SELECTION_SETS,
            name: "Selection Sets",
            label: "Selection Sets",
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::NOTES,
            name: "Notes",
            label: "Notes",
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::NOTES_END,
            name: "Notes1___EndTag___",
            label: "Notes",
        },
        Record {
            tag: "Feature",
            id: facts.id_base + ids::MARKUPS,
            name: "Markups",
            label: "Markups",
        },
    ];
    for body in facts.bodies {
        records.push(Record {
            tag: "Feature",
            id: body.feature_id,
            name: &body.name,
            label: "Imported",
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
        xml.push_str("\" Type=\"");
        push_escaped(&mut xml, record.label);
        xml.push_str("\"/>");
    }
    xml.push_str("</Keywords>");
    let mut payload = vec![KEYWORDS_LEAD];
    payload.extend_from_slice(xml.as_bytes());
    payload
}

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
