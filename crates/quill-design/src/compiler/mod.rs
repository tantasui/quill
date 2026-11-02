//! The DESIGN.md compiler: parse, extract, normalize, validate, compile.

// Everything here is used by the pipeline, which lands once its parts are in.
#[allow(dead_code)]
pub mod diagnostic;

#[allow(dead_code)]
mod css;
#[allow(dead_code)]
mod extract;
#[allow(dead_code)]
mod markdown;
#[allow(dead_code)]
mod roles;
#[allow(dead_code)]
mod yaml;
