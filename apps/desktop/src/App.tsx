import { useTranslation } from "react-i18next";

import { AppProvider, useApp, type Page } from "@/components/app-context";
import { AppSidebar } from "@/components/app-sidebar";
import { ErrorBoundary } from "@/components/error-boundary";
import { ThemeProvider } from "@/components/theme";
import { ScrollArea } from "@/components/ui/scroll-area";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";
import { Toaster } from "@/components/ui/toaster";
import { TooltipProvider } from "@/components/ui/tooltip";
import { AppsPage } from "@/pages/apps";
import { DashboardPage } from "@/pages/dashboard";
import { HistoryPage } from "@/pages/history";
import { McpPage } from "@/pages/mcp";
import { ModelsPage } from "@/pages/models";
import { ProvidersPage } from "@/pages/providers";
import { SettingsPage } from "@/pages/settings";
import { SkillsPage } from "@/pages/skills";
import { UsagePage } from "@/pages/usage";

const PAGES: Record<Page, { view: () => React.JSX.Element }> = {
  dashboard: { view: DashboardPage },
  usage: { view: UsagePage },
  history: { view: HistoryPage },
  providers: { view: ProvidersPage },
  models: { view: ModelsPage },
  apps: { view: AppsPage },
  mcp: { view: McpPage },
  skills: { view: SkillsPage },
  settings: { view: SettingsPage },
};

function Shell() {
  const { t } = useTranslation();
  const { page } = useApp();
  const { view: View } = PAGES[page];
  return (
    <SidebarProvider className="h-svh">
      <AppSidebar />
      <SidebarInset className="min-h-0 overflow-hidden">
        {/* The scrollbar is inset so it never runs into the inset card's rounded corners. */}
        <ScrollArea type="always" className="min-h-0 flex-1">
          {/* `--page-height` is what a page gets without scrolling: the viewport minus the inset margins (m-2 × 2) and this padding (py-5 × 2). */}
          <div className="px-6 py-5 lg:px-8 [--page-height:calc(100svh-3.5rem)]">
            <div
              key={page}
              className="animate-in fade-in-0 slide-in-from-bottom-2 fill-mode-both duration-250 ease-out motion-reduce:animate-none"
            >
              <ErrorBoundary title={t("errors.pageFailed")}>
                <View />
              </ErrorBoundary>
            </div>
          </div>
        </ScrollArea>
      </SidebarInset>
    </SidebarProvider>
  );
}

export default function App() {
  return (
    <ThemeProvider>
      <TooltipProvider>
        <AppProvider>
          <Shell />
          <Toaster />
        </AppProvider>
      </TooltipProvider>
    </ThemeProvider>
  );
}
