//! The built-in presets. They live in `quill-design` so the command line and the
//! server share them; "theme" remains the setting's stored name for compatibility
//! with existing databases.

pub use quill_design::presets::{find, Preset as Theme, ALL, DEFAULT};
