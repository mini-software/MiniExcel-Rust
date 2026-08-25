#![allow(dead_code)]

#[cfg(feature = "async")]
pub(crate) mod async_export;
#[cfg(feature = "async")]
pub(crate) mod async_insert;
#[cfg(feature = "async")]
pub(crate) mod async_template;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod atomic;
pub(crate) mod donor;
pub(crate) mod package;
pub(crate) mod rewrite;
pub(crate) mod style;
