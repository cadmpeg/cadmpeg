//! Feature-level rules (`g_feat.py`): `core` holds the functions both halves
//! call; `a` ports the rules defined before line 730 and `b` the rest.

mod a;
mod b;
mod core;

#[allow(unused_imports)]
pub(crate) use self::a::*;
#[allow(unused_imports)]
pub(crate) use self::b::*;
pub(crate) use self::core::*;
