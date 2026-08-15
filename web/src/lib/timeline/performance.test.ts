import { describe, expect, it } from 'vitest';
import type { TimelineItem } from '../api/types';
import { groupTimelineItems, justifyRows } from './layout';

const CATALOG_SIZE = 100_000;
const DATE_COUNT = 3_650;
const GROUPING_BUDGET_MS = 1_000;

describe('large timeline performance budgets', () => {
  it('groups a 100k catalog by date without per-photo group growth', () => {
    const items = Array.from({ length: CATALOG_SIZE }, (_, index) => timelineItem(index));
    const started = performance.now();
    const groups = groupTimelineItems(items);
    const elapsed = performance.now() - started;

    expect(groups).toHaveLength(DATE_COUNT);
    expect(groups.reduce((total, group) => total + group.items.length, 0)).toBe(CATALOG_SIZE);
    expect(elapsed).toBeLessThan(GROUPING_BUDGET_MS);
  });

  it('lays out a representative date while the API page stays bounded', () => {
    const initialPage = Array.from({ length: 120 }, (_, index) => timelineItem(index));
    const rows = justifyRows(initialPage, 1_440, 180);

    expect(initialPage).toHaveLength(120);
    expect(rows.flatMap((row) => row.cells)).toHaveLength(120);
    expect(rows.length).toBeLessThan(40);
  });
});

function timelineItem(index: number): TimelineItem {
  const day = index % DATE_COUNT;
  const date = new Date(Date.UTC(2015, 0, 1 + day)).toISOString();
  return {
    id: index + 1,
    taken_at: date,
    camera: null,
    format: 'jpeg',
    display_revision: 0,
    has_original: true,
    has_current: false,
    preview: {
      src: '',
      srcset: '',
      width: 4_032,
      height: 3_024,
      aspect_ratio: index % 5 === 0 ? 0.75 : 4 / 3,
    },
    file_url: '',
  };
}
