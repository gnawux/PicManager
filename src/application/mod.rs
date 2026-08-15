mod collection_service;
mod context;
mod dedup_service;
mod error;
mod import_service;
mod metadata_service;
mod repository;

pub use collection_service::{CollectionIdentity, CollectionService, MembershipChange};
pub use context::{Application, AuthorizationScope, CallerKind, LibraryIdentity, RequestContext};
pub use dedup_service::DedupService;
pub use error::{ServiceError, ServiceErrorCode, ServiceResult};
pub use import_service::{ImportCommand, ImportService};
pub use metadata_service::{MetadataUpdateResult, PhotoMetadataService, PhotoMetadataUpdate};
pub use repository::{PhotoLifecycle, PhotoRepository, SqlitePhotoRepository};
