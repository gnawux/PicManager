import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { ApiClient } from '../api/client';
import PlacesView from './PlacesView.svelte';

describe('PlacesView', () => {
  it('shows GPS points and browses photos by city', async () => {
    const photos = vi.fn(async () => ({
      photos: [{ id: 12, path: '/x.jpg', taken_at: null, camera: null }],
      total: 1, page: 1, per_page: 200,
    }));
    const api = {
      geo: {
        hierarchy: vi.fn(async () => ({ countries: [{
          name: '中国', photo_count: 1, states: [{
            name: '上海市', photo_count: 1, cities: [{ name: '上海', photo_count: 1 }],
          }],
        }] })),
        points: vi.fn(async () => [{ id: 12, taken_at: null, gps_lat: 31.2, gps_lon: 121.5 }]),
        photos,
        regeocode: vi.fn(async () => ({ status: 'started', count: 0 })),
      },
    } as unknown as ApiClient;
    render(PlacesView, { api });

    const point = await screen.findByRole('button', { name: '查看地图照片 12' });
    await fireEvent.click(point);
    expect(screen.getByAltText('地图照片 12')).toHaveAttribute('src', '/api/photos/12/thumb?size=512');
    await fireEvent.click(screen.getByRole('button', { name: /上海 1/ }));
    expect(await screen.findByAltText('照片 12')).toBeVisible();
    expect(photos).toHaveBeenCalledWith({ country: '中国', state: '上海市', city: '上海' });
  });
});
