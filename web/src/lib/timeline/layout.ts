import type { TimelineItem } from '../api/types';

export interface TimelineGroup {
  key: string;
  label: string;
  items: TimelineItem[];
}

export interface LayoutCell {
  item: TimelineItem;
  width: number;
}

export interface LayoutRow {
  height: number;
  cells: LayoutCell[];
}

export function groupTimelineItems(items: TimelineItem[]): TimelineGroup[] {
  const groups = new Map<string, TimelineGroup>();
  for (const item of items) {
    const key = item.taken_at?.slice(0, 10) ?? 'undated';
    let group = groups.get(key);
    if (!group) {
      group = { key, label: dateLabel(key), items: [] };
      groups.set(key, group);
    }
    group.items.push(item);
  }
  return [...groups.values()];
}

export function justifyRows(
  items: TimelineItem[],
  containerWidth: number,
  targetHeight: number,
  gap = 4,
): LayoutRow[] {
  if (items.length === 0 || containerWidth <= 0) return [];
  const rows: LayoutRow[] = [];
  let pending: TimelineItem[] = [];
  let ratioSum = 0;

  const flush = (last: boolean) => {
    if (pending.length === 0) return;
    const available = Math.max(1, containerWidth - gap * (pending.length - 1));
    const naturalHeight = available / ratioSum;
    const height = last && naturalHeight > targetHeight
      ? targetHeight
      : Math.max(targetHeight * 0.72, Math.min(targetHeight * 1.28, naturalHeight));
    rows.push({
      height,
      cells: pending.map((item) => ({ item, width: aspectRatio(item) * height })),
    });
    pending = [];
    ratioSum = 0;
  };

  for (const item of items) {
    pending.push(item);
    ratioSum += aspectRatio(item);
    const projected = ratioSum * targetHeight + gap * (pending.length - 1);
    if (projected >= containerWidth) flush(false);
  }
  flush(true);
  return rows;
}

function aspectRatio(item: TimelineItem): number {
  const ratio = item.preview.aspect_ratio;
  return ratio && Number.isFinite(ratio) && ratio > 0 ? ratio : 1;
}

function dateLabel(key: string): string {
  if (key === 'undated') return '无拍摄日期';
  const [year, month, day] = key.split('-').map(Number);
  const date = new Date(Date.UTC(year, month - 1, day));
  if (Number.isNaN(date.valueOf())) return key;
  return new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
    weekday: 'short',
    timeZone: 'UTC',
  }).format(date);
}
