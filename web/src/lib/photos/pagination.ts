export interface PhotoGridLayout {
  columns: number;
  perPage: number;
}

export function photoGridLayout(
  width: number,
  rows = 4,
  minimumTileWidth = 130,
  gap = 4,
): PhotoGridLayout {
  const columns = Math.max(1, Math.floor((Math.max(0, width) + gap) / (minimumTileWidth + gap)));
  return { columns, perPage: columns * rows };
}

export function photoPageCount(total: number, perPage: number): number {
  return Math.max(1, Math.ceil(total / Math.max(1, perPage)));
}
