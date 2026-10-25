// SPDX-License-Identifier: Apache-2.0
use super::visit_native_references;

#[test]
fn native_links_use_borrowed_registered_records_without_schema_generation() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let native = "test:native:object#Body";
    ir.model.bodies[0].native_ref = Some(native.into());
    let body = &ir.model.bodies[0];
    let mut count = 0;
    let mut found = false;
    visit_native_references(&ir.model, &mut |arena, owner, id, reference| {
        count += 1;
        if arena == "bodies" {
            assert_eq!(owner, std::any::type_name_of_val(body));
            assert_eq!(id, body.id.as_str());
            assert_eq!(reference, Some(native));
            assert_eq!(id.as_ptr(), body.id.as_str().as_ptr());
            assert_eq!(
                reference.unwrap().as_ptr(),
                body.native_ref.as_deref().unwrap().as_ptr()
            );
            found = true;
        }
    });
    assert!(found);
    assert_eq!(count, ir.model.entity_count());
}
