mod import;
mod thumbnail;

pub use import::{ImportJobHandler, ImportJobResult};
pub use thumbnail::{ThumbnailJobHandler, ThumbnailJobPayload, enqueue as enqueue_thumbnail};

use crate::application::Application;
use crate::jobs::WorkerRegistry;

pub fn registry(application: Application) -> WorkerRegistry {
    WorkerRegistry::new()
        .register("import", ImportJobHandler::new(application.clone()))
        .register("thumbnail", ThumbnailJobHandler::new(application))
}
