mod analysis;
mod import;
mod maintenance;
mod thumbnail;

pub use import::{ImportJobHandler, ImportJobResult};
pub use maintenance::{
    DedupScanJobHandler, DerivedMaintenanceJobHandler, enqueue_dedup_scan,
    enqueue_derived_maintenance,
};
pub use thumbnail::{ThumbnailJobHandler, ThumbnailJobPayload, enqueue as enqueue_thumbnail};

use crate::application::Application;
use crate::jobs::WorkerRegistry;

pub fn registry(application: Application) -> WorkerRegistry {
    WorkerRegistry::new()
        .register("import", ImportJobHandler::new(application.clone()))
        .register("face_analysis", FaceAnalysisJobHandler(application.clone()))
        .register(
            "animal_analysis",
            AnimalAnalysisJobHandler(application.clone()),
        )
        .register("geocode", GeocodeJobHandler(application.clone()))
        .register("dedup_scan", DedupScanJobHandler(application.clone()))
        .register(
            "derived_maintenance",
            DerivedMaintenanceJobHandler(application.clone()),
        )
        .register("thumbnail", ThumbnailJobHandler::new(application))
}
pub use analysis::{
    AnimalAnalysisJobHandler, FaceAnalysisJobHandler, GeocodeJobHandler, enqueue_animal_analysis,
    enqueue_face_analysis, enqueue_geocode,
};
