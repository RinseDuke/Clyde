export interface HitRegion { x: number; y: number; width: number; height: number }
export interface HitLayoutPayload {
  width: number;
  height: number;
  regions: HitRegion[];
  pointer_alpha?: number;
}

// Physical-pixel region / physical-pixel viewport = DPI-independent CSS percent.
// This also works while moving between monitors with different scale factors.
export function percentRegions(layout: HitLayoutPayload): HitRegion[] {
  if (layout.width <= 0 || layout.height <= 0) return [];
  return layout.regions.map(region => ({
    x: region.x / layout.width * 100,
    y: region.y / layout.height * 100,
    width: region.width / layout.width * 100,
    height: region.height / layout.height * 100,
  }));
}
