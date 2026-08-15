mod inventory;
mod repository;

pub use inventory::{AppleInventoryReport, ingest_inventory};
pub use repository::{
    AppleLinkCandidate, AppleSourcePage, AppleSourceView, get_source, list_link_candidates,
    list_sources, retry_source, review_link,
};
