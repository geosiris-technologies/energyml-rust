//! What is asked of a view: [`ViewAsk`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One view, the objects it shows, its settings and its size.
///
/// This is also what a drawing keeps in its metadata, its settings **completed with their defaults** by
/// [`crate::draw_svg`], so that a drawing made again does not depend on a default that changed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewAsk {
    /// The id of the view (`well-locator`).
    pub view: String,
    /// The objects shown, by full uri, in the order of the view's slots ([`crate::ObjectSlot`]).
    #[serde(default)]
    pub objects: Vec<String>,
    /// The settings given, by their declared names.
    #[serde(default)]
    pub settings: BTreeMap<String, Value>,
    /// Width and height, in pixels; `None`: the view's own size.
    #[serde(default)]
    pub size: Option<[f64; 2]>,
}

impl ViewAsk {
    /// A view, nothing else asked.
    pub fn new(view: impl Into<String>) -> Self {
        ViewAsk {
            view: view.into(),
            objects: Vec::new(),
            settings: BTreeMap::new(),
            size: None,
        }
    }

    /// One more object, by full uri.
    pub fn object(mut self, uri: impl Into<String>) -> Self {
        self.objects.push(uri.into());
        self
    }

    /// One setting.
    pub fn set(mut self, name: impl Into<String>, value: Value) -> Self {
        self.settings.insert(name.into(), value);
        self
    }

    /// A size, in pixels.
    pub fn sized(mut self, width: f64, height: f64) -> Self {
        self.size = Some([width, height]);
        self
    }
}
