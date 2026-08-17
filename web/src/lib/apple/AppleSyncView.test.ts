import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { ApiClient } from '../api/client';
import type { AppleSourceSummary } from '../api/types';
import AppleSyncView from './AppleSyncView.svelte';

const failed: AppleSourceSummary = {
  id: 3, asset_id: null, photo_id: null, external_id: 'asset-3', original_filename: 'IMG_0042.HEIC',
  media_type: 'image/heic', width: 4032, height: 3024, taken_at: '2024-01-01', sync_status: 'failed',
  exclusion_reason: null, last_error: 'iCloud offline', last_seen_at: '2024-01-02', updated_at: '2024-01-02',
};

describe('AppleSyncView', () => {
  it('hands native iCloud sync to the embedded Mac bridge and refreshes its status', async () => {
    vi.useFakeTimers();
    const sources = vi.fn(async () => ({
      sources: [], status_counts: { synced: 8, queued: 2 }, next_before_id: null,
    }));
    const postMessage = vi.fn();
    Object.defineProperty(window, 'webkit', {
      configurable: true,
      value: { messageHandlers: { picmanager: { postMessage } } },
    });
    const api = {
      apple: { sources, retry: vi.fn(), candidates: vi.fn(async () => []), recentlySynchronized: vi.fn(async () => []), reviewCandidate: vi.fn() },
    } as unknown as ApiClient;
    render(AppleSyncView, { api });

    await screen.findByText('此筛选条件下没有照片');
    await fireEvent.click(screen.getByRole('button', { name: '同步 iCloud 照片' }));
    expect(postMessage).toHaveBeenCalledWith({ action: 'syncApple' });
    expect(await screen.findByText(/当前有 2 张等待同步/)).toBeVisible();
    await vi.advanceTimersByTimeAsync(5_000);
    expect(sources).toHaveBeenCalledTimes(2);
    vi.useRealTimers();
  });

  it('shows unsynced inventory, retries failures, and reviews matches', async () => {
    const sources = vi.fn(async () => ({
      sources: [failed], status_counts: { synced: 8, discovered: 2, failed: 1, excluded: 1 }, next_before_id: null,
    }));
    const retry = vi.fn(async () => ({ job_id: 10, source: { ...failed, sync_status: 'queued', last_error: null } }));
    const reviewCandidate = vi.fn(async () => ({ job_id: null }));
    const api = {
      apple: {
        sources,
        retry,
        recentlySynchronized: vi.fn(async () => []),
        candidates: vi.fn(async () => [{
          id: 7, source_id: 3, photo_id: 22, method: 'structured_metadata', confidence: .92,
          status: 'candidate', evidence_json: null, original_filename: 'IMG_0042.HEIC', source_taken_at: null,
          source_width: 4032, source_height: 3024, photo_path: '/library/IMG_0042.jpg', photo_taken_at: null,
          photo_width: 4032, photo_height: 3024,
        }]),
        reviewCandidate,
      },
    } as unknown as ApiClient;
    render(AppleSyncView, { api });

    expect(await screen.findByText('IMG_0042.HEIC')).toBeVisible();
    expect(screen.getByText('iCloud offline')).toBeVisible();
    expect(screen.getByRole('button', { name: /尚未同步3/ })).toBeVisible();
    await fireEvent.click(screen.getByRole('button', { name: '重试' }));
    await waitFor(() => expect(retry).toHaveBeenCalledWith(3));
    expect(await screen.findByText('等待同步')).toBeVisible();

    await fireEvent.click(screen.getByRole('button', { name: '确认匹配' }));
    await waitFor(() => expect(reviewCandidate).toHaveBeenCalledWith(7, true));
    expect(screen.queryByRole('button', { name: '确认匹配' })).not.toBeInTheDocument();
  });

  it('shows photos completed within the recent synchronization window and opens the viewer', async () => {
    const api = {
      apple: {
        sources: vi.fn(async () => ({ sources: [], status_counts: { synced: 1 }, next_before_id: null })),
        recentlySynchronized: vi.fn(async () => [{
          id: 42, original_filename: 'IMG_0042.HEIC', taken_at: '2024-01-01 08:00:00',
          synchronized_at: '2026-08-17 03:00:00', has_current: false,
        }]),
        retry: vi.fn(), candidates: vi.fn(async () => []), reviewCandidate: vi.fn(),
      },
      photos: {
        get: vi.fn(async () => ({
          id: 42, path: '/library/IMG_0042.HEIC', format: 'heic', taken_at: '2024-01-01 08:00:00',
          timezone_offset: null, camera: 'iPhone', gps_lat: null, gps_lon: null, import_status: 'imported',
          width: 4032, height: 3024, sources: [{ provider: 'apple_photos', original_filename: 'IMG_0042.HEIC', sync_status: 'ready' }],
          renditions: { display: '/api/photos/42/file', original: '/api/photos/42/file', current: null },
        })),
      },
    } as unknown as ApiClient;
    render(AppleSyncView, { api });

    expect(await screen.findByText('过去 24 小时 · 1 张')).toBeVisible();
    await fireEvent.click(screen.getByRole('button', { name: '查看 IMG_0042.HEIC' }));
    expect(await screen.findByRole('dialog', { name: '照片 42' })).toBeVisible();
  });
});
