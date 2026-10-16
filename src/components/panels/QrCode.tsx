import { useEffect, useState } from "react";

export function QrCode({ text, size = 132 }: { text: string; size?: number }) {
  const [svg, setSvg] = useState<string | null>(null);
  useEffect(() => {
    let gone = false;
    setSvg(null);
    void import("qrcode").then(({ toString }) =>
      toString(text, { type: "svg", margin: 0, color: { dark: "#000000", light: "#ffffff" } }).then(
        (made) => {
          if (!gone) setSvg(made);
        },
      ),
    );
    return () => {
      gone = true;
    };
  }, [text]);
  if (!svg) return <span className="qr-code empty" style={{ width: size, height: size }} aria-hidden />;
  return (
    <img
      className="qr-code"
      width={size}
      height={size}
      alt={text}
      src={`data:image/svg+xml;utf8,${encodeURIComponent(svg)}`}
    />
  );
}
