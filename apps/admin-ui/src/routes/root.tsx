import { createRootRoute, Outlet, useLocation } from "@tanstack/react-router";
import { SidebarInset, SidebarProvider } from "@thalamiq/ui/components/sidebar";
import AppSidebar from "@/components/AppSidebar";
import { ConnectionGuard } from "@/components/ConnectionGuard";
import { AuthGuard } from "@/components/AuthGuard";
import { Toaster } from "sonner";

function RootComponent() {
  const location = useLocation();
  const isLoginPage = location.pathname === "/login";

  if (isLoginPage) {
    return (
      <AuthGuard>
        <Outlet />
        <Toaster />
      </AuthGuard>
    );
  }

  const defaultOpen =
    typeof window !== "undefined"
      ? localStorage.getItem("sidebar_state") === "true"
      : true;

  return (
    <AuthGuard>
      <SidebarProvider defaultOpen={defaultOpen}>
        <AppSidebar />
        <main className="flex-1 min-h-0 overflow-y-auto bg-background">
          <ConnectionGuard>
            <Outlet />
          </ConnectionGuard>
        </main>
        <Toaster />
      </SidebarProvider>
    </AuthGuard>
  );
}

export const rootRoute = createRootRoute({
  component: RootComponent,
});
