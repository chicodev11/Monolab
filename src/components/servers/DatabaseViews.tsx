import { useCallback, useEffect, useState } from "react";
import { commands, type TablePage } from "../../lib/bindings";
import { fileSize, relativeTime } from "../../lib/format";
import { tr } from "../../lib/i18n";
import { Banner, Button, Card, EmptyState, IconButton, List, ListRow, Skeleton } from "../primitives";
import { Dropdown } from "../ui/Dropdown";

const PAGE = 100;

/** Read-only pages of one database's tables, used for both MySQL and SQLite. */
export function TableBrowser({
  title,
  loadTables,
  loadRows,
  onBack,
}: {
  title: string;
  loadTables: () => Promise<string[]>;
  loadRows: (table: string, offset: number) => Promise<TablePage>;
  onBack: () => void;
}) {
  const [tables, setTables] = useState<string[] | null>(null);
  const [table, setTable] = useState("");
  const [offset, setOffset] = useState(0);
  const [page, setPage] = useState<TablePage | null>(null);
  const [problem, setProblem] = useState("");

  const readTables = useCallback(async () => {
    setTables(null);
    setPage(null);
    setProblem("");
    try {
      const names = await loadTables();
      setTables(names);
      setTable((now) => (names.includes(now) ? now : (names[0] ?? "")));
      setOffset(0);
    } catch (cause) {
      setTables([]);
      setProblem(String(cause));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  useEffect(() => {
    void readTables();
  }, [readTables]);

  useEffect(() => {
    if (!table || tables === null) return;
    let live = true;
    setPage(null);
    loadRows(table, offset)
      .then((got) => {
        if (live) setPage(got);
      })
      .catch((cause) => {
        if (live) setProblem(String(cause));
      });
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [table, offset, tables]);

  return (
    <div className="db-browser">
      <div className="db-browser-head">
        <IconButton label={tr("Back")} icon="arrow-left" onClick={onBack} />
        <strong className="mono db-browser-title">{title}</strong>
        {!!tables?.length && (
          <Dropdown
            className="db-browser-pick"
            value={table}
            options={tables.map((one) => ({ value: one, label: one }))}
            search={tables.length > 12 ? tr("Search") : undefined}
            onChange={(next) => {
              setTable(next);
              setOffset(0);
            }}
          />
        )}
        <IconButton label={tr("Refresh")} icon="refresh" onClick={() => void readTables()} />
      </div>
      {problem && <Banner tone="danger">{problem}</Banner>}
      {tables === null || (table && !page && !problem) ? (
        <Skeleton height={240} />
      ) : !tables.length ? (
        !problem && <EmptyState icon="database" title={tr("No tables")} />
      ) : (
        page && (
          <>
            <div className="db-grid-wrap">
              <table className="db-grid">
                <thead>
                  <tr>
                    {page.columns.map((one, at) => (
                      <th key={at}>{one}</th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {page.rows.map((row, at) => (
                    <tr key={at}>
                      {row.map((cell, slot) => (
                        <td key={slot} title={cell ?? undefined} className={cell === null ? "db-null" : undefined}>
                          {cell ?? "NULL"}
                        </td>
                      ))}
                    </tr>
                  ))}
                </tbody>
              </table>
              {!page.rows.length && <p className="muted db-grid-empty">{tr("This table is empty.")}</p>}
            </div>
            <div className="db-pager">
              <span className="muted">{page.rows.length ? tr("Rows {0} to {1}", [offset + 1, offset + page.rows.length]) : ""}</span>
              <Button size="sm" icon="arrow-left" disabled={offset === 0} onClick={() => setOffset(Math.max(0, offset - PAGE))}>
                {tr("Previous")}
              </Button>
              <Button size="sm" icon="arrow" disabled={!page.more} onClick={() => setOffset(offset + PAGE)}>
                {tr("Next")}
              </Button>
            </div>
          </>
        )
      )}
    </div>
  );
}

/** SQLite files in plugin and mod folders. Far servers are read from a copy. */
export function SqliteFiles({ serverId, far, onOpen }: { serverId: string; far: boolean; onOpen: (path: string) => void }) {
  const [scan, setScan] = useState<Awaited<ReturnType<typeof commands.serverSqliteFiles>> | null>(null);
  const [problem, setProblem] = useState("");
  const load = useCallback(async () => {
    setScan(null);
    setProblem("");
    try {
      setScan(await commands.serverSqliteFiles(serverId));
    } catch (cause) {
      setProblem(String(cause));
    }
  }, [serverId]);
  useEffect(() => {
    void load();
  }, [load]);
  return (
    <Card title={tr("SQLite files")} aside={<IconButton label={tr("Refresh")} icon="refresh" onClick={() => void load()} />}>
      {problem ? (
        <Banner tone="danger">{problem}</Banner>
      ) : !scan ? (
        <Skeleton height={60} />
      ) : !scan.files.length ? (
        <p className="muted">{tr("No SQLite files in the plugin and mod folders.")}</p>
      ) : (
        <List variant="box" className="pt-list">
          {scan.files.map((one) => (
            <ListRow
              key={one.path}
              title={<>{one.path}</>}
              subtitle={<>{fileSize(one.sizeBytes)}
                  {one.modifiedAt ? ` · ${relativeTime(one.modifiedAt)}` : ""}</>}
              actions={<><Button size="sm" icon="eye" onClick={() => onOpen(one.path)}>
                  {tr("View tables")}
                </Button></>}
            />
          ))}
        </List>
      )}
      {scan?.incomplete && <p className="muted">{tr("Some folders couldn't be read, so this list may be missing files.")}</p>}
      {far && !!scan?.files.length && <p className="muted">{tr("Opens a copy downloaded to this computer. Files over 256 MB can't be opened.")}</p>}
    </Card>
  );
}
