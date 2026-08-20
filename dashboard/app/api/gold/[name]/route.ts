import { NextRequest, NextResponse } from "next/server"

export const runtime = "nodejs"

export async function GET(_req: NextRequest, { params }: { params: { name: string } }) {
  const allowed = new Set(["stats","tech_demand","jobs_per_company","company_tech","location_heatmap","salary_stats","employment_breakdown","recent_jobs","companies"])
  const name = params.name
  if (!allowed.has(name)) return new NextResponse("Not found", { status: 404 })

  // Try HF CDN first if configured (edge cached, superfast)
  const hf = process.env.NEXT_PUBLIC_HF_DATASET || process.env.HF_DATASET
  if (hf) {
    const url = `https://huggingface.co/datasets/${hf}/resolve/main/gold/${name}.json`
    try {
      const r = await fetch(url, { next: { revalidate: 60 } })
      if (r.ok) {
        const j = await r.json()
        return NextResponse.json(j, { headers: { "Cache-Control": "public, s-maxage=60, stale-while-revalidate=300" } })
      }
    } catch {}
  }
  // Fallback to static public/gold via direct fetch (Vercel/Cloudflare serve public/)
  try {
    const url = new URL(`/gold/${name}.json`, _req.url)
    const r = await fetch(url, { next: { revalidate: 60 } })
    if (r.ok) {
      const j = await r.json()
      return NextResponse.json(j, { headers: { "Cache-Control": "public, s-maxage=60" } })
    }
  } catch {}
  return new NextResponse("Not found", { status: 404 })
}
