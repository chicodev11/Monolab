import { tr } from "../../lib/i18n";
import { useId } from "react";
export type Sample = { at: number; value: number | null };

export function Sparkline({
  samples,
  max,
  unit,
  label,
  format,
  threshold,
}: {
  samples: Sample[];
  max?: number;
  unit: string;
  label: string;
  format?: (value: number) => string;
  threshold?: number;
}) {
  const thresholdId = useId().replace(/:/g, "-");
  const points = samples.filter(
    (sample): sample is { at: number; value: number } => sample.value !== null,
  );
  const width = 520;
  const height = 96;
  const show = format ?? ((value: number) => value.toFixed(1));
  const latest = points.at(-1)?.value ?? null;

  if (points.length < 2)
    return (
      <div className="chart">
        <div className="chart-head">
          <span>{label}</span>
          <strong>{latest == null ? "—" : `${show(latest)}${unit}`}</strong>
        </div>
        <p className="chart-empty">
          {tr("The graph will appear shortly. Data is collected every few seconds.")}</p>
      </div>
    );

  const ceiling = Math.max(max ?? 0, ...points.map((point) => point.value), 1);
  const step = width / (points.length - 1);
  const y = (value: number) => height - (value / ceiling) * (height - 8) - 4;
  const line = points
    .map((point, index) => `${index === 0 ? "M" : "L"}${(index * step).toFixed(1)},${y(point.value).toFixed(1)}`)
    .join(" ");
  const area = `${line} L${width},${height} L0,${height} Z`;
  const average =
    points.reduce((sum, point) => sum + point.value, 0) / points.length;
  const peak = Math.max(...points.map((point) => point.value));

  return (
    <div className="chart">
      <div className="chart-head">
        <span>{label}</span>
        <strong>{latest == null ? "—" : `${show(latest)}${unit}`}</strong>
        <small>
          {tr("Average")}{" "}{show(average)}
          {unit} {tr("· peak")}{" "}{show(peak)}
          {unit}
        </small>
      </div>
      <div className="chart-plot">
      <svg
        className="chart-body"
        viewBox={`0 0 ${width} ${height}`}
        preserveAspectRatio="none"
        role="img"
        aria-label={tr("Recent trends for {0}", [label])}
      >
        <line className="chart-grid" x1="0" y1={y(ceiling / 2)} x2={width} y2={y(ceiling / 2)} />
        <path className="chart-area" d={area} />
        <path className="chart-line" d={line} />
        {threshold != null && (
          <>
            <clipPath id={`below-${thresholdId}`}>
              <rect x="0" y={y(threshold)} width={width} height={height - y(threshold)} />
            </clipPath>
            <path
              className="chart-line threshold"
              d={line}
              clipPath={`url(#below-${thresholdId})`}
            />
          </>
        )}
      </svg>
      </div>
      <div className="chart-scale">
        <span>
          {show(ceiling)}
          {unit}
        </span>
        <span>
          {tr("Recent")}{" "}{points.length}{tr("suffix.runsDot")}{" "}{Math.round((points.at(-1)!.at - points[0].at) / 60)}{tr(" min")}</span>
        <span>0{unit}</span>
      </div>
    </div>
  );
}
