// SPDX-License-Identifier: Apache-2.0
use super::*;

fn offset_bundle(flipped: bool, kind: &str) -> Vec<u8> {
    changed(&fixture("unsupported"), |path, value| {
        if path.ends_with("features.json") {
            let features = value["features"].as_array_mut().unwrap();
            features.truncate(2);
            features[0]["parameters"][0]["queries"] = json!([{
                "btType":"BTMIndividualCreatedByQuery-137", "featureId":"raised-datum"
            }]);
            features.insert(
                0,
                json!({
                    "btType":"BTMFeature-134", "featureId":"raised-datum", "name":"Raised datum",
                    "featureType":"cPlane", "parameters":[
                        {"parameterId":"cplaneType","value":kind},
                        {"parameterId":"entities","queries":[{
                            "btType":"BTMIndividualCreatedByQuery-137","featureId":"Top"
                        }]},
                        {"parameterId":"offset","expression":"7 mm"},
                        {"parameterId":"oppositeDirection","value":flipped}
                    ]
                }),
            );
        }
    })
}

#[test]
fn offset_datum_and_attached_sketch_have_resolved_frames() {
    let height = 7.0;
    let (model, report) = read(&offset_bundle(false, "OFFSET")).unwrap();
    assert_eq!(
        kinds(&model),
        ["datum_offset_plane", "sketch", "extrude"],
        "{:?}",
        report.losses()
    );
    let wire = timeline(&model);
    let expected = json!({"origin":{"x":0.0,"y":0.0,"z":height},
        "normal":{"x":0.0,"y":0.0,"z":1.0}, "u_axis":{"x":1.0,"y":0.0,"z":0.0}});
    assert_eq!(
        wire["model"]["features"][0]["definition"]["value"]["value"]["distance"],
        height
    );
    assert_eq!(
        wire["model"]["features"][0]["definition"]["value"]["value"]["reference"]["kind"],
        "resolved_plane"
    );
    assert_eq!(wire["model"]["sketches"][0]["placement"]["frame"], expected);
    assert_eq!(
        wire["model"]["features"][1]["dependencies"],
        json!(["onshape:model:feature#raised-datum"])
    );
    assert!(
        report
            .losses()
            .iter()
            .all(|loss| !loss.code.as_deref().is_some_and(
                |code| code.starts_with("onshape/") && code != "onshape/sketch.plane-assumed"
            )),
        "{:?}",
        report.losses()
    );
}

#[test]
fn flipped_offset_plane_keeps_native_with_named_loss() {
    let (model, report) = read(&offset_bundle(true, "OFFSET")).unwrap();
    assert_eq!(kinds(&model)[0], "native");
    assert!(report.losses().iter().any(|loss| loss.code.as_deref()
        == Some("onshape/plane.not-supported")
        && loss.detail.contains("flipped offset plane")));
}

#[test]
fn unsupported_plane_type_keeps_native_with_named_loss() {
    let (model, report) = read(&offset_bundle(false, "THREE_POINT")).unwrap();
    assert_eq!(kinds(&model)[0], "native");
    assert!(report.losses().iter().any(|loss| loss.code.as_deref()
        == Some("onshape/plane.not-supported")
        && loss.detail.contains("cplaneType")));
}

#[test]
fn offset_from_planar_face_places_a_later_sketch() {
    for (orientation, height) in [(true, 10.0), (false, -4.0)] {
        let bytes = changed(&fixture("plate-fillet"), |path, value| {
            if path.ends_with("bodydetails/2.json") {
                value["bodies"][0]["faces"] = json!([{"id":"support-face","orientation":orientation,"loops":[],
                "surface":{"type":"PLANE","origin":{"x":0.0,"y":0.0,"z":0.003},
                    "normal":{"x":0.0,"y":0.0,"z":1.0}}}]);
            }
            if path.ends_with("features.json") {
                let features = value["features"].as_array_mut().unwrap();
                features[2] = json!({"btType":"BTMFeature-134", "featureId":"face-datum",
                "featureType":"cPlane","parameters":[
                    {"parameterId":"cplaneType","value":"OFFSET"},
                    {"parameterId":"offset","expression":"7 mm"},
                    {"parameterId":"entities","queries":[{"deterministicIds":["support-face"]}]}
                ]});
                let mut sketch = features[0].clone();
                sketch["featureId"] = json!("upper-sketch");
                sketch["parameters"][0]["queries"] = json!([{"featureId":"face-datum"}]);
                features.push(sketch);
                value["rollbackIndex"] = json!(4);
            }
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(
            kinds(&model),
            ["sketch", "extrude", "datum_offset_plane", "sketch"],
            "{:?}",
            report.losses()
        );
        let wire = timeline(&model);
        let sketch = wire["model"]["sketches"]
            .as_array()
            .unwrap()
            .iter()
            .find(|sketch| sketch["id"] == "onshape:model:sketch#upper-sketch")
            .unwrap();
        assert_eq!(sketch["placement"]["frame"]["origin"]["z"], height);
        assert!(
            report
                .losses()
                .iter()
                .all(|loss| !loss
                    .code
                    .as_deref()
                    .is_some_and(|code| code.starts_with("onshape/")
                        && code != "onshape/sketch.plane-assumed")),
            "{:?}",
            report.losses()
        );
    }
}

#[test]
fn compressed_created_plane_query_retains_stored_sketch_attachment() {
    let bytes = changed(&offset_bundle(false, "OFFSET"), |path, value| {
        if path.ends_with("features.json") {
            value["features"][1]["parameters"][0]["queries"] = json!([{
                "btType":"BTMIndividualQuery-138", "deterministicIds":["datum-pick"],
                "queryString":"query=qCompressed(1.0,\"%M4Sa$entityTypeS4$FACESb$historyTypeS8$CREATIONSb$operationIdA1S14$raised-datum.planeOpS9$queryTypeS5$DUMMY\",id);"
            }]);
            value["features"][1]["plane"] = json!({
                "origin":{"x":0.002,"y":0.0,"z":0.007},
                "normal":{"x":0.0,"y":0.0,"z":1.0},
                "x":{"x":0.0,"y":1.0,"z":0.0}
            });
        }
    });
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(
        kinds(&model),
        ["datum_offset_plane", "sketch", "extrude"],
        "{:?}",
        report.losses()
    );
    let wire = timeline(&model);
    assert_eq!(
        wire["model"]["sketches"][0]["placement"]["frame"],
        json!({
            "origin":{"x":2.0,"y":0.0,"z":7.0},
            "normal":{"x":0.0,"y":0.0,"z":1.0},
            "u_axis":{"x":0.0,"y":1.0,"z":0.0}
        })
    );
    assert!(
        report.losses().iter().all(|loss| !loss
            .code
            .as_deref()
            .is_some_and(|code| code.starts_with("onshape/"))),
        "{:?}",
        report.losses()
    );
}

fn matrix_bundle(matrix: &Value) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new_append(Cursor::new(offset_bundle(false, "OFFSET"))).unwrap();
    zip.start_file(
        "partstudios/studio/sketches.json",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    let response = json!({"sketches":[{"featureId":"Base sketch", "sketchMatrix":matrix}]});
    zip.write_all(&serde_json::to_vec(&response).unwrap())
        .unwrap();
    zip.finish().unwrap().into_inner()
}

#[test]
fn sketch_response_matrix_places_sketch_on_created_datum() {
    let bytes = matrix_bundle(&json!([
        0.0, -1.0, 0.0, 0.002, 1.0, 0.0, 0.0, 0.003, 0.0, 0.0, 1.0, 0.007, 0.0, 0.0, 0.0, 1.0
    ]));
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(
        kinds(&model),
        ["datum_offset_plane", "sketch", "extrude"],
        "{:?}",
        report.losses()
    );
    let wire = timeline(&model);
    assert_eq!(
        wire["model"]["sketches"][0]["placement"]["frame"],
        json!({
        "origin":{"x":2.0,"y":3.0,"z":7.0},
        "normal":{"x":0.0,"y":0.0,"z":1.0},
        "u_axis":{"x":0.0,"y":1.0,"z":0.0}})
    );
}

#[test]
fn malformed_optional_matrix_retains_the_support_frame_with_named_assumption() {
    let (model, report) = read(&matrix_bundle(&json!([1.0, 0.0, 0.0]))).unwrap();
    assert_eq!(kinds(&model), ["datum_offset_plane", "sketch", "extrude"]);
    let wire = timeline(&model);
    assert_eq!(
        wire["model"]["sketches"][0]["placement"]["frame"]["origin"]["z"],
        7.0
    );
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.code.as_deref() == Some("onshape/sketch.plane-assumed")));
}
