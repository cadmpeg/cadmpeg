// SPDX-License-Identifier: Apache-2.0
//! Rewrite the identities owned by these fields.

use super::{Asset, AssetContent, AssetData};

rewrite_record!(Asset, []; {id, name, media_type, content, native_ref});
rewrite_enum!(AssetContent, []; {
    Embedded {data},
    External {uri},
});
rewrite_record!(AssetData, []; (field0));
