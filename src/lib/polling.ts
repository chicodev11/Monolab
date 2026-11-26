import { useEffect, useState } from "react";

export type Attention = "active" | "blurred" | "hidden";

function readAttention(): Attention {
  if (typeof document === "undefined") return "active";
  if (document.visibilityState === "hidden") return "hidden";
  return document.hasFocus() ? "active" : "blurred";
}

export function useAttention(): Attention {
  const [attention, setAttention] = useState<Attention>(readAttention);
  useEffect(() => {
    const update = () => setAttention(readAttention());
    document.addEventListener("visibilitychange", update);
    window.addEventListener("focus", update);
    window.addEventListener("blur", update);
    return () => {
      document.removeEventListener("visibilitychange", update);
      window.removeEventListener("focus", update);
      window.removeEventListener("blur", update);
    };
  }, []);
  return attention;
}

export const BLURRED_SLOWDOWN = 4;

export function period(base: number, attention: Attention): number | null {
  if (attention === "hidden") return null;
  if (attention === "blurred") return base * BLURRED_SLOWDOWN;
  return base;
}
