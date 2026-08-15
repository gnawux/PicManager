import { writable } from 'svelte/store';

export type RouteId = 'photos' | 'albums' | 'people' | 'places' | 'apple' | 'tasks';

export interface AppRoute {
  id: RouteId;
  path: string;
  label: string;
}

export const routes: readonly AppRoute[] = [
  { id: 'photos', path: '/photos', label: '照片' },
  { id: 'albums', path: '/albums', label: '相册' },
  { id: 'people', path: '/people', label: '人物' },
  { id: 'places', path: '/places', label: '地点' },
  { id: 'apple', path: '/apple', label: 'Apple 照片' },
  { id: 'tasks', path: '/tasks', label: '任务' },
] as const;

export function routeFromHash(hash: string): AppRoute {
  const path = hash.replace(/^#/, '') || '/photos';
  return routes.find((route) => route.path === path) ?? routes[0];
}

export function createHashRouter(target: Window = window) {
  const store = writable(routeFromHash(target.location.hash));
  const sync = () => store.set(routeFromHash(target.location.hash));
  return {
    subscribe: store.subscribe,
    start() {
      target.addEventListener('hashchange', sync);
      sync();
      return () => target.removeEventListener('hashchange', sync);
    },
    navigate(route: AppRoute) {
      target.location.hash = route.path;
      store.set(route);
    },
  };
}
