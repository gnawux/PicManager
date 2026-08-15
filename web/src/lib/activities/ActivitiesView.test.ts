import { fireEvent, render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { ApiClient } from '../api/client';
import type { ActivitySummary } from '../api/types';
import ActivitiesView from './ActivitiesView.svelte';

const activity: ActivitySummary = {
  id: 5, title: '滨江跑步', activity_type: 'running', start_time: '2024-06-15T10:00:00Z',
  end_time: '2024-06-15T10:30:00Z', duration_seconds: 1800, distance_meters: 5200,
  elevation_gain_meters: 42, avg_heart_rate: 142, max_heart_rate: 168, calories: 320,
  device: 'Garmin', file_format: 'fit', sensors: null,
};

describe('ActivitiesView', () => {
  it('opens an activity route, metrics, and matched photos', async () => {
    const api = {
      activities: {
        list: vi.fn(async () => ({ activities: [activity], total: 1, page: 1, per_page: 100 })),
        get: vi.fn(async () => activity),
        track: vi.fn(async () => ({
          points: [
            { ts: 'a', lat: 31.20, lon: 121.40, elevation: 1, heart_rate: 140, cadence: null, speed: null },
            { ts: 'b', lat: 31.21, lon: 121.42, elevation: 2, heart_rate: 145, cadence: null, speed: null },
          ],
          original_count: 2, downsampled: false,
        })),
        photos: vi.fn(async () => ({ photos: [{ id: 17, path: '/x.jpg', format: 'jpeg', taken_at: null, gps_lat: 31.2, gps_lon: 121.4 }] })),
      },
    } as unknown as ApiClient;
    render(ActivitiesView, { api });

    await fireEvent.click(await screen.findByRole('button', { name: /滨江跑步/ }));
    expect(await screen.findByRole('img', { name: /包含 2 个轨迹点/ })).toBeVisible();
    expect(screen.getByText('5.20 km')).toBeVisible();
    expect(screen.getByText('30 分钟')).toBeVisible();
    expect(screen.getByAltText('活动照片 17')).toHaveAttribute('src', '/api/photos/17/thumb?size=512');
  });
});
