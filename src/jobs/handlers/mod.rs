mod analysis;
mod import;
mod thumbnail;

pub use import::{ImportJobHandler, ImportJobResult};
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
        .register("thumbnail", ThumbnailJobHandler::new(application))
}
pub use analysis::{
    AnimalAnalysisJobHandler, FaceAnalysisJobHandler, GeocodeJobHandler, enqueue_animal_analysis,
    enqueue_face_analysis, enqueue_geocode,
};
