import { useState } from "react";
import {
  Badge,
  Banner,
  Button,
  Card,
  Checkbox,
  Dialog,
  DropdownMenu,
  EmptyState,
  ErrorState,
  IconButton,
  Input,
  SegmentedControl,
  Select,
  Skeleton,
  StatusDot,
  Switch,
  Table,
  Tabs,
  Toast,
  Tooltip,
} from "../components/primitives";
import { ConsoleOutput } from "../components/console/ConsoleOutput";
import { CodeEditor } from "../components/files/CodeEditor";
import { logStore } from "../lib/logStore";
import { applyTheme, type Theme } from "../lib/theme";
import "../App.css";

export function KitchenSink() {
  const [theme, setTheme] = useState<Theme>("dark");
  const [seg, setSeg] = useState("whitelist");
  const [tab, setTab] = useState("one");
  const [open, setOpen] = useState(false);
  const [checked, setChecked] = useState(true);
  const [pick, setPick] = useState("a");
  const [lines, setLines] = useState(0);
  const chatSample = () => {
    const line = (text: string, stream = "stdout") => ({ serverId: "sink", ansi: "", stream, noise: false, line: text });
    logStore.replace("sink", [
      line("[10Oct2026 21:29:20.649] [Server thread/INFO] [net.minecraft.server.MinecraftServer/]: Steve joined the game"),
      line("[10Oct2026 21:29:25.210] [Server thread/INFO] [net.minecraft.server.MinecraftServer/]: <Steve> hey! where is everyone?"),
      line("[10Oct2026 21:29:27.796] [Server thread/INFO] [net.minecraft.server.MinecraftServer/]: <Alex> near the nether fortress, this is a deliberately long message to see how the chat bubble wraps across lines"),
      line("[21:29:28 INFO]: Steve issued server command: /tp Alex"),
      line("say restarting in 10 minutes", "input"),
      line("[10Oct2026 21:29:31.436] [Server thread/INFO] [net.minecraft.server.MinecraftServer/]: [Server] restarting in 10 minutes"),
      line("[10Oct2026 21:29:34.235] [Server thread/WARN] [minecraft/MinecraftServer]: Can't keep up! Is the server overloaded?"),
    ]);
  };
  const fill = (count: number) => {
    const now = new Date().toTimeString().slice(0, 8);
    logStore.replace(
      "sink",
      Array.from({ length: count }, (_, n) => ({
        serverId: "sink",
        ansi: "",
        stream: n % 97 === 0 ? "stderr" : "stdout",
        noise: false,
        line:
          n % 401 === 0
            ? `[${now} ERROR]: java.lang.IllegalStateException: line ${n} exploded`
            : n % 401 === 1
              ? `	at net.minecraft.server.MinecraftServer.tick(MinecraftServer.java:${n})`
              : n % 53 === 0
                ? `[${now} INFO]: <Steve> hello from line ${n}`
                : `[${now} INFO]: [Server] line ${n} ${"x".repeat(n % 7 === 0 ? 160 : 40)}`,
      })),
    );
    setLines(count);
  };
  return (
    <main className="sink">
      <header className="sink-head">
        <h1>Kitchen sink</h1>
        <SegmentedControl
          value={theme}
          options={[
            { value: "dark", label: "dark" },
            { value: "light", label: "light" },
            { value: "oled", label: "oled" },
          ]}
          onChange={(next) => {
            setTheme(next);
            applyTheme(next);
          }}
        />
      </header>

      <Card title="Button">
        <div className="sink-row">
          <Button variant="primary" icon="play">Primary</Button>
          <Button variant="secondary">Secondary</Button>
          <Button variant="danger" icon="trash">Danger</Button>
          <Button variant="ghost">Ghost</Button>
          <Button variant="primary" disabled>Disabled</Button>
          <Button size="sm">Small</Button>
          <Button variant="primary" size="lg">Large</Button>
          <IconButton label="Edit" icon="pencil" />
          <IconButton label="Delete" icon="trash" disabled />
        </div>
      </Card>

      <Card title="Input · Select · Checkbox · Switch · SegmentedControl">
        <div className="sink-row">
          <Input placeholder="Server name" style={{ width: 220 }} />
          <Input placeholder="Disabled" disabled style={{ width: 160 }} />
          <div style={{ width: 180 }}>
            <Select
              value={pick}
              onChange={setPick}
              options={[
                { value: "a", label: "Paper" },
                { value: "b", label: "Fabric" },
                { value: "c", label: "NeoForge" },
              ]}
            />
          </div>
          <Checkbox label="Remember" checked={checked} onChange={(e) => setChecked(e.target.checked)} />
          <Switch checked={checked} onChange={(e) => setChecked(e.target.checked)} />
          <Switch disabled />
          <SegmentedControl
            value={seg}
            options={[
              { value: "whitelist", label: "Whitelist" },
              { value: "op", label: "OP" },
              { value: "ban", label: "Ban" },
            ]}
            onChange={setSeg}
          />
        </div>
      </Card>

      <Card title="Badge · StatusDot · Tooltip">
        <div className="sink-row">
          <Badge>Paper</Badge>
          <Badge tone="subtle">26.2</Badge>
          <Badge tone="ok">running</Badge>
          <Badge tone="warn">stopping</Badge>
          <Badge tone="danger">crashed</Badge>
          <span className="sink-dots">
            <StatusDot status="running" /> <StatusDot status="stopping" /> <StatusDot status="crashed" /> <StatusDot status="stopped" />
          </span>
          <Tooltip text="Radix tooltip — keyboard focus shows it too">
            <Button size="sm">Hover me</Button>
          </Tooltip>
          <Button size="sm" data-tip="data-tip tooltip (existing)">Hover me too</Button>
        </div>
      </Card>

      <Card title="Banner">
        <div className="sink-col">
          <Banner>The server is running. Settings are locked until it stops.</Banner>
          <Banner tone="warn" action={<Button size="sm">Stop server</Button>}>Anyone on the same Wi-Fi can see this.</Banner>
          <Banner tone="danger">The server crashed. See the crash report.</Banner>
        </div>
      </Card>

      <Card title="Tabs · Table">
        <Tabs
          value={tab}
          onChange={setTab}
          items={[
            {
              value: "one",
              label: "Players",
              content: (
                <Table columns={["Name", "Status", "Ping"]}>
                  <tr><td>Steve</td><td><StatusDot status="running" /> online</td><td>23 ms</td></tr>
                  <tr><td>Alex</td><td><StatusDot status="stopped" /> offline</td><td>—</td></tr>
                </Table>
              ),
            },
            { value: "two", label: "Events", content: <p>Second tab.</p> },
            { value: "three", label: "Files", content: <p>Third tab.</p> },
          ]}
        />
      </Card>

      <Card title="Skeleton · EmptyState · ErrorState">
        <div className="sink-col">
          <Skeleton height={40} />
          <Skeleton height={24} />
          <EmptyState icon="folder" title="No plugins installed yet" hint="Search Modrinth or drop a .jar here." action={<Button variant="primary" size="sm">Browse plugins</Button>} />
          <ErrorState title="Could not load the file list" detail="The server folder is not readable." retry={() => undefined} />
        </div>
      </Card>

      <Card title="Toast · Dialog · DropdownMenu">
        <div className="sink-col">
          <Toast>Settings saved.</Toast>
          <Toast tone="good">Server started!</Toast>
          <Toast tone="error">Could not start the server: port 25565 is in use.</Toast>
          <div className="sink-row">
            <Button onClick={() => setOpen(true)}>Open dialog</Button>
            <DropdownMenu
              trigger={<Button>Menu ⋯</Button>}
              items={[
                { label: "Rename", icon: "pencil", onSelect: () => undefined },
                { label: "Duplicate", icon: "copy", onSelect: () => undefined },
                { label: "Delete", icon: "trash", danger: true, onSelect: () => undefined },
              ]}
            />
          </div>
        </div>
      </Card>

      <Card title="CodeEditor">
        <div className="sink-editor" style={{ height: 260, border: "1px solid var(--line)", borderRadius: 11, overflow: "hidden" }}>
          <CodeEditor
            path="config.yml"
            value={"# comment\nserver:\n  port: 25565\n  online-mode: true\n  motd: \"hello\"\nlist:\n  - a\n  - b\n"}
            readOnly={false}
            onChange={() => undefined}
            onSave={() => undefined}
            onCursor={() => undefined}
          />
        </div>
      </Card>

      <Card title="ConsoleOutput (virtual)" aside={<><Button size="sm" onClick={() => chatSample()}>chat sample</Button> <Button size="sm" onClick={() => fill(10_000)}>10,000 lines</Button></>}>
        <div className="sink-console" data-lines={lines}>
          <ConsoleOutput serverId="sink" filter="" />
        </div>
      </Card>

      <Dialog
        open={open}
        onOpenChange={setOpen}
        title="Delete this server?"
        actions={
          <>
            <Button onClick={() => setOpen(false)}>Cancel</Button>
            <Button variant="danger" onClick={() => setOpen(false)}>Delete</Button>
          </>
        }
      >
        The folder moves to the trash. You can restore it from the sidebar.
      </Dialog>
    </main>
  );
}
