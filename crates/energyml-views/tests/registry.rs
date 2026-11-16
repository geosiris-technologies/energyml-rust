//! The registry of views: the caller's first, an unknown view refused with the known ones.

use energyml_domain::package_index::PackageIndex;
use energyml_sheet::draw::{DisplayList, PageUnit};
use energyml_views::{
    Drawn, ObjectSlot, Offer, Request, ViewAsk, ViewContext, ViewKind, ViewKinds, ViewRefusal, ViewSetting,
    draw_svg,
};

use crate::fixtures::{NO_MAPS, Opened, package};

/// A view of the caller's, of the same id as a built-in.
struct Mine;

impl ViewKind for Mine {
    fn id(&self) -> &'static str {
        "well-locator"
    }
    fn about(&self) -> &'static str {
        "mine"
    }
    fn objects(&self) -> Vec<ObjectSlot> {
        Vec::new()
    }
    fn settings(&self) -> Vec<ViewSetting> {
        Vec::new()
    }
    fn offers(&self, _index: &PackageIndex) -> Vec<Offer> {
        Vec::new()
    }
    fn draw(&self, request: &Request, _cx: &ViewContext<'_>) -> Result<Drawn, ViewRefusal> {
        let mut list = DisplayList::new(request.size, PageUnit::Pixel);
        list.title = "mine".to_string();
        Ok(Drawn {
            list,
            notes: Vec::new(),
        })
    }
}

#[test]
fn a_callers_view_is_tried_before_a_built_in_of_the_same_id() {
    let mut kinds = ViewKinds::with_builtins();
    kinds.register(Box::new(Mine));
    assert_eq!(kinds.find("well-locator").map(|kind| kind.about()), Some("mine"));
    assert_eq!(
        kinds.ids().iter().filter(|id| **id == "well-locator").count(),
        1,
        "one id, once"
    );
    // Break it: push the caller's view last in `register`, and the built-in answers.
}

#[test]
fn an_unknown_view_is_refused_with_the_known_ones() {
    let opened = Opened::of(package(50.0, &[20.0, 80.0]));
    let refused = draw_svg(
        &ViewKinds::with_builtins(),
        &ViewAsk::new("nothing"),
        &opened.context(&NO_MAPS),
    )
    .expect_err("refused");
    let ViewRefusal::UnknownView { known, .. } = refused else {
        panic!("{refused}")
    };
    assert!(known.contains("well-locator"), "{known}");
    // Break it: return an empty picture for an unknown id.
}
