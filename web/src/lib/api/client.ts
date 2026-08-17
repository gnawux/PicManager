import type {
  ApiErrorEnvelope,
  AppleSourcePage,
  AppleSourceSummary,
  AppleRecentPhoto,
  AppleLinkCandidate,
  AlbumPhotoPage,
  AlbumSummary,
  ActivityPage,
  ActivityPhotos,
  ActivitySummary,
  ActivityTrack,
  BatchPhotoUpdate,
  PhotoDetail,
  PersonSummary,
  PhotoPage,
  CollectionSummary,
  GeoClusterPage,
  GeoHierarchy,
  GeoNamePolicy,
  DedupGroup,
  TaskDetail,
  TaskPage,
  TimelinePage,
} from './types';

export class ApiError extends Error {
  constructor(
    public readonly status: number,
    public readonly code: string,
    message: string,
    public readonly details?: unknown,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

export interface ApiClientOptions {
  fetch?: typeof globalThis.fetch;
  baseUrl?: string;
}

export function createApiClient(options: ApiClientOptions = {}) {
  const fetcher = options.fetch ?? globalThis.fetch.bind(globalThis);
  const baseUrl = options.baseUrl ?? '';

  async function request<T>(path: string, init?: RequestInit): Promise<T> {
    const response = await fetcher(`${baseUrl}${path}`, {
      ...init,
      headers: {
        accept: 'application/json',
        ...init?.headers,
      },
    });
    if (!response.ok) {
      let envelope: ApiErrorEnvelope = {};
      try {
        envelope = (await response.json()) as ApiErrorEnvelope;
      } catch {
        // Non-JSON errors still become a stable ApiError below.
      }
      throw new ApiError(
        response.status,
        envelope.error?.code ?? 'http_error',
        envelope.error?.message ?? `Request failed with status ${response.status}`,
        envelope.error?.details,
      );
    }
    if (response.status === 204) {
      return undefined as T;
    }
    const text = await response.text();
    return (text ? JSON.parse(text) : undefined) as T;
  }

  return {
    photos: {
      get: (id: number) => request<PhotoDetail>(`/api/photos/${id}`),
      list: (page = 1, perPage = 100, order: 'asc' | 'desc' = 'desc') =>
        request<PhotoPage>(
          `/api/photos?page=${page}&per_page=${perPage}&order=${order}`,
        ),
      batchUpdate: (photoIds: number[], update: BatchPhotoUpdate) =>
        request<{ updated: number }>('/api/photos/batch-update', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ photo_ids: photoIds, ...update }),
        }),
    },
    timeline: {
      page: (cursor?: string, limit = 120, order: 'asc' | 'desc' = 'desc') => {
        const query = new URLSearchParams({ limit: String(limit), order });
        if (cursor) query.set('cursor', cursor);
        return request<TimelinePage>(`/api/timeline?${query}`);
      },
    },
    tasks: {
      list: () => request<TaskPage>('/api/tasks?limit=50'),
      get: (id: number) => request<TaskDetail>(`/api/tasks/${id}`),
      retry: (id: number) => request<TaskDetail>(`/api/tasks/${id}/retry`, { method: 'POST' }),
      cancel: (id: number) => request<TaskDetail>(`/api/tasks/${id}/cancel`, { method: 'POST' }),
    },
    dedup: {
      list: () => request<DedupGroup[]>('/api/dedup'),
      resolve: (groupId: number, keep: number[]) => request<void>(`/api/dedup/${groupId}/resolve`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ keep }),
      }),
    },
    apple: {
      recentlySynchronized: (hours = 24, limit = 100) =>
        request<AppleRecentPhoto[]>(`/api/apple/recently-synchronized?hours=${hours}&limit=${limit}`),
      sources: (status?: string, search?: string, beforeId?: number) => {
        const query = new URLSearchParams({ limit: '100' });
        if (status && status !== 'all') query.set('status', status);
        if (search) query.set('search', search);
        if (beforeId) query.set('before_id', String(beforeId));
        return request<AppleSourcePage>(`/api/apple/sources?${query}`);
      },
      retry: (sourceId: number) =>
        request<{ job_id: number; source: AppleSourceSummary }>(`/api/apple/sources/${sourceId}/retry`, { method: 'POST' }),
      candidates: () => request<AppleLinkCandidate[]>('/api/apple/link-candidates?limit=100'),
      reviewCandidate: (id: number, accept: boolean) =>
        request<{ job_id: number | null }>(`/api/apple/link-candidates/${id}/review`, {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ accept }),
        }),
    },
    albums: {
      list: () => request<AlbumSummary[]>('/api/albums'),
      photos: (id: number, page = 1, perPage = 100) =>
        request<AlbumPhotoPage>(`/api/albums/${id}/photos?page=${page}&per_page=${perPage}`),
    },
    collections: {
      list: () => request<CollectionSummary[]>('/api/collections'),
      create: (name: string) => request<{ id: number; name: string }>('/api/collections', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ name }),
      }),
      photos: (id: number, page = 1, perPage = 100) =>
        request<AlbumPhotoPage>(`/api/collections/${id}/photos?page=${page}&per_page=${perPage}`),
    },
    people: {
      list: (status = 'active') =>
        request<PersonSummary[]>(`/api/people?status=${encodeURIComponent(status)}`),
      photos: (id: number, page = 1, perPage = 100) =>
        request<PhotoPage>(`/api/people/${id}?page=${page}&per_page=${perPage}`),
      update: (id: number, update: { name?: string; status?: string }) =>
        request<void>(`/api/people/${id}`, {
          method: 'PATCH',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify(update),
        }),
      discover: () => request<{ people_created: number }>('/api/people/cluster/incremental', {
        method: 'POST',
      }),
    },
    geo: {
      hierarchy: () => request<GeoHierarchy>('/api/geo/hierarchy'),
      clusters: (columns = 48, rows = 24, bounds?: { west: number; east: number; south: number; north: number }) => {
        const query = new URLSearchParams({ columns: String(columns), rows: String(rows) });
        if (bounds) for (const [key, value] of Object.entries(bounds)) query.set(key, String(value));
        return request<GeoClusterPage>(`/api/geo/clusters?${query}`);
      },
      clusterPhotos: (bounds: { west: number; east: number; south: number; north: number }, page = 1, perPage = 20) => {
        const query = new URLSearchParams({ page: String(page), per_page: String(perPage) });
        for (const [key, value] of Object.entries(bounds)) query.set(key, String(value));
        return request<AlbumPhotoPage>(`/api/geo/cluster-photos?${query}`);
      },
      photos: (filters: { country?: string; state?: string; city?: string }, page = 1, perPage = 200) => {
        const query = new URLSearchParams({ page: String(page), per_page: String(perPage) });
        for (const [key, value] of Object.entries(filters)) if (value) query.set(key, value);
        return request<AlbumPhotoPage>(`/api/geo/photos?${query}`);
      },
      regeocode: () => request<{ status: string; count?: number }>('/api/geo/regeocode', {
        method: 'POST',
      }),
      namePolicy: () => request<GeoNamePolicy>('/api/geo/name-policy'),
      normalizeNames: () => request<{ status: string; count?: number }>('/api/geo/normalize-names', {
        method: 'POST',
      }),
    },
    activities: {
      list: (type?: string) => {
        const query = new URLSearchParams({ page: '1', per_page: '100' });
        if (type) query.set('type', type);
        return request<ActivityPage>(`/api/activities?${query}`);
      },
      get: (id: number) => request<ActivitySummary>(`/api/activities/${id}`),
      track: (id: number) => request<ActivityTrack>(`/api/activities/${id}/track`),
      photos: (id: number, page = 1, perPage = 100) =>
        request<ActivityPhotos>(`/api/activities/${id}/photos?page=${page}&per_page=${perPage}`),
      garminStatus: () => request<{ configured: boolean; authenticated: boolean; status: string; message?: string | null; downloaded: number; imported: number; skipped: number; failed: number }>('/api/activities/garmin/status'),
      authenticateGarmin: (mfaCode?: string) => request<{ configured: boolean; authenticated: boolean; status: string; message?: string | null; downloaded: number; imported: number; skipped: number; failed: number }>('/api/activities/garmin/auth', {
        method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ mfa_code: mfaCode }),
      }),
      syncGarmin: (mfaCode?: string) => request<{ configured: boolean; authenticated: boolean; status: string; message?: string | null; downloaded: number; imported: number; skipped: number; failed: number }>('/api/activities/garmin/sync', {
        method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ mfa_code: mfaCode }),
      }),
    },
    request,
  };
}

export type ApiClient = ReturnType<typeof createApiClient>;
