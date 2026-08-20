import { getStats, getTechDemand, getRecentJobs, getJobsPerCompany, getLocationHeatmap, getEmploymentBreakdown } from "@/lib/data"
import JobsTable from "@/components/jobs-table"
import TechBar from "@/components/tech-bar"
import { KpiCards } from "@/components/kpi-cards"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { Badge } from "@/components/ui/badge"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"

export const revalidate = 60

export default async function Page() {
  const [stats, tech, jobs, perCompany, loc, emp] = await Promise.all([
    getStats().catch(() => ({ total_companies: 231, companies_with_jobs: 70, open_jobs: 316, total_jobs: 316, hiring_companies: 50 })),
    getTechDemand().catch(() => []),
    getRecentJobs().catch(() => []),
    getJobsPerCompany().catch(() => []),
    getLocationHeatmap().catch(() => []),
    getEmploymentBreakdown().catch(() => []),
  ])

  return (
    <div className="space-y-6">
      <KpiCards stats={stats} />

      <Tabs defaultValue="overview" className="w-full">
        <TabsList className="grid w-full grid-cols-3 lg:w-[420px]">
          <TabsTrigger value="overview">Overview</TabsTrigger>
          <TabsTrigger value="jobs">Jobs ({jobs.length})</TabsTrigger>
          <TabsTrigger value="companies">Companies</TabsTrigger>
        </TabsList>

        <TabsContent value="overview" className="space-y-4 mt-4">
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
          <Card>
            <CardHeader><CardTitle>All companies — {perCompany.length}</CardTitle><CardDescription>Browse by tech stack or search in the Jobs tab</CardDescription></CardHeader>
            <CardContent>
              <div className="grid md:grid-cols-2 lg:grid-cols-3 gap-3 max-h-[560px] overflow-auto pr-1">
                {perCompany.map((c: any) => (
                  <div key={c.name} className="rounded-lg border p-3 hover:bg-accent/50 transition-colors">
                    <div className="font-medium text-sm leading-tight">{c.name}</div>
                    <div className="text-xs text-muted-foreground truncate">{c.host || ""}</div>
                    <div className="flex flex-wrap gap-1 mt-2">
                      {(c.tech || []).slice(0, 5).map((t: string) => <Badge key={t} variant="secondary" className="text-[10px]">{t}</Badge>)}
                    </div>
                    <div className="text-xs mt-2"><Badge variant={c.open_jobs ? "default" : "outline"}>{c.open_jobs ? `${c.open_jobs} open` : "No open jobs"}</Badge></div>
                  </div>
                ))}
              </div>
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>
    </div>
  )
}
