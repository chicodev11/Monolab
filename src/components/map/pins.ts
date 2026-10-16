import { useEffect, useRef, useState } from "react";

export type MapPin = {
  key: string;
  x: number;
  z: number;
  label: string;
  kind: "player" | "spawn" | "death" | "mark";
  icon?: string | null;
  online?: boolean;
  savedAt?: number;
};

export function useSmoothPins(pins: MapPin[], on: boolean): MapPin[] {
  const [shown, setShown] = useState(pins);
  const drawn = useRef(pins);

  useEffect(() => {
    if (!on) {
      drawn.current = pins;
      setShown(pins);
      return;
    }
    let frame = 0;
    const step = () => {
      const byKey = new Map(drawn.current.map((one) => [one.key, one]));
      let moving = false;
      const next = pins.map((want) => {
        const had = byKey.get(want.key);
        if (!had) return want;
        const dx = want.x - had.x;
        const dz = want.z - had.z;
        if (Math.abs(dx) < 0.5 && Math.abs(dz) < 0.5) return want;
        moving = true;
        return { ...want, x: had.x + dx * 0.18, z: had.z + dz * 0.18 };
      });
      drawn.current = next;
      setShown(next);
      if (moving) {
        frame = requestAnimationFrame(step);
      }
    };
    frame = requestAnimationFrame(step);
    return () => cancelAnimationFrame(frame);
  }, [on, pins]);

  return on ? shown : pins;
}
