import "./globals.css"
import type { Metadata } from "next"
import { Inter, JetBrains_Mono } from "next/font/google"
import { ThemeProvider } from "@/components/theme-provider"
import { ModeToggle } from "@/components/mode-toggle"

const inter = Inter({ subsets: ["latin"], variable: "--font-sans" })
const mono = JetBrains_Mono({ subsets: ["latin"], variable: "--font-mono" })

export const metadata: Metadata = {
  title: "BD Software Jobs — DuckDB × HF × shadcn Dashboard",
  description: "Live jobs from 230+ Bangladeshi software companies. Tech demand, salary, location analytics. Parquet on Hugging Face, edge-cached on Vercel/Cloudflare.",
  openGraph: { title: "BD Software Jobs Dashboard", description: "230+ companies, 300+ open jobs — tech demand analytics." },
}

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <body className={`${inter.variable} ${mono.variable} font-sans min-h-screen bg-background antialiased`}>
        <ThemeProvider attribute="class" defaultTheme="system" enableSystem disableTransitionOnChange>
          <header className="sticky top-0 z-40 w-full border-b bg-background/80 backdrop-blur-xl supports-[backdrop-filter]:bg-background/60">
            <div className="flex h-14 items-center px-4 max-w-7xl mx-auto justify-between gap-4">
              <div className="flex items-center gap-2 min-w-0">
                <div className="h-8 w-8 rounded-lg bg-primary text-primary-foreground grid place-items-center text-sm font-bold">🇧🇩</div>
                <span className="font-semibold tracking-tight">BD Jobs</span>
                <span className="hidden md:inline-flex items-center rounded-full bg-secondary px-2.5 py-0.5 text-xs font-medium">shadcn × DuckDB × HF</span>
              </div>
              <nav className="hidden sm:flex items-center gap-1 text-sm">
                <a href="/" className="px-3 py-1.5 rounded-md hover:bg-accent transition-colors font-medium">Dashboard</a>
                <a href="#jobs" className="px-3 py-1.5 rounded-md hover:bg-accent transition-colors">Jobs</a>
                <a href="https://huggingface.co/datasets" target="_blank" className="px-3 py-1.5 rounded-md hover:bg-accent transition-colors">HF Data</a>
                <a href="https://github.com/nurmohammed840/software-companies-in-bangladesh" target="_blank" className="px-3 py-1.5 rounded-md bg-primary text-primary-foreground hover:bg-primary/90 transition-colors">GitHub</a>
              </nav>
              <div className="flex items-center gap-2">
                <ModeToggle />
              </div>
            </div>
          </header>
          <main className="max-w-7xl mx-auto px-4 py-6">{children}</main>
          <footer className="border-t mt-8 bg-muted/30">
            <div className="max-w-7xl mx-auto px-4 py-6 flex flex-col sm:flex-row items-center justify-between gap-2 text-xs text-muted-foreground">
              <span>Rust crawler → DuckDB warehouse → Parquet on HF → Next.js 14 shadcn · ISR 60s · Edge cached</span>
              <span className="flex items-center gap-2">Vercel <span className="opacity-30">·</span> Cloudflare Workers <span className="opacity-30">·</span> Hugging Face</span>
            </div>
          </footer>
        </ThemeProvider>
      </body>
    </html>
  )
}
