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
import type { NavItem, NavSubItem } from "@/lib/config";
import { logout } from "@/lib/auth";
import { fetchMetadata } from "@/api/metadata";

interface CollapsibleNavItemProps {
  route: NavItem;
  sidebarOpen: boolean;
  isOpen: boolean;
  onOpenChange: (open: boolean) => void;
  isActive: (path: string) => boolean;
  linkTo: string;
  children: React.ReactNode;
}

function CollapsibleNavItem({
  route,
  sidebarOpen,
  isOpen,
  onOpenChange,
  isActive,
  linkTo,
  children,
}: CollapsibleNavItemProps) {
  const Icon = route.icon;
  return (
    <Collapsible
      open={isOpen}
      onOpenChange={onOpenChange}
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
            <Link to={linkTo}>
              <Icon className="w-4 h-4" />
              <span>{route.label}</span>
            </Link>
          </SidebarMenuButton>
          {sidebarOpen && (
            <Button
              variant="ghost"
              size="icon"
              className="h-8 w-8 shrink-0"
              onClick={(e) => {
                e.preventDefault();
                e.stopPropagation();
                onOpenChange(!isOpen);
              }}
              aria-label={`Toggle ${route.label} menu`}
            >
              <ChevronRight className="h-4 w-4 transition-transform duration-200 group-data-[state=open]/collapsible:rotate-90" />
            </Button>
          )}
        </div>
        {sidebarOpen && (
          <CollapsibleContent>{children}</CollapsibleContent>
        )}
      </SidebarMenuItem>
    </Collapsible>
  );
}

function FooterActions({
  requiresAuth,
  onLogout,
}: {
  requiresAuth: boolean;
  onLogout: () => void;
}) {
  return (
    <>
      <Button
        variant="ghost"
        size="icon"
        title="GitHub"
        className="h-8 w-8"
        asChild
      >
        <a href="https://github.com/thalamiq/ferrum" target="_blank">
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
      {requiresAuth && (
        <Button
          variant="ghost"
          size="icon"
          title="Logout"
          className="h-8 w-8"
          onClick={onLogout}
        >
          <LogOut className="h-4 w-4" />
        </Button>
      )}
    </>
  );
}

export default function AppSidebar() {
  const { open, toggleSidebar } = useSidebar();
  const location = useLocation();
  const pathname = location.pathname;
  const navigate = useNavigate();

  const metadataQuery = useQuery({
    queryKey: queryKeys.metadata("full"),
    queryFn: () => fetchMetadata({ mode: "full" }),
  });

  const uiConfigQuery = useQuery({
    queryKey: ["ui-config"],
    queryFn: fetchUiConfig,
  });

  const [isResourcesOpen, setIsResourcesOpen] = React.useState(false);
  const [isSearchOpen, setIsSearchOpen] = React.useState(() =>
    pathname.startsWith("/search"),
  );

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

  const requiresAuth = uiConfigQuery.data?.requires_auth ?? false;

  const navItems = React.useMemo(() => {
    return Object.values(config.nav).filter((route) => {
      if (
        route.path === config.nav.settings.path &&
        uiConfigQuery.data?.runtime_config_enabled === false
      ) {
        return false;
      }
      return true;
    });
  }, [uiConfigQuery.data?.runtime_config_enabled]);

  const renderNavItem = (route: NavItem) => {
    const Icon = route.icon;

    // Resources — collapsible with dynamic resource type sub-items
    if (
      route.path === config.nav.resources.path &&
      sortedResourceTypes.length > 0
    ) {
      return (
        <CollapsibleNavItem
          key={route.path}
          route={route}
          sidebarOpen={open}
          isOpen={isResourcesOpen}
          onOpenChange={setIsResourcesOpen}
          isActive={isActive}
          linkTo={route.path}
        >
          <SidebarMenuSub className="max-h-[60vh] overflow-y-auto">
            {sortedResourceTypes.map((resource) => (
              <SidebarMenuSubItem key={resource.resourceType}>
                <SidebarMenuSubButton asChild>
                  <Link
                    to={config.nav.api.path}
                    search={{
                      method: "GET",
                      endpoint: resource.resourceType,
                    }}
                  >
                    <span className="flex-1 truncate">
                      {resource.resourceType}
                    </span>
                    <span className="text-xs text-muted-foreground ml-auto bg-muted rounded-md px-2 py-1">
                      {resource.currentTotal}
                    </span>
                  </Link>
                </SidebarMenuSubButton>
              </SidebarMenuSubItem>
            ))}
          </SidebarMenuSub>
        </CollapsibleNavItem>
      );
    }

    // Items with static sub-items (Search)
    if (route.subItems) {
      const subItems = Object.values(route.subItems) as NavSubItem[];
      return (
        <CollapsibleNavItem
          key={route.path}
          route={route}
          sidebarOpen={open}
          isOpen={isSearchOpen}
          onOpenChange={setIsSearchOpen}
          isActive={isActive}
          linkTo={subItems[0].path}
        >
          <SidebarMenuSub>
            {subItems.map((subItem) => (
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
            ))}
          </SidebarMenuSub>
        </CollapsibleNavItem>
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
            <Icon className="w-4 h-4" />
            <span>{route.label}</span>
          </Link>
        </SidebarMenuButton>
      </SidebarMenuItem>
    );
  };

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
          <SidebarGroupContent>
            <SidebarMenu>{navItems.map(renderNavItem)}</SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter className="border-t border-sidebar-border/50 overflow-hidden">
        {open ? (
          <div className="flex items-center justify-center gap-2 py-3 px-2 overflow-hidden">
            <a
              href="https://docs.ferrum.thalamiq.io"
              target="_blank"
              className="opacity-80 hover:opacity-100 transition-opacity shrink-0"
            >
              <img
                src="/ui/logos/ferrum.svg"
                alt="Ferrum"
                width={64}
                height={64}
              />
            </a>
            <div className="flex items-center gap-2 ml-auto">
              <FooterActions
                requiresAuth={requiresAuth}
                onLogout={handleLogout}
              />
            </div>
          </div>
        ) : (
          <div className="flex flex-col items-center gap-2 py-3 px-2">
            <FooterActions
              requiresAuth={requiresAuth}
              onLogout={handleLogout}
            />
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
    </Sidebar>
  );
}
