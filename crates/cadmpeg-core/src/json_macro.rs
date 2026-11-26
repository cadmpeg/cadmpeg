// SPDX-License-Identifier: Apache-2.0
//! `json!`: the `serde_json::json!` grammar, writing each interpolated
//! value through [`crate::serde_erase::to_json_value`].
//!
//! `serde_json::json!` writes an interpolated value with
//! `serde_json::to_value`, which compiles the value's `Serialize` impl once
//! more against `serde_json`'s `Value` serializer. This macro writes it
//! through `SerAdapter`, against which each type's `Serialize` impl is
//! already compiled. The token rules are those of `serde_json` 1.0.

/// Builds a [`serde_json::Value`] from a JSON literal, as `serde_json::json!`
/// does.
#[macro_export]
macro_rules! json {
    ($($json:tt)+) => {
        $crate::__json_internal!($($json)+)
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! __json_internal {

    (@array [$($elems:expr,)*]) => {
        ::std::vec![$($elems,)*]
    };

    (@array [$($elems:expr),*]) => {
        ::std::vec![$($elems),*]
    };

    (@array [$($elems:expr,)*] null $($rest:tt)*) => {
        $crate::__json_internal!(@array [$($elems,)* $crate::__json_internal!(null)] $($rest)*)
    };

    (@array [$($elems:expr,)*] true $($rest:tt)*) => {
        $crate::__json_internal!(@array [$($elems,)* $crate::__json_internal!(true)] $($rest)*)
    };

    (@array [$($elems:expr,)*] false $($rest:tt)*) => {
        $crate::__json_internal!(@array [$($elems,)* $crate::__json_internal!(false)] $($rest)*)
    };

    (@array [$($elems:expr,)*] [$($array:tt)*] $($rest:tt)*) => {
        $crate::__json_internal!(@array [$($elems,)* $crate::__json_internal!([$($array)*])] $($rest)*)
    };

    (@array [$($elems:expr,)*] {$($map:tt)*} $($rest:tt)*) => {
        $crate::__json_internal!(@array [$($elems,)* $crate::__json_internal!({$($map)*})] $($rest)*)
    };

    (@array [$($elems:expr,)*] $next:expr, $($rest:tt)*) => {
        $crate::__json_internal!(@array [$($elems,)* $crate::__json_internal!($next),] $($rest)*)
    };

    (@array [$($elems:expr,)*] $last:expr) => {
        $crate::__json_internal!(@array [$($elems,)* $crate::__json_internal!($last)])
    };

    (@array [$($elems:expr),*] , $($rest:tt)*) => {
        $crate::__json_internal!(@array [$($elems,)*] $($rest)*)
    };

    (@array [$($elems:expr),*] $unexpected:tt $($rest:tt)*) => {
        $crate::__json_unexpected!($unexpected)
    };

    (@object $object:ident () () ()) => {};

    (@object $object:ident [$($key:tt)+] ($value:expr) , $($rest:tt)*) => {
        $object.insert(($($key)+).into(), $value);
        $crate::__json_internal!(@object $object () ($($rest)*) ($($rest)*));
    };

    (@object $object:ident [$($key:tt)+] ($value:expr) $unexpected:tt $($rest:tt)*) => {
        $crate::__json_unexpected!($unexpected);
    };

    (@object $object:ident [$($key:tt)+] ($value:expr)) => {
        $object.insert(($($key)+).into(), $value);
    };

    (@object $object:ident ($($key:tt)+) (: null $($rest:tt)*) $copy:tt) => {
        $crate::__json_internal!(@object $object [$($key)+] ($crate::__json_internal!(null)) $($rest)*);
    };

    (@object $object:ident ($($key:tt)+) (: true $($rest:tt)*) $copy:tt) => {
        $crate::__json_internal!(@object $object [$($key)+] ($crate::__json_internal!(true)) $($rest)*);
    };

    (@object $object:ident ($($key:tt)+) (: false $($rest:tt)*) $copy:tt) => {
        $crate::__json_internal!(@object $object [$($key)+] ($crate::__json_internal!(false)) $($rest)*);
    };

    (@object $object:ident ($($key:tt)+) (: [$($array:tt)*] $($rest:tt)*) $copy:tt) => {
        $crate::__json_internal!(@object $object [$($key)+] ($crate::__json_internal!([$($array)*])) $($rest)*);
    };

    (@object $object:ident ($($key:tt)+) (: {$($map:tt)*} $($rest:tt)*) $copy:tt) => {
        $crate::__json_internal!(@object $object [$($key)+] ($crate::__json_internal!({$($map)*})) $($rest)*);
    };

    (@object $object:ident ($($key:tt)+) (: $value:expr , $($rest:tt)*) $copy:tt) => {
        $crate::__json_internal!(@object $object [$($key)+] ($crate::__json_internal!($value)) , $($rest)*);
    };

    (@object $object:ident ($($key:tt)+) (: $value:expr) $copy:tt) => {
        $crate::__json_internal!(@object $object [$($key)+] ($crate::__json_internal!($value)));
    };

    (@object $object:ident ($($key:tt)+) (:) $copy:tt) => {
        $crate::__json_internal!();
    };

    (@object $object:ident ($($key:tt)+) () $copy:tt) => {
        $crate::__json_internal!();
    };

    (@object $object:ident () (: $($rest:tt)*) ($colon:tt $($copy:tt)*)) => {
        $crate::__json_unexpected!($colon);
    };

    (@object $object:ident ($($key:tt)*) (, $($rest:tt)*) ($comma:tt $($copy:tt)*)) => {
        $crate::__json_unexpected!($comma);
    };

    (@object $object:ident () (($key:expr) : $($rest:tt)*) $copy:tt) => {
        $crate::__json_internal!(@object $object ($key) (: $($rest)*) (: $($rest)*));
    };

    (@object $object:ident ($($key:tt)*) (: $($unexpected:tt)+) $copy:tt) => {
        $crate::__json_expect_expr_comma!($($unexpected)+);
    };

    (@object $object:ident ($($key:tt)*) ($tt:tt $($rest:tt)*) $copy:tt) => {
        $crate::__json_internal!(@object $object ($($key)* $tt) ($($rest)*) ($($rest)*));
    };

    (null) => {
        $crate::__serde_json::Value::Null
    };

    (true) => {
        $crate::__serde_json::Value::Bool(true)
    };

    (false) => {
        $crate::__serde_json::Value::Bool(false)
    };

    ([]) => {
        $crate::__serde_json::Value::Array(::std::vec![])
    };

    ([ $($tt:tt)+ ]) => {
        $crate::__serde_json::Value::Array($crate::__json_internal!(@array [] $($tt)+))
    };

    ({}) => {
        $crate::__serde_json::Value::Object($crate::__serde_json::Map::new())
    };

    ({ $($tt:tt)+ }) => {
        $crate::__serde_json::Value::Object({
            let mut object = $crate::__serde_json::Map::new();
            $crate::__json_internal!(@object object () ($($tt)+) ($($tt)+));
            object
        })
    };

    ($other:expr) => {
        $crate::serde_erase::json_value_from_erased(&$other)
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! __json_unexpected {
    () => {};
}

#[macro_export]
#[doc(hidden)]
macro_rules! __json_expect_expr_comma {
    ($e:expr , $($tt:tt)*) => {};
}
