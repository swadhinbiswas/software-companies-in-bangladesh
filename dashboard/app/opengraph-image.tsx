import { ImageResponse } from "next/og"

export const alt = "BD Software Jobs — live job dashboard for Bangladeshi tech companies"
export const size = { width: 1200, height: 630 }
export const contentType = "image/png"

async function getStats() {
  try {
    const res = await fetch(
      "https://huggingface.co/datasets/swadhinbiswas/bangladeshi-jobs/resolve/main/gold/stats.json",
      { cache: "no-store" },
    )
    if (!res.ok) throw new Error(String(res.status))
    const rows = await res.json()
    return {
      companies: String(rows[0]?.total_companies ?? 230),
      open: String(rows[0]?.open_jobs ?? 400),
      hiring: String(rows[0]?.hiring_companies ?? 60),
    }
  } catch {
    return { companies: "230", open: "400", hiring: "60" }
  }
}

export default async function OgImage() {
  const { companies, open, hiring } = await getStats()

  return new ImageResponse(
    (
      <div
        style={{
          width: "100%",
          height: "100%",
          display: "flex",
          flexDirection: "column",
          justifyContent: "space-between",
          padding: 64,
          background: "linear-gradient(135deg, #0a0f1e 0%, #10233f 55%, #0d3b2e 100%)",
          color: "#fafafa",
          fontFamily: "sans-serif",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 20 }}>
          <div
            style={{
              width: 72,
              height: 72,
              borderRadius: 18,
              background: "#006a4e",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
            }}
          >
            <div style={{ width: 34, height: 34, borderRadius: 99, background: "#f42a41" }} />
          </div>
          <div style={{ display: "flex", flexDirection: "column" }}>
            <div style={{ fontSize: 44, fontWeight: 700 }}>BD Software Jobs</div>
            <div style={{ fontSize: 24, color: "#93c5fd" }}>Live dashboard · Bangladeshi tech hiring</div>
          </div>
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <div style={{ fontSize: 58, fontWeight: 700, lineHeight: 1.15, maxWidth: 900 }}>
            {`${open} open roles across ${hiring} software companies`}
          </div>
          <div style={{ fontSize: 28, color: "#9ca3af" }}>
            Tech demand · Salaries · Remote jobs — refreshed weekly from career pages
          </div>
        </div>

        <div style={{ display: "flex", gap: 24 }}>
          {[
            { label: "Companies tracked", value: companies },
            { label: "Open jobs", value: open },
            { label: "Hiring now", value: hiring },
          ].map((s) => (
            <div
              key={s.label}
              style={{
                display: "flex",
                flexDirection: "column",
                gap: 6,
                padding: "22px 32px",
                borderRadius: 16,
                background: "rgba(255,255,255,0.07)",
                border: "1px solid rgba(255,255,255,0.14)",
              }}
            >
              <div style={{ fontSize: 44, fontWeight: 700 }}>{s.value}</div>
              <div style={{ fontSize: 20, color: "#9ca3af" }}>{s.label}</div>
            </div>
          ))}
          <div
            style={{
              display: "flex",
              alignItems: "center",
              marginLeft: "auto",
              fontSize: 22,
              color: "#86efac",
            }}
          >
            Open data · CC-BY-4.0
          </div>
        </div>
      </div>
    ),
    { ...size },
  )
}
