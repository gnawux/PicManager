import { get } from 'svelte/store';
import { describe, expect, it, vi } from 'vitest';
import type { ApiClient } from '../api/client';
import { createLibraryStore } from './library';

describe('library state', () => {
  it('moves from loading to ready with typed page data', async () => {
    const list = vi.fn(async () => ({ photos: [], total: 12, page: 1, per_page: 100 }));
    const store = createLibraryStore({ photos: { list } } as unknown as ApiClient);
    const pending = store.load();
    expect(get(store).status).toBe('loading');
    await pending;
    expect(get(store)).toMatchObject({ status: 'ready', data: { total: 12 } });
  });

  it('retains a normalized error state', async () => {
    const store = createLibraryStore({
      photos: { list: vi.fn(async () => Promise.reject(new Error('offline'))) },
    } as unknown as ApiClient);
    await store.load();
    expect(get(store)).toMatchObject({ status: 'error', error: { message: 'offline' } });
  });
});
