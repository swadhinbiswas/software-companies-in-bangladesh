import "./globals.css"
import type { Metadata } from "next"
import { Inter, JetBrains_Mono } from "next/font/google"
import { ThemeProvider } from "@/components/theme-provider"
import { ModeToggle } from "@/components/mode-toggle"
import { SITE_URL } from "@/lib/site"

const inter = Inter({ subsets: ["latin"], variable: "--font-sans" })
const mono = JetBrains_Mono({ subsets: ["latin"], variable: "--font-mono" })

export const metadata: Metadata = {
  metadataBase: new URL(SITE_URL),
  title: {
    default: "BD Software Jobs — Live Bangladeshi Tech Job Dashboard",
    template: "%s · BD Software Jobs",
  },
  description:
    "Live job openings from 230+ Bangladeshi software companies. Track tech demand, salary ranges, remote roles and hiring trends — updated weekly from an open-data pipeline.",
  keywords: [
    "Bangladesh software jobs",
    "BD tech jobs",
    "Dhaka developer jobs",
    "Bangladeshi IT careers",
    "software engineer salary Bangladesh",
    "remote jobs Bangladesh",
    "tech job dashboard",
  ],
  authors: [{ name: "swadhinbiswas", url: "https://github.com/swadhinbiswas" }],
  creator: "swadhinbiswas",
  alternates: { canonical: "/" },
  openGraph: {
    type: "website",
    locale: "en_US",
    url: SITE_URL,
    siteName: "BD Software Jobs",
    title: "BD Software Jobs — Live Bangladeshi Tech Job Dashboard",
    description:
      "Live openings from 230+ Bangladeshi software companies with tech demand, salary and location analytics. Open data, refreshed weekly.",
  },
  twitter: {
    card: "summary_large_image",
    title: "BD Software Jobs — Live Bangladeshi Tech Job Dashboard",
    description:
      "Live openings from 230+ Bangladeshi software companies with tech demand, salary and location analytics.",
  },
  robots: {
    index: true,
    follow: true,
    googleBot: { index: true, follow: true, "max-image-preview": "large", "max-snippet": -1 },
  },
  category: "technology",
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
              </div>
              <nav className="hidden sm:flex items-center gap-1 text-sm">
                <a href="/" className="px-3 py-1.5 rounded-md hover:bg-accent transition-colors font-medium">Dashboard</a>
                <a href="https://huggingface.co/datasets/swadhinbiswas/bangladeshi-jobs" target="_blank" rel="noreferrer" className="px-3 py-1.5 rounded-md hover:bg-accent transition-colors">Dataset</a>
                <a href="https://github.com/swadhinbiswas/software-companies-in-bangladesh" target="_blank" rel="noreferrer" className="px-3 py-1.5 rounded-md bg-primary text-primary-foreground hover:bg-primary/90 transition-colors">GitHub</a>
              </nav>
              <div className="flex items-center gap-2">
                <ModeToggle />
              </div>
            </div>
          </header>
          <main className="max-w-7xl mx-auto px-4 py-6">{children}</main>
          <footer className="border-t mt-8 bg-muted/30">
            <div className="max-w-7xl mx-auto px-4 py-6 text-xs text-muted-foreground text-center">
              Software Companies in Bangladesh — {new Date().getFullYear()}
            </div>
          </footer>
        </ThemeProvider>
      </body>
    </html>
  )
}
