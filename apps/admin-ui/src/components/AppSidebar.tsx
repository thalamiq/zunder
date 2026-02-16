import React from "react";
import { Link, useLocation, useNavigate } from "@tanstack/react-router";
import {
  SidebarCloseIcon,
  SidebarOpenIcon,
  ChevronRight,
  LogOut,
} from "lucide-react";
import {
  Sidebar,
  SidebarContent,
  SidebarMenu,
  SidebarMenuItem,
  SidebarHeader,
  useSidebar,
  SidebarGroup,
  SidebarGroupLabel,
  SidebarGroupContent,
  SidebarMenuButton,
  SidebarMenuSub,
  SidebarMenuSubItem,
  SidebarMenuSubButton,
  SidebarFooter,
} from "@thalamiq/ui/components/sidebar";
import {
  Collapsible,
  CollapsibleContent,
} from "@thalamiq/ui/components/collapsible";
import { Button } from "@thalamiq/ui/components/button";
import { useQuery } from "@tanstack/react-query";
import { fetchResources } from "@/api/resources";
import { queryKeys } from "@/api/query-keys";
import ThemeToggle from "./ThemeToggle";
import { ConnectionIndicator } from "./ConnectionIndicator";
import { config, fetchUiConfig } from "@/lib/config";
import { logout } from "@/lib/auth";
import { fetchMetadata } from "@/api/metadata";

export default function AppSidebar() {
  const { open, toggleSidebar } = useSidebar();
  const location = useLocation();
  const pathname = location.pathname;
  const navigate = useNavigate();

  // Query
  const metadataQuery = useQuery({
    queryKey: queryKeys.metadata("full"),
    queryFn: () => fetchMetadata({ mode: "full" }),
  });

  // Check if auth is required
  const uiConfigQuery = useQuery({
    queryKey: ["ui-config"],
    queryFn: fetchUiConfig,
  });

  // State
  const [isResourcesOpen, setIsResourcesOpen] = React.useState(false);
  const [isSearchOpen, setIsSearchOpen] = React.useState(() =>
    pathname.startsWith("/search"),
  );

  // Auto-open search menu when on a search page
  React.useEffect(() => {
    if (pathname.startsWith("/search")) {
      setIsSearchOpen(true);
    }
  }, [pathname]);

  const handleLogout = async () => {
    await logout();
    navigate({ to: "/login" });
  };

  const isActive = (path: string) => pathname.startsWith(path);

  // Fetch resource types
  const resourcesQuery = useQuery({
    queryKey: queryKeys.resources,
    queryFn: fetchResources,
  });

  const resourceTypes = resourcesQuery.data?.resourceTypes || [];
  const sortedResourceTypes = React.useMemo(() => {
    return [...resourceTypes].sort((a, b) =>
      a.resourceType.localeCompare(b.resourceType),
    );
  }, [resourceTypes]);

  return (
    <Sidebar variant="sidebar" collapsible="icon">
      <SidebarContent className="gap-0">
        <SidebarHeader>
          <SidebarMenu>
            <SidebarMenuItem>
              {open ? (
                <div className="flex w-full items-center justify-between gap-2">
                  <Button
                    variant="ghost"
                    size="icon"
                    title="Toggle sidebar"
                    className="h-8 w-8 shrink-0"
                    onClick={toggleSidebar}
                  >
                    <SidebarCloseIcon />
                  </Button>
                  <Link
                    to="/"
                    className="text-sm font-bold truncate flex-1 min-w-0 hover:text-primary transition-colors"
                  >
                    {metadataQuery.data?.title}
                  </Link>
                  <ConnectionIndicator />
                </div>
              ) : (
                <div className="flex flex-col items-center gap-2 w-full">
                  <Button
                    variant="ghost"
                    size="icon"
                    title="Toggle sidebar"
                    className="h-8 w-8"
                    onClick={toggleSidebar}
                  >
                    <SidebarOpenIcon />
                  </Button>
                  <ConnectionIndicator />
                </div>
              )}
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarHeader>
        <SidebarGroup>
          {open && <SidebarGroupLabel>Navigation</SidebarGroupLabel>}
          <SidebarGroupContent>
            <SidebarMenu>
              {Object.values(config.nav)
                .filter((route: any) => {
                  if (
                    route.path === config.nav.settings.path &&
                    uiConfigQuery.data?.runtime_config_enabled === false
                  ) {
                    return false;
                  }
                  return true;
                })
                .map((route: any) => {
                  // Make Resources collapsible with resource types as sub-items
                  if (
                    route.path === config.nav.resources.path &&
                    sortedResourceTypes.length > 0
                  ) {
                    return (
                      <Collapsible
                        key={route.path}
                        open={isResourcesOpen}
                        onOpenChange={setIsResourcesOpen}
                        className="group/collapsible"
                      >
                        <SidebarMenuItem>
                          <div className="flex items-center w-full">
                            <SidebarMenuButton
                              asChild
                              tooltip={route.label}
                              isActive={isActive(route.path)}
                              className="flex-1"
                            >
                              <Link to={route.path}>
                                <route.icon className="w-4 h-4" />
                                <span>{route.label}</span>
                              </Link>
                            </SidebarMenuButton>
                            {open && (
                              <Button
                                variant="ghost"
                                size="icon"
                                className="h-8 w-8 shrink-0"
                                onClick={(e) => {
                                  e.preventDefault();
                                  e.stopPropagation();
                                  setIsResourcesOpen(!isResourcesOpen);
                                }}
                                aria-label="Toggle resources menu"
                              >
                                <ChevronRight className="h-4 w-4 transition-transform duration-200 group-data-[state=open]/collapsible:rotate-90" />
                              </Button>
                            )}
                          </div>
                          {open && (
                            <CollapsibleContent>
                              <SidebarMenuSub className="max-h-[60vh] overflow-y-auto">
                                {sortedResourceTypes.map((resource) => (
                                  <SidebarMenuSubItem
                                    key={resource.resourceType}
                                  >
                                    <SidebarMenuSubButton asChild>
                                      <Link
                                        to={config.nav.api.path}
                                        search={{
                                          method: "GET",
                                          endpoint: resource.resourceType,
                                        }}
                                      >
                                        <span className="flex-1 truncate">{resource.resourceType}</span>
                                        <span className="text-xs text-muted-foreground ml-auto bg-muted rounded-md px-2 py-1">
                                          {resource.currentTotal}
                                        </span>
                                      </Link>
                                    </SidebarMenuSubButton>
                                  </SidebarMenuSubItem>
                                ))}
                              </SidebarMenuSub>
                            </CollapsibleContent>
                          )}
                        </SidebarMenuItem>
                      </Collapsible>
                    );
                  }
                  // Make Search collapsible with sub-items
                  if (route.subItems) {
                    return (
                      <Collapsible
                        key={route.path}
                        open={isSearchOpen}
                        onOpenChange={setIsSearchOpen}
                        className="group/collapsible"
                      >
                        <SidebarMenuItem>
                          <div className="flex items-center w-full">
                            <SidebarMenuButton
                              asChild
                              tooltip={route.label}
                              className="flex-1"
                            >
                              <Link
                                to={route.subItems.searchParameters.path}
                              >
                                <route.icon className="w-4 h-4" />
                                <span>{route.label}</span>
                              </Link>
                            </SidebarMenuButton>
                            {open && (
                              <Button
                                variant="ghost"
                                size="icon"
                                className="h-8 w-8 shrink-0"
                                onClick={(e) => {
                                  e.preventDefault();
                                  e.stopPropagation();
                                  setIsSearchOpen(!isSearchOpen);
                                }}
                                aria-label="Toggle search menu"
                              >
                                <ChevronRight className="h-4 w-4 transition-transform duration-200 group-data-[state=open]/collapsible:rotate-90" />
                              </Button>
                            )}
                          </div>
                          {open && (
                            <CollapsibleContent>
                              <SidebarMenuSub>
                                {Object.values(route.subItems).map(
                                  (subItem: any) => (
                                    <SidebarMenuSubItem key={subItem.path}>
                                      <SidebarMenuSubButton
                                        asChild
                                        isActive={isActive(subItem.path)}
                                      >
                                        <Link to={subItem.path}>
                                          <span>{subItem.label}</span>
                                        </Link>
                                      </SidebarMenuSubButton>
                                    </SidebarMenuSubItem>
                                  ),
                                )}
                              </SidebarMenuSub>
                            </CollapsibleContent>
                          )}
                        </SidebarMenuItem>
                      </Collapsible>
                    );
                  }
                  // Regular menu items
                  return (
                    <SidebarMenuItem key={route.path}>
                      <SidebarMenuButton
                        asChild
                        isActive={isActive(route.path)}
                        tooltip={route.label}
                      >
                        <Link to={route.path}>
                          <route.icon className="w-4 h-4" />
                          <span>{route.label}</span>
                        </Link>
                      </SidebarMenuButton>
                    </SidebarMenuItem>
                  );
                })}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter className="border-t border-sidebar-border/50 overflow-hidden">
        {open ? (
          <div className="flex flex-col gap-3 py-3 px-2 overflow-hidden">
            <div className="flex items-center justify-center gap-2 overflow-hidden">
              <a
                href="https://thalamiq.io"
                target="_blank"
                className="opacity-80 hover:opacity-100 transition-opacity overflow-hidden shrink-0"
              >
                <img
                  className="overflow-hidden shrink-0"
                  src="/ui/logos/ferrum.svg"
                  alt="Ferrum"
                  width={64}
                  height={64}
                />
              </a>
              <div className="flex items-center gap-2 ml-auto">
                <Button
                  variant="ghost"
                  size="icon"
                  title="GitHub"
                  className="h-8 w-8"
                  asChild
                >
                  <a
                    href="https://github.com/thalamiq/ferrum"
                    target="_blank"
                  >
                    <img
                      src="/ui/icons/github.svg"
                      alt="GitHub"
                      width={16}
                      height={16}
                      className="dark:invert"
                    />
                  </a>
                </Button>
                <ThemeToggle />
                {uiConfigQuery.data?.requires_auth && (
                  <Button
                    variant="ghost"
                    size="icon"
                    title="Logout"
                    className="h-8 w-8"
                    onClick={handleLogout}
                  >
                    <LogOut className="h-4 w-4" />
                  </Button>
                )}
              </div>
            </div>
          </div>
        ) : (
          <div className="flex flex-col items-center gap-2 py-3 px-2">
            <Button
              variant="ghost"
              size="icon"
              title="GitHub"
              className="h-8 w-8"
              asChild
            >
              <a
                href="https://github.com/thalamiq/ferrum"
                target="_blank"
              >
                <img
                  src="/ui/icons/github.svg"
                  alt="GitHub"
                  width={20}
                  height={20}
                  className="dark:invert"
                />
              </a>
            </Button>
            <ThemeToggle />
            {uiConfigQuery.data?.requires_auth && (
              <Button
                variant="ghost"
                size="icon"
                title="Logout"
                className="h-8 w-8"
                onClick={handleLogout}
              >
                <LogOut className="h-4 w-4" />
              </Button>
            )}
            <a
              href="https://thalamiq.io"
              target="_blank"
              className="opacity-60 hover:opacity-100 transition-opacity mt-4"
            >
              <img
                src="/ui/logos/fe.svg"
                alt="Ferrum"
                width={20}
                height={20}
              />
            </a>
          </div>
        )}
      </SidebarFooter>
    </Sidebar >
  );
}
