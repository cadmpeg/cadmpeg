// SPDX-License-Identifier: Apache-2.0
//! Synthetic evaluated assembly definitions.

use super::step;
use serde_json::{json, Value};
use std::io::{Cursor, Write};

pub(crate) fn placement(x: f64, y: f64) -> Value {
    json!([0., -1., 0., x, 1., 0., 0., y, 0., 0., 1., 0., 0., 0., 0., 1.])
}
pub(crate) fn part(id: &str) -> Value {
    json!({"id":id,"type":"Part","name":id,"documentId":"document","elementId":"studio","partId":"part","configuration":"default","suppressed":false})
}
pub(crate) fn mate(id: &str, kind: &str) -> Value {
    let cs = json!({"origin":[0.001,0.002,0.003],"xAxis":[1.,0.,0.],"yAxis":[0.,1.,0.],"zAxis":[0.,0.,1.]});
    json!({"id":id,"featureType":"mate","suppressed":false,"featureData":{"mateType":kind,"matedEntities":[
        {"matedOccurrence":["first"],"matedCS":cs},
        {"matedOccurrence":["second"],"matedCS":cs}
    ]}})
}
pub(crate) fn definition() -> Value {
    json!({"rootAssembly":{"instances":[part("first"),part("second")],"occurrences":[
        {"path":["first"],"transform":placement(0.01,0.02),"fixed":true},
        {"path":["second"],"transform":placement(0.03,0.04),"fixed":false}
    ],"features":[]},"subAssemblies":[],"parts":[{"documentId":"document","elementId":"studio","partId":"part","configuration":"default","name":"unit cube"}]})
}
pub(crate) fn bundle(definition: Value) -> Vec<u8> {
    bundle_with_studios(definition, false)
}
pub(crate) fn bundle_with_studios(definition: Value, extra: bool) -> Vec<u8> {
    let mut members = json!({
        "manifest.json":{"schema":"dutchman.onshape-bundle/1","document":{"id":"document","microversion":"microversion"},"root_element":"assembly","elements":[{"id":"assembly","type":"ASSEMBLY"},{"id":"studio","type":"PARTSTUDIO"}]},
        "assemblies/assembly/definition.json":null,
        "assemblies/assembly/features.json":{"features":[]},
        "partstudios/studio/features.json":{"features":[]},
        "partstudios/studio/bodydetails/index.json":[]
    });
    members["assemblies/assembly/definition.json"] = definition;
    if extra {
        members["manifest.json"]["elements"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"other","type":"PARTSTUDIO"}));
        members["partstudios/other/features.json"] = json!({"features":[]});
    }
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, value) in members.as_object().unwrap() {
        zip.start_file(name, options).unwrap();
        zip.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
    }
    zip.start_file("partstudios/studio/export.step", options)
        .unwrap();
    zip.write_all(step()).unwrap();
    if extra {
        zip.start_file("partstudios/other/export.step", options)
            .unwrap();
        zip.write_all(step()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
