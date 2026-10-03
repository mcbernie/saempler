use std::fmt;

use serde::{Deserialize, Serialize};

/// Version of the serialized project layout understood by this build.
///
/// Every stored project carries this number so that future layout changes can
/// be detected instead of silently misinterpreting old data.
pub const PROJECT_VERSION: u32 = 1;

/// Waveform of the development test tone.
///
/// This is the stand-in signal source used until the sample engine exists. It
/// lives in the project rather than in the host parameters because it is a
/// discrete editing choice, not an automatable control.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Waveform {
    #[default]
    Sine,
    Saw,
    Square,
}

impl Waveform {
    /// All variants in the order they are presented in the user interface.
    pub const ALL: [Waveform; 3] = [Waveform::Sine, Waveform::Saw, Waveform::Square];

    /// Short label for display in the user interface.
    pub fn label(self) -> &'static str {
        match self {
            Waveform::Sine => "Sine",
            Waveform::Saw => "Saw",
            Waveform::Square => "Square",
        }
    }
}

/// Editable project state that is not exposed as a host parameter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    pub waveform: Waveform,
}

/// Versioned envelope around [`Project`].
///
/// Serialization always goes through this type so that the version travels
/// with the data. Fields added to [`Project`] later are covered by serde
/// defaults; structural changes are handled in [`ProjectFile::migrate`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectFile {
    pub version: u32,
    pub project: Project,
}

impl Default for ProjectFile {
    fn default() -> Self {
        Self {
            version: PROJECT_VERSION,
            project: Project::default(),
        }
    }
}

impl ProjectFile {
    /// Bring a loaded project up to [`PROJECT_VERSION`].
    ///
    /// Returns an error for projects written by a newer build, because their
    /// contents cannot be interpreted correctly here.
    pub fn migrate(&mut self) -> Result<(), ProjectError> {
        if self.version > PROJECT_VERSION {
            return Err(ProjectError::UnsupportedVersion(self.version));
        }

        // No structural migrations exist yet. Older versions only ever differ
        // by added fields, which serde fills in with their defaults.
        self.version = PROJECT_VERSION;
        Ok(())
    }
}

/// Failures that can occur while loading persisted project state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectError {
    /// The project was written by a build with a newer project version.
    UnsupportedVersion(u32),
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectError::UnsupportedVersion(version) => write!(
                f,
                "Projektversion {version} wird von dieser Version nicht unterstützt \
                 (unterstützt bis {PROJECT_VERSION})"
            ),
        }
    }
}

impl std::error::Error for ProjectError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_project() {
        let original = ProjectFile {
            version: PROJECT_VERSION,
            project: Project {
                waveform: Waveform::Square,
            },
        };

        let json = serde_json::to_string(&original).expect("serialization must succeed");
        let restored: ProjectFile =
            serde_json::from_str(&json).expect("deserialization must succeed");

        assert_eq!(restored, original);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let restored: ProjectFile =
            serde_json::from_str(r#"{"version":1,"project":{}}"#).expect("must deserialize");

        assert_eq!(restored.project.waveform, Waveform::Sine);
    }

    #[test]
    fn migrate_accepts_current_version() {
        let mut file = ProjectFile::default();

        assert_eq!(file.migrate(), Ok(()));
        assert_eq!(file.version, PROJECT_VERSION);
    }

    #[test]
    fn migrate_rejects_future_versions() {
        let mut file = ProjectFile {
            version: PROJECT_VERSION + 1,
            project: Project::default(),
        };

        assert_eq!(
            file.migrate(),
            Err(ProjectError::UnsupportedVersion(PROJECT_VERSION + 1))
        );
    }
}
