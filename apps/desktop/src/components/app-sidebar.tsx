import { useTranslation } from "react-i18next";

import { useApp, type Page } from "@/components/app-context";
import { AppWindow, BarChart3, Boxes, History, LayoutDashboard, Plug, Power, Server, Settings, WandSparkles } from "@/components/icons";
import { Spinner } from "@/components/ui/spinner";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar";

/** `label` is a translation key under `nav`. */
type Item = { page: Page; label: string; icon: typeof LayoutDashboard };

const MONITOR: Item[] = [
  { page: "dashboard", label: "nav.dashboard", icon: LayoutDashboard },
  { page: "usage", label: "nav.usage", icon: BarChart3 },
  { page: "history", label: "nav.history", icon: History },
];

const CONFIGURE: Item[] = [
  { page: "providers", label: "nav.providers", icon: Server },
  { page: "models", label: "nav.models", icon: Boxes },
  { page: "apps", label: "nav.apps", icon: AppWindow },
  { page: "mcp", label: "nav.mcp", icon: Plug },
  { page: "skills", label: "nav.skills", icon: WandSparkles },
  { page: "settings", label: "nav.settings", icon: Settings },
];

export function AppSidebar() {
  const { t } = useTranslation();
  const { page, navigate, status, gateway, gatewayBusy } = useApp();
  const running = status?.gatewayRunning ?? false;

  const group = (label: string, items: Item[], className?: string) => (
    <SidebarGroup className={className}>
      <SidebarGroupLabel>{label}</SidebarGroupLabel>
      <SidebarGroupContent>
        <SidebarMenu>
          {items.map((item) => (
            <SidebarMenuItem key={item.page}>
              <SidebarMenuButton isActive={page === item.page} onClick={() => navigate(item.page)} tooltip={t(item.label)}>
                <item.icon />
                <span>{t(item.label)}</span>
              </SidebarMenuButton>
            </SidebarMenuItem>
          ))}
        </SidebarMenu>
      </SidebarGroupContent>
    </SidebarGroup>
  );

  return (
    <Sidebar collapsible="offcanvas" variant="inset">
      <SidebarHeader className="pb-0">
        <div className="flex h-12 items-center px-3 text-lg font-semibold tracking-tight">{t("nav.brand")}</div>
      </SidebarHeader>
      <SidebarContent>
        {group(t("nav.monitor"), MONITOR, "pt-0")}
        {group(t("nav.configure"), CONFIGURE)}
      </SidebarContent>
      <SidebarFooter>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              size="lg"
              tooltip={running ? t("nav.stopGateway") : t("nav.startGateway")}
              disabled={gatewayBusy || !status?.configExists}
              onClick={() => void gateway(running ? "stop" : "start")}
            >
              <span className={`flex size-8 shrink-0 items-center justify-center rounded-full shadow-sm ${running ? "bg-emerald-500 text-white" : "bg-foreground text-background"}`}>
                {gatewayBusy ? <Spinner /> : <Power />}
              </span>
              <div className="grid min-w-0 leading-tight">
                <span className="truncate text-sm font-medium">{running ? t("nav.running") : t("nav.stopped")}</span>
                <span className="truncate font-mono text-xs text-muted-foreground">{status?.gatewayAddress ?? "…"}</span>
              </div>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
    </Sidebar>
  );
}
