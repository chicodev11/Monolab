/** The middle of each open map (by cache key) in blocks, with pixels per block, so a share link can open the same spot. */
export const lastViews = new Map<string, { x: number; z: number; scale: number }>();
