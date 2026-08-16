import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { ApiClient } from '../api/client';
import PeopleView from './PeopleView.svelte';

function mockApi() {
  const update = vi.fn(async () => undefined);
  const photos = vi.fn(async () => ({
    photos: [{ id: 21, path: '/p.jpg', format: 'jpeg', taken_at: null, camera: null, import_status: 'imported' }],
    total: 1, page: 1, per_page: 16,
  }));
  return {
    api: {
      people: {
        list: vi.fn(async () => [{ id: 4, name: null, parent_id: null, cover_face_id: 9, face_count: 3, photo_count: 1, status: 'active' }]),
        update,
        photos,
        discover: vi.fn(async () => ({ people_created: 0 })),
      },
    } as unknown as ApiClient,
    update,
    photos,
  };
}

describe('PeopleView', () => {
  it('names and ignores a detected person', async () => {
    const { api, update } = mockApi();
    render(PeopleView, { api });
    expect(await screen.findByRole('button', { name: '打开未命名人物 4' })).toBeVisible();

    await fireEvent.click(screen.getByRole('button', { name: '命名' }));
    await fireEvent.input(screen.getByRole('textbox', { name: '人物 4 名称' }), { target: { value: '小明' } });
    await fireEvent.click(screen.getByRole('button', { name: '保存' }));
    await waitFor(() => expect(update).toHaveBeenCalledWith(4, { name: '小明' }));
    expect(screen.getByText('小明')).toBeVisible();

    await fireEvent.click(screen.getByRole('button', { name: '忽略' }));
    await waitFor(() => expect(update).toHaveBeenLastCalledWith(4, { status: 'ignored' }));
    expect(screen.queryByText('小明')).not.toBeInTheDocument();
  });

  it('opens the photo set for a person', async () => {
    const { api, photos } = mockApi();
    render(PeopleView, { api });
    await fireEvent.click(await screen.findByRole('button', { name: '打开未命名人物 4' }));
    expect(await screen.findByAltText('照片 21')).toHaveAttribute('src', '/api/photos/21/thumb?size=512');
    expect(photos).toHaveBeenCalledWith(4, 1, 16);
  });

  it('loads additional person photo pages', async () => {
    const { api, photos } = mockApi();
    photos
      .mockResolvedValueOnce({
        photos: [{ id: 21, path: '/p.jpg', format: 'jpeg', taken_at: null, camera: null, import_status: 'imported' }],
        total: 17, page: 1, per_page: 16,
      })
      .mockResolvedValueOnce({
        photos: [{ id: 22, path: '/q.jpg', format: 'jpeg', taken_at: null, camera: null, import_status: 'imported' }],
        total: 17, page: 2, per_page: 16,
      });
    render(PeopleView, { api });
    await fireEvent.click(await screen.findByRole('button', { name: '打开未命名人物 4' }));
    await fireEvent.click(await screen.findByRole('button', { name: '下一页' }));
    expect(await screen.findByAltText('照片 22')).toBeVisible();
    expect(screen.queryByAltText('照片 21')).not.toBeInTheDocument();
    expect(photos).toHaveBeenLastCalledWith(4, 2, 16);
  });
});
