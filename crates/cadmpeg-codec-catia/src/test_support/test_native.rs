// SPDX-License-Identifier: Apache-2.0
//! Native-record field mutation helpers for CATIA tests.

pub(crate) struct NativeFieldsMut<'a> {
    record: &'a mut cadmpeg_ir::NativeRecord,
    fields: Option<serde_json::Map<String, serde_json::Value>>,
}

impl std::ops::Deref for NativeFieldsMut<'_> {
    type Target = serde_json::Map<String, serde_json::Value>;

    fn deref(&self) -> &Self::Target {
        self.fields.as_ref().expect("native fields guard")
    }
}

impl std::ops::DerefMut for NativeFieldsMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.fields.as_mut().expect("native fields guard")
    }
}

impl Drop for NativeFieldsMut<'_> {
    fn drop(&mut self) {
        let id = self.record.id().to_owned();
        let fields = self.fields.take().expect("native fields guard");
        *self.record = cadmpeg_ir::NativeRecord::new(id, fields);
    }
}

pub(crate) trait NativeRecordTestExt {
    fn fields_mut(&mut self) -> NativeFieldsMut<'_>;
}

impl NativeRecordTestExt for cadmpeg_ir::NativeRecord {
    fn fields_mut(&mut self) -> NativeFieldsMut<'_> {
        let fields = self.fields();
        NativeFieldsMut {
            record: self,
            fields: Some(fields),
        }
    }
}
