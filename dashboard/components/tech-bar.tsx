"use client"
import { Badge } from "@/components/ui/badge"

export default function TechBar({ data }: { data: { tag: string; jobs: number; companies: number }[] }) {
  if (!data.length) return <div className="text-sm text-muted-foreground">No job data available yet.</div>
  const max = Math.max(...data.map(d => d.jobs))
  return (
    <div className="space-y-2">
      {data.map(d => (
        <div key={d.tag} className="flex items-center gap-3">
          <div className="w-36 text-xs font-medium truncate" title={d.tag}>{d.tag}</div>
          <div className="flex-1 h-2.5 bg-secondary rounded-full overflow-hidden">
            <div className="h-full bg-primary rounded-full transition-all" style={{ width: `${(d.jobs / max) * 100}%` }} />
          </div>
          <div className="w-20 text-xs text-right font-mono">{d.jobs} jobs</div>
          <Badge variant="outline" className="text-[10px] hidden sm:inline-flex">{d.companies} co</Badge>
        </div>
      ))}
    </div>
  )
}
