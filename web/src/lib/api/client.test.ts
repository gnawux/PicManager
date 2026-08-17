import { describe, expect, it, vi } from 'vitest';
import { ApiError, createApiClient } from './client';

describe('API client', () => {
  it('builds typed photo queries and decodes successful responses', async () => {
    const fetcher = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
      new Response(JSON.stringify({ photos: [], total: 0, page: 2, per_page: 40 }), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
    );
    const client = createApiClient({ fetch: fetcher as typeof fetch, baseUrl: 'http://test' });
    const page = await client.photos.list(2, 40, 'asc');
    expect(page.total).toBe(0);
    expect(fetcher).toHaveBeenCalledWith(
      'http://test/api/photos?page=2&per_page=40&order=asc',
      expect.objectContaining({ headers: expect.objectContaining({ accept: 'application/json' }) }),
    );
  });

  it('loads a photo detail by stable catalog id', async () => {
    const fetcher = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
      new Response(JSON.stringify({ id: 42 }), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
    );
    const client = createApiClient({ fetch: fetcher as typeof fetch });
    expect((await client.photos.get(42)).id).toBe(42);
    expect(fetcher.mock.calls[0][0]).toBe('/api/photos/42');
  });

  it('normalizes structured backend errors', async () => {
    const fetcher = vi.fn(async () =>
      new Response(
        JSON.stringify({ error: { code: 'invalid_task_transition', message: 'cannot retry' } }),
        { status: 409, headers: { 'content-type': 'application/json' } },
      ),
    );
    const client = createApiClient({ fetch: fetcher as typeof fetch });
    const expected: Partial<ApiError> = {
      status: 409,
      code: 'invalid_task_transition',
      message: 'cannot retry',
    };
    await expect(client.tasks.list()).rejects.toMatchObject(expected);
  });

  it('passes opaque timeline cursors without inspecting them', async () => {
    const fetcher = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
      new Response(JSON.stringify({ items: [], next_cursor: null, has_more: false }), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
    );
    const client = createApiClient({ fetch: fetcher as typeof fetch });
    await client.timeline.page('opaque_cursor', 80, 'desc');
    expect(fetcher.mock.calls[0][0]).toBe(
      '/api/timeline?limit=80&order=desc&cursor=opaque_cursor',
    );
  });

  it('posts batch photo updates with stable field names', async () => {
    const fetcher = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
      new Response(JSON.stringify({ updated: 2 }), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
    );
    const client = createApiClient({ fetch: fetcher as typeof fetch });
    await client.photos.batchUpdate([7, 9], { rotation_delta: 90, flip_h_toggle: true });
    expect(fetcher).toHaveBeenCalledWith(
      '/api/photos/batch-update',
      expect.objectContaining({
        method: 'POST',
        headers: expect.objectContaining({ 'content-type': 'application/json' }),
        body: JSON.stringify({
          photo_ids: [7, 9],
          rotation_delta: 90,
          flip_h_toggle: true,
        }),
      }),
    );
  });

  it('creates curated collections with a JSON body', async () => {
    const fetcher = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
      new Response(JSON.stringify({ id: 3, name: '旅行' }), {
        status: 201,
        headers: { 'content-type': 'application/json' },
      }),
    );
    const client = createApiClient({ fetch: fetcher as typeof fetch });
    await client.collections.create('旅行');
    expect(fetcher).toHaveBeenCalledWith('/api/collections', expect.objectContaining({
      method: 'POST',
      body: JSON.stringify({ name: '旅行' }),
    }));
  });

  it('accepts empty successful responses when updating people', async () => {
    const fetcher = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
      new Response(null, { status: 200 }),
    );
    const client = createApiClient({ fetch: fetcher as typeof fetch });
    await expect(client.people.update(7, { name: '小明' })).resolves.toBeUndefined();
    expect(fetcher).toHaveBeenCalledWith('/api/people/7', expect.objectContaining({
      method: 'PATCH',
      body: JSON.stringify({ name: '小明' }),
    }));
  });

  it('encodes geographic filters without manual string concatenation', async () => {
    const fetcher = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
      new Response(JSON.stringify({ photos: [], total: 0, page: 1, per_page: 200 }), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
    );
    const client = createApiClient({ fetch: fetcher as typeof fetch });
    await client.geo.photos({ country: '中国', state: '上海市', city: '上海' }, 2, 75);
    expect(fetcher.mock.calls[0][0]).toBe(
      '/api/geo/photos?page=2&per_page=75&country=%E4%B8%AD%E5%9B%BD&state=%E4%B8%8A%E6%B5%B7%E5%B8%82&city=%E4%B8%8A%E6%B5%B7',
    );
  });

  it('requests bounded activity photo pages', async () => {
    const fetcher = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) => new Response(JSON.stringify({
      photos: [], total: 0, page: 3, per_page: 40,
    }), { status: 200, headers: { 'content-type': 'application/json' } }));
    const client = createApiClient({ fetch: fetcher as typeof fetch });
    await client.activities.photos(9, 3, 40);
    expect(fetcher.mock.calls[0][0]).toBe('/api/activities/9/photos?page=3&per_page=40');
  });

  it('requests a bounded geographic cluster grid', async () => {
    const fetcher = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
      new Response(JSON.stringify({ clusters: [], total_photos: 0 }), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
    );
    const client = createApiClient({ fetch: fetcher as typeof fetch });
    await client.geo.clusters();
    expect(fetcher.mock.calls[0][0]).toBe('/api/geo/clusters?columns=48&rows=24');
  });

  it('encodes map viewport bounds for clusters and cluster photos', async () => {
    const fetcher = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
      new Response(JSON.stringify({ clusters: [], total_photos: 0, photos: [], total: 0 }), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
    );
    const client = createApiClient({ fetch: fetcher as typeof fetch });
    const bounds = { west: 100, east: 110, south: 20, north: 30 };
    await client.geo.clusters(32, 16, bounds);
    await client.geo.clusterPhotos(bounds);
    expect(fetcher.mock.calls[0][0]).toBe(
      '/api/geo/clusters?columns=32&rows=16&west=100&east=110&south=20&north=30',
    );
    expect(fetcher.mock.calls[1][0]).toBe(
      '/api/geo/cluster-photos?page=1&per_page=20&west=100&east=110&south=20&north=30',
    );
  });

  it('starts explicit geographic name normalization', async () => {
    const fetcher = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
      new Response(JSON.stringify({ status: 'started', count: 42 }), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
    );
    const client = createApiClient({ fetch: fetcher as typeof fetch });
    await client.geo.normalizeNames();
    expect(fetcher).toHaveBeenCalledWith('/api/geo/normalize-names', expect.objectContaining({
      method: 'POST',
    }));
  });

  it('queries Apple inventory by status and original filename', async () => {
    const fetcher = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
      new Response(JSON.stringify({ sources: [], status_counts: {}, next_before_id: null }), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
    );
    const client = createApiClient({ fetch: fetcher as typeof fetch });
    await client.apple.sources('failed', 'IMG 42');
    expect(fetcher.mock.calls[0][0]).toBe('/api/apple/sources?limit=100&status=failed&search=IMG+42');
  });

  it('queries recently synchronized Apple photos with an explicit time window', async () => {
    const fetcher = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
      new Response(JSON.stringify([]), {
        status: 200,
        headers: { 'content-type': 'application/json' },
      }),
    );
    const client = createApiClient({ fetch: fetcher as typeof fetch });
    await client.apple.recentlySynchronized(24, 80);
    expect(fetcher.mock.calls[0][0]).toBe('/api/apple/recently-synchronized?hours=24&limit=80');
  });

  it('posts explicit keep ids when resolving duplicate groups', async () => {
    const fetcher = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) =>
      new Response(null, { status: 200 }),
    );
    const client = createApiClient({ fetch: fetcher as typeof fetch });
    await client.dedup.resolve(4, [10, 12]);
    expect(fetcher).toHaveBeenCalledWith('/api/dedup/4/resolve', expect.objectContaining({
      method: 'POST',
      body: JSON.stringify({ keep: [10, 12] }),
    }));
  });
});
