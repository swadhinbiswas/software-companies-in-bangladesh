import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Badge } from "@/components/ui/badge"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import { HF_DATASET } from "@/lib/data"

const HF_URL = `https://huggingface.co/datasets/${HF_DATASET}`
const FILES = [
  { name: "gold/recent_jobs", rows: "one row per job posting", use: "dashboards, alerts, LLM/RAG corpora" },
  { name: "gold/companies", rows: "one row per company", use: "enrichment, CRM, market maps" },
  { name: "gold/tech_demand", rows: "tag × jobs × companies", use: "skill-demand analytics" },
  { name: "gold/salary_stats", rows: "percentiles per role group", use: "compensation benchmarking" },
  { name: "gold/jobs_per_company", rows: "hiring velocity per company", use: "growth signals, lead scoring" },
  { name: "gold/location_heatmap", rows: "jobs per location", use: "geo analysis" },
  { name: "fact_job / dim_company / bridge_job_tag", rows: "star-schema tables", use: "BI tools (Metabase, Superset, PowerBI)" },
]

function Code({ children }: { children: string }) {
  return (
    <pre className="rounded-lg bg-zinc-950 text-zinc-100 p-3 text-xs overflow-auto leading-relaxed">
      <code>{children}</code>
    </pre>
  )
}

export function DataPlatform() {
  return (
    <div className="space-y-4 mt-4">
      <Card>
        <CardHeader>
          <CardTitle>Use this data in your own project</CardTitle>
          <CardDescription>
            Everything the dashboard shows is open data — refreshed weekly, published as JSON +
            Parquet on Hugging Face, free for any use with attribution.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex flex-wrap gap-2">
            <a href={HF_URL} target="_blank" rel="noreferrer"><Badge>🤗 {HF_DATASET}</Badge></a>
            <Badge variant="secondary">JSON + Parquet</Badge>
            <Badge variant="secondary">Weekly refresh</Badge>
            <Badge variant="outline">CC-BY-4.0</Badge>
          </div>

          <div className="grid lg:grid-cols-2 gap-4">
            <div className="space-y-2">
              <p className="text-sm font-medium">DuckDB — query Parquet directly (no server)</p>
              <Code>{`-- latest jobs mentioning React
SELECT company_name, title, salary_min, salary_max
FROM read_json_auto(
  'https://huggingface.co/datasets/${HF_DATASET}/resolve/main/gold/recent_jobs.json')
WHERE list_contains(tags, 'React')
ORDER BY salary_max DESC NULLS LAST;`}</Code>
            </div>
            <div className="space-y-2">
              <p className="text-sm font-medium">pandas — one line to a DataFrame</p>
              <Code>{`import pandas as pd

df = pd.read_json(
  "https://huggingface.co/datasets/${HF_DATASET}/resolve/main/gold/recent_jobs.json")
print(df.groupby("company_name").size().sort_values(ascending=False).head())`}</Code>
            </div>
          </div>

          <div className="space-y-2">
            <p className="text-sm font-medium">curl — raw files</p>
            <Code>{`curl -L ${HF_URL}/resolve/main/gold/stats.json
curl -L ${HF_URL}/resolve/main/gold/recent_jobs.parquet -o jobs.parquet`}</Code>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Datasets &amp; schema</CardTitle>
          <CardDescription>All tables are stable, typed and versioned by snapshot date.</CardDescription>
        </CardHeader>
        <CardContent className="p-0">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>File</TableHead><TableHead>Grain</TableHead><TableHead>Typical use</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {FILES.map((f) => (
                <TableRow key={f.name}>
                  <TableCell className="font-mono text-xs">{f.name}</TableCell>
                  <TableCell className="text-xs">{f.rows}</TableCell>
                  <TableCell className="text-xs text-muted-foreground">{f.use}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      <div className="grid md:grid-cols-2 gap-4">
        <Card>
          <CardHeader>
            <CardTitle className="text-base">Notes for data engineers</CardTitle>
          </CardHeader>
          <CardContent className="text-sm text-muted-foreground space-y-2">
            <p>
              <strong className="text-foreground">Warehouse:</strong> DuckDB star schema —
              <code className="mx-1 text-xs">fact_job</code>,
              <code className="mx-1 text-xs">dim_company</code>,
              <code className="mx-1 text-xs">bridge_job_tag</code>,
              <code className="mx-1 text-xs">job_snapshot</code> (SCD-style history).
              Build: <code className="text-xs">warehouse/build.py</code>, idempotent by
              <code className="mx-1 text-xs">job_id</code> PK.
            </p>
            <p>
              <strong className="text-foreground">Lineage:</strong> career pages → crawler
              (ATS APIs / JSON-LD / markdown) → LLM extraction + refinement → deterministic
              enhancer → warehouse → gold views → HF bucket. Every job keeps its
              <code className="mx-1 text-xs">source_url</code> for provenance.
            </p>
            <p>
              <strong className="text-foreground">Quality:</strong> dedup by
              (title, location), confidence ≥ 0.5 gate, salary sanity (min ≤ max),
              expired-deadline filtering. Re-runs are incremental via a 24h crawl cache.
            </p>
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle className="text-base">Notes for developers</CardTitle>
          </CardHeader>
          <CardContent className="text-sm text-muted-foreground space-y-2">
            <p>
              <strong className="text-foreground">Job JSON API:</strong> point any client at
              <code className="mx-1 text-xs">gold/recent_jobs.json</code> — fields are camelCase,
              null-safe, and stable. Tags are canonical tech names, safe for filtering UIs.
            </p>
            <p>
              <strong className="text-foreground">Embeddings / RAG:</strong> descriptions are
              cleaned Markdown (~≤8k chars) — chunk on headings. Pair
              <code className="mx-1 text-xs">title + company + location</code> as document metadata.
            </p>
            <p>
              <strong className="text-foreground">Contribute:</strong> add or fix a company in
              <code className="mx-1 text-xs">data/companies.toml</code> (website + verified
              <code className="mx-1 text-xs">job</code> page) — CI validates every entry against
              the tech/type schema on every push.
            </p>
          </CardContent>
        </Card>
      </div>
    </div>
  )
}
