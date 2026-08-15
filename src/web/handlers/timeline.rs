use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Sqlite};

use crate::web::AppState;

const DEFAULT_LIMIT: u32 = 120;
const MAX_LIMIT: u32 = 500;
const PREVIEW_WIDTHS: [u32; 3] = [256, 512, 1024];

#[derive(Debug, Deserialize, Default)]
pub struct TimelineQuery {
    pub cursor: Option<String>,
    pub limit: Option<u32>,
    pub order: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct TimelineCursor {
    taken_at: Option<String>,
    id: i64,
    order: String,
}

#[derive(Debug, Serialize)]
pub struct TimelinePreview {
    pub src: String,
    pub srcset: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub aspect_ratio: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct TimelineItem {
    pub id: i64,
    pub taken_at: Option<String>,
    pub camera: Option<String>,
    pub format: String,
    pub display_revision: i64,
    pub has_original: bool,
    pub has_current: bool,
    pub preview: TimelinePreview,
    pub file_url: String,
}

#[derive(Debug, Serialize)]
pub struct TimelinePage {
    pub items: Vec<TimelineItem>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

type TimelineRow = (
    i64,
    Option<String>,
    Option<i64>,
    Option<i64>,
    String,
    Option<String>,
    i64,
    i64,
    i64,
);

pub async fn list_timeline(
    State(state): State<AppState>,
    Query(params): Query<TimelineQuery>,
) -> Result<Json<TimelinePage>, StatusCode> {
    let order = match params.order.as_deref().unwrap_or("desc") {
        "asc" => "asc",
        "desc" => "desc",
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    let cursor = params
        .cursor
        .as_deref()
        .map(decode_cursor)
        .transpose()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    if cursor.as_ref().is_some_and(|cursor| cursor.order != order) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    let mut query = QueryBuilder::<Sqlite>::new(
        "SELECT p.id, p.taken_at, COALESCE(dv.width, p.width), \
                COALESCE(dv.height, p.height), p.format, p.camera, p.render_revision, \
                EXISTS(SELECT 1 FROM asset_variants ov \
                       WHERE ov.asset_id = a.id AND ov.role = 'original'), \
                EXISTS(SELECT 1 FROM asset_variants cv \
                       WHERE cv.asset_id = a.id AND cv.role = 'current') \
         FROM photos p \
         LEFT JOIN assets a ON a.photo_id = p.id \
         LEFT JOIN asset_variants dv ON dv.id = a.display_variant_id \
         WHERE p.import_status = 'imported'",
    );
    if let Some(cursor) = &cursor {
        push_cursor_predicate(&mut query, cursor, order);
    }
    if order == "asc" {
        query.push(" ORDER BY p.taken_at ASC NULLS LAST, p.id ASC");
    } else {
        query.push(" ORDER BY p.taken_at DESC NULLS LAST, p.id DESC");
    }
    query.push(" LIMIT ").push_bind(i64::from(limit) + 1);

    let mut rows: Vec<TimelineRow> = query
        .build_query_as()
        .fetch_all(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let has_more = rows.len() > limit as usize;
    if has_more {
        rows.truncate(limit as usize);
    }
    let next_cursor = if has_more {
        rows.last()
            .map(|row| {
                encode_cursor(&TimelineCursor {
                    taken_at: row.1.clone(),
                    id: row.0,
                    order: order.to_owned(),
                })
            })
            .transpose()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        None
    };
    Ok(Json(TimelinePage {
        items: rows.into_iter().map(row_to_item).collect(),
        next_cursor,
        has_more,
    }))
}

fn push_cursor_predicate(
    query: &mut QueryBuilder<'_, Sqlite>,
    cursor: &TimelineCursor,
    order: &str,
) {
    match (&cursor.taken_at, order) {
        (Some(taken_at), "asc") => {
            query
                .push(" AND ((p.taken_at > ")
                .push_bind(taken_at.clone())
                .push(") OR (p.taken_at = ")
                .push_bind(taken_at.clone())
                .push(" AND p.id > ")
                .push_bind(cursor.id)
                .push(") OR p.taken_at IS NULL)");
        }
        (Some(taken_at), _) => {
            query
                .push(" AND ((p.taken_at < ")
                .push_bind(taken_at.clone())
                .push(") OR (p.taken_at = ")
                .push_bind(taken_at.clone())
                .push(" AND p.id < ")
                .push_bind(cursor.id)
                .push(") OR p.taken_at IS NULL)");
        }
        (None, "asc") => {
            query
                .push(" AND p.taken_at IS NULL AND p.id > ")
                .push_bind(cursor.id);
        }
        (None, _) => {
            query
                .push(" AND p.taken_at IS NULL AND p.id < ")
                .push_bind(cursor.id);
        }
    }
}

fn row_to_item(row: TimelineRow) -> TimelineItem {
    let (id, taken_at, width, height, format, camera, revision, original, current) = row;
    let srcset = PREVIEW_WIDTHS
        .iter()
        .map(|size| format!("/api/photos/{id}/thumb?size={size} {size}w"))
        .collect::<Vec<_>>()
        .join(", ");
    TimelineItem {
        id,
        taken_at,
        camera,
        format,
        display_revision: revision,
        has_original: original != 0,
        has_current: current != 0,
        preview: TimelinePreview {
            src: format!("/api/photos/{id}/thumb?size=512"),
            srcset,
            width,
            height,
            aspect_ratio: width.zip(height).and_then(|(width, height)| {
                (width > 0 && height > 0).then_some(width as f64 / height as f64)
            }),
        },
        file_url: format!("/api/photos/{id}/file"),
    }
}

fn encode_cursor(cursor: &TimelineCursor) -> Result<String, serde_json::Error> {
    serde_json::to_vec(cursor).map(|bytes| URL_SAFE_NO_PAD.encode(bytes))
}

fn decode_cursor(value: &str) -> Result<TimelineCursor, ()> {
    let bytes = URL_SAFE_NO_PAD.decode(value).map_err(|_| ())?;
    let cursor: TimelineCursor = serde_json::from_slice(&bytes).map_err(|_| ())?;
    if cursor.id <= 0 || !matches!(cursor.order.as_str(), "asc" | "desc") {
        return Err(());
    }
    Ok(cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_round_trip_preserves_null_dates_and_order() {
        for cursor in [
            TimelineCursor {
                taken_at: Some("2024-01-01T00:00:00".into()),
                id: 42,
                order: "desc".into(),
            },
            TimelineCursor {
                taken_at: None,
                id: 7,
                order: "asc".into(),
            },
        ] {
            let encoded = encode_cursor(&cursor).unwrap();
            assert_eq!(decode_cursor(&encoded), Ok(cursor));
        }
    }

    #[test]
    fn malformed_cursors_are_rejected() {
        assert!(decode_cursor("not-base64!").is_err());
        let invalid = URL_SAFE_NO_PAD.encode(br#"{"taken_at":null,"id":0,"order":"desc"}"#);
        assert!(decode_cursor(&invalid).is_err());
    }
}
