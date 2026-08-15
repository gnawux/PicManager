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
});
