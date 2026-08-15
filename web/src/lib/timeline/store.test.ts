import { get } from 'svelte/store';
import { describe, expect, it, vi } from 'vitest';
import type { ApiClient } from '../api/client';
import type { TimelineItem } from '../api/types';
import { createTimelineStore } from './store';

const item = (id: number) => ({ id }) as TimelineItem;

describe('timeline store', () => {
  it('appends cursor pages and removes defensive duplicates', async () => {
    const page = vi
      .fn()
      .mockResolvedValueOnce({ items: [item(1), item(2)], next_cursor: 'next', has_more: true })
      .mockResolvedValueOnce({ items: [item(2), item(3)], next_cursor: null, has_more: false });
    const store = createTimelineStore({ timeline: { page } } as unknown as ApiClient);
    await store.reload();
    await store.loadMore();
    expect(get(store).items.map(({ id }) => id)).toEqual([1, 2, 3]);
    expect(page).toHaveBeenNthCalledWith(2, 'next');
  });

  it('retains loaded photos when a later page fails', async () => {
    const page = vi
      .fn()
      .mockResolvedValueOnce({ items: [item(1)], next_cursor: 'next', has_more: true })
      .mockRejectedValueOnce(new Error('offline'));
    const store = createTimelineStore({ timeline: { page } } as unknown as ApiClient);
    await store.reload();
    await store.loadMore();
    expect(get(store)).toMatchObject({ status: 'error', items: [{ id: 1 }], error: { message: 'offline' } });
  });
});
