import { render, screen, waitFor } from '@testing-library/svelte';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import App from './App.svelte';

describe('application shell', () => {
  beforeEach(() => {
    window.location.hash = '';
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        new Response(JSON.stringify({ items: [], next_cursor: null, has_more: false }), {
          status: 200,
          headers: { 'content-type': 'application/json' },
        }),
      ),
    );
  });

  it('renders the routed shell, loads the library, and keeps a legacy escape hatch', async () => {
    render(App);
    expect(screen.getByRole('heading', { name: '所有照片' })).toBeVisible();
    expect(screen.getByRole('navigation', { name: '主要导航' })).toBeVisible();
    expect(screen.getByRole('link', { name: '经典界面' })).toHaveAttribute('href', '/legacy/');
    await waitFor(() => expect(screen.getByText('图库还是空的')).toBeVisible());
  });
});
