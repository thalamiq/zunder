"use client";

import { useTheme } from "next-themes";
import { Button } from "@thalamiq/ui/components/button";
import { Sun, Moon, Monitor } from "lucide-react";
import { useMounted } from "@/hooks/useMounted";
import CustomTooltip from "./CustomTooltip";

const ThemeToggle = () => {
  const { theme, setTheme } = useTheme();
  const mounted = useMounted();

  const handleThemeToggle = () => {
    // Cycle through: light -> dark -> system -> light
    if (theme === "light") {
      setTheme("dark");
    } else if (theme === "dark") {
      setTheme("system");
    } else {
      setTheme("light");
    }
  };

  const getTitle = () => {
    if (!mounted) return "Toggle theme";
    if (theme === "light") return "Switch to dark theme";
    if (theme === "dark") return "Switch to system theme";
    return "Switch to light theme";
  };

  const getIcon = () => {
    if (!mounted) {
      // Render a placeholder during SSR to avoid hydration mismatch
      return (
        <Sun className="h-4 w-4 transition-transform duration-200 opacity-0" />
      );
    }

    if (theme === "dark") {
      return <Sun className="h-4 w-4 transition-transform duration-200" />;
    } else if (theme === "system") {
      return <Monitor className="h-4 w-4 transition-transform duration-200" />;
    } else {
      return <Moon className="h-4 w-4 transition-transform duration-200" />;
    }
  };

  return (
    <CustomTooltip content={getTitle()}>
      <Button
        variant="ghost"
        size="icon"
        className="h-8 w-8"
        onClick={handleThemeToggle}
      >
        {getIcon()}
      </Button>
    </CustomTooltip>
  );
};

export default ThemeToggle;
