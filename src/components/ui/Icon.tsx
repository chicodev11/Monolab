import {
  Layers,
  PanelLeftClose, PanelLeftOpen, Paperclip, Archive, Monitor, ArrowUp, ArrowDown, ArrowRight, Ban, Check, ChevronDown, Clock, Cloud, Copy, Cpu, Download,
  Ellipsis, FileText, Folder, House, Info, List, LogOut, Map as MapIcon, MemoryStick, Pencil,
  Pause, Play, Plug, Plus, RefreshCw, Search, Server, Settings, Shield, Square, Tag, Terminal,
  Trash2, TriangleAlert, Undo2, X, LayoutGrid, type LucideIcon,
  Compass, Sparkles, Coins, Sword, Utensils, Cog, Library, WandSparkles, Gamepad2, Skull, Gauge,
  MessageCircle, Package, Truck, Wrench, Mountain, Ghost, Puzzle, Heart, User, ShieldCheck, Blocks,
  Power, PowerOff,
  Image as ImageIcon,
  Sun, Moon,
  Database, Network, Users, SlidersVertical, CalendarClock, ChartLine, CloudUpload, SquareTerminal,
  Eye, EyeOff, HardDrive,
} from "lucide-react";

const ICONS: Record<string, LucideIcon> = {
  map: MapIcon,
  disk: HardDrive,
  sun: Sun,
  moon: Moon,
  layers: Layers,
  monitor: Monitor,
  "arrow-up": ArrowUp,
  shield: Shield,
  ban: Ban,
  exit: LogOut,
  info: Info,
  plus: Plus,
  import: Download,
  tag: Tag,
  console: Terminal,
  search: Search,
  play: Play,
  pause: Pause,
  stop: Square,
  list: List,
  chevron: ChevronDown,
  "panel-open": PanelLeftOpen,
  "panel-close": PanelLeftClose,
  check: Check,
  folder: Folder,
  archive: Archive,
  settings: Settings,
  file: FileText,
  image: ImageIcon,
  home: House,
  undo: Undo2,
  "arrow-down": ArrowDown,
  arrow: ArrowRight,
  clock: Clock,
  plug: Plug,
  memory: MemoryStick,
  cpu: Cpu,
  server: Server,
  cloud: Cloud,
  trash: Trash2,
  dots: Ellipsis,
  copy: Copy,
  pencil: Pencil,
  alert: TriangleAlert,
  refresh: RefreshCw,
  close: X,
  paperclip: Paperclip,
  grid: LayoutGrid,
  compass: Compass,
  sparkles: Sparkles,
  coins: Coins,
  sword: Sword,
  utensils: Utensils,
  cog: Cog,
  library: Library,
  wand: WandSparkles,
  gamepad: Gamepad2,
  skull: Skull,
  gauge: Gauge,
  message: MessageCircle,
  package: Package,
  truck: Truck,
  wrench: Wrench,
  mountain: Mountain,
  ghost: Ghost,
  puzzle: Puzzle,
  heart: Heart,
  user: User,
  "shield-check": ShieldCheck,
  blocks: Blocks,
  power: Power,
  "power-off": PowerOff,
  database: Database,
  network: Network,
  users: Users,
  sliders: SlidersVertical,
  schedule: CalendarClock,
  activity: ChartLine,
  "cloud-up": CloudUpload,
  terminal: SquareTerminal,
  eye: Eye,
  "eye-off": EyeOff,
};

export function Icon({ name }: { name: string }) {
  if (name === "brand")
    return (
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth={2.6}
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden
      >
        <path d="M6.5 8.5h11M6.5 12h11M6.5 15.5h11" />
      </svg>
    );
  const Drawn = ICONS[name] ?? LayoutGrid;
  return <Drawn strokeWidth={1.75} aria-hidden />;
}
