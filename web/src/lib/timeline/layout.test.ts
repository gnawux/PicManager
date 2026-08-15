import { describe, expect, it } from 'vitest';
import type { TimelineItem } from '../api/types';
import { groupTimelineItems, justifyRows } from './layout';

function item(id: number, ratio: number, takenAt: string | null = '2024-01-02T10:00:00'): TimelineItem {
  return {
    id,
    taken_at: takenAt,
    camera: null,
    format: 'jpeg',
    display_revision: 0,
    has_original: true,
    has_current: false,
    preview: { src: '', srcset: '', width: null, height: null, aspect_ratio: ratio },
    file_url: '',
  };
}

describe('justified timeline layout', () => {
  it('places every photo exactly once and bounds row heights', () => {
    const items = [item(1, 1.5), item(2, 1), item(3, 2), item(4, 0.75), item(5, 1.2)];
    const rows = justifyRows(items, 900, 180, 4);
    expect(rows.flatMap((row) => row.cells.map((cell) => cell.item.id))).toEqual([1, 2, 3, 4, 5]);
    expect(rows.every((row) => row.height >= 129.6 && row.height <= 230.4)).toBe(true);
    expect(rows.every((row) => row.cells.every((cell) => cell.width > 0))).toBe(true);
  });

  it('keeps an incomplete final row at target height instead of stretching it', () => {
    const rows = justifyRows([item(1, 1), item(2, 1)], 1200, 200);
    expect(rows).toHaveLength(1);
    expect(rows[0].height).toBe(200);
  });

  it('groups dates in source order and keeps undated photos visible', () => {
    const groups = groupTimelineItems([
      item(1, 1, '2024-01-02T10:00:00'),
      item(2, 1, '2024-01-02T09:00:00'),
      item(3, 1, null),
    ]);
    expect(groups.map((group) => [group.key, group.items.length])).toEqual([
      ['2024-01-02', 2],
      ['undated', 1],
    ]);
  });
});
