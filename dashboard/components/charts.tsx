"use client"
import { BarChart, Bar, XAxis, YAxis, Tooltip, ResponsiveContainer, PieChart, Pie, Cell } from "recharts"

export function TechDemandChart({ data }: { data: { tag: string; jobs: number }[] }) {
  const slice = data.slice(0, 12)
  return (
    <div className="h-[280px] w-full">
      <ResponsiveContainer width="100%" height="100%">
        <BarChart data={slice} layout="vertical" margin={{ left: 30, right: 10, top: 5, bottom: 5 }}>
          <XAxis type="number" hide />
          <YAxis dataKey="tag" type="category" width={90} tick={{ fontSize: 11 }} />
          <Tooltip cursor={{ fill: "hsl(var(--muted))" }} contentStyle={{ borderRadius: 12, border: "1px solid hsl(var(--border))" }} />
          <Bar dataKey="jobs" radius={[0, 8, 8, 0]} fill="hsl(var(--primary))" />
        </BarChart>
      </ResponsiveContainer>
    </div>
  )
}

const COLORS = ["hsl(var(--primary))", "hsl(var(--chart-2))", "hsl(var(--chart-3))", "hsl(var(--chart-4))", "hsl(var(--chart-5))"]

export function EmploymentPie({ data }: { data: { employment_type: string | null; jobs: number }[] }) {
  const d = data.filter(x=>x.employment_type).slice(0,5)
  return (
    <div className="h-[200px] w-full">
      <ResponsiveContainer width="100%" height="100%">
        <PieChart>
          <Pie data={d} dataKey="jobs" nameKey="employment_type" cx="50%" cy="50%" outerRadius={80} label={({ name, percent })=>`${name} ${(percent*100).toFixed(0)}%`}>
            {d.map((_, i)=><Cell key={i} fill={COLORS[i%COLORS.length]} />)}
          </Pie>
          <Tooltip />
        </PieChart>
      </ResponsiveContainer>
    </div>
  )
}
