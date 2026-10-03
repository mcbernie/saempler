//! egui interface for Sämpler.
//!
//! This crate owns the look of the plugin: a central [`theme`], the custom
//! audio widgets drawn with `egui::Painter`, and the screens that compose
//! them. It contains no audio processing and no host integration; it reads
//! project state and engine meters and emits commands.

pub mod screens;
pub mod theme;
pub mod widgets;

pub use screens::main::{draw, EditorState, ViewState, MIN_EDITOR_SIZE};
pub use screens::modifiers::sync_modifiers;
pub use theme::Theme;
pub use widgets::ViewRange;
