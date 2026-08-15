import { describe, expect, it } from 'vitest';
import { routeFromHash } from './router';

describe('application routes', () => {
  it('defaults unknown and empty paths to photos', () => {
    expect(routeFromHash('').id).toBe('photos');
    expect(routeFromHash('#/missing').id).toBe('photos');
  });

  it('resolves feature routes without server-side rewrites', () => {
    expect(routeFromHash('#/apple').id).toBe('apple');
    expect(routeFromHash('#/tasks').label).toBe('任务');
    expect(routeFromHash('#/activities').id).toBe('activities');
  });
});
