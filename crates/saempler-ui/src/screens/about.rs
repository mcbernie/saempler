use nih_plug_egui::egui::{self, pos2, vec2, Align2, FontId, Id, Rect, Sense, Ui};

use crate::screens::main::{hint_light, logo_mark, THEME};
use crate::widgets::inset;

/// Version of the running build, from the crate metadata.
///
/// Taken at compile time rather than written down, so the window cannot
/// disagree with the binary it is part of.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

const REPOSITORY: &str = "https://github.com/mcbernie/saempler";
const SPONSORS: &str = "https://github.com/sponsors/mcbernie";
const LICENSE_URL: &str = "https://github.com/mcbernie/saempler/blob/main/LICENSING.md";

/// Memory key for whether the about window is open.
fn open_id() -> Id {
    Id::new("about-open")
}

/// Ask for the about window to appear.
pub fn request(ui: &Ui) {
    ui.memory_mut(|memory| memory.data.insert_temp(open_id(), true));
}

/// Whether a click landed on the name plate, which is what opens the window.
///
/// The plate rather than a button of its own: the name is already there, it is
/// where anyone looks for a version number, and the masthead has no room to
/// spare.
pub fn name_plate_clicked(ui: &mut Ui, plate: Rect) -> bool {
    ui.interact(plate, Id::new("about-plate"), Sense::click())
        .on_hover_text("Über Sämpler")
        .clicked()
}

/// Version, licence and where to find the project.
pub fn window(ui: &Ui) {
    let mut open = ui.memory(|memory| memory.data.get_temp(open_id()).unwrap_or(false));
    if !open {
        return;
    }

    egui::Window::new("Über Sämpler")
        .id(Id::new("about-window"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_pos(pos2(420.0, 300.0))
        .show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing = vec2(THEME.spacing_sm, THEME.spacing_sm);
            ui.set_min_width(340.0);

            let (rect, _) = ui.allocate_exact_size(vec2(324.0, 56.0), Sense::hover());
            inset(ui.painter(), &THEME, rect, THEME.waveform_bg);
            logo_mark(ui.painter(), pos2(rect.min.x + 24.0, rect.center().y));
            ui.painter().text(
                pos2(rect.min.x + 46.0, rect.center().y - 8.0),
                Align2::LEFT_CENTER,
                "SÄMPLER",
                FontId::proportional(THEME.font_lg),
                THEME.accent,
            );
            ui.painter().text(
                pos2(rect.min.x + 46.0, rect.center().y + 10.0),
                Align2::LEFT_CENTER,
                format!("Version {VERSION}"),
                FontId::monospace(THEME.font_sm),
                THEME.label,
            );

            ui.add_space(THEME.spacing_sm);
            hint_light(
                ui,
                "Slice- und Remix-Instrument für VST3, CLAP und standalone",
            );

            ui.add_space(THEME.spacing_sm);
            ui.hyperlink_to("Projektseite und Quellcode", REPOSITORY);
            ui.hyperlink_to("Lizenz: GPL-3.0 oder kommerziell", LICENSE_URL);
            ui.hyperlink_to("♥  Entwicklung unterstützen", SPONSORS);

            ui.add_space(THEME.spacing_sm);
            hint_light(
                ui,
                "Updates stehen auf der Projektseite. Diese Fassung prüft nichts im Netz.",
            );
            hint_light(
                ui,
                "VST3 ist eine Marke der Steinberg Media Technologies GmbH.",
            );
        });

    ui.memory_mut(|memory| memory.data.insert_temp(open_id(), open));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_comes_from_the_build() {
        // A number typed into the window would drift from the one the host
        // reports, and the two are the same claim.
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
        assert!(
            VERSION.split('.').count() >= 3,
            "expected a semantic version, got {VERSION}"
        );
    }

    #[test]
    fn every_link_is_https() {
        // These open a browser on the user's machine from inside their host.
        for link in [REPOSITORY, SPONSORS, LICENSE_URL] {
            assert!(link.starts_with("https://"), "{link}");
        }
    }
}
