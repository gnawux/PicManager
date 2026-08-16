import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { ApiClient } from '../api/client';
import AlbumsView from './AlbumsView.svelte';

describe('AlbumsView', () => {
  it('groups albums in a split view, browses photos, and creates collections', async () => {
    const listCollections = vi.fn()
      .mockResolvedValueOnce([{ id: 8, name: '家人', photo_count: 3, created_at: '2024-01-01' }])
      .mockResolvedValueOnce([
        { id: 9, name: '旅行', photo_count: 0, created_at: '2024-01-02' },
        { id: 8, name: '家人', photo_count: 3, created_at: '2024-01-01' },
      ]);
    const create = vi.fn(async () => ({ id: 9, name: '旅行' }));
    const albumPhotos = vi.fn(async () => ({
      photos: [{ id: 42, path: '/photo.jpg', taken_at: '2024-01-01', camera: 'Camera' }],
      total: 1, page: 1, per_page: 100,
    }));
    const api = {
      albums: {
        list: vi.fn(async () => [
          { id: 2, name: '2024年1月', kind: 'time', photo_count: 1, latest_photo_at: '2024-01-01' },
          { id: 3, name: '上海', kind: 'location', photo_count: 12, latest_photo_at: '2024-01-02' },
          { id: 4, name: 'Leica Q', kind: 'camera', photo_count: 5, latest_photo_at: '2024-01-03' },
        ]),
        photos: albumPhotos,
      },
      collections: {
        list: listCollections,
        create,
        photos: vi.fn(),
      },
    } as unknown as ApiClient;
    render(AlbumsView, { api });

    expect(await screen.findByRole('complementary', { name: '相册导航' })).toBeVisible();
    expect(screen.getByText('按月份')).toBeVisible();
    expect(screen.getByText('按地点')).toBeVisible();
    expect(screen.getByText('按相机')).toBeVisible();
    expect(await screen.findByRole('button', { name: /2024年1月/ })).toBeVisible();
    expect(screen.getByRole('button', { name: /家人/ })).toBeVisible();
    await fireEvent.click(screen.getByRole('button', { name: /2024年1月/ }));
    expect(await screen.findByAltText('照片 42')).toHaveAttribute('src', '/api/photos/42/thumb?size=512');
    expect(albumPhotos).toHaveBeenCalledWith(2);
    expect(screen.getByRole('button', { name: /2024年1月/ })).toBeVisible();

    await fireEvent.input(screen.getByRole('textbox', { name: '新精选集名称' }), { target: { value: '旅行' } });
    await fireEvent.click(screen.getByRole('button', { name: '创建' }));
    await waitFor(() => expect(create).toHaveBeenCalledWith('旅行'));
    expect(await screen.findByRole('button', { name: /旅行/ })).toBeVisible();
  });
});
