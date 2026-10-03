use nih_plug::prelude::nih_export_standalone;
use saempler_plugin::Saempler;

fn main() {
    nih_export_standalone::<Saempler>();
}
