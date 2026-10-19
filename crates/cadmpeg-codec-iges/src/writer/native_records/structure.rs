// SPDX-License-Identifier: Apache-2.0
//! Typed group and associativity parameter grammars.

use super::*;

pub(super) fn shape(record:&NativeRecord)->Result<(u32,i64),String> {
    Ok(match record.field("kind").and_then(Value::as_str) {
        Some("definition")=>(302,record.field("associativity_form").and_then(Value::as_i64).ok_or("associativity definition has no form")?),
        Some("label_display")=>(402,5),Some("view_list")=>(402,6),Some("single_parent")=>(402,9),Some("external_reference_index")=>(402,12),Some("legacy_signal_string")=>(402,8),Some("legacy_text_node")=>(402,10),Some("legacy_connect_node")=>(402,11),Some("dimensioned_geometry")=>(402,13),Some("planar")=>(402,16),Some("flow")=>(402,record.field("form").and_then(Value::as_i64).ok_or("flow has no form")?),Some("recalculable_dimension")=>(402,21),_=>return Err("associativity has no typed kind".into())
    })
}

fn pointers(fields:&mut Fields<'_>,name:&str)->Result<Vec<String>,String> {
    fields.array(name)?.iter().map(|value| {
        if value.is_null() {return Err(format!("{name} holds an unresolved member"));}
        fields.reference(Some(value))
    }).collect()
}
fn run(fields:&mut Fields<'_>,count_name:&str,name:&str)->Result<Vec<String>,String> {
    let values=pointers(fields,name)?;let mut result=vec![count(&fields.values,count_name,values.len())?];result.extend(values);Ok(result)
}
fn token(value:&Value)->Result<String,String> {
    match value.get("kind").and_then(Value::as_str) {
        Some("omitted")=>Ok(String::new()),Some("integer")=>integer(value.get("value")),Some("real")=>real(value.get("value")),Some("string")=>text(value.get("value")),_=>Err("connection data has an unreadable typed literal".into())
    }
}

pub(super) fn encode(fields:&mut Fields<'_>,type_code:u32,form:i64)->Result<Entity,String> {
    let mut values=Vec::new();
    match (type_code,form) {
        (402,1|7|14|15)=>values=run(fields,"declared_member_count","members")?,
        (302,_)=> {
            let classes=fields.array("classes")?;values.push(count(&fields.values,"declared_class_count",classes.len())?);
            for class in classes {
                let class=class.as_object().ok_or("associativity class has no typed fields")?;
                values.push(boolean(class.get("back_pointers_required"))?);values.push(boolean(class.get("ordered"))?);
                let items=class.get("item_types").and_then(Value::as_array).ok_or("associativity class has no item types")?;
                values.push(count(class,"declared_item_count",items.len())?);for item in items {values.push(integer(Some(item))?);}
            }
        }
        (402,5)=> {
            let placements=fields.array("placements")?;values.push(count(&fields.values,"declared_count",placements.len())?);
            for placement in placements {let placement=placement.as_object().ok_or("label placement has no typed fields")?;
                values.push(fields.reference(placement.get("view"))?);values.extend(coordinates(placement.get("text_location"),3)?);
                values.push(fields.reference(placement.get("leader"))?);values.push(integer(placement.get("label_level"))?);values.push(fields.reference(placement.get("entity"))?);
            }
        }
        (402,6|9|13|16)=> {
            let (count_name,list_name,owner)=match form {6=>("declared_visible_count","visible_entities","view"),9=>("declared_child_count","children","parent"),13=>("declared_geometry_count","geometry","dimension"),_=>("declared_entity_count","entities","plane_transform")};
            let items=pointers(fields,list_name)?;values.extend(["1".into(),count(&fields.values,count_name,items.len())?,fields.pointer(owner)?]);values.extend(items);
        }
        (402,2|12)=> {
            let entries=fields.array("entries")?;values.push(count(&fields.values,"declared_count",entries.len())?);
            for entry in entries {let entry=entry.as_object().ok_or("external index entry has no typed fields")?;values.push(text(entry.get("symbolic_name"))?);values.push(fields.reference(entry.get("entity"))?);}
        }
        (402,8)=> {
            for (count_name,list_name) in [("declared_signal_name_count","signal_names"),("declared_connection_count","connections"),("declared_schematic_count","schematic_entities"),("declared_physical_count","physical_entities")] {values.push(count(&fields.values,count_name,fields.array(list_name)?.len())?);}
            for name in fields.array("signal_names")? {values.push(text(Some(&name))?);}
            for name in ["connections","schematic_entities","physical_entities"] {values.extend(pointers(fields,name)?);}
        }
        (402,10)=> {
            let geometry=pointers(fields,"geometry")?;values.push(count(&fields.values,"declared_geometry_count",geometry.len())?);
            if fields.value("declared_text_description_count").and_then(Value::as_i64)!=Some(1) {return Err("text node does not hold one typed description".into());}values.push("1".into());values.extend(geometry);
            values.push(fields.real("box_width")?);values.push(fields.real("box_height")?);
            values.push(if fields.value("font_definition").is_some() {format!("-{}",fields.pointer("font_definition")?)} else {let code=fields.integer("font_characteristic")?;if code.starts_with('-') {return Err("negative text font has no current definition".into());}code});
            values.push(fields.real("slant_angle")?);values.push(fields.real("rotation_angle")?);values.push(fields.integer("mirror_flag")?);values.push(fields.integer("rotate_internal_flag")?);
        }
        (402,11)=> {
            let points=pointers(fields,"points")?;let data=fields.array("data")?;
            values.push(count(&fields.values,"declared_point_count",points.len())?);values.push(count(&fields.values,"declared_data_count",data.len())?);values.extend(points);for value in &data {values.push(token(value)?);}
        }
        (402,18|20)=> {
            values.push(if form==18 {"2"} else {"1"}.into());
            for (count_name,list_name) in [("declared_associated_flow_count","associated_flows"),("declared_connection_count","connections"),("declared_join_count","joins"),("declared_name_count","names"),("declared_name_display_count","name_displays"),("declared_continuation_count","continuations")] {values.push(count(&fields.values,count_name,fields.array(list_name)?.len())?);}
            values.push(fields.integer("type_flag")?);if form==18 {values.push(fields.integer("function_flag")?);}
            for name in ["associated_flows","connections","joins"] {values.extend(pointers(fields,name)?);}
            for name in fields.array("names")? {values.push(text(Some(&name))?);}
            for name in ["name_displays","continuations"] {values.extend(pointers(fields,name)?);}
        }
        (402,21)=> {
            let geometry=fields.array("geometry")?;values.extend(["1".into(),count(&fields.values,"declared_geometry_count",geometry.len())?,fields.pointer("dimension")?,fields.integer("orientation_flag")?,fields.real("angle")?]);
            for item in geometry {let item=item.as_object().ok_or("dimension geometry has no typed fields")?;values.push(fields.reference(item.get("geometry"))?);values.push(integer(item.get("location_flag"))?);values.extend(coordinates(item.get("point"),3)?);}
        }
        _=>return Err("associativity has no exact typed parameter grammar".into())
    }
    Ok(entity(type_code,form,values))
}
