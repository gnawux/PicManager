import { render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { ApiClient } from '../api/client';
import type { TimelineItem } from '../api/types';
import TimelineView from './TimelineView.svelte';

function item(id: number, takenAt: string): TimelineItem {
  return {
    id,
    taken_at: takenAt,
    camera: 'Camera',
    format: 'jpeg',
    display_revision: 0,
    has_original: true,
    has_current: id === 2,
    preview: {
      src: `/thumb/${id}`,
      srcset: `/thumb/${id}?size=256 256w`,
      width: 400,
      height: 300,
      aspect_ratio: 4 / 3,
    },
    file_url: `/file/${id}`,
  };
}

describe('TimelineView', () => {
  it('renders date-grouped justified photos and density control', async () => {
    const api = {
      timeline: {
        page: vi.fn(async () => ({
          items: [item(1, '2024-01-02T10:00:00'), item(2, '2024-01-02T09:00:00')],
          next_cursor: null,
          has_more: false,
        })),
      },
    } as unknown as ApiClient;
    render(TimelineView, { api });

    await waitFor(() => expect(screen.getAllByRole('button', { name: /打开照片/ })).toHaveLength(2));
    expect(screen.getByRole('slider', { name: '照片密度' })).toBeVisible();
    expect(screen.getByText('已编辑')).toBeVisible();
    expect(screen.getByText('已到达图库开端')).toBeVisible();
  });
});
