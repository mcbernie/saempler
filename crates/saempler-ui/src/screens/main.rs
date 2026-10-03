use std::sync::{Arc, Mutex};

use nih_plug::prelude::{FloatParam, ParamSetter};
use nih_plug_egui::egui::{self, pos2, vec2, Align2, FontId, Frame, Sense, Stroke, Ui};
use nih_plug_egui::{resizable_window::ResizableWindow, EguiState};
use saempler_audio::{CellSpec, CommandProducer, EngineCommand, Meters, SampleBuffer, SliceBounds};
use saempler_core::PeakCache;
use saempler_model::{Modifier, ProjectFile};

use crate::screens::cell::cell_section;
use crate::screens::modifiers::modifier_section;
use crate::screens::performance::performance_section;
use crate::screens::source::source_section;
use crate::theme::Theme;
use crate::widgets::{knob, led, readout, stereo_meter, tab_bar, ViewRange};

pub(crate) const THEME: Theme = Theme::dark();

const KNOB_DIAMETER: f32 = 48.0;
const METER_WIDTH: f32 = 150.0;

/// Smallest the editor window may be dragged to.
pub const MIN_EDITOR_SIZE: (f32, f32) = (720.0, 470.0);

/// The pages of the editor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Sample,
    Perform,
    Cell,
    Modifiers,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Sample, Tab::Perform, Tab::Cell, Tab::Modifiers];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Sample => "SAMPLE",
            Tab::Perform => "PERFORM",
            Tab::Cell => "CELL",
            Tab::Modifiers => "MODIFIERS",
        }
    }
}

/// What the editor keeps between frames.
///
/// Owned by the plugin, so it survives the window being closed and reopened.
/// The peaks drive the waveform overview; the buffer is only read when the
/// view is zoomed in past the resolution of the peak cache.
#[derive(Default)]
pub struct EditorState {
    pub peaks: PeakCache,
    /// The decoded audio, shared with the engine.
    pub buffer: Option<Arc<SampleBuffer>>,
    /// Section of the sample the waveform is showing.
    pub view: ViewRange,
    /// Message from the last import attempt, shown until the next one.
    pub status: Option<String>,
    /// Whether an import is running right now.
    pub loading: bool,
    /// The page being shown.
    pub tab: Tab,
}

impl EditorState {
    /// Show the whole sample again.
    pub fn reset_view(&mut self) {
        self.view = ViewRange::full(self.peaks.frames());
    }
}

/// Everything the editor needs to draw a frame.
///
/// The fields are borrowed rather than owned so that the plugin keeps
/// ownership of its state and the interface stays a pure view over it.
pub struct ViewState<'a> {
    /// Persisted project state. Locked on the UI thread only.
    pub project: &'a Mutex<ProjectFile>,
    /// Peaks, audio, import status and the current page.
    pub sample: &'a Mutex<EditorState>,
    /// Producing end of the engine command queue. Locked on the UI thread
    /// only; the audio thread owns the consumer and never blocks on this.
    pub commands: &'a Mutex<CommandProducer>,
    /// Values published by the audio thread.
    pub meters: &'a Meters,
    /// Master output gain, exposed to the host as an automatable parameter.
    pub gain: &'a FloatParam,
    /// Window size, which the resize corner writes back to.
    pub editor_state: &'a EguiState,
}

impl ViewState<'_> {
    /// Queue a command for the engine.
    ///
    /// A full queue means the engine has not run since the last few hundred
    /// edits, which in practice only happens while audio is stopped. Dropping
    /// the command is preferable to blocking the interface.
    pub(crate) fn send(&self, command: EngineCommand) {
        if let Ok(mut producer) = self.commands.lock() {
            let _ = producer.push(command);
        }
    }
}

/// A one-shot audition of a region, with the cell's own settings left out.
pub(crate) fn preview_spec(start_frame: u64, end_frame: u64) -> CellSpec {
    CellSpec {
        bounds: SliceBounds {
            start_frame,
            end_frame,
        },
        ..CellSpec::default()
    }
}

/// Apply the product theme to egui's own surfaces.
pub fn apply_style(ctx: &egui::Context, theme: &Theme) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = theme.window_bg;
    visuals.window_fill = theme.window_bg;
    visuals.extreme_bg_color = theme.control_pressed_bg;
    visuals.override_text_color = Some(theme.text);
    visuals.resize_corner_size = 14.0;
    ctx.set_visuals(visuals);
}

/// Draw the whole editor. Returns true when the user asked to import a file.
pub fn draw(ctx: &egui::Context, setter: &ParamSetter, state: &ViewState<'_>) -> bool {
    apply_style(ctx, &THEME);
    let mut import_requested = false;

    // The playheads move with every processed block, so while something is
    // sounding the editor cannot wait for the next input event to redraw.
    if state.meters.any_playhead() {
        ctx.request_repaint();
    }

    ResizableWindow::new("saempler-window")
        .min_size(vec2(MIN_EDITOR_SIZE.0, MIN_EDITOR_SIZE.1))
        .show(ctx, state.editor_state, |ui| {
            Frame::new()
                .fill(THEME.window_bg)
                .inner_margin(THEME.spacing_lg)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(THEME.spacing_md, THEME.spacing_md);
                    header(ui, state);

                    let tab = current_tab(state);
                    let labels: Vec<&str> = Tab::ALL.iter().map(|tab| tab.label()).collect();
                    if let Some(index) = tab_bar(ui, &THEME, &labels, tab as usize) {
                        set_tab(state, Tab::ALL[index]);
                    }
                    ui.add_space(THEME.spacing_md);

                    // The page scrolls; the header and the footer stay put, so
                    // the meters and modifier lamps are always in view.
                    // Height of the footer below: its knob plus the knob's two
                    // label lines, the section title and the frame margins.
                    let footer = KNOB_DIAMETER + THEME.font_sm * 4.6 + THEME.spacing_lg * 2.0;
                    egui::ScrollArea::vertical()
                        .max_height((ui.available_height() - footer).max(140.0))
                        .auto_shrink([false, false])
                        .show(ui, |ui| match tab {
                            Tab::Sample => import_requested = source_section(ui, state),
                            Tab::Perform => performance_section(ui, state),
                            Tab::Cell => cell_section(ui, state),
                            Tab::Modifiers => modifier_section(ui, state),
                        });

                    ui.add_space(THEME.spacing_sm);
                    footer_section(ui, setter, state);
                });
        });

    import_requested
}

/// The page currently being shown.
fn current_tab(state: &ViewState<'_>) -> Tab {
    state
        .sample
        .lock()
        .map(|sample| sample.tab)
        .unwrap_or_default()
}

fn set_tab(state: &ViewState<'_>, tab: Tab) {
    if let Ok(mut sample) = state.sample.lock() {
        sample.tab = tab;
    }
}

/// Product name and the sample currently loaded.
fn header(ui: &mut Ui, state: &ViewState<'_>) {
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), THEME.font_lg * 1.5),
        Sense::hover(),
    );
    let painter = ui.painter();

    painter.text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        "SÄMPLER",
        FontId::proportional(THEME.font_lg),
        THEME.accent,
    );

    let subtitle = match state.project.lock() {
        Ok(project) => match project.project.sample.as_ref() {
            Some(sample) => format!(
                "{}  ·  {:.2} s  ·  {} Hz  ·  {} ch",
                sample.display_name(),
                sample.duration_seconds(),
                sample.sample_rate,
                sample.channels
            ),
            None => "kein Sample geladen".to_owned(),
        },
        Err(_) => String::new(),
    };
    painter.text(
        rect.right_center(),
        Align2::RIGHT_CENTER,
        subtitle,
        FontId::proportional(THEME.font_sm),
        THEME.text_dim,
    );
}

/// Output level, voice count and the modifier lamps.
///
/// Kept out of the tabs on purpose: while performing you need to see what the
/// modifiers are doing whichever page is open.
fn footer_section(ui: &mut Ui, setter: &ParamSetter, state: &ViewState<'_>) {
    section(ui, "OUTPUT", |ui| {
        ui.horizontal(|ui| {
            knob(ui, &THEME, state.gain, setter, KNOB_DIAMETER);
            ui.add_space(THEME.spacing_lg);

            ui.vertical(|ui| {
                stereo_meter(ui, &THEME, state.meters.peaks(), METER_WIDTH);
                readout(
                    ui,
                    &THEME,
                    "Voices",
                    &state.meters.active_voices().to_string(),
                    METER_WIDTH,
                );
            });

            ui.add_space(THEME.spacing_lg);
            modifier_lamps(ui, state);
        });
    });
}

/// One lamp per modifier, lit while it is in effect.
fn modifier_lamps(ui: &mut Ui, state: &ViewState<'_>) {
    let engaged = state.meters.modifiers();

    ui.horizontal(|ui| {
        for modifier in Modifier::ALL {
            let lit = engaged & (1 << modifier.index()) != 0;
            let (rect, _) = ui.allocate_exact_size(vec2(78.0, 36.0), Sense::hover());

            led(
                ui,
                &THEME,
                pos2(rect.center().x, rect.min.y + 8.0),
                lit.then_some(THEME.active),
            );
            ui.painter().text(
                pos2(rect.center().x, rect.max.y - 2.0),
                Align2::CENTER_BOTTOM,
                modifier.label(),
                FontId::proportional(THEME.font_sm),
                if lit { THEME.text } else { THEME.text_dim },
            );
        }
    });
}

/// A titled, framed group of controls.
pub(crate) fn section(ui: &mut Ui, title: &str, contents: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(THEME.panel_bg)
        .stroke(THEME.outline_stroke())
        .corner_radius(THEME.radius_md)
        .inner_margin(THEME.spacing_md)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            let (rect, _) = ui.allocate_exact_size(
                vec2(ui.available_width(), THEME.font_sm + THEME.spacing_sm),
                Sense::hover(),
            );
            let painter = ui.painter();

            painter.text(
                rect.left_center(),
                Align2::LEFT_CENTER,
                title,
                FontId::proportional(THEME.font_sm),
                THEME.text_dim,
            );

            // A rule from the title to the right edge, like a panel legend.
            let text_width = title.chars().count() as f32 * THEME.font_sm * 0.68 + THEME.spacing_md;
            if rect.width() > text_width {
                painter.line_segment(
                    [
                        pos2(rect.min.x + text_width, rect.center().y),
                        pos2(rect.max.x, rect.center().y),
                    ],
                    Stroke::new(1.0, THEME.outline),
                );
            }

            contents(ui);
        });
}

/// A dimmed line of explanatory text.
///
/// Sized to the text rather than to the available width, so a hint placed in a
/// row leaves room for what follows it.
pub(crate) fn hint(ui: &mut Ui, text: &str) {
    let width = ui.fonts(|fonts| {
        fonts
            .layout_no_wrap(
                text.to_owned(),
                FontId::proportional(THEME.font_sm),
                THEME.text_dim,
            )
            .size()
            .x
    });
    let (rect, _) = ui.allocate_exact_size(
        vec2(width.min(ui.available_width()), THEME.font_sm * 1.7),
        Sense::hover(),
    );
    ui.painter().text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(THEME.font_sm),
        THEME.text_dim,
    );
}

/// Text standing in for a page that has nothing to show yet.
pub(crate) fn placeholder(ui: &mut Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 72.0), Sense::hover());
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(THEME.font_md),
        THEME.text_dim,
    );
}
