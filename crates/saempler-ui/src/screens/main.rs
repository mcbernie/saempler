use std::sync::{Arc, Mutex};

use nih_plug::prelude::{FloatParam, ParamSetter};
use nih_plug_egui::egui::{self, Align2, CentralPanel, FontId, Frame, Ui};
use saempler_audio::{CellSpec, CommandProducer, EngineCommand, Meters, SampleBuffer, SliceBounds};
use saempler_core::PeakCache;
use saempler_model::ProjectFile;

use crate::screens::performance::{cell_section, performance_section, sync_cells};
use crate::theme::Theme;
use crate::widgets::{
    button, knob, readout, segmented, stereo_meter, waveform, ViewRange, WaveformSource,
};

pub(crate) const THEME: Theme = Theme::dark();

const KNOB_DIAMETER: f32 = 52.0;
const METER_WIDTH: f32 = 170.0;
const WAVEFORM_HEIGHT: f32 = 170.0;

/// Slice counts offered by the quick division buttons.
const EVEN_DIVISIONS: [u32; 4] = [4, 8, 16, 32];

/// What the interface knows about the sample currently loaded.
///
/// Owned by the plugin and filled in by the import task. The peaks drive the
/// overview; the buffer is only read when the view is zoomed in past the
/// resolution of the peak cache.
#[derive(Default)]
pub struct SampleView {
    pub peaks: PeakCache,
    /// The decoded audio, shared with the engine.
    pub buffer: Option<Arc<SampleBuffer>>,
    /// Section of the sample the waveform is showing.
    pub view: ViewRange,
    /// Message from the last import attempt, shown until the next one.
    pub status: Option<String>,
    /// Whether an import is running right now.
    pub loading: bool,
}

impl SampleView {
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
    /// Peaks, audio and import status. Written by the import task, read here.
    pub sample: &'a Mutex<SampleView>,
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
    pub(crate) fn send(&self, command: EngineCommand) {
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

/// Draw the whole editor. Returns true when the user asked to import a file.
pub fn draw(ctx: &egui::Context, setter: &ParamSetter, state: &ViewState<'_>) -> bool {
    apply_style(ctx, &THEME);
    let mut import_requested = false;

    // The playheads move with every processed block, so while something is
    // sounding the editor cannot wait for the next input event to redraw.
    if state.meters.any_playhead() {
        ctx.request_repaint();
    }

    CentralPanel::default()
        .frame(
            Frame::new()
                .fill(THEME.window_bg)
                .inner_margin(THEME.spacing_lg),
        )
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(THEME.spacing_md, THEME.spacing_md);

            egui::ScrollArea::vertical().show(ui, |ui| {
                header(ui, state);
                ui.add_space(THEME.spacing_md);
                import_requested = source_section(ui, state);
                ui.add_space(THEME.spacing_md);
                performance_section(ui, state);
                ui.add_space(THEME.spacing_md);
                cell_section(ui, state);
                ui.add_space(THEME.spacing_md);
                output_section(ui, setter, state);
            });
        });

    import_requested
}

/// Product name and the sample currently loaded.
fn header(ui: &mut Ui, state: &ViewState<'_>) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), THEME.font_lg * 1.6),
        egui::Sense::hover(),
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

/// Waveform, slicing controls and selection. Returns true on an import request.
fn source_section(ui: &mut Ui, state: &ViewState<'_>) -> bool {
    let mut import_requested = false;

    section(ui, "SOURCE SAMPLE", |ui| {
        import_requested = toolbar(ui, state);
        ui.add_space(THEME.spacing_sm);

        let Ok(mut project) = state.project.lock() else {
            return;
        };
        let Ok(mut sample) = state.sample.lock() else {
            return;
        };

        let total = sample.peaks.frames();
        if sample.view.is_empty() && total > 0 {
            sample.view = ViewRange::full(total);
        }

        // Collected once per frame: the widget reads the positions several
        // times while drawing, and they must not change underneath it.
        let playheads: Vec<u64> = state.meters.playheads().collect();
        let action = waveform(
            ui,
            &THEME,
            &WaveformSource {
                peaks: &sample.peaks,
                buffer: sample.buffer.as_deref(),
                slices: project.project.slices(),
                selected: project.project.selection(),
                playheads: &playheads,
                view: sample.view,
            },
            WAVEFORM_HEIGHT,
        );

        if let Some(view) = action.view {
            sample.view = view;
        }

        let mut selection_changed = false;

        if let Some(id) = action.select {
            project.project.select(Some(id));
            selection_changed = true;
            // A click auditions what it selected; the voice ends by itself at
            // the end of the slice, so nothing has to release it.
            if let Some(slice) = project.project.slice(id) {
                state.send(EngineCommand::Preview(CellSpec {
                    bounds: SliceBounds {
                        start_frame: slice.start_frame,
                        end_frame: slice.end_frame,
                    },
                    ..CellSpec::default()
                }));
            }
        }

        if let Some((id, frame)) = action.split {
            if project.project.split_slice(id, frame).is_some() {
                selection_changed = true;
            }
        }

        if let Some((from, to)) = action.move_boundary {
            if project.project.move_boundary(from, to, total) {
                selection_changed = true;
            }
        }

        if let Some(frame) = action.remove_boundary {
            if project.project.remove_boundary(frame) {
                selection_changed = true;
            }
        }

        // Editing slices can move or remove what the cells play, so the
        // keyboard mapping is pushed again.
        if selection_changed {
            sync_cells(state, &project);
        }

        ui.add_space(THEME.spacing_sm);
        status_line(ui, &project, &sample, action.hovered_frame, total);
    });

    import_requested
}

/// Import button, quick divisions, zoom and the selected slice controls.
fn toolbar(ui: &mut Ui, state: &ViewState<'_>) -> bool {
    let mut import_requested = false;
    let loading = state
        .sample
        .lock()
        .map(|sample| sample.loading)
        .unwrap_or(false);

    ui.horizontal(|ui| {
        let label = if loading {
            "Lädt …"
        } else {
            "Sample laden …"
        };
        if button(ui, &THEME, label) && !loading {
            import_requested = true;
        }

        ui.add_space(THEME.spacing_md);

        let has_sample = state
            .project
            .lock()
            .map(|project| project.project.sample.is_some())
            .unwrap_or(false);
        if !has_sample {
            return;
        }

        let labels: Vec<String> = EVEN_DIVISIONS
            .iter()
            .map(|count| format!("{count}"))
            .collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();

        // No division is "current", so the selector is drawn without one.
        if let Some(index) = segmented(ui, &THEME, &refs, usize::MAX) {
            divide_evenly(state, EVEN_DIVISIONS[index]);
        }

        ui.add_space(THEME.spacing_md);
        if button(ui, &THEME, "Slice löschen") {
            remove_selected(state);
        }
        if button(ui, &THEME, "All Notes Off") {
            state.send(EngineCommand::AllNotesOff);
        }
    });

    import_requested
}

/// Replace the slices with `count` equal divisions and select the first.
fn divide_evenly(state: &ViewState<'_>, count: u32) {
    let Ok(mut project) = state.project.lock() else {
        return;
    };
    project.project.slice_evenly(count);
    let first = project.project.slices().first().map(|slice| slice.id);
    project.project.select(first);
    // Dividing replaces every slice, so whatever the notes played is gone.
    project.project.clear_cells();
    sync_cells(state, &project);
}

/// Remove the selected slice and clear what the engine plays.
fn remove_selected(state: &ViewState<'_>) {
    let Ok(mut project) = state.project.lock() else {
        return;
    };
    let Some(id) = project.project.selection() else {
        return;
    };
    project.project.remove_slice(id);
    sync_cells(state, &project);
}

/// One line of context under the waveform.
fn status_line(
    ui: &mut Ui,
    project: &ProjectFile,
    sample: &SampleView,
    hovered_frame: Option<u64>,
    total: u64,
) {
    let slices = project.project.slices().len();
    let selection = project
        .project
        .selected()
        .map(|slice| {
            let index = project
                .project
                .slices()
                .iter()
                .position(|candidate| candidate.id == slice.id)
                .map(|index| index + 1)
                .unwrap_or(0);
            format!("Slice {index}  ·  {} Frames", slice.len_frames())
        })
        .unwrap_or_else(|| "kein Slice gewählt".to_owned());

    let position = hovered_frame
        .map(|frame| format!("  ·  Frame {frame}"))
        .unwrap_or_default();
    let zoom = if total == 0 || sample.view.is_full(total) {
        String::new()
    } else {
        format!(
            "  ·  Zoom {:.0} %",
            total as f64 / sample.view.len_frames().max(1) as f64 * 100.0
        )
    };
    let gestures = if total == 0 {
        String::new()
    } else {
        "   |   Rad: Zoom  ·  ziehen: verschieben  ·  Marker ziehen: Grenze  ·  Rechtsklick auf Marker: entfernen  ·  Doppelklick: teilen"
            .to_owned()
    };

    let text = match sample.status.as_deref() {
        Some(status) => status.to_owned(),
        None => format!("{slices} Slices  ·  {selection}{position}{zoom}{gestures}"),
    };
    let color = if sample.status.is_some() {
        THEME.danger
    } else {
        THEME.text_dim
    };

    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), THEME.font_sm * 1.6),
        egui::Sense::hover(),
    );
    ui.painter().text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(THEME.font_sm),
        color,
    );
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
