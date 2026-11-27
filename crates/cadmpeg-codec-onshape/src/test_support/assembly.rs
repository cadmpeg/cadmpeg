// SPDX-License-Identifier: Apache-2.0
//! Synthetic evaluated assembly definitions.

use super::step;
use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json;
use serde_json::Value;
use std::io::{Cursor, Read, Write};

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
    bundle_with_other_studio(definition, extra.then(step))
}
/// A bundle whose second studio, `other`, exports the STEP text `other`
/// states, where `other` is given.
pub(crate) fn bundle_with_other_studio(definition: Value, other: Option<&[u8]>) -> Vec<u8> {
    let extra = other.is_some();
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
    pack(&members, other)
}
pub(crate) fn pack(members: &Value, other: Option<&[u8]>) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, value) in members.as_object().unwrap() {
        zip.start_file(name, options).unwrap();
        zip.write_all(&json::to_vec(value).unwrap()).unwrap();
    }
    zip.start_file("partstudios/studio/export.step", options)
        .unwrap();
    zip.write_all(step()).unwrap();
    if let Some(other) = other {
        zip.start_file("partstudios/other/export.step", options)
            .unwrap();
        zip.write_all(other).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

pub(crate) fn edited(definition: Value, edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bundle(definition))).unwrap();
    let mut members = serde_json::Map::new();
    for ordinal in 0..archive.len() {
        let mut member = archive.by_index(ordinal).unwrap();
        let name = member.name().unwrap().into_owned();
        if name
            .rsplit_once('.')
            .is_some_and(|(_, extension)| extension == "json")
        {
            let mut bytes = Vec::new();
            member.read_to_end(&mut bytes).unwrap();
            members.insert(name, json::from_slice(&bytes).unwrap());
        }
    }
    let mut members = Value::Object(members);
    edit(&mut members);
    pack(&members, None)
}
pub(crate) fn nested() -> Value {
    let mut definition = definition();
    definition["rootAssembly"]["instances"] = json!([{"id":"sub","name":"sub","type":"Assembly","documentId":"document","elementId":"nested","configuration":"default"}]);
    definition["rootAssembly"]["occurrences"] = json!([
        {"path":["sub"],"transform":placement(0.01,0.02)},
        {"path":["sub","first"],"transform":placement(0.01,0.03)},
        {"path":["sub","second"],"transform":placement(0.01,0.04)}
    ]);
    definition["subAssemblies"] = json!([{"documentId":"document","elementId":"nested","configuration":"default","instances":[part("first"),part("second")],"features":[mate("nestedMate","REVOLUTE")]}]);
    definition
}
