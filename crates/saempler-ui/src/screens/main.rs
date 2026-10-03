use std::sync::Mutex;

use nih_plug::prelude::{FloatParam, ParamSetter};
use nih_plug_egui::egui::{self, Align2, CentralPanel, FontId, Frame, Layout, Ui};
use saempler_audio::{CommandProducer, EngineCommand, Meters};
use saempler_model::{ProjectFile, Waveform};

use crate::theme::Theme;
use crate::widgets::{button, knob, readout, segmented, stereo_meter};

const THEME: Theme = Theme::dark();

const KNOB_DIAMETER: f32 = 56.0;
const METER_WIDTH: f32 = 180.0;

/// Everything the editor needs to draw a frame.
///
/// The fields are borrowed rather than owned so that the plugin keeps
/// ownership of its state and the interface stays a pure view over it.
pub struct ViewState<'a> {
    /// Persisted project state. Locked on the UI thread only.
    pub project: &'a Mutex<ProjectFile>,
    /// Producing end of the engine command queue. Locked on the UI thread
    /// only; the audio thread owns the consumer and never blocks on this.
    pub commands: &'a Mutex<CommandProducer>,
    /// Values published by the audio thread.
    pub meters: &'a Meters,
    /// Master output gain, exposed to the host as an automatable parameter.
    pub gain: &'a FloatParam,
}

impl ViewState<'_> {
    /// Queue a command for the engine.
    ///
    /// A full queue means the engine has not run since the last few hundred
    /// edits, which in practice only happens while audio is stopped. Dropping
    /// the command is preferable to blocking the interface.
    fn send(&self, command: EngineCommand) {
        if let Ok(mut producer) = self.commands.lock() {
            let _ = producer.push(command);
        }
    }
}

/// Apply the product theme to egui's own surfaces.
pub fn apply_style(ctx: &egui::Context, theme: &Theme) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = theme.window_bg;
    visuals.window_fill = theme.window_bg;
    visuals.extreme_bg_color = theme.control_bg;
    visuals.override_text_color = Some(theme.text);
    ctx.set_visuals(visuals);
}

/// Draw the whole editor.
pub fn draw(ctx: &egui::Context, setter: &ParamSetter, state: &ViewState<'_>) {
    apply_style(ctx, &THEME);

    CentralPanel::default()
        .frame(
            Frame::new()
                .fill(THEME.window_bg)
                .inner_margin(THEME.spacing_lg),
        )
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(THEME.spacing_md, THEME.spacing_md);

            header(ui);
            ui.add_space(THEME.spacing_md);
            test_signal_section(ui, state);
            ui.add_space(THEME.spacing_md);
            output_section(ui, setter, state);
        });
}

/// Product name and build identity.
fn header(ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.painter().text(
            ui.cursor().min,
            Align2::LEFT_TOP,
            "SÄMPLER",
            FontId::proportional(THEME.font_lg),
            THEME.accent,
        );
        ui.add_space(170.0);
        ui.with_layout(Layout::right_to_left(egui::Align::Min), |ui| {
            ui.painter().text(
                ui.cursor().max,
                Align2::RIGHT_TOP,
                concat!("v", env!("CARGO_PKG_VERSION")),
                FontId::proportional(THEME.font_sm),
                THEME.text_dim,
            );
        });
    });
    ui.add_space(THEME.font_lg);
}

/// Waveform selection and the panic button.
fn test_signal_section(ui: &mut Ui, state: &ViewState<'_>) {
    section(ui, "TEST SIGNAL", |ui| {
        let labels: Vec<&str> = Waveform::ALL.iter().map(|w| w.label()).collect();
        let selected = current_waveform(state);
        let selected_index = Waveform::ALL
            .iter()
            .position(|w| *w == selected)
            .unwrap_or(0);

        let width = ui.available_width();
        if let Some(index) = segmented(ui, &THEME, &labels, selected_index, width) {
            let waveform = Waveform::ALL[index];
            if waveform != selected {
                if let Ok(mut project) = state.project.lock() {
                    project.project.waveform = waveform;
                }
                state.send(EngineCommand::SetWaveform(waveform));
            }
        }

        if button(ui, &THEME, "All Notes Off", width) {
            state.send(EngineCommand::AllNotesOff);
        }
    });
}

/// Master gain plus the values published by the engine.
fn output_section(ui: &mut Ui, setter: &ParamSetter, state: &ViewState<'_>) {
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
        });
    });
}

/// Waveform the project is currently set to.
fn current_waveform(state: &ViewState<'_>) -> Waveform {
    state
        .project
        .lock()
        .map(|project| project.project.waveform)
        .unwrap_or_default()
}

/// A titled, framed group of controls.
fn section(ui: &mut Ui, title: &str, contents: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(THEME.panel_bg)
        .stroke(THEME.outline_stroke())
        .corner_radius(THEME.radius_md)
        .inner_margin(THEME.spacing_md)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), THEME.font_sm + THEME.spacing_sm),
                egui::Sense::hover(),
            );
            ui.painter().text(
                rect.left_center(),
                Align2::LEFT_CENTER,
                title,
                FontId::proportional(THEME.font_sm),
                THEME.text_dim,
            );
            contents(ui);
        });
}
