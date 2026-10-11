import {
  Code2,
  Download,
  FolderSearch,
  History,
  Library,
  MessageSquare,
  PenLine,
  Settings,
  Shield,
  Sparkles,
  type LucideIcon,
} from "lucide-react";
import { Navigate, createHashRouter, type RouteObject } from "react-router-dom";

import { AppShell } from "@/components/shell/AppShell";
import { ChatView } from "@/features/chat/ChatView";
import { AssistantsPage } from "@/features/assistants/AssistantsPage";
import { AnalyzerPage } from "@/features/analyzer/AnalyzerPage";
import { ReportPage } from "@/features/analyzer/ReportPage";
import { ChangesPage } from "@/features/changes/ChangesPage";
import { LibraryPage } from "@/features/library/LibraryPage";
import { ModelsPage } from "@/features/models/ModelsPage";
import { PermissionsPage } from "@/features/permissions/PermissionsPage";
import { IS_WEB } from "@/lib/target";
import { SharingSettings } from "@/features/share/SharingSettings";
import { DesktopOnly } from "@/web/DesktopOnly";
import { EditorPage } from "@/features/editor/EditorPage";
import { RenamePage } from "@/features/rename/RenamePage";
import { SettingsPage } from "@/features/settings/SettingsPage";
import { PlaceholderPage } from "@/features/placeholders/PlaceholderPage";

export interface NavSection {
  path: string;
  label: string;
  icon: LucideIcon;
  tagline: string;
  /** Shortcut digit (⌘1…⌘8) for the main sections. */
  digit?: number;
}

/** The nine main sections, in sidebar order, plus Settings. */
export const NAV: NavSection[] = [
  { path: "/", label: "Chat", icon: MessageSquare, tagline: "Talk to a private assistant running on your computer.", digit: 1 },
  { path: "/assistants", label: "Assistants", icon: Sparkles, tagline: "Specialists for files, code, and writing.", digit: 2 },
  { path: "/models", label: "Models", icon: Download, tagline: "Download, switch, and manage models.", digit: 3 },
  { path: "/analyzer", label: "Analyzer", icon: FolderSearch, tagline: "Scan a folder or file and get a report.", digit: 4 },
  { path: "/rename", label: "Rename Forge", icon: PenLine, tagline: "Rename symbols and files across a project.", digit: 5 },
  { path: "/editor", label: "Editor", icon: Code2, tagline: "A code editor with AI at hand.", digit: 6 },
  { path: "/changes", label: "Changes", icon: History, tagline: "Review, apply, and undo planned changes.", digit: 7 },
  { path: "/permissions", label: "Permissions", icon: Shield, tagline: "Choose which folders NanoNexus can use.", digit: 8 },
  { path: "/library", label: "Library", icon: Library, tagline: "Offline reference books the assistant can search and cite.", digit: 9 },
];

export const SETTINGS_SECTION: NavSection = {
  path: "/settings",
  label: "Settings",
  icon: Settings,
  tagline: "Appearance, runtime, models, and privacy.",
};

const DESKTOP_ONLY = new Set(["/rename", "/editor", "/changes", "/permissions", "/models", "/library"]);

function pageFor(s: NavSection) {
  if (IS_WEB && DESKTOP_ONLY.has(s.path)) return <DesktopOnly title={s.label} />;
  if (s.path === "/") return <ChatView />;
  if (s.path === "/assistants") return <AssistantsPage />;
  if (s.path === "/models") return <ModelsPage />;
  if (s.path === "/analyzer") return <AnalyzerPage />;
  if (s.path === "/editor") return <EditorPage />;
  if (s.path === "/changes") return <ChangesPage />;
  if (s.path === "/permissions") return <PermissionsPage />;
  if (s.path === "/library") return <LibraryPage />;
  if (s.path === "/rename") return <RenamePage />;
  if (s.path === "/settings") return <SettingsPage />;
  return <PlaceholderPage section={s} />;
}

export const routes: RouteObject[] = [
  {
    path: "/",
    element: <AppShell />,
    children: [{ path: "c/:conversationId", element: <ChatView /> }, { path: "analyzer/:id", element: <ReportPage /> }, { path: "settings/sharing", element: IS_WEB ? <DesktopOnly title="Sharing settings" /> : <SharingSettings /> }, ...[...NAV, SETTINGS_SECTION].map((s) => ({
      ...(s.path === "/" ? { index: true as const } : { path: s.path.slice(1) }),
      element: pageFor(s),
    }))],
  },
];

// Unknown addresses go home instead of showing the router's raw error page.
routes.push({ path: "*", element: <Navigate to="/" replace /> });

export const createAppRouter = () => createHashRouter(routes);
