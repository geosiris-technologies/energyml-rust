//! One kind of view ([`ViewKind`]) and the registry of them ([`ViewKinds`]).

use energyml_domain::ArraySource;
use energyml_domain::package_index::PackageIndex;
use energyml_domain::palettes::Palettes;
use energyml_sheet::draw::DisplayList;
use serde::Serialize;

use crate::colour::ColourDoor;
use crate::refusal::ViewRefusal;
use crate::setting::{Settled, ViewSetting};

/// What a view reads through.
pub struct ViewContext<'a> {
    /// The objects and their arrays.
    pub source: &'a dyn ArraySource,
    /// The package index: detection, lookups, the one door from a name to an object.
    pub index: &'a PackageIndex,
    /// The colour the package gives an object ([`crate::colour`]).
    pub colours: &'a dyn ColourDoor,
    /// The palettes, for what the file gives no colour map.
    pub palettes: &'a Palettes,
}

/// The objects of one role a view takes.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectSlot {
    /// The role (`well`, `property`, `focus`).
    pub role: &'static str,
    /// The types accepted, by their name without package or release (`WellboreFrameRepresentation`): every
    /// release is taken, a RESQML 2.0.1 `obj_` prefix included ([`local_type`]).
    pub types: &'static [&'static str],
    /// How many at least.
    pub least: usize,
    /// How many at most.
    pub most: usize,
    /// What it is, in one sentence.
    pub about: &'static str,
}

/// What a view can show in a package, read from the index alone: the request a face can send as it is.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Offer {
    /// The view.
    pub view: &'static str,
    /// The objects it would show, by full uri, in the order of its slots.
    pub objects: Vec<String>,
    /// What it would show, in words (the titles of the objects).
    pub title: String,
    /// What a request must still give (a setting the file does not say), in words; empty: ready.
    pub missing: Vec<String>,
}

/// A part a view left out while drawing the rest.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ViewNote {
    /// A stable word for a face to translate (`head-portion-assumed`).
    pub key: &'static str,
    /// The sentence, in English.
    pub says: String,
}

/// What a view drew.
#[derive(Clone, Debug, PartialEq)]
pub struct Drawn {
    /// The picture.
    pub list: DisplayList,
    /// What it left out.
    pub notes: Vec<ViewNote>,
}

/// A request checked against the view's declarations: what a view draws from.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    /// The objects, by role, each a full uri the package holds, in the order given.
    pub objects: Vec<(&'static str, String)>,
    /// The settings, checked, defaults filled.
    pub settings: Settled,
    /// Width and height, in pixels.
    pub size: [f64; 2],
}

impl Request {
    /// The first object of a role.
    pub fn object(&self, role: &str) -> Option<&str> {
        self.objects
            .iter()
            .find(|(slot, _)| *slot == role)
            .map(|(_, uri)| uri.as_str())
    }

    /// Every object of a role, in order.
    pub fn objects_of(&self, role: &str) -> Vec<&str> {
        self.objects
            .iter()
            .filter(|(slot, _)| *slot == role)
            .map(|(_, uri)| uri.as_str())
            .collect()
    }
}

/// One kind of view. A view lays out what the other crates compute; it never computes a second time what they do.
pub trait ViewKind: Send + Sync {
    /// Its id, `kebab-case`, in the registry and in a drawing's metadata.
    fn id(&self) -> &'static str;
    /// What it shows, in one sentence.
    fn about(&self) -> &'static str;
    /// The objects it takes, slot by slot, in order.
    fn objects(&self) -> Vec<ObjectSlot>;
    /// The settings it reads.
    fn settings(&self) -> Vec<ViewSetting>;
    /// Its size when a request gives none, in pixels.
    fn size(&self) -> [f64; 2] {
        [800.0, 600.0]
    }
    /// What it can show in this package, from the index alone (no array read).
    fn offers(&self, index: &PackageIndex) -> Vec<Offer>;
    /// The picture. Pure: the same request over the same package draws the same list.
    ///
    /// # Errors
    ///
    /// The named refusal of what it lays out.
    fn draw(&self, request: &Request, cx: &ViewContext<'_>) -> Result<Drawn, ViewRefusal>;
}

/// The views: the caller's tried first, then the built-ins.
pub struct ViewKinds {
    kinds: Vec<Box<dyn ViewKind>>,
}

impl ViewKinds {
    /// No view at all.
    pub fn empty() -> Self {
        ViewKinds { kinds: Vec::new() }
    }

    /// The views of this crate.
    pub fn with_builtins() -> Self {
        ViewKinds {
            kinds: vec![
                Box::new(crate::along_well::AlongWell),
                Box::new(crate::image_track::ImageTrack),
                Box::new(crate::md_coverage::MdCoverage),
                Box::new(crate::property_insight::PropertyInsight),
                Box::new(crate::provenance_graph::ProvenanceView),
                Box::new(crate::survey_qc::SurveyQc),
                Box::new(crate::table_curve::TableCurve),
                Box::new(crate::table_schema::TableSchemaView),
                Box::new(crate::target::Target),
                Box::new(crate::well_locator::WellLocator),
                Box::new(crate::well_sheet::WellSheet),
            ],
        }
    }

    /// A view tried before the others: one of the same id replaces a built-in.
    pub fn register(&mut self, view: Box<dyn ViewKind>) {
        self.kinds.insert(0, view);
    }

    /// The view of this id: the first registered.
    pub fn find(&self, id: &str) -> Option<&dyn ViewKind> {
        self.kinds
            .iter()
            .find(|kind| kind.id() == id)
            .map(|kind| kind.as_ref())
    }

    /// Every id, one each, in the order they are tried.
    pub fn ids(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for kind in &self.kinds {
            if !out.contains(&kind.id()) {
                out.push(kind.id());
            }
        }
        out
    }

    /// What every view can show in this package, view by view in the order of [`ViewKinds::ids`].
    pub fn offers(&self, index: &PackageIndex) -> Vec<Offer> {
        self.ids()
            .into_iter()
            .filter_map(|id| self.find(id))
            .flat_map(|kind| kind.offers(index))
            .collect()
    }
}

impl Default for ViewKinds {
    fn default() -> Self {
        Self::with_builtins()
    }
}

/// The name of a type without its package, its release and RESQML 2.0.1's `obj_` prefix:
/// `resqml20.obj_WellboreFrameRepresentation` is `WellboreFrameRepresentation`.
pub fn local_type(qualified_type: &str) -> &str {
    let local = qualified_type.rsplit('.').next().unwrap_or(qualified_type);
    local.strip_prefix("obj_").unwrap_or(local)
}
