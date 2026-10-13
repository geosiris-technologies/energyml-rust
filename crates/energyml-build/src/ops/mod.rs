//! The built-in operations — one file each, and [`builtin`] is the only list.
//!
//! Adding an operation is a file and a line here, plus its canonical template
//! in `rc/activityTemplates.json`; `tests/operations.rs` checks the two agree.

pub mod attach;
pub mod boundary;
pub mod business_associate;
pub mod change_frame;
pub mod collection;
pub mod colormap;
pub mod computed_tops;
pub mod contacts;
pub mod contacts_rank;
pub mod crs;
pub mod graphical_information;
pub mod grid;
pub mod grid_properties;
pub mod horizon_law;
pub mod estimate;
pub mod ijk_grid;
pub mod interpolate;
mod interpolate_mba;
pub mod intersect;
mod oblique_grid;
pub mod cross_section;
pub mod clip;
pub mod delete_nodes;
pub mod polyline_through;
pub mod densify;
pub mod extend;
pub mod earth_model;
pub mod earth_model_from_stratigraphy;
pub mod edit_object;
pub mod new_version;
pub mod export_media;
pub mod fault;
pub mod faulted_grid;
pub mod fault_throw;
pub mod isocontours;
pub mod loft;
pub mod local_grid;
pub mod media;
pub mod outline;
pub mod section_view;
pub mod pointset;
pub mod polylines;
pub mod polylines_from_cloud;
pub mod project;
pub mod property;
pub mod property_time_series;
pub mod remesh;
pub mod resurface;
pub mod sigeol;
pub mod sigeol_boreholes;
pub mod wells_from_table;
pub mod properties_from_table;
pub mod wells_and_properties;
pub mod wellheads;
pub mod extend_boreholes;
pub mod surface_map;
pub mod stratigraphy;
pub mod structural_organisation;
pub mod surface_framework;
pub mod support;
pub mod terrain;
pub mod thickness;
pub mod measure;
pub mod zone_statistics;
pub mod time_series;
pub mod transform;
pub mod triangles;
pub mod triangulate;
pub mod well;
pub mod well_plan;
pub mod wells_from_polylines;
pub mod witsml_wells;
pub mod witsml_logs;
pub mod prodml_doublet;
mod prodml_writer;
pub mod fluid_sample;
pub mod dts_installation;
pub mod injection_test;
pub mod pta;
pub mod product_flow_model;
pub mod well_logs;
pub mod representation_set;
pub mod subrepresentation;
pub mod blocked_wellbore;
pub mod written;

use crate::operation::AnyOperation;

/// The built-ins, in the order `--list` shows them.
pub fn builtin() -> Vec<Box<dyn AnyOperation>> {
    vec![
        crate::operation::typed(crs::Crs),
        crate::operation::typed(time_series::TimeSeries),
        crate::operation::typed(pointset::Pointset),
        crate::operation::typed(polylines::Polylines),
        crate::operation::typed(polylines_from_cloud::PolylinesFromCloud),
        crate::operation::typed(triangles::Triangles),
        crate::operation::typed(triangulate::Triangulate),
        crate::operation::typed(isocontours::Isocontours),
        crate::operation::typed(cross_section::CrossSection),
        crate::operation::typed(intersect::Intersect),
        crate::operation::typed(remesh::Remesh),
        crate::operation::typed(transform::Transform),
        crate::operation::typed(densify::Densify),
        crate::operation::typed(extend::Extend),
        crate::operation::typed(project::Project),
        crate::operation::typed(loft::Loft),
        crate::operation::typed(fault::FaultFromSticks),
        crate::operation::typed(fault_throw::FaultThrow),
        crate::operation::typed(delete_nodes::DeleteNodes),
        crate::operation::typed(clip::Clip),
        crate::operation::typed(terrain::Terrain),
        crate::operation::typed(polyline_through::PolylineThrough),
        crate::operation::typed(grid::Grid),
        crate::operation::typed(interpolate::Interpolate),
        crate::operation::typed(estimate::Estimate),
        crate::operation::typed(resurface::Resurface),
        crate::operation::typed(horizon_law::HorizonFromLaw),
        crate::operation::typed(ijk_grid::IjkGridFromHorizons),
        crate::operation::typed(faulted_grid::FaultedIjkGrid),
        crate::operation::typed(grid_properties::GridProperties),
        crate::operation::typed(local_grid::LocalGrid),
        crate::operation::typed(subrepresentation::Subrepresentation),
        crate::operation::typed(representation_set::RepresentationSet),
        crate::operation::typed(blocked_wellbore::BlockedWellbore),
        crate::operation::typed(change_frame::ChangeFrame),
        crate::operation::typed(outline::Outline),
        crate::operation::typed(boundary::Boundary),
        crate::operation::typed(surface_map::SurfaceMap),
        crate::operation::typed(thickness::Thickness),
        crate::operation::typed(measure::Measure),
        crate::operation::typed(zone_statistics::ZoneStatistics),
        crate::operation::typed(property::Property),
        crate::operation::typed(property_time_series::PropertyTimeSeries),
        crate::operation::typed(colormap::Colormap),
        crate::operation::typed(graphical_information::GraphicalInformation),
        crate::operation::typed(well::WellFromPolyline),
        crate::operation::typed(wells_from_polylines::WellsFromPolylines),
        crate::operation::typed(well_plan::WellFromPlan),
        crate::operation::typed(witsml_wells::WitsmlWells),
        crate::operation::typed(witsml_logs::WitsmlLogs),
        crate::operation::typed(prodml_doublet::ProdmlDoublet),
        crate::operation::typed(fluid_sample::FluidSample),
        crate::operation::typed(dts_installation::DtsInstallation),
        crate::operation::typed(injection_test::InjectionTest),
        crate::operation::typed(pta::PressureTransientAnalysis),
        crate::operation::typed(product_flow_model::ProductFlowModel),
        crate::operation::typed(sigeol::SigeolUnits),
        crate::operation::typed(sigeol_boreholes::SigeolBoreholes),
        crate::operation::typed(stratigraphy::StratigraphyFromGeolts),
        crate::operation::typed(earth_model::EarthModel),
        crate::operation::typed(earth_model_from_stratigraphy::EarthModelFromStratigraphy),
        crate::operation::typed(structural_organisation::StructuralOrganisation),
        crate::operation::typed(surface_framework::SurfaceFramework),
        crate::operation::typed(collection::Collection),
        crate::operation::typed(edit_object::EditObject),
        crate::operation::typed(business_associate::BusinessAssociate),
        crate::operation::typed(wells_from_table::WellsFromTable),
        crate::operation::typed(wellheads::Wellheads),
        crate::operation::typed(properties_from_table::PropertiesFromTable),
        crate::operation::typed(wells_and_properties::WellsAndProperties),
        crate::operation::typed(contacts::ContactsFromFrames),
        crate::operation::typed(extend_boreholes::ExtendBoreholes),
        crate::operation::typed(well_logs::WellLogs),
        crate::operation::typed(computed_tops::ComputedTops),
        crate::operation::typed(media::Media),
        crate::operation::typed(attach::Attach),
        crate::operation::typed(section_view::SectionView),
        crate::operation::typed(export_media::ExportMedia),
        crate::operation::typed(new_version::NewVersion),
    ]
}
