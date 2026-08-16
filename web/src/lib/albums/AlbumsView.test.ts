import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';
import type { ApiClient } from '../api/client';
import AlbumsView from './AlbumsView.svelte';

const photoDetail = (id: number) => ({
  id, path: `/${id}.jpg`, format: 'jpeg', taken_at: null, timezone_offset: null,
  camera: null, gps_lat: null, gps_lon: null, import_status: 'imported',
  width: 1200, height: 800, sources: [],
  renditions: { display: `/api/photos/${id}/file`, original: null, current: null },
});

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
      photos: { get: vi.fn(async (id: number) => photoDetail(id)) },
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
    await fireEvent.click(screen.getByRole('button', { name: '打开照片 42' }));
    expect(await screen.findByRole('dialog', { name: '照片 42' })).toBeVisible();
    await fireEvent.click(screen.getByRole('button', { name: '关闭查看器' }));
    expect(albumPhotos).toHaveBeenCalledWith(2, 1, 16);
    expect(screen.getByRole('button', { name: /2024年1月/ })).toBeVisible();

    await fireEvent.input(screen.getByRole('textbox', { name: '新精选集名称' }), { target: { value: '旅行' } });
    await fireEvent.click(screen.getByRole('button', { name: '创建' }));
    await waitFor(() => expect(create).toHaveBeenCalledWith('旅行'));
    expect(await screen.findByRole('button', { name: /旅行/ })).toBeVisible();
  });

  it('replaces pages for smart albums and collections', async () => {
    const page = (id: number, current: number) => ({
      photos: [{ id, path: `/${id}.jpg`, taken_at: null, camera: null }],
      total: 17, page: current, per_page: 16,
    });
    const albumPhotos = vi.fn().mockResolvedValueOnce(page(1, 1)).mockResolvedValueOnce(page(2, 2));
    const collectionPhotos = vi.fn().mockResolvedValueOnce(page(3, 1)).mockResolvedValueOnce(page(4, 2));
    const api = {
      albums: {
        list: vi.fn(async () => [{ id: 2, name: '房山区', kind: 'location', photo_count: 2, latest_photo_at: null }]),
        photos: albumPhotos,
      },
      collections: {
        list: vi.fn(async () => [{ id: 8, name: '旅行', photo_count: 2, created_at: '2024-01-01' }]),
        create: vi.fn(), photos: collectionPhotos,
      },
    } as unknown as ApiClient;
    render(AlbumsView, { api });

    await fireEvent.click(await screen.findByRole('button', { name: /房山区/ }));
    await fireEvent.click(await screen.findByRole('button', { name: '下一页' }));
    expect(await screen.findByAltText('照片 2')).toBeVisible();
    expect(screen.queryByAltText('照片 1')).not.toBeInTheDocument();
    expect(albumPhotos).toHaveBeenLastCalledWith(2, 2, 16);

    await fireEvent.click(screen.getByRole('button', { name: /旅行/ }));
    await fireEvent.click(await screen.findByRole('button', { name: '下一页' }));
    expect(await screen.findByAltText('照片 4')).toBeVisible();
    expect(screen.queryByAltText('照片 3')).not.toBeInTheDocument();
    expect(collectionPhotos).toHaveBeenLastCalledWith(8, 2, 16);
  });

  it('sorts album navigation and qualifies location names', async () => {
    const api = {
      albums: {
        list: vi.fn(async () => [
          { id: 1, name: '西城区', parent_name: '北京市', kind: 'location', photo_count: 8, latest_photo_at: '2024-01-01' },
          { id: 2, name: '香港', parent_name: null, kind: 'location', photo_count: 9, latest_photo_at: '2024-02-01' },
          { id: 3, name: 'Alpha', parent_name: null, kind: 'camera', photo_count: 2, latest_photo_at: '2024-01-01' },
          { id: 4, name: 'Beta', parent_name: null, kind: 'camera', photo_count: 10, latest_photo_at: '2024-03-01' },
        ]),
        photos: vi.fn(),
      },
      collections: { list: vi.fn(async () => []), create: vi.fn(), photos: vi.fn() },
    } as unknown as ApiClient;
    const { container } = render(AlbumsView, { api });

    expect(await screen.findByRole('button', { name: /西城区（北京市）/ })).toBeVisible();
    expect(screen.getByRole('button', { name: /香港/ })).toBeVisible();
    const cameraSection = [...container.querySelectorAll('.album-section')]
      .find((section) => section.textContent?.includes('按相机')) as HTMLElement;
    const names = () => [...cameraSection.querySelectorAll('.album-list strong')].map((node) => node.textContent);
    expect(names()).toEqual(['Beta', 'Alpha']);

    await fireEvent.change(screen.getByLabelText('排序'), { target: { value: 'name' } });
    expect(names()).toEqual(['Alpha', 'Beta']);
    await fireEvent.change(screen.getByLabelText('排序'), { target: { value: 'recent' } });
    expect(names()).toEqual(['Beta', 'Alpha']);
  });
});
