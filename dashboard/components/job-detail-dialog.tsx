"use client"
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from "@/components/ui/dialog"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Separator } from "@/components/ui/separator"
import { MapPin, Briefcase, Banknote, Calendar, ExternalLink, Mail, Building2, Tag } from "lucide-react"
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

// very small markdown renderer for job description (no heavy dep)
function Markdown({ text }: { text: string }) {
  // naive: split by lines, handle headings, lists, links
  const lines = text.split("\n")
  return (
    <div className="prose max-w-none prose-zinc text-sm leading-6">
      {lines.map((l, i) => {
        const t = l.trim()
        if (!t) return <div key={i} className="h-2" />
        if (t.startsWith("### ")) return <h3 key={i} className="text-base font-semibold mt-4">{t.slice(4)}</h3>
        if (t.startsWith("## ")) return <h3 key={i} className="text-base font-semibold mt-4">{t.slice(3)}</h3>
        if (t.startsWith("# ")) return <h3 key={i} className="text-lg font-bold mt-4">{t.slice(2)}</h3>
        if (t.startsWith("- ") || t.startsWith("* ")) return <li key={i} className="ml-5 list-disc">{renderInline(t.slice(2))}</li>
        if (t.startsWith("1. ")) return <li key={i} className="ml-5 list-decimal">{renderInline(t.slice(3))}</li>
        return <p key={i} className="my-2">{renderInline(t)}</p>
      })}
    </div>
  )
}
function renderInline(s: string) {
  // links [text](url)
  const parts = s.split(/(\[.*?\]\(.*?\))/g)
  return parts.map((p, i) => {
    const m = p.match(/\[(.*?)\]\((.*?)\)/)
    if (m) return <a key={i} href={m[2]} target="_blank" className="text-primary underline underline-offset-4">{m[1]}</a>
    // bold ** **
    if (p.includes("**")) {
      const b = p.split(/\*\*(.*?)\*\*/g)
      return <span key={i}>{b.map((x, j) => j % 2 ? <strong key={j}>{x}</strong> : x)}</span>
    }
    return <span key={i}>{p}</span>
  })
}

export default function JobDetailDialog({ job, onClose }: { job: Job | null, onClose: () => void }) {
  if (!job) return null
  const open = !!job
  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-w-3xl">
        <DialogHeader className="pr-8">
          <DialogTitle className="text-left flex items-start gap-3">
            <span className="leading-tight">{job.title}</span>
          </DialogTitle>
          <DialogDescription className="text-left flex items-center gap-2">
            <Building2 className="h-3.5 w-3.5" />{job.company_name}
            {job.location_text && <><span className="mx-1">·</span><MapPin className="h-3.5 w-3.5" />{job.location_text}</>}
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-wrap gap-2 text-xs">
          {job.employment_type && <Badge variant="outline"><Briefcase className="h-3 w-3 mr-1" />{job.employment_type}</Badge>}
          {job.location_type && <Badge variant={job.location_type==="Remote"?"default":"secondary"}>{job.location_type}</Badge>}
          {formatSalary(job.salary_min, job.salary_max, job.salary_currency) && <Badge variant="secondary"><Banknote className="h-3 w-3 mr-1" />{formatSalary(job.salary_min, job.salary_max, job.salary_currency)}</Badge>}
          {job.experience && <Badge variant="outline">Exp: {job.experience}</Badge>}
          {job.last_seen_at && <Badge variant="outline"><Calendar className="h-3 w-3 mr-1" />Seen {new Date(job.last_seen_at).toLocaleDateString()}</Badge>}
        </div>

        {job.tags && job.tags.length > 0 && (
          <div className="flex flex-wrap gap-1.5">
            <span className="text-xs text-muted-foreground inline-flex items-center gap-1"><Tag className="h-3 w-3" /> Tags:</span>
            {job.tags.map(t => <Badge key={t} variant="secondary" className="text-xs">{t}</Badge>)}
          </div>
        )}

        <Separator />

        <div className="overflow-auto max-h-[42vh] pr-2 scrollbar-thin">
          {job.description_md ? <Markdown text={job.description_md} /> : <p className="text-sm text-muted-foreground">No description captured. Visit the source link.</p>}
        </div>

        <Separator />

        <div className="flex flex-wrap gap-2">
          {job.source_url && <Button asChild size="sm"><a href={job.source_url} target="_blank"><ExternalLink className="h-4 w-4 mr-2" />View source</a></Button>}
          {(job.apply_links||[]).map((a, i) => (
            a.startsWith("mailto:") ?
              <Button key={i} variant="outline" size="sm" asChild><a href={a}><Mail className="h-4 w-4 mr-2" />{a.replace("mailto:","")}</a></Button> :
              <Button key={i} variant="secondary" size="sm" asChild><a href={a} target="_blank"><ExternalLink className="h-4 w-4 mr-2" />Apply</a></Button>
          ))}
          {!job.source_url && (!job.apply_links||job.apply_links.length===0) && <span className="text-xs text-muted-foreground">Application link not captured — check company career page.</span>}
        </div>
      </DialogContent>
    </Dialog>
  )
}
