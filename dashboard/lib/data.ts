function normalizeHfId(raw: string): string {
  if (!raw) return "swadhinbiswas/bangladeshi-jobs";
  let s = raw.trim();
  for (const p of ["hf://buckets/", "hf://datasets/", "hf://", "datasets/"]) if (s.startsWith(p)) { s = s.slice(p.length); break; }
  if (s.includes("huggingface.co/datasets/")) s = s.split("huggingface.co/datasets/")[1].split("/resolve")[0].split("?")[0];
  return s.replace(/^\/+|\/+$/g, "") || "swadhinbiswas/bangladeshi-jobs";
}

const _raw = process.env.NEXT_PUBLIC_HF_DATASET || process.env.HF_DATASET || process.env.NEXT_PUBLIC_HF_BUCKET || "swadhinbiswas/bangladeshi-jobs";
export const HF_DATASET = normalizeHfId(_raw);
const HF_BASE = `https://huggingface.co/datasets/${HF_DATASET}/resolve/main`;

export type Stats = {
  total_companies: number;
  companies_with_jobs: number;
  open_jobs: number;
  total_jobs: number;
  hiring_companies: number;
};
export type TechDemand = { tag: string; jobs: number; companies: number };
export type CompanyRow = { name: string; host: string; tech: string[]; open_jobs: number; open_current: number };
export type JobRow = {
  company_name: string; title: string; location_text: string | null;
  location_type: string | null; employment_type: string | null;
  salary_min: number | null; salary_max: number | null; salary_currency: string | null;
  tags: string[]; source_url: string | null; apply_links: string[]; last_seen_at: string;
  description_md?: string | null; experience?: string | null;
};

const REVALIDATE = 60;

async function fetchGold<T>(name: string): Promise<T> {
  if (typeof window === "undefined") {
    try {
      const { readFile } = await import("fs/promises");
      const { join } = await import("path");
      const cwd = process.cwd();
      const candidates = [
        join(cwd, "public", "gold", `${name}.json`),
        join(cwd, "dashboard", "public", "gold", `${name}.json`),
        join(cwd, "..", "data", "gold", `${name}.json`),
        join(cwd, "data", "gold", `${name}.json`),
        join(cwd, "..", "..", "data", "gold", `${name}.json`),
      ];
      for (const p of candidates) {
        try {
          const txt = await readFile(p, "utf-8");
          return JSON.parse(txt) as T;
        } catch {}
      }
    } catch {}
  }

  const urls: string[] = [];
  urls.push(`${HF_BASE}/gold/${name}.json`);
  urls.push(`/gold/${name}.json`);
  urls.push(`/api/gold/${name}`);

  let lastErr: any;
  for (const u of urls) {
    try {
      const fetchUrl = u.startsWith("/") && typeof window === "undefined" ? `http://localhost:3000${u}` : u;
      const r = await fetch(fetchUrl, typeof window === "undefined" ? { cache: "no-store" } : { next: { revalidate: REVALIDATE } } as any);
      if (!r.ok) throw new Error(`${u} -> ${r.status}`);
      return (await r.json()) as T;
    } catch (e) {
      lastErr = e;
    }
  }
  throw lastErr;
}

export async function getStats(): Promise<Stats> {
  const rows = await fetchGold<any[]>("stats");
  if (Array.isArray(rows) && rows.length) return rows[0] as Stats;
  return rows as unknown as Stats;
}
export async function getTechDemand(): Promise<TechDemand[]> { return fetchGold<TechDemand[]>("tech_demand"); }
export async function getJobsPerCompany(): Promise<CompanyRow[]> { return fetchGold<CompanyRow[]>("jobs_per_company"); }
export async function getRecentJobs(): Promise<JobRow[]> { return fetchGold<JobRow[]>("recent_jobs"); }
export async function getCompanies(): Promise<any[]> { return fetchGold<any[]>("companies"); }
export async function getLocationHeatmap(): Promise<any[]> { return fetchGold<any[]>("location_heatmap"); }
export async function getSalaryStats(): Promise<any[]> { return fetchGold<any[]>("salary_stats"); }
export async function getEmploymentBreakdown(): Promise<any[]> { return fetchGold<any[]>("employment_breakdown"); }
