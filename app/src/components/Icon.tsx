import {
  AlertTriangle,
  ArrowDownLeft,
  ArrowUp,
  ArrowUpRight,
  Bookmark,
  BookmarkPlus,
  Bot,
  Bug,
  Calendar,
  CheckSquare,
  ChevronsUp,
  ChevronsUpDown,
  CloudOff,
  Command,
  Database,
  Edit,
  ExternalLink,
  Files,
  FileText,
  Folder,
  FolderPlus,
  GitBranch,
  HelpCircle,
  History,
  Indent,
  LayoutGrid,
  ListChecks,
  ListTree,
  MessageSquare,
  Minus,
  Network,
  Play,
  PlayCircle,
  Plug,
  Plus,
  Puzzle,
  Search,
  Settings,
  Sparkles,
  Square,
  Tag,
  Terminal,
  Upload,
  Workflow,
  X,
  type LucideIcon,
} from "lucide-react";

/**
 * Icon name → Lucide component. Names come from layout preset TOMLs
 * (`crates/nexus-theme/presets/*.layout.toml`). Most map to Lucide 1:1;
 * a handful are aliases where no exact Lucide match exists.
 */
const REGISTRY: Record<string, LucideIcon> = {
  "alert-triangle": AlertTriangle,
  "arrow-up": ArrowUp,
  "bookmark": Bookmark,
  "bookmark-plus": BookmarkPlus,
  "bot": Bot,
  "bug": Bug,
  "calendar": Calendar,
  "check-square": CheckSquare,
  "chevrons-up": ChevronsUp,
  "chevrons-up-down": ChevronsUpDown,
  "cloud-off": CloudOff,
  "command": Command,
  "database": Database,
  "edit": Edit,
  "external-link": ExternalLink,
  "files": Files,
  "file-text": FileText,
  "folder": Folder,
  "folder-plus": FolderPlus,
  "git-branch": GitBranch,
  "graph": Network,
  "help-circle": HelpCircle,
  "history": History,
  "layout-grid": LayoutGrid,
  "link-incoming": ArrowDownLeft,
  "link-outgoing": ArrowUpRight,
  "list-checks": ListChecks,
  "list-indent": Indent,
  "list-tree": ListTree,
  "message": MessageSquare,
  "minus": Minus,
  "play": Play,
  "play-circle": PlayCircle,
  "plug": Plug,
  "plus": Plus,
  "puzzle": Puzzle,
  "search": Search,
  "settings": Settings,
  "sparkles": Sparkles,
  "stop": Square,
  "tag": Tag,
  "terminal": Terminal,
  "upload": Upload,
  "workflow": Workflow,
  "x": X,
};

interface IconProps {
  name: string;
  size?: number;
  className?: string;
}

/**
 * Renders a Lucide icon by name. Unknown names fall back to a
 * single-letter glyph derived from the id, so a preset referencing
 * an unregistered icon never renders blank. Always `aria-hidden` —
 * callers should put the label on their button/link.
 */
export function Icon({ name, size = 16, className }: IconProps) {
  const LucideComponent = REGISTRY[name];
  if (LucideComponent) {
    return (
      <LucideComponent
        size={size}
        className={className}
        aria-hidden="true"
        focusable="false"
      />
    );
  }
  return (
    <span aria-hidden="true" className={className}>
      {fallbackGlyph(name)}
    </span>
  );
}

function fallbackGlyph(name: string): string {
  const trimmed = name.trim();
  if (!trimmed) return "◇";
  return trimmed[0].toUpperCase();
}
