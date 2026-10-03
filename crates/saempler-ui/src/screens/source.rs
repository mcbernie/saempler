use nih_plug_egui::egui::{self, Align2, FontId, Ui};
use saempler_audio::EngineCommand;
use saempler_model::ProjectFile;

use crate::screens::main::{preview_spec, section, EditorState, ViewState, THEME};
use crate::screens::performance::sync_cells;
use crate::widgets::{button, segmented, waveform, ViewRange, WaveformSource};

const WAVEFORM_HEIGHT: f32 = 190.0;

/// Slice counts offered by the quick division buttons.
const EVEN_DIVISIONS: [u32; 4] = [4, 8, 16, 32];

/// Waveform, slicing controls and selection. Returns true on an import request.
pub fn source_section(ui: &mut Ui, state: &ViewState<'_>) -> bool {
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
                state.send(EngineCommand::Preview(preview_spec(
                    slice.start_frame,
                    slice.end_frame,
                )));
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
    sample: &EditorState,
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
