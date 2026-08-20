import { cp, mkdir } from "fs/promises"
import { existsSync } from "fs"
import { join } from "path"

const root = join(import.meta.dirname, "../..")
const src = join(root, "data/gold")
const dest = join(import.meta.dirname, "../public/gold")

if (!existsSync(src)) {
  console.log("no data/gold to copy, skipping")
  process.exit(0)
}
await mkdir(dest, { recursive: true })
await cp(src, dest, { recursive: true })
console.log(`copied ${src} -> ${dest}`)
