import { useEffect, useState } from "react";
import { useNavigate, useLocation } from "@tanstack/react-router";
import { hasSession } from "@/lib/auth";
import { fetchUiConfig } from "@/lib/config";
import { LoadingArea } from "./Loading";

export function AuthGuard({ children }: { children: React.ReactNode }) {
  const navigate = useNavigate();
  const location = useLocation();
  const pathname = location.pathname;
  const [isChecking, setIsChecking] = useState(true);
  const [requiresAuth, setRequiresAuth] = useState(false);
  const [isAuthed, setIsAuthed] = useState(false);

  useEffect(() => {
    async function checkAuth() {
      try {
        const config = await fetchUiConfig();
        setRequiresAuth(config.requires_auth);

        const sessionOk = config.requires_auth ? await hasSession() : true;
        setIsAuthed(sessionOk);

        if (config.requires_auth && !sessionOk && pathname !== "/login") {
          navigate({ to: "/login" });
          return;
        }

        if (sessionOk && pathname === "/login") {
          navigate({ to: "/dashboard" });
          return;
        }
      } catch (error) {
        console.error("Auth check failed:", error);
      } finally {
        setIsChecking(false);
      }
    }

    checkAuth();
  }, [navigate, pathname]);

  if (isChecking) {
    return <LoadingArea />;
  }

  if (pathname === "/login" || !requiresAuth || isAuthed) {
    return <>{children}</>;
  }

  return null;
}
