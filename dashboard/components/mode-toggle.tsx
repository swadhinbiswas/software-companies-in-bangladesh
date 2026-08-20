"use client"
import { Moon, Sun, Monitor } from "lucide-react"
import { useTheme } from "next-themes"
import { Button } from "@/components/ui/button"

export function ModeToggle() {
  const { theme, setTheme } = useTheme()
  return (
    <div className="flex items-center rounded-full border bg-card p-1 gap-1">
      <Button variant={theme === "light" ? "secondary" : "ghost"} size="icon" className="h-7 w-7 rounded-full" onClick={() => setTheme("light")} aria-label="Light">
        <Sun className="h-3.5 w-3.5" />
      </Button>
      <Button variant={theme === "dark" ? "secondary" : "ghost"} size="icon" className="h-7 w-7 rounded-full" onClick={() => setTheme("dark")} aria-label="Dark">
        <Moon className="h-3.5 w-3.5" />
      </Button>
      <Button variant={theme === "system" ? "secondary" : "ghost"} size="icon" className="h-7 w-7 rounded-full" onClick={() => setTheme("system")} aria-label="System">
        <Monitor className="h-3.5 w-3.5" />
      </Button>
    </div>
  )
}
