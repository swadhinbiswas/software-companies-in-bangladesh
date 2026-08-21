import { getStats, getTechDemand, getRecentJobs, getJobsPerCompany, getLocationHeatmap, getEmploymentBreakdown, getSalaryStats, getCompanies, getFallbackStats } from "@/lib/data"
import JobsTable from "@/components/jobs-table"
import TechBar from "@/components/tech-bar"
import { KpiCards } from "@/components/kpi-cards"
import { DataPlatform } from "@/components/data-platform"
import CompanyGrid from "@/components/company-grid"
import { MomentumChart, SalaryHistogram } from "@/components/charts"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { Badge } from "@/components/ui/badge"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import { SITE_URL } from "@/lib/site"

export const revalidate = 60

function deriveExtras(jobs: any[], salary: any[], tech: any[]) {
  const weekMs = 7 * 24 * 3600 * 1000
  const now = Date.now()
  const withSalary = jobs.filter((j) => j.salary_max > 0)
  const remote = jobs.filter((j) => j.location_type === "Remote").length
  return {
    medianSalary: salary.length ? Math.max(...salary.map((s: any) => s.median_max || 0)) || null : null,
    remoteShare: jobs.length ? Math.round((remote / jobs.length) * 100) : null,
    newThisWeek: jobs.filter((j) => j.last_seen_at && now - new Date(j.last_seen_at).getTime() < weekMs).length,
    skillsCount: tech.length,
  }
}

// Postings first-seen per week (last 8 weeks) for the momentum chart.
function weeklyMomentum(jobs: any[]) {
  const weeks: { week: string; jobs: number }[] = []
  const now = new Date()
  for (let i = 7; i >= 0; i--) {
    const d = new Date(now.getTime() - i * 7 * 24 * 3600 * 1000)
    weeks.push({ week: `${d.getMonth() + 1}/${d.getDate()}`, jobs: 0 })
  }
  const start = now.getTime() - 8 * 7 * 24 * 3600 * 1000
  for (const j of jobs) {
    if (!j.last_seen_at) continue
    const t = new Date(j.last_seen_at).getTime()
    if (t < start) continue
    const idx = Math.min(7, Math.floor((t - start) / (7 * 24 * 3600 * 1000)))
    weeks[idx].jobs++
  }
  return weeks
}

// Salary distribution in 25k BDT buckets from explicit salary_max figures.
function salaryBuckets(jobs: any[]) {
  const edges = [0, 25, 50, 75, 100, 150, 200, 999]
  const labels = ["<25k", "25–50k", "50–75k", "75–100k", "100–150k", "150–200k", "200k+"]
  const counts = new Array(labels.length).fill(0)
  for (const j of jobs) {
    if (!j.salary_max || j.salary_currency !== "BDT") continue
    const k = j.salary_max / 1000
    const idx = edges.findIndex((e, i) => i < edges.length - 1 && k >= e && k < edges[i + 1])
    if (idx >= 0) counts[idx]++
  }
  return labels.map((range, i) => ({ range, jobs: counts[i] }))
}

export default async function Page() {
  const [stats, tech, jobs, perCompany, loc, emp, salary, companies] = await Promise.all([
    getStats().catch(() => getFallbackStats()),
    getTechDemand().catch(() => []),
    getRecentJobs().catch(() => []),
    getJobsPerCompany().catch(() => []),
    getLocationHeatmap().catch(() => []),
    getEmploymentBreakdown().catch(() => []),
    getSalaryStats().catch(() => []),
    getCompanies().catch(() => []),
  ])
  const extras = deriveExtras(jobs, salary, tech)
  // Merge career URLs from the registry into the hiring-velocity rows.
  const companyRows = perCompany.map((c: any) => ({
    ...c,
    job_url: companies.find((r: any) => r.name === c.name)?.job_url ?? null,
  }))

  const jsonLd = {
    "@context": "https://schema.org",
    "@graph": [
      {
        "@type": "WebSite",
        name: "BD Software Jobs",
        url: SITE_URL,
        description: "Live job dashboard for Bangladeshi software companies.",
      },
      {
        "@type": "Dataset",
        name: "Bangladeshi Tech Jobs — Open Dataset",
        description: `Weekly-refreshed collection of ${stats.open_jobs} open job postings from Bangladeshi software companies, with tech demand, salary and location analytics.`,
        url: "https://huggingface.co/datasets/swadhinbiswas/bangladeshi-jobs",
        license: "https://creativecommons.org/licenses/by/4.0/",
        creator: { "@type": "Person", name: "swadhinbiswas" },
        distribution: [
          {
            "@type": "DataDownload",
            encodingFormat: "application/json",
            contentUrl:
              "https://huggingface.co/datasets/swadhinbiswas/bangladeshi-jobs/resolve/main/gold/recent_jobs.json",
          },
          {
            "@type": "DataDownload",
            encodingFormat: "application/x-parquet",
            contentUrl:
              "https://huggingface.co/datasets/swadhinbiswas/bangladeshi-jobs/resolve/main/parquet/fact_job.parquet",
          },
        ],
        isAccessibleForFree: true,
        keywords: ["jobs", "Bangladesh", "tech", "software", "hiring", "salary"],
      },
    ],
  }

  return (
    <div className="space-y-6">
      <script type="application/ld+json" dangerouslySetInnerHTML={{ __html: JSON.stringify(jsonLd) }} />

      <div className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Bangladeshi Software Jobs Dashboard</h1>
          <p className="text-sm text-muted-foreground mt-1">
            Live openings from {stats.total_companies}+ companies · tech demand, salaries & remote roles ·
            open data refreshed weekly
          </p>
        </div>
        <Badge variant="outline" className="gap-1.5 text-emerald-600 border-emerald-600/30 bg-emerald-500/10">
          <span className="h-1.5 w-1.5 rounded-full bg-emerald-500 animate-pulse" />
          {stats.open_jobs} open jobs · updated weekly
        </Badge>
      </div>

      <KpiCards stats={stats} extra={extras} />

      <Tabs defaultValue="overview" className="w-full">
        <TabsList className="grid w-full grid-cols-4 lg:w-[560px]">
          <TabsTrigger value="overview">Overview</TabsTrigger>
          <TabsTrigger value="jobs">Jobs ({jobs.length})</TabsTrigger>
          <TabsTrigger value="companies">Companies</TabsTrigger>
          <TabsTrigger value="data">Open Data</TabsTrigger>
        </TabsList>

        <TabsContent value="overview" className="space-y-4 mt-4">
          <div className="grid lg:grid-cols-2 gap-4">
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm">Hiring momentum</CardTitle>
                <CardDescription>Postings first seen per week (last 8 weeks)</CardDescription>
              </CardHeader>
              <CardContent><MomentumChart data={weeklyMomentum(jobs)} /></CardContent>
            </Card>
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm">Salary distribution</CardTitle>
                <CardDescription>Explicit monthly salary ceilings (BDT)</CardDescription>
              </CardHeader>
              <CardContent><SalaryHistogram data={salaryBuckets(jobs)} /></CardContent>
            </Card>
          </div>

          <div className="grid lg:grid-cols-3 gap-4">
            <Card className="lg:col-span-2">
              <CardHeader>
                <CardTitle>Tech demand — top tags</CardTitle>
                <CardDescription>{tech.length} distinct skills extracted from job descriptions</CardDescription>
              </CardHeader>
              <CardContent><TechBar data={tech.slice(0, 18)} /></CardContent>
            </Card>

            <div className="space-y-4">
              <Card>
                <CardHeader className="pb-2"><CardTitle className="text-sm">Work modes</CardTitle></CardHeader>
                <CardContent className="space-y-3">
                  <div className="space-y-1.5">
                    {emp.map((e: any) => (
                      <div key={e.employment_type || "unknown"} className="flex justify-between text-sm">
                        <span className="text-muted-foreground">{e.employment_type || "Unknown"}</span>
                        <span className="font-mono font-medium">{e.jobs}</span>
                      </div>
                    ))}
                  </div>
                  <div className="pt-2 border-t space-y-1.5 max-h-40 overflow-auto pr-1">
                    {loc.slice(0, 12).map((r: any, i: number) => (
                      <div key={i} className="flex justify-between text-xs">
                        <span className="truncate mr-2">{r.location_text || r.location_type}</span>
                        <span className="font-mono">{r.jobs}</span>
                      </div>
                    ))}
                  </div>
                </CardContent>
              </Card>

              <Card>
                <CardHeader className="pb-2"><CardTitle className="text-sm">Salary ranges (BDT)</CardTitle><CardDescription>Explicit figures only</CardDescription></CardHeader>
                <CardContent>
                  {salary.length ? (
                    <div className="space-y-2 text-sm">
                      {salary.slice(0, 6).map((s: any, i: number) => (
                        <div key={i} className="flex justify-between items-center">
                          <span className="text-muted-foreground">{s.salary_currency} · median</span>
                          <span className="font-mono">{Math.round(s.median_min / 1000)}k – {Math.round(s.median_max / 1000)}k</span>
                        </div>
                      ))}
                      <p className="text-xs text-muted-foreground pt-1 border-t">n = {salary[0]?.n ?? 0} postings with explicit salary</p>
                    </div>
                  ) : (
                    <p className="text-sm text-muted-foreground">No salary data yet</p>
                  )}
                </CardContent>
              </Card>

              <Card>
                <CardHeader className="pb-2"><CardTitle className="text-sm">Top hiring</CardTitle><CardDescription>By open jobs</CardDescription></CardHeader>
                <CardContent className="p-0">
                  <Table>
                    <TableHeader><TableRow><TableHead>Company</TableHead><TableHead className="text-right">Open</TableHead></TableRow></TableHeader>
                    <TableBody>
                      {perCompany.slice(0, 8).map((c: any) => (
                        <TableRow key={c.name}><TableCell className="font-medium py-1.5">{c.name}</TableCell><TableCell className="text-right font-mono">{c.open_jobs ?? c.open_current ?? "—"}</TableCell></TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </CardContent>
              </Card>
            </div>
          </div>

          <Card>
            <CardHeader>
              <CardTitle>Hiring leaders</CardTitle>
              <CardDescription>Companies with active job openings</CardDescription>
            </CardHeader>
            <CardContent className="p-0">
              <div className="overflow-auto">
                <Table>
                  <TableHeader><TableRow><TableHead>#</TableHead><TableHead>Company</TableHead><TableHead>Open</TableHead><TableHead className="min-w-[320px]">Stack</TableHead></TableRow></TableHeader>
                  <TableBody>
                    {perCompany.slice(0, 20).map((c: any, i: number) => (
                      <TableRow key={c.name}>
                        <TableCell className="text-muted-foreground">{i + 1}</TableCell>
                        <TableCell className="font-medium">{c.name}</TableCell>
                        <TableCell><Badge variant="secondary">{c.open_jobs ?? c.open_current}</Badge></TableCell>
                        <TableCell className="text-xs">
                          <div className="flex flex-wrap gap-1 max-w-[520px]">
                            {(Array.isArray(c.tech) ? c.tech : []).slice(0, 8).map((t: string) => <Badge key={t} variant="outline" className="text-[10px]">{t}</Badge>)}
                            {Array.isArray(c.tech) && c.tech.length > 8 && <span className="text-muted-foreground">+{c.tech.length - 8}</span>}
                          </div>
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="jobs" className="mt-4">
          <JobsTable jobs={jobs} />
        </TabsContent>

        <TabsContent value="companies" className="mt-4">
          <CompanyGrid rows={companyRows} />
        </TabsContent>

        <TabsContent value="data">
          <DataPlatform />
        </TabsContent>
      </Tabs>
    </div>
  )
}
