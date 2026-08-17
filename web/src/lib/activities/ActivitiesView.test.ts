import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
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
  it('requests native credentials only after an explicit authenticate action', async () => {
    const postMessage = vi.fn();
    Object.defineProperty(window, 'webkit', {
      configurable: true,
      value: { messageHandlers: { picmanager: { postMessage } } },
    });
    const authenticateGarmin = vi.fn(async () => ({
      configured: true, authenticated: true, status: 'authenticated', error_code: null,
      retryable: false, downloaded: 0, imported: 0, skipped: 0, failed: 0,
    }));
    const syncGarmin = vi.fn(async () => ({
      configured: true, authenticated: true, status: 'completed', error_code: null,
      retryable: false, downloaded: 0, imported: 0, skipped: 0, failed: 0,
    }));
    const api = {
      activities: {
        list: vi.fn(async () => ({ activities: [], total: 0, page: 1, per_page: 100 })),
        garminStatus: vi.fn(async () => ({ configured: false, authenticated: false, status: 'not_configured', error_code: 'not_configured', retryable: false, downloaded: 0, imported: 0, skipped: 0, failed: 0 })),
        authenticateGarmin,
        syncGarmin,
      },
    } as unknown as ApiClient;

    render(ActivitiesView, { api });
    await screen.findByText('Garmin 状态：not_configured');
    expect(postMessage).not.toHaveBeenCalled();

    await fireEvent.click(screen.getByRole('button', { name: '验证登录' }));
    expect(authenticateGarmin).not.toHaveBeenCalled();
    const request = postMessage.mock.calls[0][0] as { action: string; request_id: string; operation: string };
    expect(request).toMatchObject({ action: 'prepareGarmin', operation: 'authenticate' });
    window.dispatchEvent(new CustomEvent('picmanager:garmin-prepared', {
      detail: { request_id: request.request_id, outcome: 'ready' },
    }));
    await waitFor(() => expect(authenticateGarmin).toHaveBeenCalledOnce());

    await fireEvent.click(screen.getByRole('button', { name: '同步 Garmin 数据' }));
    expect(syncGarmin).not.toHaveBeenCalled();
    const syncRequest = postMessage.mock.calls[1][0] as { action: string; request_id: string; operation: string };
    expect(syncRequest).toMatchObject({ action: 'prepareGarmin', operation: 'sync' });
    window.dispatchEvent(new CustomEvent('picmanager:garmin-prepared', {
      detail: { request_id: syncRequest.request_id, outcome: 'ready' },
    }));
    await waitFor(() => expect(syncGarmin).toHaveBeenCalledOnce());
    Object.defineProperty(window, 'webkit', { configurable: true, value: undefined });
  });

  it('consumes native credential outcomes and guides classified Garmin authentication failures', async () => {
    const api = {
      activities: {
        list: vi.fn(async () => ({ activities: [], total: 0, page: 1, per_page: 100 })),
        garminStatus: vi.fn(async () => ({ configured: true, authenticated: false, status: 'ready_to_authenticate', error_code: null, retryable: false, downloaded: 0, imported: 0, skipped: 0, failed: 0 })),
        authenticateGarmin: vi.fn(async () => ({ configured: true, authenticated: false, status: 'network_error', error_code: 'network_error', retryable: true, downloaded: 0, imported: 0, skipped: 0, failed: 0 })),
        syncGarmin: vi.fn(),
      },
    } as unknown as ApiClient;
    render(ActivitiesView, { api });

    window.dispatchEvent(new CustomEvent('picmanager:garmin-credentials', {
      detail: { outcome: 'cancelled' },
    }));
    expect(await screen.findByText('已取消 Garmin 凭据编辑。')).toBeVisible();

    window.dispatchEvent(new CustomEvent('picmanager:garmin-credentials', {
      detail: { outcome: 'saved', message: 'Credentials saved and local service restarted.' },
    }));
    expect(await screen.findByText('Credentials saved and local service restarted.')).toBeVisible();

    await fireEvent.click(screen.getByRole('button', { name: '验证登录' }));
    expect(await screen.findByText('无法连接 Garmin Connect；请检查网络或系统代理后重试。')).toBeVisible();
  });

  it('distinguishes a literal MFA challenge from a rejected MFA code', async () => {
    const api = {
      activities: {
        list: vi.fn(async () => ({ activities: [], total: 0, page: 1, per_page: 100 })),
        garminStatus: vi.fn(async () => ({ configured: true, authenticated: false, status: 'ready_to_authenticate', error_code: null, retryable: false, downloaded: 0, imported: 0, skipped: 0, failed: 0 })),
        authenticateGarmin: vi.fn()
          .mockResolvedValueOnce({ configured: true, authenticated: false, status: 'mfa_required', error_code: 'mfa_required', retryable: false, phase: 'credential_login', downloaded: 0, imported: 0, skipped: 0, failed: 0 })
          .mockResolvedValueOnce({ configured: true, authenticated: false, status: 'invalid_mfa', error_code: 'invalid_mfa', retryable: false, phase: 'mfa_login', exception_class: 'GarminConnectAuthenticationError', http_status: 401, downloaded: 0, imported: 0, skipped: 0, failed: 0 }),
        syncGarmin: vi.fn(),
      },
    } as unknown as ApiClient;
    render(ActivitiesView, { api });

    await fireEvent.click(await screen.findByRole('button', { name: '验证登录' }));
    expect(await screen.findByLabelText('Garmin MFA 验证码')).toBeVisible();
    await fireEvent.input(screen.getByLabelText('Garmin MFA 验证码'), { target: { value: '123456' } });
    await fireEvent.click(screen.getByRole('button', { name: '验证登录' }));
    expect(await screen.findByText('Garmin 未接受一次性 MFA 验证码；请重新输入最新验证码。')).toBeVisible();
  });

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
        photos: vi.fn(async () => ({
          photos: [{ id: 17, path: '/x.jpg', format: 'jpeg', taken_at: null, gps_lat: 31.2, gps_lon: 121.4 }],
          total: 1, page: 1, per_page: 16,
        })),
      },
    } as unknown as ApiClient;
    render(ActivitiesView, { api });

    await fireEvent.click(await screen.findByRole('button', { name: /滨江跑步/ }));
    expect(await screen.findByRole('img', { name: /包含 2 个轨迹点/ })).toBeVisible();
    expect(screen.getByText('5.20 km')).toBeVisible();
    expect(screen.getByText('30 分钟')).toBeVisible();
    expect(screen.getByAltText('活动照片 17')).toHaveAttribute('src', '/api/photos/17/thumb?size=512');
  });

  it('loads additional matched activity photos', async () => {
    const photos = vi.fn()
      .mockResolvedValueOnce({
        photos: [{ id: 17, path: '/x.jpg', format: 'jpeg', taken_at: null, gps_lat: 31.2, gps_lon: 121.4 }],
        total: 17, page: 1, per_page: 16,
      })
      .mockResolvedValueOnce({
        photos: [{ id: 18, path: '/y.jpg', format: 'jpeg', taken_at: null, gps_lat: 31.2, gps_lon: 121.4 }],
        total: 17, page: 2, per_page: 16,
      });
    const api = {
      activities: {
        list: vi.fn(async () => ({ activities: [activity], total: 1, page: 1, per_page: 100 })),
        get: vi.fn(async () => activity),
        track: vi.fn(async () => ({ points: [], original_count: 0, downsampled: false })),
        photos,
      },
    } as unknown as ApiClient;
    render(ActivitiesView, { api });
    await fireEvent.click(await screen.findByRole('button', { name: /滨江跑步/ }));
    await fireEvent.click(await screen.findByRole('button', { name: '下一页' }));
    expect(await screen.findByAltText('活动照片 18')).toBeVisible();
    expect(screen.queryByAltText('活动照片 17')).not.toBeInTheDocument();
    expect(photos).toHaveBeenLastCalledWith(5, 2, 16);
  });
});
