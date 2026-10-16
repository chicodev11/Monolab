import { tr } from "../../lib/i18n";
import { Icon } from "../ui/Icon";
import { Banner, EmptyState } from "../primitives";
import { date } from "../../lib/format";

export type CrashReport = {
  fileName: string;
  createdAt: number;
  description: string;
  exception: string;
  suspects: string[];
  excerpt: string;
};

export function CrashPanel({
  reports,
  openReport,
  onToggle,
  onReload,
}: {
  reports: CrashReport[];
  openReport: string | null;
  onToggle: (fileName: string | null) => void;
  onReload: () => void;
}) {
  return (

<div className="tool-content crash-page">
  <Banner
    action={
      <button className="button secondary small" onClick={() => onReload()}>
        <Icon name="refresh" />
        {tr("Reload")}
      </button>
    }
  >
    {tr("Reads recent reports from crash-reports and compares stack traces with installed mods and plugins to suggest possible causes. Use these as a starting point, not a confirmed diagnosis.")}
  </Banner>
  {reports.length ? (
    <div className="crash-list">
      {reports.map((report) => (
        <div className="crash-item" key={report.fileName}>
          <button
            className="crash-head"
            onClick={() =>
              onToggle(openReport === report.fileName ? null : report.fileName)
            }
          >
            <div>
              <strong>{report.description}</strong>
              <small>
                {date(report.createdAt)} · {report.fileName}
              </small>
            </div>
            <Icon name="chevron" />
          </button>
          {report.suspects.length > 0 && (
            <div className="crash-suspects">
              <span>{tr("Suspects")}</span>
              {report.suspects.map((suspect) => (
                <code key={suspect}>{suspect}</code>
              ))}
            </div>
          )}
          {openReport === report.fileName && (
            <div className="crash-detail">
              {report.exception && <code>{report.exception}</code>}
              <pre>{report.excerpt}</pre>
            </div>
          )}
        </div>
      ))}
    </div>
  ) : (
    <EmptyState
      icon="check"
      title={tr("No crash reports.")}
      hint={tr("The server is shutting down normally. If it shuts down unexpectedly, a report is saved in crash-reports and appears here.")}
    />
  )}
</div>
  );
}
