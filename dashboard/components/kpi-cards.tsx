import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Building2, Briefcase, Users, TrendingUp, Layers, Wallet, Home, Sparkles, Code2 } from "lucide-react"

type Stats = { total_companies: number; companies_with_jobs: number; open_jobs: number; total_jobs: number; hiring_companies: number }

export type ExtraStats = {
  medianSalary: number | null
  remoteShare: number | null
  newThisWeek: number
  skillsCount: number
}

export function KpiCards({ stats, extra }: { stats: Stats; extra?: ExtraStats }) {
  const items = [
    { label: "Companies", value: stats.total_companies, icon: Building2, sub: `${stats.companies_with_jobs} with career page` },
    { label: "Hiring now", value: stats.hiring_companies, icon: TrendingUp, sub: "Active this crawl" },
    { label: "Open jobs", value: stats.open_jobs, icon: Briefcase, sub: `${stats.total_jobs} total postings`, accent: "text-emerald-600" },
    { label: "Avg per hirer", value: stats.hiring_companies ? (stats.open_jobs / stats.hiring_companies).toFixed(1) : "—", icon: Users, sub: "Jobs / company" },
    { label: "Coverage", value: `${Math.round((stats.companies_with_jobs / stats.total_companies)*100)}%`, icon: Layers, sub: "Career page linked" },
  ]
  const extras = extra ? [
    { label: "Median salary", value: extra.medianSalary ? `৳${Math.round(extra.medianSalary / 1000)}k` : "—", icon: Wallet, sub: "Max, explicit figures" },
    { label: "Remote share", value: extra.remoteShare !== null ? `${extra.remoteShare}%` : "—", icon: Home, sub: "Fully remote openings" },
    { label: "New this week", value: extra.newThisWeek, icon: Sparkles, sub: "Seen in last 7 days", accent: "text-amber-500" },
    { label: "Skills tracked", value: extra.skillsCount, icon: Code2, sub: "Distinct tech tags" },
  ] : []
  return (
    <div className="space-y-4">
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-5 gap-4">
        {items.map(i => (
          <Card key={i.label}>
            <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
              <CardTitle className="text-sm font-medium">{i.label}</CardTitle>
              <i.icon className="h-4 w-4 text-muted-foreground" />
            </CardHeader>
            <CardContent>
              <div className={`text-2xl font-bold tracking-tight ${("accent" in i ? (i as any).accent : "")}`}>{i.value}</div>
              <p className="text-xs text-muted-foreground mt-1">{i.sub}</p>
            </CardContent>
          </Card>
        ))}
      </div>
      {extras.length > 0 && (
        <div className="grid grid-cols-2 lg:grid-cols-4 gap-4">
          {extras.map(i => (
            <Card key={i.label} className="bg-gradient-to-br from-card to-muted/40">
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-1">
                <CardTitle className="text-xs font-medium text-muted-foreground">{i.label}</CardTitle>
                <i.icon className="h-3.5 w-3.5 text-muted-foreground" />
              </CardHeader>
              <CardContent>
                <div className={`text-xl font-bold tracking-tight ${("accent" in i ? (i as any).accent : "")}`}>{i.value}</div>
                <p className="text-[11px] text-muted-foreground mt-0.5">{i.sub}</p>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  )
}
