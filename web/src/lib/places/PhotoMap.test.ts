import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { ApiClient } from '../api/client';
import type { GeoClusterPage } from '../api/types';
import PhotoMap from './PhotoMap.svelte';

const cluster = {
  x_bin: 1,
  y_bin: 1,
  gps_lat: 22.3,
  gps_lon: 114.2,
  photo_count: 2,
  representative_photo_id: 7,
  west: 114,
  east: 114.5,
  south: 22,
  north: 22.5,
};

describe('PhotoMap', () => {
  it('zooms by viewport, opens every cluster photo, and supports fullscreen', async () => {
    type Bounds = { west: number; east: number; south: number; north: number };
    const clusters = vi.fn(async (_columns: number, _rows: number, _bounds?: Bounds) => (
      { clusters: [cluster], total_photos: 2 }
    ));
    const clusterPhotos = vi.fn(async (_bounds: Bounds) => ({
      photos: [
        { id: 7, path: '/7.jpg', taken_at: null, camera: null },
        { id: 8, path: '/8.jpg', taken_at: null, camera: null },
      ],
      total: 2,
      page: 1,
      per_page: 200,
    }));
    const api = { geo: { clusters, clusterPhotos } } as unknown as ApiClient;
    const initialPage: GeoClusterPage = { clusters: [cluster], total_photos: 2 };
    const { container } = render(PhotoMap, { api, initialPage });

    await fireEvent.click(screen.getByRole('button', { name: '放大地图' }));
    await waitFor(() => expect(clusters).toHaveBeenCalled());
    const bounds = clusters.mock.calls.at(-1)?.[2];
    expect(bounds && bounds.east - bounds.west).toBeLessThan(200);
    expect(bounds?.west).toBeLessThan(cluster.gps_lon);
    expect(bounds?.east).toBeGreaterThan(cluster.gps_lon);
    expect(bounds?.south).toBeLessThan(cluster.gps_lat);
    expect(bounds?.north).toBeGreaterThan(cluster.gps_lat);

    await fireEvent.click(screen.getByRole('button', { name: '查看此区域的 2 张照片' }));
    expect(await screen.findByAltText('地图照片 7')).toBeVisible();
    expect(screen.getByAltText('地图照片 8')).toBeVisible();
    expect(clusterPhotos).toHaveBeenCalledWith({ west: 114, east: 114.5, south: 22, north: 22.5 });

    await fireEvent.click(screen.getByRole('button', { name: '全屏显示地图' }));
    expect(container.querySelector('.map-shell')).toHaveClass('fullscreen');
    expect(screen.getByRole('button', { name: '退出全屏地图' })).toBeVisible();
  });
});
