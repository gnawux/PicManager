import type {
  ApiErrorEnvelope,
  AppleSourcePage,
  BatchPhotoUpdate,
  PhotoDetail,
  PhotoPage,
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
    return (await response.json()) as T;
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
    },
    apple: {
      sources: (status?: string) => {
        const query = status ? `?status=${encodeURIComponent(status)}` : '';
        return request<AppleSourcePage>(`/api/apple/sources${query}`);
      },
      retry: (sourceId: number) =>
        request<unknown>(`/api/apple/sources/${sourceId}/retry`, { method: 'POST' }),
    },
    request,
  };
}

export type ApiClient = ReturnType<typeof createApiClient>;
