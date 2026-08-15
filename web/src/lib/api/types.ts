export interface PhotoSummary {
  id: number;
  path: string;
  format: string;
  taken_at: string | null;
  camera: string | null;
  import_status: string;
}

export interface PhotoPage {
  photos: PhotoSummary[];
  total: number;
  page: number;
  per_page: number;
}

export interface PhotoDetail {
  id: number;
  path: string;
  format: string;
  taken_at: string | null;
  timezone_offset: number | null;
  camera: string | null;
  gps_lat: number | null;
  gps_lon: number | null;
  import_status: string;
  width: number | null;
  height: number | null;
  sources: Array<{
    provider: string;
    original_filename: string | null;
    sync_status: string;
  }>;
  renditions: {
    display: string;
    original: string | null;
    current: string | null;
  };
}

export interface BatchPhotoUpdate {
  taken_at?: string;
  timezone_offset?: number;
  rotation_delta?: number;
  flip_h_toggle?: boolean;
  flip_v_toggle?: boolean;
}

export interface TimelineItem {
  id: number;
  taken_at: string | null;
  camera: string | null;
  format: string;
  display_revision: number;
  has_original: boolean;
  has_current: boolean;
  preview: {
    src: string;
    srcset: string;
    width: number | null;
    height: number | null;
    aspect_ratio: number | null;
  };
  file_url: string;
}

export interface TimelinePage {
  items: TimelineItem[];
  next_cursor: string | null;
  has_more: boolean;
}

export interface TaskSummary {
  id: number;
  kind: string;
  provider: string;
  status: string;
  total_items: number;
  completed_items: number;
  failed_items: number;
  created_at: string;
}

export interface TaskPage {
  tasks: TaskSummary[];
  next_cursor: number | null;
}

export interface AppleSourceSummary {
  id: number;
  asset_id: number | null;
  photo_id: number | null;
  external_id: string;
  original_filename: string | null;
  media_type: string | null;
  width: number | null;
  height: number | null;
  taken_at: string | null;
  sync_status: string;
  exclusion_reason: string | null;
  last_error: string | null;
  last_seen_at: string | null;
  updated_at: string;
}

export interface AppleSourcePage {
  sources: AppleSourceSummary[];
  status_counts: Record<string, number>;
  next_before_id: number | null;
}

export interface AppleLinkCandidate {
  id: number;
  source_id: number;
  photo_id: number;
  method: string;
  confidence: number;
  status: string;
  evidence_json: string | null;
  original_filename: string | null;
  source_taken_at: string | null;
  source_width: number | null;
  source_height: number | null;
  photo_path: string;
  photo_taken_at: string | null;
  photo_width: number | null;
  photo_height: number | null;
}

export interface AlbumSummary {
  id: number;
  name: string;
  kind: string;
  photo_count: number;
  latest_photo_at: string | null;
}

export interface CollectionSummary {
  id: number;
  name: string;
  photo_count: number;
  created_at: string;
}

export interface AlbumPhotoPage {
  photos: Array<{
    id: number;
    path: string;
    taken_at: string | null;
    camera: string | null;
  }>;
  total: number;
  page: number;
  per_page: number;
}

export interface PersonSummary {
  id: number;
  name: string | null;
  parent_id: number | null;
  cover_face_id: number | null;
  face_count: number;
  photo_count: number;
  status: string;
}

export interface GeoHierarchy {
  countries: Array<{
    name: string;
    photo_count: number;
    states: Array<{
      name: string;
      photo_count: number;
      cities: Array<{ name: string; photo_count: number }>;
    }>;
  }>;
}

export interface GeoPoint {
  id: number;
  taken_at: string | null;
  gps_lat: number;
  gps_lon: number;
}

export interface ActivitySummary {
  id: number;
  title: string | null;
  activity_type: string;
  start_time: string | null;
  end_time: string | null;
  duration_seconds: number | null;
  distance_meters: number | null;
  elevation_gain_meters: number | null;
  avg_heart_rate: number | null;
  max_heart_rate: number | null;
  calories: number | null;
  device: string | null;
  file_format: string;
  sensors: unknown;
}

export interface ActivityPage {
  activities: ActivitySummary[];
  total: number;
  page: number;
  per_page: number;
}

export interface ActivityTrack {
  points: Array<{
    ts: string;
    lat: number;
    lon: number;
    elevation: number | null;
    heart_rate: number | null;
    cadence: number | null;
    speed: number | null;
  }>;
  original_count: number;
  downsampled: boolean;
}

export interface ActivityPhotos {
  photos: Array<{
    id: number;
    path: string;
    format: string;
    taken_at: string | null;
    gps_lat: number | null;
    gps_lon: number | null;
  }>;
}

export interface ApiErrorEnvelope {
  error?: {
    code?: string;
    message?: string;
    details?: unknown;
  };
}
