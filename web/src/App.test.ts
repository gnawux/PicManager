import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import App from './App.svelte';

describe('application shell', () => {
  it('renders the modern shell and legacy escape hatch', () => {
    render(App);
    expect(screen.getByRole('heading', { name: '你的照片，终于有足够的空间。' })).toBeVisible();
    expect(screen.getByRole('navigation', { name: '主要导航' })).toBeVisible();
    expect(screen.getByRole('link', { name: '经典界面' })).toHaveAttribute('href', '/');
  });
});
