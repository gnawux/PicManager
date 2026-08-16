import { describe, expect, it } from 'vitest';
import { photoGridLayout, photoPageCount } from './pagination';

describe('photo pagination', () => {
  it('uses a whole number of visible grid rows', () => {
    expect(photoGridLayout(666)).toEqual({ columns: 5, perPage: 20 });
    expect(photoGridLayout(532)).toEqual({ columns: 4, perPage: 16 });
  });

  it('keeps narrow and empty layouts usable', () => {
    expect(photoGridLayout(0)).toEqual({ columns: 1, perPage: 4 });
    expect(photoPageCount(41, 20)).toBe(3);
    expect(photoPageCount(0, 20)).toBe(1);
  });
});
