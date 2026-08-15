import { get, writable } from 'svelte/store';
import type { ApiClient } from '../api/client';
import type { TimelineItem } from '../api/types';

export interface TimelineState {
  status: 'idle' | 'loading' | 'ready' | 'error';
  items: TimelineItem[];
  nextCursor: string | null;
  hasMore: boolean;
  error: Error | null;
}

const initial: TimelineState = {
  status: 'idle',
  items: [],
  nextCursor: null,
  hasMore: true,
  error: null,
};

export function createTimelineStore(api: ApiClient) {
  const store = writable<TimelineState>(initial);

  async function load(reset: boolean) {
    const current = get(store);
    if (current.status === 'loading' || (!reset && !current.hasMore)) return;
    const cursor = reset ? undefined : current.nextCursor ?? undefined;
    store.set({ ...(reset ? initial : current), status: 'loading', error: null });
    try {
      const page = await api.timeline.page(cursor);
      store.update((state) => {
        const prior = reset ? [] : state.items;
        const seen = new Set(prior.map((item) => item.id));
        return {
          status: 'ready',
          items: [...prior, ...page.items.filter((item) => !seen.has(item.id))],
          nextCursor: page.next_cursor,
          hasMore: page.has_more,
          error: null,
        };
      });
    } catch (error) {
      store.update((state) => ({
        ...state,
        status: 'error',
        error: error instanceof Error ? error : new Error(String(error)),
      }));
    }
  }

  return {
    subscribe: store.subscribe,
    reload: () => load(true),
    loadMore: () => load(false),
  };
}
