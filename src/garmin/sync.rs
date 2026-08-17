use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;
use thiserror::Error;

use super::client::{GarminClient, GarminError};
use super::token_store::GarminTokens;

const PAGE_SIZE: usize = 100;
const DEFAULT_MAX_PAGES: usize = 20;
const MAX_PROVIDER_BODY: usize = 256 * 1024 * 1024;
const MAX_JOURNAL_SIZE: u64 = 16 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum SyncError {
    #[error("Garmin provider request failed")]
    Client(#[from] GarminError),
    #[error("Garmin sync journal is unavailable")]
    Journal,
    #[error("Garmin returned an invalid activity identifier")]
    InvalidActivityId,
    #[error("Garmin returned an invalid ORIGINAL activity file")]
    InvalidDownload,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingActivity {
    pub id: String,
    pub file: String,
    pub start_time: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncBatch {
    pub downloaded: usize,
    pub items: Vec<PendingActivity>,
}

#[derive(Clone, Debug)]
pub struct GarminSync {
    root: PathBuf,
    max_pages: usize,
}

impl GarminSync {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_owned(),
            max_pages: DEFAULT_MAX_PAGES,
        }
    }

    pub fn downloads(&self) -> PathBuf {
        self.root.join("downloads")
    }

    pub fn journal_path(&self) -> PathBuf {
        self.root.join("journal.json")
    }

    pub async fn synchronize(
        &self,
        client: &GarminClient,
        tokens: &mut GarminTokens,
        after: Option<&str>,
    ) -> Result<SyncBatch, SyncError> {
        fs::create_dir_all(self.downloads()).map_err(|_| SyncError::Journal)?;
        let mut journal = self.load_journal()?;
        let cutoff = journal.checkpoint.as_deref().or(after).map(str::to_owned);
        let mut downloaded = 0;
        let mut offset = 0;

        for _ in 0..self.max_pages {
            let response = client
                .authenticated_get(
                    tokens,
                    "activitylist-service/activities/search/activities",
                    &[
                        ("start", offset.to_string()),
                        ("limit", PAGE_SIZE.to_string()),
                    ],
                    "activity_list",
                )
                .await?;
            let activities = response
                .json::<Vec<ActivitySummary>>()
                .await
                .map_err(|_| SyncError::InvalidDownload)?;
            if activities.is_empty() {
                break;
            }
            let page_len = activities.len();
            let mut older_than_cutoff = false;
            let mut ordered = activities;
            ordered.sort_by(|left, right| left.start_time().cmp(&right.start_time()));
            for activity in ordered {
                let id = activity.id()?;
                let start_time = activity.start_time().map(str::to_owned);
                if cutoff
                    .as_deref()
                    .zip(start_time.as_deref())
                    .is_some_and(|(cutoff, start)| start <= cutoff)
                {
                    older_than_cutoff = true;
                    continue;
                }
                let target = self.downloads().join(format!("{id}.fit"));
                if journal.item_is_complete(&id, &target) {
                    continue;
                }
                let response = client
                    .authenticated_get(
                        tokens,
                        &format!("download-service/files/activity/{id}"),
                        &[],
                        "download",
                    )
                    .await?;
                let original = bounded_body(response).await?;
                let fit = original_fit(&original)?;
                write_atomic(&target, &fit).map_err(|_| SyncError::Journal)?;
                journal.items.insert(
                    id,
                    JournalItem {
                        state: ItemState::Downloaded,
                        start_time,
                        sha256: hex::encode(Sha256::digest(&fit)),
                    },
                );
                self.save_journal(&journal)?;
                downloaded += 1;
            }
            if page_len < PAGE_SIZE || older_than_cutoff {
                break;
            }
            offset += page_len;
        }

        let mut items: Vec<_> = journal
            .items
            .iter()
            .filter(|(_, item)| item.state == ItemState::Downloaded)
            .map(|(id, item)| PendingActivity {
                id: id.clone(),
                file: format!("{id}.fit"),
                start_time: item.start_time.clone(),
            })
            .collect();
        items.sort_by(|left, right| left.start_time.cmp(&right.start_time));
        Ok(SyncBatch { downloaded, items })
    }

    pub fn acknowledge(&self, ids: &[String]) -> Result<Option<String>, SyncError> {
        let mut journal = self.load_journal()?;
        let ids: HashSet<&str> = ids.iter().map(String::as_str).collect();
        for (id, item) in &mut journal.items {
            if ids.contains(id.as_str()) && item.state == ItemState::Downloaded {
                item.state = ItemState::Imported;
                if let Some(start_time) = item.start_time.as_ref()
                    && journal
                        .checkpoint
                        .as_ref()
                        .is_none_or(|checkpoint| start_time > checkpoint)
                {
                    journal.checkpoint = Some(start_time.clone());
                }
            }
        }
        self.save_journal(&journal)?;
        Ok(journal.checkpoint)
    }

    fn load_journal(&self) -> Result<Journal, SyncError> {
        let path = self.journal_path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(SyncError::Journal),
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Journal::default());
            }
            Err(_) => return Err(SyncError::Journal),
        };
        if metadata.len() > MAX_JOURNAL_SIZE {
            return Err(SyncError::Journal);
        }
        let payload = fs::read(path).map_err(|_| SyncError::Journal)?;
        let journal: Journal = serde_json::from_slice(&payload).map_err(|_| SyncError::Journal)?;
        if journal.version != 2 {
            return Err(SyncError::Journal);
        }
        for id in journal.items.keys() {
            validate_id(id)?;
        }
        Ok(journal)
    }

    fn save_journal(&self, journal: &Journal) -> Result<(), SyncError> {
        let path = self.journal_path();
        let parent = path.parent().ok_or(SyncError::Journal)?.to_owned();
        fs::create_dir_all(&parent).map_err(|_| SyncError::Journal)?;
        if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(SyncError::Journal);
        }
        let mut temporary = NamedTempFile::new_in(&parent).map_err(|_| SyncError::Journal)?;
        set_owner_only(temporary.as_file()).map_err(|_| SyncError::Journal)?;
        serde_json::to_writer(temporary.as_file_mut(), journal).map_err(|_| SyncError::Journal)?;
        temporary
            .as_file_mut()
            .write_all(b"\n")
            .map_err(|_| SyncError::Journal)?;
        temporary
            .as_file_mut()
            .flush()
            .map_err(|_| SyncError::Journal)?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|_| SyncError::Journal)?;
        temporary.persist(path).map_err(|_| SyncError::Journal)?;
        File::open(&parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| SyncError::Journal)?;
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActivitySummary {
    activity_id: Value,
    #[serde(default, rename = "startTimeGMT")]
    start_time_gmt: Option<String>,
    #[serde(default, rename = "startTimeLocal")]
    start_time_local: Option<String>,
}

impl ActivitySummary {
    fn id(&self) -> Result<String, SyncError> {
        let id = match &self.activity_id {
            Value::Number(number) => number.as_u64().map(|value| value.to_string()),
            Value::String(value) => Some(value.clone()),
            _ => None,
        }
        .ok_or(SyncError::InvalidActivityId)?;
        validate_id(&id)?;
        Ok(id)
    }

    fn start_time(&self) -> Option<&str> {
        self.start_time_gmt
            .as_deref()
            .or(self.start_time_local.as_deref())
            .filter(|value| !value.is_empty() && value.len() <= 64)
    }
}

fn validate_id(id: &str) -> Result<(), SyncError> {
    if id.is_empty() || id.len() > 32 || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(SyncError::InvalidActivityId);
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Journal {
    version: u8,
    #[serde(default)]
    items: BTreeMap<String, JournalItem>,
    #[serde(default)]
    checkpoint: Option<String>,
}

impl Default for Journal {
    fn default() -> Self {
        Self {
            version: 2,
            items: BTreeMap::new(),
            checkpoint: None,
        }
    }
}

impl Journal {
    fn item_is_complete(&self, id: &str, target: &Path) -> bool {
        let Some(item) = self.items.get(id) else {
            return false;
        };
        if item.state == ItemState::Imported {
            return true;
        }
        let Ok(payload) = fs::read(target) else {
            return false;
        };
        valid_fit(&payload) && hex::encode(Sha256::digest(&payload)) == item.sha256
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct JournalItem {
    state: ItemState,
    #[serde(default)]
    start_time: Option<String>,
    sha256: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum ItemState {
    Downloaded,
    Imported,
}

async fn bounded_body(mut response: reqwest::Response) -> Result<Vec<u8>, SyncError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_PROVIDER_BODY as u64)
    {
        return Err(SyncError::InvalidDownload);
    }
    let mut payload = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| SyncError::InvalidDownload)?
    {
        if payload.len().saturating_add(chunk.len()) > MAX_PROVIDER_BODY {
            return Err(SyncError::InvalidDownload);
        }
        payload.extend_from_slice(&chunk);
    }
    Ok(payload)
}

fn original_fit(payload: &[u8]) -> Result<Vec<u8>, SyncError> {
    if valid_fit(payload) {
        return Ok(payload.to_vec());
    }
    let mut archive =
        zip::ZipArchive::new(Cursor::new(payload)).map_err(|_| SyncError::InvalidDownload)?;
    let candidates: Vec<_> = (0..archive.len())
        .filter(|index| {
            archive.by_index(*index).is_ok_and(|file| {
                !file.is_dir() && file.name().to_ascii_lowercase().ends_with(".fit")
            })
        })
        .collect();
    if candidates.len() != 1 {
        return Err(SyncError::InvalidDownload);
    }
    let file = archive
        .by_index(candidates[0])
        .map_err(|_| SyncError::InvalidDownload)?;
    if file.size() > MAX_PROVIDER_BODY as u64 {
        return Err(SyncError::InvalidDownload);
    }
    let mut fit = Vec::with_capacity(file.size() as usize);
    file.take(MAX_PROVIDER_BODY as u64 + 1)
        .read_to_end(&mut fit)
        .map_err(|_| SyncError::InvalidDownload)?;
    if fit.len() > MAX_PROVIDER_BODY || !valid_fit(&fit) {
        return Err(SyncError::InvalidDownload);
    }
    Ok(fit)
}

fn valid_fit(payload: &[u8]) -> bool {
    payload.len() >= 12 && &payload[8..12] == b".FIT"
}

fn write_atomic(path: &Path, payload: &[u8]) -> Result<(), std::io::Error> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("missing staging parent"))?;
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    set_owner_only(temporary.as_file())?;
    temporary.as_file_mut().write_all(payload)?;
    temporary.as_file_mut().flush()?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(unix)]
fn set_owner_only(file: &File) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_owner_only(_file: &File) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::body::Body;
    use axum::extract::{Path as AxumPath, Query, State};
    use axum::http::{HeaderMap, Response, StatusCode};
    use axum::routing::get;
    use axum::{Json, Router};
    use reqwest::Url;

    use super::*;

    #[derive(Clone, Default)]
    struct FakeState {
        downloads: Arc<Mutex<Vec<String>>>,
    }

    async fn activities(Query(query): Query<BTreeMap<String, String>>) -> Json<Value> {
        assert_eq!(query.get("limit").map(String::as_str), Some("100"));
        Json(serde_json::json!([
            {"activityId": 101, "startTimeGMT": "2026-08-17 10:00:00"},
            {"activityId": 100, "startTimeGMT": "2026-08-16 10:00:00"}
        ]))
    }

    async fn download(
        State(state): State<FakeState>,
        AxumPath(id): AxumPath<String>,
        headers: HeaderMap,
    ) -> Response<Body> {
        assert!(headers.get("authorization").is_some());
        state.downloads.lock().unwrap().push(id);
        Response::builder()
            .status(StatusCode::OK)
            .body(Body::from(fit_zip()))
            .unwrap()
    }

    fn fit_bytes() -> Vec<u8> {
        let mut fit = vec![0_u8; 24];
        fit[8..12].copy_from_slice(b".FIT");
        fit
    }

    fn fit_zip() -> Vec<u8> {
        let mut output = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut output);
            writer
                .start_file("activity.fit", zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(&fit_bytes()).unwrap();
            writer.finish().unwrap();
        }
        output.into_inner()
    }

    #[tokio::test]
    async fn fake_provider_downloads_writes_ahead_and_resumes_until_ack() {
        let state = FakeState::default();
        let app = Router::new()
            .route(
                "/activitylist-service/activities/search/activities",
                get(activities),
            )
            .route("/download-service/files/activity/{id}", get(download))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client =
            GarminClient::for_test(Url::parse(&format!("http://{address}/")).unwrap()).unwrap();
        let mut tokens = GarminTokens {
            di_token: "opaque-test-access-token".to_owned(),
            di_refresh_token: None,
            di_client_id: None,
        };
        let directory = tempfile::tempdir().unwrap();
        let sync = GarminSync::new(directory.path());

        let first = sync
            .synchronize(&client, &mut tokens, Some("2026-08-16 12:00:00"))
            .await
            .unwrap();
        assert_eq!(first.downloaded, 1);
        assert_eq!(first.items[0].id, "101");
        assert_eq!(state.downloads.lock().unwrap().as_slice(), &["101"]);

        let resumed = sync
            .synchronize(&client, &mut tokens, Some("2026-08-16 12:00:00"))
            .await
            .unwrap();
        assert_eq!(resumed.downloaded, 0);
        assert_eq!(resumed.items.len(), 1);
        assert_eq!(state.downloads.lock().unwrap().as_slice(), &["101"]);

        assert_eq!(
            sync.acknowledge(&["101".to_owned()]).unwrap().as_deref(),
            Some("2026-08-17 10:00:00")
        );
        assert!(
            sync.synchronize(&client, &mut tokens, None)
                .await
                .unwrap()
                .items
                .is_empty()
        );
    }

    #[test]
    fn original_requires_exactly_one_valid_fit() {
        assert_eq!(original_fit(&fit_bytes()).unwrap(), fit_bytes());
        assert!(original_fit(b"not a fit or zip").is_err());

        let mut output = Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut output);
            for name in ["one.fit", "two.fit"] {
                writer
                    .start_file(name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(&fit_bytes()).unwrap();
            }
            writer.finish().unwrap();
        }
        assert!(original_fit(&output.into_inner()).is_err());
    }

    #[test]
    fn literal_python_journal_schema_is_compatible() {
        let directory = tempfile::tempdir().unwrap();
        let sync = GarminSync::new(directory.path());
        fs::create_dir_all(directory.path()).unwrap();
        fs::write(
            sync.journal_path(),
            r#"{"version":2,"items":{"123":{"state":"downloaded","start_time":"2026-08-17 10:00:00","sha256":"abc"}},"checkpoint":null}"#,
        )
        .unwrap();
        let journal = sync.load_journal().unwrap();
        assert_eq!(journal.items["123"].state, ItemState::Downloaded);
    }
}
