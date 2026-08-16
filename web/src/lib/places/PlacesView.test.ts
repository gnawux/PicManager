import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { ApiClient } from '../api/client';
import PlacesView from './PlacesView.svelte';

describe('PlacesView', () => {
  it('shows GPS clusters and browses photos by city', async () => {
    const photos = vi.fn(async () => ({
      photos: [{ id: 12, path: '/x.jpg', taken_at: null, camera: null }],
      total: 1, page: 1, per_page: 200,
    }));
    const api = {
      geo: {
        hierarchy: vi.fn(async () => ({ countries: [{
          name: '中国', query_value: '中国', photo_count: 1, states: [{
            name: '上海市', query_value: '上海市', photo_count: 1,
            cities: [{ name: '上海', query_value: '上海', photo_count: 1 }],
          }],
        }] })),
        clusters: vi.fn(async () => ({
          clusters: [{
            x_bin: 40, y_bin: 7, gps_lat: 31.2, gps_lon: 121.5,
            photo_count: 1, representative_photo_id: 12,
          }],
          total_photos: 1,
        })),
        photos,
        regeocode: vi.fn(async () => ({ status: 'started', count: 0 })),
        namePolicy: vi.fn(async () => ({ revision: 1, language_preference: 'zh-CN,zh,en', outdated_photos: 0 })),
        normalizeNames: vi.fn(),
      },
    } as unknown as ApiClient;
    render(PlacesView, { api });

    const cluster = await screen.findByRole('button', { name: '查看此区域的 1 张照片' });
    await fireEvent.click(cluster);
    expect(screen.getByAltText('地图照片 12')).toHaveAttribute('src', '/api/photos/12/thumb?size=512');
    await fireEvent.click(screen.getByRole('button', { name: /上海 1/ }));
    expect(await screen.findByAltText('照片 12')).toBeVisible();
    expect(photos).toHaveBeenCalledWith({ country: '中国', state: '上海市', city: '上海' });
  });

  it('uses null sentinels when browsing unknown hierarchy entries', async () => {
    const photos = vi.fn(async () => ({
      photos: [{ id: 21, path: '/unknown.jpg', taken_at: null, camera: null }],
      total: 1, page: 1, per_page: 200,
    }));
    const api = {
      geo: {
        hierarchy: vi.fn(async () => ({ countries: [{
          name: 'Unknown', query_value: '__null__', photo_count: 1, states: [{
            name: 'Unknown', query_value: '__null__', photo_count: 1,
            cities: [{ name: 'Unknown', query_value: '__null__', photo_count: 1 }],
          }],
        }] })),
        clusters: vi.fn(async () => ({ clusters: [], total_photos: 0 })),
        photos,
        regeocode: vi.fn(),
        namePolicy: vi.fn(async () => ({ revision: 1, language_preference: 'zh-CN,zh,en', outdated_photos: 0 })),
        normalizeNames: vi.fn(),
      },
    } as unknown as ApiClient;
    const { container } = render(PlacesView, { api });

    await screen.findAllByRole('button', { name: /Unknown 1/ });
    await fireEvent.click(container.querySelector('.cities button') as HTMLButtonElement);
    expect(await screen.findByAltText('照片 21')).toBeVisible();
    expect(photos).toHaveBeenCalledWith({
      country: '__null__', state: '__null__', city: '__null__',
    });
  });

  it('offers a safe background repair for legacy place names', async () => {
    const normalizeNames = vi.fn(async () => ({ status: 'started', count: 42 }));
    const api = {
      geo: {
        hierarchy: vi.fn(async () => ({ countries: [] })),
        clusters: vi.fn(async () => ({ clusters: [], total_photos: 0 })),
        photos: vi.fn(),
        regeocode: vi.fn(),
        namePolicy: vi.fn(async () => ({
          revision: 1,
          language_preference: 'zh-CN,zh-Hans,zh,en-US,en',
          outdated_photos: 42,
        })),
        normalizeNames,
      },
    } as unknown as ApiClient;
    render(PlacesView, { api });

    await fireEvent.click(await screen.findByRole('button', { name: '统一已有地名（42）' }));
    expect(normalizeNames).toHaveBeenCalledOnce();
    expect(await screen.findByText('已开始统一 42 张照片的地名')).toBeVisible();
    expect(screen.getByText(/网络失败不会覆盖现有名称/)).toBeVisible();
  });

  it('keeps map DOM work bounded for a large photo library', async () => {
    const clusters = Array.from({ length: 1152 }, (_, index) => ({
      x_bin: index % 48,
      y_bin: Math.floor(index / 48),
      gps_lat: 89 - Math.floor(index / 48) * 7.5,
      gps_lon: -176 + (index % 48) * 7.5,
      photo_count: 87,
      representative_photo_id: index + 1,
    }));
    const api = {
      geo: {
        hierarchy: vi.fn(async () => ({ countries: [] })),
        clusters: vi.fn(async () => ({ clusters, total_photos: 100_000 })),
        photos: vi.fn(),
        regeocode: vi.fn(),
        namePolicy: vi.fn(async () => ({ revision: 1, language_preference: 'zh-CN,zh,en', outdated_photos: 0 })),
        normalizeNames: vi.fn(),
      },
    } as unknown as ApiClient;

    const { container } = render(PlacesView, { api });
    expect(await screen.findByText('100000 张照片带有位置信息')).toBeVisible();
    expect(container.querySelectorAll('.map-cluster')).toHaveLength(1152);
  });
});
