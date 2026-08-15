import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { ApiClient } from '../api/client';
import type { TaskDetail, TaskSummary } from '../api/types';
import TaskCenter from './TaskCenter.svelte';

const task: TaskSummary = {
  id: 6, kind: 'apple_full', provider: 'apple_photos', status: 'failed',
  checkpoint_before: null, checkpoint_after: null, total_items: 2, completed_items: 1,
  failed_items: 1, error: 'offline', created_at: '2024-01-01', started_at: '2024-01-01',
  finished_at: '2024-01-01', updated_at: '2024-01-01',
};
const detail: TaskDetail = {
  ...task,
  items: [{ id: 1, job_id: 6, source_id: 3, external_id: 'asset-3', operation: 'download', status: 'failed', attempt_count: 3, max_attempts: 3, last_error: 'iCloud offline' }],
};

describe('TaskCenter', () => {
  it('shows task failures, retries work, and resolves duplicates with confirmation', async () => {
    const retry = vi.fn(async () => ({ ...detail, status: 'queued', failed_items: 0, items: [{ ...detail.items[0], status: 'queued', last_error: null }] }));
    const resolve = vi.fn(async () => undefined);
    const api = {
      tasks: {
        list: vi.fn(async () => ({ tasks: [task], next_before_id: null })),
        get: vi.fn(async () => detail), retry, cancel: vi.fn(),
      },
      dedup: {
        list: vi.fn(async () => [{ group_id: 9, status: 'pending', members: [
          { photo_id: 10, path: '/a.jpg', filename: 'a.jpg', taken_at: null, camera: 'A', width: 4000, height: 3000, keep: false },
          { photo_id: 11, path: '/b.jpg', filename: 'b.jpg', taken_at: null, camera: 'B', width: 1200, height: 900, keep: false },
        ] }]),
        resolve,
      },
    } as unknown as ApiClient;
    render(TaskCenter, { api });

    await fireEvent.click(await screen.findByRole('button', { name: /apple full/ }));
    expect(await screen.findByText('iCloud offline')).toBeVisible();
    await fireEvent.click(screen.getByRole('button', { name: '重试失败项' }));
    await waitFor(() => expect(retry).toHaveBeenCalledWith(6));
    expect(await screen.findByText('等待中')).toBeVisible();

    const member = screen.getByRole('button', { name: /a.jpg/ });
    await fireEvent.click(member);
    await fireEvent.click(screen.getByRole('button', { name: '确认选择' }));
    expect(screen.getByText(/原文件不会立即删除/)).toBeVisible();
    await fireEvent.click(screen.getByRole('button', { name: '确认保留并隐藏其余项' }));
    await waitFor(() => expect(resolve).toHaveBeenCalledWith(9, [10]));
    expect(screen.queryByText('重复组 #9')).not.toBeInTheDocument();
  });
});
