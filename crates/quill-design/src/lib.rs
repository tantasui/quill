//! Quill's design model: typed design tokens, layouts and presets.
//!
//! This crate does no I/O and knows nothing about the web server, so the server
//! and the command line share one set of validation rules.

mod color;
mod error;
mod font;
mod length;

pub use color::Color;
pub use error::TokenError;
pub use font::{FontStack, FontWeight};
pub use length::{Length, Offset};
