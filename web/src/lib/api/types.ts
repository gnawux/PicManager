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
  external_id: string;
  original_filename: string | null;
  media_type: string | null;
  width: number | null;
  height: number | null;
  taken_at: string | null;
  status: string;
  exclusion_reason: string | null;
  last_error: string | null;
}

export interface AppleSourcePage {
  sources: AppleSourceSummary[];
  total: number;
  counts: Record<string, number>;
  next_cursor: number | null;
}

export interface ApiErrorEnvelope {
  error?: {
    code?: string;
    message?: string;
    details?: unknown;
  };
}
