"use client"
import { useMemo, useState } from "react"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Search, ExternalLink, Briefcase } from "lucide-react"

type Row = {
  name: string
  host?: string | null
  tech?: string[] | null
  open_jobs?: number | null
  job_url?: string | null
}

function faviconUrl(host?: string | null) {
  if (!host) return null
  return `https://www.google.com/s2/favicons?domain=${host}&sz=64`
}

export default function CompanyGrid({ rows }: { rows: Row[] }) {
  const [q, setQ] = useState("")
  const [hiringOnly, setHiringOnly] = useState(false)

  const filtered = useMemo(() => {
    const needle = q.toLowerCase()
    return rows
      .filter(c => {
        if (hiringOnly && !c.open_jobs) return false
        if (q && !`${c.name} ${(c.tech || []).join(" ")}`.toLowerCase().includes(needle)) return false
        return true
      })
      .sort((a, b) => (b.open_jobs || 0) - (a.open_jobs || 0) || a.name.localeCompare(b.name))
  }, [rows, q, hiringOnly])

  const hiring = filtered.filter(c => c.open_jobs).length

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          All companies <Badge variant="secondary" className="ml-1">{filtered.length}</Badge>
          <span className="text-xs font-normal text-muted-foreground">· {hiring} hiring</span>
        </CardTitle>
        <div className="flex flex-wrap gap-2 pt-2">
          <div className="relative w-full sm:w-72">
            <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
            <Input placeholder="Search company or tech…" className="pl-8" value={q} onChange={e => setQ(e.target.value)} />
          </div>
          <Button size="sm" variant={hiringOnly ? "default" : "outline"} onClick={() => setHiringOnly(v => !v)}>
            <Briefcase className="h-3.5 w-3.5 mr-1" /> Hiring only
          </Button>
        </div>
      </CardHeader>
      <CardContent>
        <div className="grid md:grid-cols-2 lg:grid-cols-3 gap-3 max-h-[600px] overflow-auto pr-1">
          {filtered.map(c => {
            const icon = faviconUrl(c.host)
            return (
              <div key={c.name} className="rounded-lg border p-3 hover:bg-accent/50 transition-colors group">
                <div className="flex items-start gap-2.5">
                  {icon && (
                    // eslint-disable-next-line @next/next/no-img-element
                    <img src={icon} alt="" width={28} height={28} className="rounded mt-0.5 bg-muted p-0.5" loading="lazy" />
                  )}
                  <div className="min-w-0 flex-1">
                    <div className="font-medium text-sm leading-tight truncate">{c.name}</div>
                    <div className="text-xs text-muted-foreground truncate">{c.host || ""}</div>
                  </div>
                  {c.open_jobs ? (
                    <Badge variant="default" className="shrink-0">{c.open_jobs} open</Badge>
                  ) : (
                    <Badge variant="outline" className="shrink-0 text-muted-foreground">—</Badge>
                  )}
                </div>
                <div className="flex flex-wrap gap-1 mt-2 min-h-[20px]">
                  {(c.tech || []).slice(0, 5).map(t => <Badge key={t} variant="secondary" className="text-[10px]">{t}</Badge>)}
                  {(c.tech || []).length > 5 && <span className="text-xs text-muted-foreground self-center">+{(c.tech || []).length - 5}</span>}
                </div>
                <div className="flex gap-2 mt-2 opacity-0 group-hover:opacity-100 transition-opacity">
                  {c.job_url && (
                    <a href={c.job_url} target="_blank" rel="noreferrer">
                      <Button size="sm" variant="outline" className="h-6 px-2 text-xs"><Briefcase className="h-3 w-3 mr-1" />Careers</Button>
                    </a>
                  )}
                  {c.host && (
                    <a href={`https://${c.host}`} target="_blank" rel="noreferrer">
                      <Button size="sm" variant="ghost" className="h-6 px-2 text-xs"><ExternalLink className="h-3 w-3 mr-1" />Site</Button>
                    </a>
                  )}
                </div>
              </div>
            )
          })}
          {filtered.length === 0 && (
            <p className="col-span-full text-center py-8 text-sm text-muted-foreground">No companies match.</p>
          )}
        </div>
      </CardContent>
    </Card>
  )
}
