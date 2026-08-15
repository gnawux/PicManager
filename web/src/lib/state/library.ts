import { writable } from 'svelte/store';
import type { ApiClient } from '../api/client';
import type { PhotoPage } from '../api/types';

export type LibraryState =
  | { status: 'idle'; data: null; error: null }
  | { status: 'loading'; data: PhotoPage | null; error: null }
  | { status: 'ready'; data: PhotoPage; error: null }
  | { status: 'error'; data: PhotoPage | null; error: Error };

export function createLibraryStore(api: ApiClient) {
  const store = writable<LibraryState>({ status: 'idle', data: null, error: null });
  return {
    subscribe: store.subscribe,
    async load() {
      store.update((state) => ({ status: 'loading', data: state.data, error: null }));
      try {
        const data = await api.photos.list();
        store.set({ status: 'ready', data, error: null });
      } catch (error) {
        store.update((state) => ({
          status: 'error',
          data: state.data,
          error: error instanceof Error ? error : new Error(String(error)),
        }));
      }
    },
  };
}
