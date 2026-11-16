//! Why a view drew nothing: [`ViewRefusal`].

use energyml_sheet::draw::{DrawError, MetadataError};

/// Why no picture was made. Every refusal is named: a view never answers with an empty picture.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ViewRefusal {
    /// The registry has no view of that id.
    #[error("no view `{asked}`: the views are {known}")]
    UnknownView {
        /// The id asked.
        asked: String,
        /// The ids the registry holds, comma-separated.
        known: String,
    },
    /// A setting the view does not declare.
    #[error("the view `{view}` takes no setting `{name}`: it takes {known}")]
    UnknownSetting {
        /// The view.
        view: String,
        /// The name given.
        name: String,
        /// The names it declares, comma-separated (or `none`).
        known: String,
    },
    /// A setting whose value is of the wrong nature or out of its bounds.
    #[error("setting `{name}`: {why}")]
    BadSetting {
        /// The setting.
        name: String,
        /// Why the value does not fit.
        why: String,
    },
    /// A setting the view needs and nobody gave.
    #[error("the view `{view}` needs the setting `{name}`: {about}")]
    MissingSetting {
        /// The view.
        view: String,
        /// The setting.
        name: String,
        /// What it is.
        about: String,
    },
    /// Fewer objects of a role than the view needs.
    #[error("the view `{view}` needs {least} {role} ({types}), {given} given")]
    MissingObject {
        /// The view.
        view: String,
        /// The role of the slot.
        role: String,
        /// The types it accepts, comma-separated.
        types: String,
        /// How many it needs.
        least: usize,
        /// How many were given.
        given: usize,
    },
    /// An object that fits no slot of the view, or one too many.
    #[error("the view `{view}` takes no `{object}` ({qualified_type}): it takes {slots}")]
    ObjectNotTaken {
        /// The view.
        view: String,
        /// The object's uri.
        object: String,
        /// Its qualified type.
        qualified_type: String,
        /// The slots of the view, said in words.
        slots: String,
    },
    /// An object the package does not hold.
    #[error("the package holds no `{object}`")]
    NotInPackage {
        /// The uri asked.
        object: String,
    },
    /// The computing the view lays out refused, by its own name.
    #[error("{view}: {why}")]
    Computing {
        /// The view.
        view: String,
        /// The refusal of the computing, as it says it.
        why: String,
    },
    /// No palette of that name, or one for another reading.
    #[error("palette `{asked}`: {why}")]
    Palette {
        /// The name asked.
        asked: String,
        /// Why it does not fit, with the names that would.
        why: String,
    },
    /// The display list did not write.
    #[error("drawing: {0}")]
    Draw(#[from] DrawError),
    /// A drawing whose metadata is missing, of another format, or does not read.
    #[error("the drawing's metadata: {0}")]
    Metadata(String),
}

impl From<MetadataError> for ViewRefusal {
    fn from(error: MetadataError) -> Self {
        ViewRefusal::Metadata(error.to_string())
    }
}

impl ViewRefusal {
    /// The refusal of the computing a view lays out, by its own words.
    pub fn computing(view: &str, why: impl std::fmt::Display) -> Self {
        ViewRefusal::Computing {
            view: view.to_string(),
            why: why.to_string(),
        }
    }
}
