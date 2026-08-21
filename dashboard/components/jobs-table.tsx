"use client"
import { useMemo, useState } from "react"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import { Search, MapPin, Briefcase, Building2, ExternalLink, Mail, Home, ArrowUpDown, Sparkles } from "lucide-react"
import JobDetailDialog from "@/components/job-detail-dialog"
import { formatSalary } from "@/lib/utils"

type Job = {
  company_name: string
  title: string
  location_text: string | null
  location_type: string | null
  employment_type: string | null
  salary_min: number | null
  salary_max: number | null
  salary_currency: string | null
  tags: string[] | null
  source_url: string | null
  apply_links: string[] | null
  last_seen_at: string | null
  description_md?: string | null
  experience?: string | null
}

const WEEK_MS = 7 * 24 * 3600 * 1000

export default function JobsTable({ jobs }: { jobs: Job[] }) {
  const [q, setQ] = useState("")
  const [company, setCompany] = useState<string>("all")
  const [type, setType] = useState<string>("all")
  const [location, setLocation] = useState<string>("all")
  const [tag, setTag] = useState<string>("all")
  const [remoteOnly, setRemoteOnly] = useState(false)
  const [sort, setSort] = useState<"recent" | "salary">("recent")
  const [selected, setSelected] = useState<Job | null>(null)
  const [page, setPage] = useState(0)
  const PAGE = 20

  const companies = useMemo(() => Array.from(new Set(jobs.map(j => j.company_name))).sort(), [jobs])
  const types = useMemo(() => Array.from(new Set(jobs.map(j => j.employment_type).filter(Boolean) as string[])), [jobs])
  const locs = useMemo(() => Array.from(new Set(jobs.map(j => j.location_type).filter(Boolean) as string[])), [jobs])
  const topTags = useMemo(() => {
    const c = new Map<string, number>()
    for (const j of jobs) for (const t of j.tags || []) c.set(t, (c.get(t) || 0) + 1)
    return Array.from(c.entries()).sort((a, b) => b[1] - a[1]).slice(0, 10).map(([t]) => t)
  }, [jobs])

  const filtered = useMemo(() => {
    const needle = q.toLowerCase()
    const out = jobs.filter(j => {
      if (company !== "all" && j.company_name !== company) return false
      if (type !== "all" && j.employment_type !== type) return false
      if (location !== "all" && j.location_type !== location) return false
      if (tag !== "all" && !(j.tags || []).includes(tag)) return false
      if (remoteOnly && j.location_type !== "Remote") return false
      if (q) {
        const hay = `${j.title} ${j.company_name} ${(j.tags||[]).join(" ")} ${j.location_text||""}`.toLowerCase()
        if (!hay.includes(needle)) return false
      }
      return true
    })
    if (sort === "salary") out.sort((a, b) => (b.salary_max || 0) - (a.salary_max || 0))
    return out
  }, [jobs, q, company, type, location, tag, remoteOnly, sort])

  const paged = filtered.slice(page*PAGE, (page+1)*PAGE)
  const pages = Math.ceil(filtered.length / PAGE)

  return (
    <Card className="border-zinc-200">
      <CardHeader className="pb-3">
        <CardTitle className="flex items-center gap-2 text-base"><Building2 className="h-4 w-4" /> Open Positions <Badge variant="secondary" className="ml-2">{filtered.length}</Badge></CardTitle>
        <div className="grid grid-cols-1 md:grid-cols-12 gap-2 pt-2">
          <div className="md:col-span-5 relative">
            <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
            <Input placeholder="Search title, company, tags, location…" className="pl-8" value={q} onChange={e=>{setQ(e.target.value); setPage(0)}} />
          </div>
          <div className="md:col-span-3">
            <Select value={company} onValueChange={v=>{setCompany(v); setPage(0)}}>
              <SelectTrigger><SelectValue placeholder="Company" /></SelectTrigger>
              <SelectContent>{[<SelectItem key="all-c" value="all">All companies</SelectItem>, ...companies.map(c=><SelectItem key={c} value={c}>{c}</SelectItem>)]}</SelectContent>
            </Select>
          </div>
          <div className="md:col-span-2">
            <Select value={type} onValueChange={v=>{setType(v); setPage(0)}}>
              <SelectTrigger><SelectValue placeholder="Type" /></SelectTrigger>
              <SelectContent>{[<SelectItem key="all-t" value="all">All types</SelectItem>, ...types.map(t=><SelectItem key={t} value={t}>{t}</SelectItem>)]}</SelectContent>
            </Select>
          </div>
          <div className="md:col-span-2">
            <Select value={location} onValueChange={v=>{setLocation(v); setPage(0)}}>
              <SelectTrigger><SelectValue placeholder="Location" /></SelectTrigger>
              <SelectContent>{[<SelectItem key="all-l" value="all">All locations</SelectItem>, ...locs.map(l=><SelectItem key={l} value={l}>{l}</SelectItem>)]}</SelectContent>
            </Select>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-1.5 pt-2">
          <Home className={`h-3.5 w-3.5 ${remoteOnly ? "text-primary" : "text-muted-foreground"}`} />
          <Button size="sm" variant={remoteOnly ? "default" : "outline"} className="h-6 px-2 text-xs rounded-full" onClick={()=>{setRemoteOnly(v=>!v); setPage(0)}}>Remote only</Button>
          <span className="mx-1 h-4 w-px bg-border" />
          {topTags.map(t => (
            <button key={t} onClick={()=>{setTag(tag===t ? "all" : t); setPage(0)}} className="focus:outline-none">
              <Badge variant={tag===t ? "default" : "secondary"} className="cursor-pointer text-[10px] px-2 py-0 hover:opacity-80">{t}</Badge>
            </button>
          ))}
          <span className="mx-1 h-4 w-px bg-border" />
          <Button size="sm" variant={sort==="salary" ? "default" : "outline"} className="h-6 px-2 text-xs rounded-full" onClick={()=>setSort(s=>s==="salary"?"recent":"salary")}>
            <ArrowUpDown className="h-3 w-3 mr-1" /> Salary
          </Button>
        </div>
      </CardHeader>
      <CardContent className="p-0">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-[36%]">Role</TableHead>
              <TableHead>Company</TableHead>
              <TableHead>Location</TableHead>
              <TableHead>Type</TableHead>
              <TableHead>Salary</TableHead>
              <TableHead>Tags</TableHead>
              <TableHead className="text-right">Action</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {paged.map((j, i) => (
              <TableRow key={`${j.company_name}-${j.title}-${i}`} className="group">
                <TableCell className="font-medium">
                  <div className="line-clamp-2 leading-tight">{j.title}
                    {j.last_seen_at && Date.now() - new Date(j.last_seen_at).getTime() < WEEK_MS && (
                      <Badge className="ml-2 bg-amber-500/15 text-amber-600 dark:text-amber-400 border-0 text-[10px] px-1.5 py-0"><Sparkles className="h-2.5 w-2.5 mr-0.5" />NEW</Badge>
                    )}
                  </div>
                  <div className="text-xs text-muted-foreground md:hidden">{j.company_name}</div>
                </TableCell>
                <TableCell className="hidden md:table-cell"><span className="font-medium">{j.company_name}</span></TableCell>
                <TableCell>
                  <span className="inline-flex items-center gap-1 text-xs"><MapPin className="h-3 w-3" />{j.location_text || j.location_type || "—"}</span>
                  {j.location_type && <Badge variant={j.location_type==="Remote"?"default":"secondary"} className="ml-2 hidden lg:inline-flex text-[10px]">{j.location_type}</Badge>}
                </TableCell>
                <TableCell><Badge variant="outline" className="text-xs"><Briefcase className="h-3 w-3 mr-1" />{j.employment_type || "—"}</Badge></TableCell>
                <TableCell className="text-xs">{formatSalary(j.salary_min, j.salary_max, j.salary_currency) || <span className="text-muted-foreground">—</span>}</TableCell>
                <TableCell>
                  <div className="flex flex-wrap gap-1 max-w-[220px]">
                    {(j.tags||[]).slice(0,3).map(t=><Badge key={t} variant="secondary" className="text-[10px] px-1.5 py-0">{t}</Badge>)}
                    {(j.tags||[]).length>3 && <span className="text-xs text-muted-foreground">+{j.tags!.length-3}</span>}
                  </div>
                </TableCell>
                <TableCell className="text-right">
                  <Button size="sm" variant="outline" onClick={()=>setSelected(j)}>View</Button>
                </TableCell>
              </TableRow>
            ))}
            {paged.length===0 && <TableRow><TableCell colSpan={7} className="text-center py-8 text-muted-foreground">No jobs match your filters.</TableCell></TableRow>}
          </TableBody>
        </Table>
        {pages>1 && (
          <div className="flex items-center justify-between p-3 border-t">
            <span className="text-xs text-muted-foreground">Page {page+1} of {pages} — {filtered.length} jobs</span>
            <div className="flex gap-2">
              <Button variant="outline" size="sm" disabled={page===0} onClick={()=>setPage(p=>Math.max(0,p-1))}>Prev</Button>
              <Button variant="outline" size="sm" disabled={page>=pages-1} onClick={()=>setPage(p=>Math.min(pages-1,p+1))}>Next</Button>
            </div>
          </div>
        )}
      </CardContent>
      <JobDetailDialog job={selected} onClose={()=>setSelected(null)} />
    </Card>
  )
}
