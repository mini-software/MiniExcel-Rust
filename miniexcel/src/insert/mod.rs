#![allow(dead_code)]

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod atomic;
pub(crate) mod donor;
pub(crate) mod package;
pub(crate) mod rewrite;
pub(crate) mod style;
